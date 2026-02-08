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
        let msg = MessageStorage::get(msg_id.clone())
            .await
            .ok_or(anyhow!("消息不存在"))?;
        let reply = llm_msg_from_message(&reply_msg).await;
        let mut msgs = vec![msg];
        msgs.extend(reply);

        let embedding = get_chat_embedding(msgs).await?;

        Ok(Self {
            msg_id,
            reply: reply_msg,
            embedding,
        })
    }

    pub async fn insert(msg_id: String, reply_msg: MessageSend) -> Result<()> {
        let store = Self::new(msg_id, reply_msg).await?;
        MESSAGE_SEARCH_DB.insert(Arc::new(store)).await?;
        Ok(())
    }

    pub async fn search(key: Vec<f32>, top_k: usize) -> Result<Vec<(String, MessageSend)>> {
        let results = MESSAGE_SEARCH_DB.search(key, top_k).await?;
        let mapped = results
            .into_iter()
            .map(|(_, store)| (store.msg_id.clone(), store.reply.clone()))
            .collect();
        Ok(mapped)
    }
}
