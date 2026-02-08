use crate::{
    abi::message::MessageSend,
    api::{
        llm::chat::{
            archive::message_storage::MessageStorage, audit::bridge::llm_msg_from_message,
            llm::get_chat_embedding,
        },
        storage::{HasEmbedding, VectorSearchEngine},
    },
};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};
use tracing::{debug, error, info, warn};

static MESSAGE_SEARCH_DB: LazyLock<VectorSearchEngine<MessageSearchStore>> =
    LazyLock::new(|| VectorSearchEngine::new("llm_chat_message_search_store"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSearchStore {
    pub msg_id: String,
    pub reply: MessageSend,
    pub embedding: Vec<f32>,
}

impl HasEmbedding for MessageSearchStore {
    fn get_embedding(&self) -> &[f32] {
        &self.embedding
    }
}

impl MessageSearchStore {
    async fn new(msg_id: String, reply_msg: MessageSend) -> Result<Self> {
        debug!(msg_id = %msg_id, "开始创建 MessageSearchStore 实例");
        let msg = MessageStorage::get(msg_id.clone()).await.ok_or_else(|| {
            warn!(msg_id = %msg_id, "原始消息在 MessageStorage 中不存在");
            anyhow!("消息不存在")
        })?;

        // reply 消息也需要转为 LLM 消息格式以便嵌入
        let reply = llm_msg_from_message(&reply_msg).await;
        let mut msgs = vec![msg];
        msgs.extend(reply);

        let embedding = get_chat_embedding(msgs).await.map_err(|e| {
            error!(msg_id = %msg_id, error = ?e, "计算消息嵌入向量失败");
            e
        })?;

        debug!(msg_id = %msg_id, embedding_size = ?embedding.len(), "消息嵌入向量计算完成");

        Ok(Self {
            msg_id,
            reply: reply_msg,
            embedding,
        })
    }

    pub async fn insert(msg_id: String, reply_msg: MessageSend) -> Result<()> {
        info!(msg_id = %msg_id, "尝试插入消息搜索存储");
        let store = Self::new(msg_id, reply_msg).await.map_err(|e| {
            error!(error = ?e, "创建 MessageSearchStore 实例失败");
            e
        })?;
        let msg_id = store.msg_id.clone();
        MESSAGE_SEARCH_DB
            .insert(Arc::new(store))
            .await
            .map_err(|e| {
                error!(msg_id = msg_id, error = ?e, "插入向量数据库失败");
                e
            })?;
        info!(msg_id = msg_id, "消息搜索存储插入成功");
        Ok(())
    }

    pub async fn search(key: Vec<f32>, top_k: usize) -> Result<Vec<(String, MessageSend)>> {
        debug!(top_k = ?top_k, "开始在向量数据库中搜索");
        let results = MESSAGE_SEARCH_DB.search(key, top_k).await.map_err(|e| {
            error!(top_k = ?top_k, error = ?e, "向量数据库搜索失败");
            e
        })?;

        debug!(results_count = ?results.len(), "向量数据库搜索完成");

        let mapped = results
            .into_iter()
            .map(|(_, store)| (store.msg_id.clone(), store.reply.clone()))
            .collect();
        Ok(mapped)
    }
}
