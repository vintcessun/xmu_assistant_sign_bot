use crate::api::{
    llm::{
        chat::{archive::message_storage::MessageStorage, llm::get_single_text_embedding},
        tool::ask_as,
    },
    storage::{HasEmbedding, VectorSearchEngine},
};
use anyhow::Result;
use genai::chat::ChatMessage;
use llm_xml_caster::llm_prompt;
use serde::{Deserialize, Serialize};
use std::time;
use std::{
    sync::{Arc, LazyLock},
    time::UNIX_EPOCH,
};
use tracing::{error, info, warn};
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

#[llm_prompt]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ChatSegmentLlmResponse {
    #[prompt("这段对话的摘要信息")]
    pub summary: String,
    #[prompt("这段对话的关键词（用于过滤或辅助匹配）")]
    pub keywords: Vec<String>,
}

const CHAT_SEGMENT_VALID_EXAMPLE: &str = r#"
<ChatSegmentLlmResponse>
  <summary><![CDATA[这是对话的摘要信息]]></summary>
  <keywords>
    <item><![CDATA[关键词1]]></item>
    <item><![CDATA[关键词2]]></item>
  </keywords>
</ChatSegmentLlmResponse>"#;

#[cfg(test)]
#[test]
fn test_chat_segment_llm_response_parsing() {
    let example_response =
        quick_xml::de::from_str::<ChatSegmentLlmResponse>(CHAT_SEGMENT_VALID_EXAMPLE)
            .expect("解析示例 XML 失败");
    assert_eq!(
        example_response,
        ChatSegmentLlmResponse {
            summary: "这是对话的摘要信息".to_string(),
            keywords: vec!["关键词1".to_string(), "关键词2".to_string()],
        }
    );
}

impl HasEmbedding for ChatSegment {
    fn get_embedding(&self) -> &[f32] {
        &self.embedding
    }
}

impl ChatSegment {
    pub async fn generate(group_id: i64, message_id: Vec<String>, request: String) -> Result<Self> {
        info!(group_id = ?group_id, message_count = ?message_id.len(), "开始生成记忆片段");
        let mut messages = Vec::with_capacity(message_id.len());
        for msg in &message_id {
            if let Some(m) = MessageStorage::get(msg).await {
                messages.push(m)
            } else {
                warn!(message_id = ?msg, "对话消息片段缺失，无法添加到记忆生成上下文");
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

        let response = ask_as::<ChatSegmentLlmResponse>(message, CHAT_SEGMENT_VALID_EXAMPLE)
            .await
            .map_err(|e| {
                error!(error = ?e, "LLM 调用失败，无法生成对话摘要和关键词");
                e
            })?;

        let timestamp = time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|e| {
                error!(error = ?e, "获取系统时间失败，系统时间早于 UNIX 纪元");
                time::Duration::from_secs(0)
            })
            .as_secs();

        let embedding = get_single_text_embedding(format!(
            "对话摘要: {}\n对话关键词: {:?}",
            response.summary, response.keywords
        ))
        .await
        .map_err(|e| {
            error!(error = ?e, "文本嵌入生成失败");
            e
        })?;

        info!(group_id = ?group_id, summary = %response.summary, "记忆片段生成成功");

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
        key: &[f32],
        top_k: usize,
    ) -> anyhow::Result<Vec<(Uuid, Arc<ChatSegment>)>> {
        info!(key_len = ?key.len(), top_k = ?top_k, "开始搜索记忆片段");
        let result = MEMO_FRAGMENT_DB.search(key, top_k).await.map_err(|e| {
            error!(error = ?e, "记忆片段向量搜索失败");
            e
        })?;
        info!(result_count = ?result.len(), "记忆片段搜索完成");
        Ok(result)
    }

    pub async fn insert(group_id: i64, message_id: Vec<String>, request: String) -> Result<()> {
        info!(group_id = ?group_id, message_count = ?message_id.len(), "开始插入新的记忆片段");
        let msg = ChatSegment::generate(group_id, message_id, request)
            .await
            .map_err(|e| {
                error!(group_id = ?group_id, error = ?e, "生成记忆片段失败");
                e
            })?;
        let fragment = Arc::new(msg);
        MEMO_FRAGMENT_DB.insert(fragment).await.map_err(|e| {
            error!(group_id = ?group_id, error = ?e, "插入向量数据库失败");
            e
        })?;
        info!(group_id = ?group_id, "记忆片段插入成功");
        Ok(())
    }
}
