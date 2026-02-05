use crate::api::storage::ColdTable;
use genai::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use std::{sync::LazyLock, time};

static MESSAGE_DB: LazyLock<ColdTable<String, MessageStore>> =
    LazyLock::new(|| ColdTable::new("llm_chat_message_storage"));

static NOTICE_DB: LazyLock<ColdTable<i64, MessageStore>> =
    LazyLock::new(|| ColdTable::new("llm_chat_notice_storage"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageStore {
    pub msg: ChatMessage,
    pub timestamp: u64,
}

pub struct MessageStorage;

impl MessageStorage {
    pub async fn get(key: String) -> Option<ChatMessage> {
        let msg = MESSAGE_DB.get_async(key).await.unwrap_or_default();
        msg.map(|m| m.msg)
    }

    pub async fn save(key: String, message: Vec<ChatMessage>) {
        let mut msg_contents = vec![];
        for msg in message {
            msg_contents.extend(msg.content);
        }
        let _ = MESSAGE_DB
            .insert(
                key,
                MessageStore {
                    msg: ChatMessage::user(msg_contents),
                    timestamp: time::SystemTime::now()
                        .duration_since(time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                },
            )
            .await;
    }

    pub async fn get_range(start_time: u64, end_time: u64) -> Vec<(String, ChatMessage)> {
        let segments = MESSAGE_DB.get_all_async().await.unwrap_or_default();
        let start_idx = segments.partition_point(|s| s.1.timestamp < start_time);

        // 2. 找到第一个时间戳 > end_time 的索引 (上界)
        let end_idx = segments.partition_point(|s| s.1.timestamp <= end_time);

        segments[start_idx..end_idx]
            .iter()
            .map(|(k, v)| (k.clone(), v.msg.clone()))
            .collect()
    }
}

pub struct NoticeStorage;

impl NoticeStorage {
    pub async fn get(key: i64) -> Option<ChatMessage> {
        let msg = NOTICE_DB.get_async(key).await.unwrap_or_default();
        msg.map(|m| m.msg)
    }

    pub async fn save(key: i64, message: ChatMessage) {
        let _ = NOTICE_DB
            .insert(
                key,
                MessageStore {
                    msg: message,
                    timestamp: time::SystemTime::now()
                        .duration_since(time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                },
            )
            .await;
    }

    pub async fn get_range(start_time: u64, end_time: u64) -> Vec<ChatMessage> {
        let segments = NOTICE_DB.get_all_async().await.unwrap_or_default();
        let start_idx = segments.partition_point(|s| s.1.timestamp < start_time);

        // 2. 找到第一个时间戳 > end_time 的索引 (上界)
        let end_idx = segments.partition_point(|s| s.1.timestamp <= end_time);

        segments[start_idx..end_idx]
            .iter()
            .map(|(_, v)| v.msg.clone())
            .collect()
    }
}
