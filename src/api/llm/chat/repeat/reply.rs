use crate::{
    abi::message::MessageSend,
    api::{llm::chat::audit::backlist::Backlist, storage::ColdTable},
};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tracing::{debug, error, info};

static MESSAGE_FAST_DB: LazyLock<ColdTable<MessageAbstract, MessageSend>> =
    LazyLock::new(|| ColdTable::new("message_fast_abstract_reply"));

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct MessageAbstract {
    pub qq: i64,
    pub msg_text: String,
}

pub struct RepeatReply;

impl RepeatReply {
    pub async fn get(key: MessageAbstract) -> Option<MessageSend> {
        debug!(message_abstract = ?key, "尝试获取热回复");
        match Backlist::get(key.clone()).await {
            Some(e) => {
                info!(message_abstract = ?key, hit_entry = ?e, "消息命中黑名单，拒绝热回复");
                None
            }
            None => MESSAGE_FAST_DB
                .get_async(key)
                .await
                .map_err(|e| {
                    error!(error = ?e, "查询热回复数据库失败，返回 None");
                    e
                })
                .unwrap_or_default(),
        }
    }

    pub async fn insert(key: MessageAbstract, message: MessageSend) {
        debug!(message_abstract = ?key, "插入热回复到数据库");
        if let Err(e) = MESSAGE_FAST_DB.insert(key, message).await {
            error!(error = ?e, "插入热回复到数据库失败");
        }
    }

    pub async fn remove(key: MessageAbstract) {
        debug!(message_abstract = ?key, "从数据库移除热回复");
        if let Err(e) = MESSAGE_FAST_DB.remove(key).await {
            error!(error = ?e, "从数据库移除热回复失败");
        }
    }
}
