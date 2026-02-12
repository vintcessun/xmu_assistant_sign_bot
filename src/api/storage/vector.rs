use crate::api::storage::ColdTable;
use anyhow::Result;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use hnsw_rs::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use tokio::task::block_in_place;
use tracing::{debug, error, info, trace, warn};
use uuid::Uuid;

// 需要实现这个 trait 来提供向量
pub trait HasEmbedding {
    fn get_embedding(&self) -> &[f32];
}

pub struct VectorSearchEngine<V>
where
    V: Serialize + DeserializeOwned + Send + Sync + HasEmbedding + 'static,
{
    // 你原有的持久化表
    kv_table: ColdTable<Uuid, Arc<V>>,
    // 内存向量索引
    index: ArcSwap<Hnsw<'static, f32, DistCosine>>,
    // 内存 ID 映射：HNSW 内部 ID -> 业务 UUID
    id_map: ArcSwap<DashMap<usize, Uuid>>,
}

impl<V> VectorSearchEngine<V>
where
    V: Serialize + DeserializeOwned + Send + Sync + HasEmbedding + 'static,
{
    fn build_index_sync(
        records: Vec<(Uuid, Arc<V>)>,
    ) -> (Hnsw<'static, f32, DistCosine>, DashMap<usize, Uuid>) {
        debug!(count = records.len(), "开始同步构建 HNSW 向量索引");
        // 初始化 HNSW 参数
        // M=16, max_elements=100万, ef_construction=200, ef_search=20
        let index = Hnsw::new(16, 1000000, 200, 20, DistCosine {});
        let id_map = DashMap::new();

        let records_len = records.len();

        for (i, (uuid, value)) in records.into_iter().enumerate() {
            // 插入索引：(向量数据, 内部自增ID)
            index.insert((value.get_embedding(), i));
            // 映射关系存入 DashMap
            id_map.insert(i, uuid);
            trace!(internal_id = i, uuid = ?uuid, "插入索引点");
        }
        debug!(count = records_len, "HNSW 向量索引构建完成");
        (index, id_map)
    }

    /// 1. 加载并重建索引
    pub fn new(table_name: &'static str) -> Self {
        debug!(table_name = table_name, "初始化向量搜索引擎");
        let kv_table: ColdTable<Uuid, Arc<V>> = ColdTable::new(table_name);

        // 从 redb 读取所有数据
        let all_records = kv_table.get_all().unwrap_or_else(|e| {
            debug!(table_name = table_name, error = ?e, "从 ColdTable 读取所有记录失败，返回空集");
            Vec::new()
        });

        info!(
            table_name = table_name,
            count = all_records.len(),
            "正在重建向量索引"
        );

        let (index, id_map) = Self::build_index_sync(all_records);

        Self {
            kv_table,
            index: ArcSwap::new(Arc::new(index)),
            id_map: ArcSwap::new(Arc::new(id_map)),
        }
    }

    /// 2. 插入新数据（同步写入磁盘和内存索引）
    pub async fn insert(&self, value: Arc<V>) -> Result<Uuid> {
        let uuid = Uuid::new_v4();
        let embedding = value.get_embedding().to_vec();
        debug!(uuid = ?uuid, "开始插入新向量数据");

        // A. 写入持久化数据库 (ColdTable)
        self.kv_table.insert(&uuid, &value).await?;
        debug!(uuid = ?uuid, "持久化数据写入 ColdTable 成功");

        // B. 更新内存索引
        // 注意：这里需要确定一个新的 internal_id，通常可以用 id_map 的长度
        let current_id_map = self.id_map.load();
        let current_index = self.index.load();
        let internal_id = current_id_map.len();

        // HNSW 插入 (阻塞操作，需要 spawn_blocking)
        tokio::task::spawn_blocking(move || {
            current_index.insert((&embedding, internal_id));
        })
        .await
        .map_err(|e| {
            error!(uuid = ?uuid, error = ?e, "HNSW 索引插入任务失败");
            anyhow::anyhow!("HNSW 索引插入任务失败: {}", e)
        })?;

        // ID 映射插入
        current_id_map.insert(internal_id, uuid);

        debug!(uuid = ?uuid, internal_id = internal_id, "内存向量索引更新成功");
        Ok(uuid)
    }

    /// 3. 向量搜索 (语义搜索)
    pub async fn search(&self, query_vec: &[f32], top_k: usize) -> Result<Vec<(Uuid, Arc<V>)>> {
        debug!(top_k = top_k, "开始向量搜索");
        let index = self.index.load();
        let id_map = self.id_map.load();

        let neighbor_ids = block_in_place(move || {
            trace!("执行 HNSW 搜索");
            // search 参数：查询向量，返回数量，ef_search（搜索精度）
            index.search(query_vec, top_k, 32)
        });

        let mut results: Vec<(Uuid, Arc<V>)> = Vec::new();
        for neighbor in neighbor_ids {
            trace!(internal_id = neighbor.d_id, "发现邻近 ID");
            // 从 DashMap 获取 UUID
            if let Some(uuid) = id_map.get(&neighbor.d_id) {
                // 从 ColdTable 获取完整磁盘数据
                match self.kv_table.get_async(&uuid).await {
                    Ok(Some(data)) => {
                        results.push((*uuid, data));
                        debug!(uuid = ?uuid, "成功检索到匹配的向量数据");
                    }
                    Ok(None) => {
                        warn!(uuid = ?uuid, "向量索引命中了记录，但在 ColdTable 中未找到数据");
                    }
                    Err(e) => {
                        error!(uuid = ?uuid, error = ?e, "从 ColdTable 获取数据失败");
                        return Err(e); // 立即返回错误
                    }
                }
            } else {
                warn!(
                    internal_id = neighbor.d_id,
                    "HNSW 索引命中了内部 ID，但在 ID 映射中未找到对应的 UUID"
                );
            }
        }
        debug!(
            count = results.len(),
            "向量搜索完成，返回 {} 条结果",
            results.len()
        );
        Ok(results)
    }

    /// 删除数据
    /// 这非常昂贵，因为要重建索引
    pub async fn remove(&self, uuids: Vec<Uuid>) -> Result<()> {
        debug!(count = uuids.len(), "开始执行向量数据删除和索引重建操作");

        // 1. 从持久化数据库删除
        for uuid in uuids {
            self.kv_table.remove(&uuid).await.map_err(|e| {
                error!(uuid = ?uuid, error = ?e, "从 ColdTable 删除数据失败");
                e
            })?;
            debug!(uuid = ?uuid, "数据从 ColdTable 删除成功");
        }

        // 2. 获取当前所有剩余数据
        let all_active_records = self.kv_table.get_all_async().await.map_err(|e| {
            error!(error = ?e, "获取 ColdTable 剩余记录失败");
            e
        })?;

        // 3. 在后台线程（Blocking）重建索引，避免阻塞异步运行时
        info!(
            remaining_count = all_active_records.len(),
            "因数据删除操作，正在重建向量索引"
        );

        let (new_index, new_id_map) =
            tokio::task::spawn_blocking(move || Self::build_index_sync(all_active_records))
                .await
                .map_err(|e| {
                    error!(error = ?e, "向量索引重建任务执行失败");
                    anyhow::anyhow!("向量索引重建任务执行失败: {}", e)
                })?;

        // 4. 原子替换！
        self.index.store(Arc::new(new_index));
        self.id_map.store(Arc::new(new_id_map));

        info!("向量索引重建完成并已热替换");
        Ok(())
    }

    /// 获取消息记录通过Uuid
    pub async fn get(&self, uuid: Uuid) -> Option<Arc<V>> {
        trace!(uuid = ?uuid, "尝试从 ColdTable 获取向量记录");
        match self.kv_table.get_async(&uuid).await {
            Ok(Some(data)) => {
                debug!(uuid = ?uuid, "成功获取向量记录");
                Some(data)
            }
            Ok(None) => {
                trace!(uuid = ?uuid, "未找到指定的向量记录");
                None
            }
            Err(e) => {
                error!(uuid = ?uuid, error = ?e, "从 ColdTable 获取向量记录失败");
                None
            }
        }
    }
}
