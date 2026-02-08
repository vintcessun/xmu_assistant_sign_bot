const BASE: &str = "hot.redb";

use super::BASE_DATA_DIR;
use super::BINCODE_CONFIG;
use ahash::RandomState;
use anyhow::Result;
use bytes::Bytes;
use const_format::concatcp;
use dashmap::DashMap;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::LazyLock;
use std::{path::Path, sync::Arc};
use tokio::sync::mpsc::{self, UnboundedSender};

enum StoreOp {
    Upsert {
        table_name: &'static str,
        key: Bytes,
        value: Bytes,
    },
    Delete {
        table_name: &'static str,
        key: Bytes,
    },
}

pub mod send_engine {
    use super::*;

    pub fn insert<K, V>(table_name: &'static str, key: &K, value: &V) -> Result<()>
    where
        K: Serialize,
        V: Serialize,
    {
        let key_vec = bincode::serde::encode_to_vec(key, BINCODE_CONFIG)?;
        let key_bytes = Bytes::from(key_vec);

        let msg = {
            let val_vec = bincode::serde::encode_to_vec(value, BINCODE_CONFIG)?;
            StoreOp::Upsert {
                table_name,
                key: key_bytes,
                value: Bytes::from(val_vec),
            }
        };

        HOT_ENGINE.send(msg)?;

        Ok(())
    }

    pub fn delete<K>(table_name: &'static str, key: &K) -> Result<()>
    where
        K: Serialize,
    {
        let key_vec = bincode::serde::encode_to_vec(key, BINCODE_CONFIG)?;
        let key_bytes = Bytes::from(key_vec);

        let msg = StoreOp::Delete {
            table_name,
            key: key_bytes,
        };

        HOT_ENGINE.send(msg)?;

        Ok(())
    }
}

static HOT_ENGINE: LazyLock<StorageEngine> = LazyLock::new(StorageEngine::create);

struct StorageEngine {
    pub sender: UnboundedSender<StoreOp>,
    pub db: Arc<Database>,
}

impl StorageEngine {
    pub fn create() -> Self {
        let path = Path::new(concatcp!(BASE_DATA_DIR, "/", BASE));

        let db: Database = Database::builder().create(path).unwrap();

        let db_arc = Arc::from(db);

        let (tx, mut rx) = mpsc::unbounded_channel::<StoreOp>();

        let db_in = db_arc.clone();
        tokio::task::spawn_blocking(move || {
            let db = db_in;

            while let Some(first_op) = rx.blocking_recv() {
                let write_txn = db.begin_write().unwrap();

                process_op(&write_txn, first_op);

                while let Ok(next_op) = rx.try_recv() {
                    process_op(&write_txn, next_op);
                }

                write_txn.commit().unwrap();
            }
        });

        Self {
            sender: tx,
            db: db_arc,
        }
    }

    pub fn send(&self, op: StoreOp) -> Result<()> {
        self.sender.send(op)?;
        Ok(())
    }

    pub fn read_table<K, V>(&self, table_name: &'static str) -> DashMap<K, Arc<V>, RandomState>
    where
        K: Serialize + DeserializeOwned + std::hash::Hash + Eq,
        V: Serialize + DeserializeOwned,
    {
        let cache = DashMap::with_hasher(RandomState::default());

        let read_txn = match self.db.begin_read() {
            Ok(txn) => txn,
            Err(e) => {
                eprintln!("Failed to begin read transaction: {}", e);
                return cache;
            }
        };

        let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);

        let table = match read_txn.open_table(definition) {
            Ok(t) => t,
            Err(_) => {
                // 如果表不存在（可能是第一次启动），直接返回空的 DashMap
                return cache;
            }
        };

        // 获取迭代器
        if let Ok(iter) = table.iter() {
            for (key_access, val_access) in iter.flatten() {
                let k_bytes = key_access.value();
                let v_bytes = val_access.value();

                let k_res: Result<(K, usize), _> =
                    bincode::serde::decode_from_slice(k_bytes, BINCODE_CONFIG);
                let v_res: Result<(V, usize), _> =
                    bincode::serde::decode_from_slice(v_bytes, BINCODE_CONFIG);

                if let (Ok((key, _)), Ok((value, _))) = (k_res, v_res) {
                    cache.insert(key, Arc::new(value));
                }
            }
        }

        cache
    }
}

fn process_op(txn: &redb::WriteTransaction, op: StoreOp) {
    match op {
        StoreOp::Upsert {
            table_name,
            key,
            value,
        } => {
            let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);
            let mut table = txn.open_table(definition).unwrap();
            table.insert(key.as_ref(), value.as_ref()).unwrap();
        }
        StoreOp::Delete { table_name, key } => {
            let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);
            let mut table = txn.open_table(definition).unwrap();
            table.remove(key.as_ref()).unwrap();
        }
    }
}

pub struct HotTable<K, V>
where
    K: Serialize + DeserializeOwned + std::hash::Hash + Eq,
    V: Serialize + DeserializeOwned,
{
    table_name: &'static str,
    cache: DashMap<K, Arc<V>, RandomState>,
}

impl<K, V> HotTable<K, V>
where
    K: Serialize + DeserializeOwned + std::hash::Hash + Eq,
    V: Serialize + DeserializeOwned,
{
    pub fn new(table_name: &'static str) -> Self {
        HotTable {
            table_name,
            cache: HOT_ENGINE.read_table(table_name),
        }
    }

    pub fn insert(&self, key: K, value: Arc<V>) -> Result<()> {
        send_engine::insert(self.table_name, &key, &*value)?;
        self.cache.insert(key, value.clone());
        Ok(())
    }

    pub fn remove(&self, key: &K) -> Result<()> {
        send_engine::delete(self.table_name, key)?;
        self.cache.remove(key);
        Ok(())
    }

    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        self.cache.get(key).map(|v| v.clone())
    }
}
