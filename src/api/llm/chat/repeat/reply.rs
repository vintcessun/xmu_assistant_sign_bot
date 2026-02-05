use crate::{
    abi::message::MessageSend,
    api::{llm::chat::audit::backlist::Backlist, storage::ColdTable},
};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tracing::info;

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
        match Backlist::get(key.clone()).await {
            Some(e) => {
                info!("消息命中黑名单，拒绝回复: {:?}", e);
                None
            }
            None => MESSAGE_FAST_DB.get_async(key).await.unwrap_or_default(),
        }
    }

    pub async fn insert(key: MessageAbstract, message: MessageSend) {
        let _ = MESSAGE_FAST_DB.insert(key, message).await;
    }

    pub async fn remove(key: MessageAbstract) {
        let _ = MESSAGE_FAST_DB.remove(key).await;
    }
}
