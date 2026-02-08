const BASE: &str = "cold.redb";

use super::BASE_DATA_DIR;
use super::BINCODE_CONFIG;
use anyhow::Result;
use const_format::concatcp;
use redb::Database;
use redb::ReadableDatabase;
use redb::ReadableTable;
use redb::TableDefinition;
use serde::{Serialize, de::DeserializeOwned};
use std::sync::LazyLock;
use std::{path::Path, sync::Arc};
use tokio::task;
use tokio::task::block_in_place;

static COLD_ENGINE: LazyLock<Arc<Database>> = LazyLock::new(|| {
    let path = Path::new(concatcp!(BASE_DATA_DIR, "/", BASE));
    let db = Database::builder().create(path).unwrap();
    Arc::new(db)
});
pub struct ColdTable<K, V>
where
    K: Serialize + DeserializeOwned + Send + Sync + 'static,
    V: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    table_name: &'static str,
    _phantom: std::marker::PhantomData<(K, V)>,
}

impl<K, V> ColdTable<K, V>
where
    K: Serialize + DeserializeOwned + Send + Sync + 'static,
    V: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    pub fn new(table_name: &'static str) -> Self {
        Self {
            table_name,
            _phantom: std::marker::PhantomData,
        }
    }

    /// 异步插入：不阻塞主事件循环，保证磁盘同步性
    pub async fn insert(&self, key: K, value: V) -> Result<()> {
        let table_name = self.table_name;

        // 将阻塞的磁盘操作移交给外部线程池
        block_in_place(move || {
            let key_vec = bincode::serde::encode_to_vec(&key, BINCODE_CONFIG)?;
            let val_vec = bincode::serde::encode_to_vec(&value, BINCODE_CONFIG)?;

            let db = &COLD_ENGINE;
            let txn = db.begin_write()?;
            {
                let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);
                let mut table = txn.open_table(definition)?;
                table.insert(key_vec.as_slice(), val_vec.as_slice())?;
            }
            txn.commit()?; // 这里的 fsync 会在后台线程执行
            Ok(())
        }) // 等待后台线程完成
    }

    pub async fn get_async(&self, key: K) -> Result<Option<V>> {
        block_in_place(|| self.get(key))
    }

    /// 异步查询
    pub fn get(&self, key: K) -> Result<Option<V>> {
        let table_name = self.table_name;

        let key_vec = bincode::serde::encode_to_vec(&key, BINCODE_CONFIG)?;

        let db = &COLD_ENGINE;
        let read_txn = db.begin_read()?;
        let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);

        let table = match read_txn.open_table(definition) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        match table.get(key_vec.as_slice())? {
            Some(access) => {
                let v_bytes = access.value();
                let (decoded, _): (V, usize) =
                    bincode::serde::decode_from_slice(v_bytes, BINCODE_CONFIG)?;
                Ok(Some(decoded))
            }
            None => Ok(None),
        }
    }

    /// 异步删除
    pub async fn remove(&self, key: K) -> Result<()> {
        let table_name = self.table_name;
        task::spawn_blocking(move || {
            let key_vec = bincode::serde::encode_to_vec(&key, BINCODE_CONFIG)?;
            let db = &COLD_ENGINE;
            let txn = db.begin_write()?;
            {
                let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);
                let mut table = txn.open_table(definition)?;
                table.remove(key_vec.as_slice())?;
            }
            txn.commit()?;
            Ok(())
        })
        .await?
    }

    pub async fn get_all_async(&self) -> Result<Vec<(K, V)>> {
        block_in_place(|| self.get_all())
    }

    pub fn get_all(&self) -> Result<Vec<(K, V)>> {
        let table_name = self.table_name;
        let db = &COLD_ENGINE;
        let read_txn = db.begin_read()?;
        let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);
        let table = read_txn.open_table(definition)?;

        let mut results = Vec::new();
        for item in table.iter()? {
            let (k_access, v_access) = item?;
            let (k, _): (K, usize) =
                bincode::serde::decode_from_slice(k_access.value(), BINCODE_CONFIG)?;
            let (v, _): (V, usize) =
                bincode::serde::decode_from_slice(v_access.value(), BINCODE_CONFIG)?;
            results.push((k, v));
        }
        Ok(results)
    }
}
