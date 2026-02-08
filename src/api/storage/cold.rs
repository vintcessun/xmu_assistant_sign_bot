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
use tokio::task::block_in_place;
use tracing::{debug, error, info, trace, warn};

static COLD_ENGINE: LazyLock<Arc<Database>> = LazyLock::new(|| {
    let path = Path::new(concatcp!(BASE_DATA_DIR, "/", BASE));
    info!(path = ?path, "初始化 Cold 存储引擎 (redb)");
    let db = Database::builder().create(path).unwrap_or_else(|e| {
        error!(path = ?path, error = ?e, "redb 数据库创建或打开失败");
        panic!("redb 数据库初始化失败: {}", e);
    });
    info!("Cold 存储引擎初始化成功");
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
    pub async fn insert(&self, key: &K, value: &V) -> Result<()> {
        let table_name = self.table_name;
        trace!(table = table_name, "Cold 存储开始插入数据");

        // 将阻塞的磁盘操作移交给外部线程池 (恢复为 block_in_place 以提升性能)
        block_in_place(move || {
            let key_vec = bincode::serde::encode_to_vec(key, BINCODE_CONFIG).map_err(|e| {
                error!(error = ?e, "Cold 存储键序列化失败");
                e
            })?;
            let val_vec = bincode::serde::encode_to_vec(value, BINCODE_CONFIG).map_err(|e| {
                error!(error = ?e, "Cold 存储值序列化失败");
                e
            })?;

            let db = &COLD_ENGINE;
            let txn = db.begin_write().map_err(|e| {
                error!(table = table_name, error = ?e, "Cold 存储开启写事务失败");
                e
            })?;
            {
                let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);
                let mut table = txn.open_table(definition).map_err(|e| {
                    error!(table = table_name, error = ?e, "Cold 存储打开表失败");
                    e
                })?;
                table
                    .insert(key_vec.as_slice(), val_vec.as_slice())
                    .map_err(|e| {
                        error!(table = table_name, error = ?e, "Cold 存储插入操作失败");
                        e
                    })?;
            }
            txn.commit().map_err(|e| {
                error!(table = table_name, error = ?e, "Cold 存储提交事务失败");
                e
            })?; // 这里的 fsync 会在后台线程执行
            trace!(table = table_name, "Cold 存储插入数据并提交成功");
            Ok(())
        }) // 等待后台线程完成
    }

    pub async fn get_async(&self, key: &K) -> Result<Option<V>> {
        block_in_place(|| self.get(key))
    }

    /// 异步查询
    pub fn get(&self, key: &K) -> Result<Option<V>> {
        let table_name = self.table_name;
        trace!(table = table_name, "Cold 存储开始查询数据");

        let key_vec = bincode::serde::encode_to_vec(key, BINCODE_CONFIG).map_err(|e| {
            error!(error = ?e, "Cold 存储键序列化失败 (get)");
            e
        })?;

        let db = &COLD_ENGINE;
        let read_txn = db.begin_read().map_err(|e| {
            error!(table = table_name, error = ?e, "Cold 存储开启读事务失败");
            e
        })?;
        let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);

        let table = match read_txn.open_table(definition) {
            Ok(t) => t,
            Err(e) => {
                // 如果是 TableNotFound，则返回 None，否则记录错误
                warn!(table = table_name, error = ?e, "Cold 存储尝试打开表失败");
                return Ok(None);
            }
        };

        match table.get(key_vec.as_slice()).map_err(|e| {
            error!(table = table_name, error = ?e, "Cold 存储查询操作失败");
            e
        })? {
            Some(access) => {
                let v_bytes = access.value();
                let (decoded, _): (V, usize) =
                    bincode::serde::decode_from_slice(v_bytes, BINCODE_CONFIG).map_err(|e| {
                        error!(table = table_name, error = ?e, "Cold 存储值反序列化失败");
                        e
                    })?;
                trace!(table = table_name, "Cold 存储查询数据成功");
                Ok(Some(decoded))
            }
            None => {
                trace!(table = table_name, "Cold 存储未找到数据");
                Ok(None)
            }
        }
    }

    /// 异步删除
    pub async fn remove(&self, key: &K) -> Result<()> {
        let table_name = self.table_name;
        trace!(table = table_name, "Cold 存储开始删除数据");
        block_in_place(move || {
            let key_vec = bincode::serde::encode_to_vec(key, BINCODE_CONFIG).map_err(|e| {
                error!(error = ?e, "Cold 存储键序列化失败 (remove)");
                e
            })?;

            let db = &COLD_ENGINE;
            let txn = db.begin_write().map_err(|e| {
                error!(table = table_name, error = ?e, "Cold 存储开启写事务失败 (remove)");
                e
            })?;
            {
                let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);
                let mut table = txn.open_table(definition).map_err(|e| {
                    error!(table = table_name, error = ?e, "Cold 存储打开表失败 (remove)");
                    e
                })?;
                table.remove(key_vec.as_slice()).map_err(|e| {
                    error!(table = table_name, error = ?e, "Cold 存储删除操作失败");
                    e
                })?;
            }
            txn.commit().map_err(|e| {
                error!(table = table_name, error = ?e, "Cold 存储提交事务失败 (remove)");
                e
            })?;
            trace!(table = table_name, "Cold 存储删除数据并提交成功");
            Ok(())
        })
    }

    pub async fn get_all_async(&self) -> Result<Vec<(K, V)>> {
        block_in_place(|| self.get_all())
    }

    pub fn get_all(&self) -> Result<Vec<(K, V)>> {
        let table_name = self.table_name;
        trace!(table = table_name, "Cold 存储开始获取所有数据");

        let db = &COLD_ENGINE;
        let read_txn = db.begin_read().map_err(|e| {
            error!(table = table_name, error = ?e, "Cold 存储开启读事务失败 (get_all)");
            e
        })?;
        let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);
        let table = read_txn.open_table(definition).map_err(|e| {
            error!(table = table_name, error = ?e, "Cold 存储打开表失败 (get_all)");
            e
        })?;

        let mut results = Vec::new();
        for item in table.iter().map_err(|e| {
            error!(table = table_name, error = ?e, "Cold 存储迭代器初始化失败");
            e
        })? {
            let (k_access, v_access) = item.map_err(|e| {
                error!(table = table_name, error = ?e, "Cold 存储迭代器获取项失败");
                e
            })?;

            let (k, _): (K, usize) =
                bincode::serde::decode_from_slice(k_access.value(), BINCODE_CONFIG).map_err(
                    |e| {
                        error!(table = table_name, error = ?e, "Cold 存储键反序列化失败 (get_all)");
                        e
                    },
                )?;
            let (v, _): (V, usize) =
                bincode::serde::decode_from_slice(v_access.value(), BINCODE_CONFIG).map_err(
                    |e| {
                        error!(table = table_name, error = ?e, "Cold 存储值反序列化失败 (get_all)");
                        e
                    },
                )?;
            results.push((k, v));
        }

        debug!(
            table = table_name,
            count = results.len(),
            "Cold 存储成功获取所有数据"
        );
        Ok(results)
    }
}
