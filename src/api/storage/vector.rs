use crate::api::storage::ColdTable;
use anyhow::Result;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use hnsw_rs::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use tracing::info;
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
        // 初始化 HNSW 参数
        // M=16, max_elements=100万, ef_construction=200, ef_search=20
        let index = Hnsw::new(16, 1000000, 200, 20, DistCosine {});
        let id_map = DashMap::new();

        for (i, (uuid, value)) in records.into_iter().enumerate() {
            // 插入索引：(向量数据, 内部自增ID)
            index.insert((value.get_embedding(), i));
            // 映射关系存入 DashMap
            id_map.insert(i, uuid);
        }
        (index, id_map)
    }

    /// 1. 加载并重建索引
    pub fn new(table_name: &'static str) -> Self {
        let kv_table: ColdTable<Uuid, Arc<V>> = ColdTable::new(table_name);

        // 从 redb 读取所有数据
        let all_records = kv_table.get_all().unwrap_or_default();

        info!("正在重建索引，总计 {} 条记录...", all_records.len());

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

        // A. 写入持久化数据库 (ColdTable)
        self.kv_table.insert(uuid, value).await?;

        // B. 更新内存索引
        // 注意：这里需要确定一个新的 internal_id，通常可以用 id_map 的长度
        let current_id_map = self.id_map.load();
        let current_index = self.index.load();
        let internal_id = current_id_map.len();
        current_index.insert((&embedding, internal_id));
        current_id_map.insert(internal_id, uuid);

        Ok(uuid)
    }

    /// 3. 向量搜索 (语义搜索)
    pub async fn search(&self, query_vec: Vec<f32>, top_k: usize) -> Result<Vec<(Uuid, Arc<V>)>> {
        let index = self.index.load();
        let id_map = self.id_map.load();

        let neighbor_ids = tokio::task::spawn_blocking(move || {
            // search 参数：查询向量，返回数量，ef_search（搜索精度）
            index.search(&query_vec, top_k, 32)
        })
        .await?;

        let mut results = Vec::new();
        for neighbor in neighbor_ids {
            // 从 DashMap 获取 UUID
            if let Some(uuid) = id_map.get(&neighbor.d_id) {
                // 从 ColdTable 获取完整磁盘数据
                if let Some(data) = self.kv_table.get_async(*uuid).await? {
                    results.push((*uuid, data));
                }
            }
        }

        Ok(results)
    }

    /// 删除数据
    /// 这非常昂贵，因为要重建索引
    pub async fn remove(&self, uuids: Vec<Uuid>) -> Result<()> {
        // 从持久化数据库删除
        for uuid in uuids {
            self.kv_table.remove(uuid).await?;
        }

        // 2. 获取当前所有剩余数据
        let all_active_records = self.kv_table.get_all_async().await?;

        // 3. 在后台线程（Blocking）重建索引，避免阻塞异步运行时
        info!(
            "正在因为删除操作重建向量索引，剩余记录: {}",
            all_active_records.len()
        );

        let (new_index, new_id_map) =
            tokio::task::spawn_blocking(move || Self::build_index_sync(all_active_records)).await?;

        // 4. 原子替换！
        self.index.store(Arc::new(new_index));
        self.id_map.store(Arc::new(new_id_map));

        info!("索引重建完成并已热替换");
        Ok(())
    }

    /// 获取消息记录通过Uuid
    pub async fn get(&self, uuid: Uuid) -> Option<Arc<V>> {
        self.kv_table.get_async(uuid).await.ok().flatten()
    }
}
