use crate::api::{
    llm::{
        chat::{archive::message_storage::MessageStorage, llm::get_single_text_embedding},
        tool::{LlmPrompt, LlmVec, ask_as},
    },
    storage::{HasEmbedding, VectorSearchEngine},
};
use anyhow::Result;
use genai::chat::ChatMessage;
use helper::LlmPrompt;
use serde::{Deserialize, Serialize};
use std::time;
use std::{
    sync::{Arc, LazyLock},
    time::UNIX_EPOCH,
};
use uuid::Uuid;

static MEMO_FRAGMENT_DB: LazyLock<VectorSearchEngine<ChatSegment>> =
    LazyLock::new(|| VectorSearchEngine::new("llm_chat_memo_fragment"));

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatSegment {
    pub group_id: i64, // 来源群组

    // --- L1/L2: 原始数据与上下文 ---
    pub message_id: Vec<String>, // 记录消息序列号

    // --- L3: 反思与蒸馏结果 ---
    pub summary: String,       // 对该段对话的 AI 摘要
    pub keywords: Vec<String>, // 提取的关键词（可用于过滤或辅助匹配）

    // --- 向量层 ---
    pub embedding: Vec<f32>, // 对应 summary 或 content 的向量
    pub timestamp: u64,      // 归档时间
}

#[derive(Serialize, Deserialize, Clone, Debug, LlmPrompt)]
pub struct ChatSegmentLlmResponse {
    #[prompt("这段对话的摘要信息")]
    pub summary: String,
    #[prompt("这段对话的关键词（用于过滤或辅助匹配）")]
    pub keywords: LlmVec<String>,
}

impl HasEmbedding for ChatSegment {
    fn get_embedding(&self) -> &[f32] {
        &self.embedding
    }
}

impl ChatSegment {
    pub async fn generate(group_id: i64, message_id: Vec<String>, request: String) -> Result<Self> {
        let mut messages = Vec::with_capacity(message_id.len());
        for msg in &message_id {
            if let Some(m) = MessageStorage::get(msg.clone()).await {
                messages.push(m)
            }
        }

        let message = [
            vec![
                ChatMessage::system(
                "你是一个专业的将消息进行总结的助手，请提取以下对话的关键信息，生成简洁的摘要和关键词",
                ),
                ChatMessage::user("请根据以下模型输出的要求和细节进行总结："),
                ChatMessage::user(request),
                ChatMessage::user("请根据以下对话内容生成摘要和关键词："),
            ],
            messages
        ].concat();

        let response = ask_as::<ChatSegmentLlmResponse>(message).await?;

        let timestamp = time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let embedding = get_single_text_embedding(format!(
            "对话摘要: {}\n对话关键词: {:?}",
            response.summary, response.keywords
        ))
        .await?;

        Ok(Self {
            group_id,
            message_id,
            summary: response.summary,
            keywords: response.keywords.to_vec(),
            embedding,
            timestamp,
        })
    }
}

pub struct MemoFragment;

impl MemoFragment {
    pub async fn search(
        key: Vec<f32>,
        top_k: usize,
    ) -> anyhow::Result<Vec<(Uuid, Arc<ChatSegment>)>> {
        MEMO_FRAGMENT_DB.search(key, top_k).await
    }

    pub async fn insert(group_id: i64, message_id: Vec<String>, request: String) -> Result<()> {
        let msg = ChatSegment::generate(group_id, message_id, request).await?;
        let fragment = Arc::new(msg);
        MEMO_FRAGMENT_DB.insert(fragment).await?;
        Ok(())
    }
}
