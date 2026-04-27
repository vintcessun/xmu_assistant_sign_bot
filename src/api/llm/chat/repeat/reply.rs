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
    /// 群组 ID（私聊取负）
    pub group_id: i64,
    /// 文本内容经过空白折叠、标点归一、大小写归一、emoji 占位后的标准化字符串
    pub normalized_text: String,
}

/// 将消息文本进行归一化处理：空白折叠、全角转半角、小写、emoji 占位
///
/// 用于 L0 匹配键生成，提高群级词汇命中率。
pub fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_space = true; // 开头设为 true 避免前导空白
    for ch in text.chars() {
        // emoji 占位：Unicode 表情/符号区域替换为空格
        if is_emoji(ch) {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            continue;
        }
        // 全角转半角 ASCII
        let ch = fullwidth_to_ascii(ch);
        // 小写化
        let ch_lower = ch.to_lowercase().next().unwrap_or(ch);
        // 空白折叠：空白/标点 一律当做一个空格
        if ch_lower.is_whitespace() || is_punctuation(ch_lower) {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch_lower);
            last_space = false;
        }
    }
    out.trim_end().to_string()
}

#[inline]
fn fullwidth_to_ascii(ch: char) -> char {
    let cp = ch as u32;
    // 全角 ASCII 可打印字符 U+FF01..=U+FF5E 对应到 U+0021..=U+007E
    if (0xFF01..=0xFF5E).contains(&cp) {
        char::from_u32(cp - 0xFEE0).unwrap_or(ch)
    } else if ch == '\u{3000}' {
        ' '
    } else {
        ch
    }
}

#[inline]
fn is_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '!' | '"' | '#' | '$' | '%' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | '-' |
        '.' | '/' | ':' | ';' | '<' | '=' | '>' | '?' | '@' | '[' | '\\' | ']' | '^' |
        '_' | '`' | '{' | '|' | '}' | '~' |
        // CJK 标点
        '\u{3002}' | '\u{FF1F}' | '\u{FF01}' | '\u{3001}' | '\u{300C}' | '\u{300D}' |
        '\u{300E}' | '\u{300F}' | '\u{300A}' | '\u{300B}' | '\u{3008}' | '\u{3009}' |
        '\u{2026}' | '\u{2014}' | '\u{3010}' | '\u{3011}' | '\u{2018}' | '\u{2019}' |
        '\u{201C}' | '\u{201D}'
    )
}

#[inline]
fn is_emoji(ch: char) -> bool {
    let cp = ch as u32;
    matches!(
        cp,
        0x1F300..=0x1F9FF  // Misc Symbols, Emoticons, Transport, etc.
        | 0x2600..=0x27BF   // Misc symbols
        | 0xFE00..=0xFE0F   // Variation selectors
        | 0x1F1E0..=0x1F1FF // Flags
        | 0x200D            // ZWJ
        | 0x20E3            // Combining enclosing keycap
    )
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
                .get_async(&key)
                .await
                .map_err(|e| {
                    error!(error = ?e, "查询热回复数据库失败，返回 None");
                    e
                })
                .unwrap_or_default(),
        }
    }

    pub async fn insert(key: &MessageAbstract, message: &MessageSend) {
        debug!(message_abstract = ?key, "插入热回复到数据库");
        if let Err(e) = MESSAGE_FAST_DB.insert(key, message).await {
            error!(error = ?e, "插入热回复到数据库失败");
        }
    }

    pub async fn remove(key: &MessageAbstract) {
        debug!(message_abstract = ?key, "从数据库移除热回复");
        if let Err(e) = MESSAGE_FAST_DB.remove(key).await {
            error!(error = ?e, "从数据库移除热回复失败");
        }
    }
}
