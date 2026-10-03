use super::Params;
use crate::abi::message::api::data;
use helper::api;
use serde::{Deserialize, Serialize};

/// 给消息贴表情。`emoji_id` 为 QQ 表情编号，或 emoji 码点的十进制串（如 "128514" 即 😂）。
#[api("/set_msg_emoji_like", data::EmojiLikeResponse)]
pub struct SetMsgEmojiLike {
    message_id: i64,
    emoji_id: String,
    set: bool,
}

impl SetMsgEmojiLike {
    pub fn new(message_id: i64, emoji_id: String) -> Self {
        Self {
            message_id,
            emoji_id,
            set: true,
        }
    }
}
