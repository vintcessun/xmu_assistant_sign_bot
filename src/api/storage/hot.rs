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
use tracing::{debug, error, info, trace, warn};

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
        trace!(table = table_name, "Hot 存储请求插入/更新");
        let key_vec = bincode::serde::encode_to_vec(key, BINCODE_CONFIG).map_err(|e| {
            error!(table = table_name, error = ?e, "Hot 存储键序列化失败 (insert)");
            e
        })?;
        let key_bytes = Bytes::from(key_vec);

        let msg = {
            let val_vec = bincode::serde::encode_to_vec(value, BINCODE_CONFIG).map_err(|e| {
                error!(table = table_name, error = ?e, "Hot 存储值序列化失败 (insert)");
                e
            })?;
            StoreOp::Upsert {
                table_name,
                key: key_bytes,
                value: Bytes::from(val_vec),
            }
        };

        HOT_ENGINE.send(msg).map_err(|e| {
            error!(table = table_name, error = ?e, "Hot 存储发送操作到后台队列失败");
            e
        })?;
        trace!(table = table_name, "Hot 存储插入操作已发送");

        Ok(())
    }

    pub fn delete<K>(table_name: &'static str, key: &K) -> Result<()>
    where
        K: Serialize,
    {
        trace!(table = table_name, "Hot 存储请求删除");
        let key_vec = bincode::serde::encode_to_vec(key, BINCODE_CONFIG).map_err(|e| {
            error!(table = table_name, error = ?e, "Hot 存储键序列化失败 (delete)");
            e
        })?;
        let key_bytes = Bytes::from(key_vec);

        let msg = StoreOp::Delete {
            table_name,
            key: key_bytes,
        };

        HOT_ENGINE.send(msg).map_err(|e| {
            error!(table = table_name, error = ?e, "Hot 存储发送删除操作到后台队列失败");
            e
        })?;
        trace!(table = table_name, "Hot 存储删除操作已发送");

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
        info!(path = ?path, "初始化 Hot 存储引擎 (redb)");

        let db: Database = Database::builder().create(path).unwrap_or_else(|e| {
            error!(path = ?path, error = ?e, "redb 数据库创建或打开失败");
            panic!("redb 数据库初始化失败: {}", e);
        });
        info!("Hot 存储引擎初始化成功");

        let db_arc = Arc::from(db);

        let (tx, mut rx) = mpsc::unbounded_channel::<StoreOp>();

        let db_in = db_arc.clone();
        tokio::task::spawn_blocking(move || {
            let db = db_in;
            info!("Hot 存储后台写入协程已启动");

            while let Some(first_op) = rx.blocking_recv() {
                // 1. 开启事务
                let write_txn = db.begin_write().unwrap_or_else(|e| {
                    error!(error = ?e, "Hot 存储后台写入：开启写事务失败");
                    panic!("Hot 存储后台写入：开启写事务失败: {}", e);
                });
                trace!("Hot 存储后台写入：开启写事务");

                // 2. 处理第一个操作
                process_op(&write_txn, first_op);

                // 3. 批量处理后续操作 (去抖动)
                let mut batch_count = 1;
                while let Ok(next_op) = rx.try_recv() {
                    process_op(&write_txn, next_op);
                    batch_count += 1;
                }
                debug!(
                    count = batch_count,
                    "Hot 存储后台写入：批量处理 {} 个操作", batch_count
                );

                // 4. 提交事务
                write_txn.commit().unwrap_or_else(|e| {
                    error!(error = ?e, "Hot 存储后台写入：提交事务失败");
                    // 提交失败通常意味着数据丢失，但 redb 会保持一致性，我们记录错误并继续
                });
                trace!("Hot 存储后台写入：事务提交完成");
            }
            warn!("Hot 存储后台写入协程退出");
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
        info!(table = table_name, "Hot 存储开始从磁盘加载表数据到内存缓存");
        let cache = DashMap::with_hasher(RandomState::default());

        let read_txn = match self.db.begin_read() {
            Ok(txn) => txn,
            Err(e) => {
                error!(table = table_name, error = ?e, "Hot 存储加载：开启读事务失败");
                return cache;
            }
        };

        let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);

        let table = match read_txn.open_table(definition) {
            Ok(t) => t,
            Err(e) => {
                // 如果是 TableNotFound，则返回 None，否则记录错误
                warn!(table = table_name, error = ?e, "Hot 存储加载：尝试打开表失败");
                return cache;
            }
        };

        // 获取迭代器
        let mut loaded_count = 0;
        match table.iter() {
            Ok(iter) => {
                for item in iter {
                    match item {
                        Ok((key_access, val_access)) => {
                            let k_bytes = key_access.value();
                            let v_bytes = val_access.value();

                            let k_res: Result<(K, usize), _> =
                                bincode::serde::decode_from_slice(k_bytes, BINCODE_CONFIG);
                            let v_res: Result<(V, usize), _> =
                                bincode::serde::decode_from_slice(v_bytes, BINCODE_CONFIG);

                            match (k_res, v_res) {
                                (Ok((key, _)), Ok((value, _))) => {
                                    cache.insert(key, Arc::new(value));
                                    loaded_count += 1;
                                }
                                (Err(e), _) => {
                                    error!(table = table_name, error = ?e, "Hot 存储加载：键反序列化失败，跳过记录");
                                }
                                (_, Err(e)) => {
                                    error!(table = table_name, error = ?e, "Hot 存储加载：值反序列化失败，跳过记录");
                                }
                            }
                        }
                        Err(e) => {
                            error!(table = table_name, error = ?e, "Hot 存储加载：迭代器获取项失败");
                        }
                    }
                }
            }
            Err(e) => {
                error!(table = table_name, error = ?e, "Hot 存储加载：初始化迭代器失败");
            }
        }

        info!(
            table = table_name,
            count = loaded_count,
            "Hot 存储加载完成，加载 {} 条记录到内存缓存",
            loaded_count
        );
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
            match txn.open_table(definition) {
                Ok(mut table) => match table.insert(key.as_ref(), value.as_ref()) {
                    Ok(_) => trace!(
                        table = table_name,
                        key_size = key.len(),
                        "Hot 存储后台写入: 插入/更新成功"
                    ),
                    Err(e) => {
                        error!(table = table_name, key_size = key.len(), error = ?e, "Hot 存储后台写入: 插入/更新操作失败")
                    }
                },
                Err(e) => {
                    error!(table = table_name, error = ?e, "Hot 存储后台写入: 打开表失败 (Upsert)")
                }
            }
        }
        StoreOp::Delete { table_name, key } => {
            let definition: TableDefinition<&[u8], &[u8]> = TableDefinition::new(table_name);
            match txn.open_table(definition) {
                Ok(mut table) => match table.remove(key.as_ref()) {
                    Ok(_) => trace!(
                        table = table_name,
                        key_size = key.len(),
                        "Hot 存储后台写入: 删除成功"
                    ),
                    Err(e) => {
                        error!(table = table_name, key_size = key.len(), error = ?e, "Hot 存储后台写入: 删除操作失败")
                    }
                },
                Err(e) => {
                    error!(table = table_name, error = ?e, "Hot 存储后台写入: 打开表失败 (Delete)")
                }
            }
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
        debug!(table = table_name, "初始化 HotTable");
        HotTable {
            table_name,
            cache: HOT_ENGINE.read_table(table_name),
        }
    }

    pub fn insert(&self, key: K, value: Arc<V>) -> Result<()> {
        trace!(table = self.table_name, "HotTable 插入数据，触发后台写入");
        send_engine::insert(self.table_name, &key, &*value)?;
        self.cache.insert(key, value.clone());
        trace!(table = self.table_name, "HotTable 内存缓存更新成功");
        Ok(())
    }

    pub fn remove(&self, key: &K) -> Result<()> {
        trace!(table = self.table_name, "HotTable 删除数据，触发后台写入");
        send_engine::delete(self.table_name, key)?;
        self.cache.remove(key);
        trace!(table = self.table_name, "HotTable 内存缓存删除成功");
        Ok(())
    }

    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        let result = self.cache.get(key).map(|v| v.clone());
        if result.is_some() {
            trace!(table = self.table_name, "HotTable 命中缓存");
        } else {
            trace!(table = self.table_name, "HotTable 未命中缓存");
        }
        result
    }
}

impl<K, V> IntoIterator for HotTable<K, V>
where
    K: Serialize + DeserializeOwned + std::hash::Hash + Eq + Send + 'static,
    V: Serialize + DeserializeOwned + Send + 'static,
{
    type Item = (K, Arc<V>);
    type IntoIter = dashmap::iter::OwningIter<K, Arc<V>, RandomState>;

    fn into_iter(self) -> Self::IntoIter {
        self.cache.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a HotTable<K, V>
where
    K: Serialize + DeserializeOwned + std::hash::Hash + Eq + Send + Sync + 'static,
    V: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    type Item = dashmap::mapref::multiple::RefMulti<'a, K, Arc<V>>;
    type IntoIter =
        dashmap::iter::Iter<'a, K, Arc<V>, RandomState, DashMap<K, Arc<V>, RandomState>>;

    fn into_iter(self) -> Self::IntoIter {
        self.cache.iter()
    }
}
