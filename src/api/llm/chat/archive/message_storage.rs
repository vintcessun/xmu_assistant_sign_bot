use crate::api::storage::ColdTable;
use genai::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use std::{sync::LazyLock, time};
use tracing::{error, trace};

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
    pub async fn get(key: &String) -> Option<ChatMessage> {
        let msg = MESSAGE_DB.get_async(key).await.unwrap_or_else(|e| {
            error!(key = ?key, error = ?e, "获取消息记录失败");
            None
        });
        msg.map(|m| m.msg)
    }

    pub async fn save(key: &String, message: Vec<ChatMessage>) {
        trace!(key = ?key, message = ?message, "正在尝试保存消息");

        let mut msg_contents = vec![];
        for msg in message {
            msg_contents.extend(msg.content);
        }

        match async {
            MESSAGE_DB
                .insert(
                    key,
                    &MessageStore {
                        msg: ChatMessage::user(msg_contents),
                        timestamp: time::SystemTime::now()
                            .duration_since(time::UNIX_EPOCH)?
                            .as_secs(),
                    },
                )
                .await?;
            Ok::<(), anyhow::Error>(())
        }
        .await
        {
            Ok(_) => {
                trace!(key = ?key, "消息记录保存成功");
            }
            Err(e) => {
                error!(key = ?key, error = ?e, "消息记录保存失败");
            }
        }
    }

    pub async fn get_range(start_time: u64, end_time: u64) -> Vec<(String, ChatMessage)> {
        trace!(start_time = ?start_time, end_time = ?end_time, "开始获取指定时间范围内的消息记录");
        let segments = MESSAGE_DB.get_all_async().await.unwrap_or_else(|e| {
            error!(error = ?e, "获取所有消息记录失败，返回空列表");
            vec![]
        });
        let mut ret = Vec::with_capacity(segments.len());

        for segment in segments {
            let (id, MessageStore { msg, timestamp }) = segment;
            if timestamp >= start_time && timestamp <= end_time {
                ret.push((id, msg));
            }
        }

        ret
    }
}

pub struct NoticeStorage;

impl NoticeStorage {
    pub async fn get(key: i64) -> Option<ChatMessage> {
        let msg = NOTICE_DB.get_async(&key).await.unwrap_or_else(|e| {
            error!(key = ?key, error = ?e, "获取通知记录失败");
            None
        });
        msg.map(|m| m.msg)
    }

    pub async fn save(key: i64, message: ChatMessage) {
        let ret = NOTICE_DB
            .insert(
                &key,
                &MessageStore {
                    msg: message,
                    timestamp: time::SystemTime::now()
                        .duration_since(time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                },
            )
            .await;

        match ret {
            Ok(_) => {
                trace!(key = ?key, "通知记录保存成功");
            }
            Err(e) => {
                error!(key = ?key, error = ?e, "保存通知记录失败");
            }
        }
    }

    pub async fn get_range(start_time: u64, end_time: u64) -> Vec<ChatMessage> {
        trace!(start_time = ?start_time, end_time = ?end_time, "开始获取指定时间范围内的通知记录");
        let segments = NOTICE_DB.get_all_async().await.unwrap_or_else(|e| {
            error!(error = ?e, "获取所有通知记录失败，返回空列表");
            vec![]
        });
        let mut ret = Vec::with_capacity(segments.len());
        for segment in segments {
            let (_, MessageStore { msg, timestamp }) = segment;
            if timestamp >= start_time && timestamp <= end_time {
                ret.push(msg);
            }
        }
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_message_range() {
        let segments = MESSAGE_DB.get_all_async().await.unwrap_or_default();
        println!("当前所有消息记录共 {} 条", segments.len());
        println!(
            "当前所有消息记录时间戳: {:?}",
            segments.iter().map(|s| s.1.timestamp).collect::<Vec<_>>()
        );

        println!(
            "最后消息: {:?}",
            segments.iter().rev().take(5).collect::<Vec<_>>()
        );

        let now = time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let msg = MessageStorage::get_range(now - 3600, now).await;
        println!("获取到的消息记录: {:?}", msg);
    }
}
