use crate::{
    abi::message::MessageSend,
    api::{llm::chat::audit::backlist::Backlist, storage::ColdTable},
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, error, info};

static MESSAGE_FAST_DB: LazyLock<ColdTable<MessageAbstract, MessageSend>> =
    LazyLock::new(|| ColdTable::new("message_fast_abstract_reply"));

/// group_id -> (last_refresh_secs, keys)，用于 L0b 近似匹配键缓存
static GROUP_KEY_CACHE: LazyLock<DashMap<i64, (u64, Vec<MessageAbstract>)>> =
    LazyLock::new(DashMap::new);

// ── L0 命中统计（用于 L0b 自动降级判断）──
static L0A_HITS: AtomicU64 = AtomicU64::new(0);
static L0B_HITS: AtomicU64 = AtomicU64::new(0);
static L0B_MISSES: AtomicU64 = AtomicU64::new(0);

/// 键缓存 TTL（秒）
const KEY_CACHE_TTL_SECS: u64 = 30;
/// L0b Jaccard 相似度阈值
const JACCARD_THRESHOLD: f32 = 0.5;
/// L0b 最低有效命中率，低于此值自动降级跳过 L0b
const L0B_MIN_HIT_RATE: f32 = 0.05;
/// 最低样本量，样本不足时不降级
const L0B_MIN_SAMPLES: u64 = 100;

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

fn current_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 生成字符串的字符三元组集合（用于 Jaccard 相似度计算）
fn char_trigrams(s: &str) -> HashSet<(char, char, char)> {
    let chars: Vec<char> = s.chars().collect();
    chars.windows(3).map(|w| (w[0], w[1], w[2])).collect()
}

/// 计算两个三元组集合的 Jaccard 相似度
fn jaccard_similarity(a: &HashSet<(char, char, char)>, b: &HashSet<(char, char, char)>) -> f32 {
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

/// 判断 L0b 是否应启用（样本不足时始终启用；命中率低于阈值则自动降级跳过）
fn should_use_l0b() -> bool {
    let total = L0B_HITS.load(Ordering::Relaxed) + L0B_MISSES.load(Ordering::Relaxed);
    if total < L0B_MIN_SAMPLES {
        return true;
    }
    let hits = L0B_HITS.load(Ordering::Relaxed);
    hits as f32 / total as f32 >= L0B_MIN_HIT_RATE
}

/// 获取同群组的已存储键列表（带缓存，TTL 30s）
async fn get_group_keys(group_id: i64) -> Vec<MessageAbstract> {
    let now = current_secs();
    {
        let entry = GROUP_KEY_CACHE.get(&group_id);
        if let Some(e) = entry
            && now.saturating_sub(e.0) < KEY_CACHE_TTL_SECS
        {
            return e.1.clone();
        }
    }
    let all_keys = MESSAGE_FAST_DB.get_keys_async().await.unwrap_or_default();
    let group_keys: Vec<MessageAbstract> = all_keys
        .into_iter()
        .filter(|k| k.group_id == group_id)
        .collect();
    GROUP_KEY_CACHE.insert(group_id, (now, group_keys.clone()));
    group_keys
}

pub struct RepeatReply;

impl RepeatReply {
    pub async fn get(key: MessageAbstract) -> Option<MessageSend> {
        debug!(message_abstract = ?key, "尝试获取热回复");

        // 黑名单检查
        if let Some(e) = Backlist::get(key.clone()).await {
            info!(message_abstract = ?key, hit_entry = ?e, "消息命中黑名单，拒绝热回复");
            return None;
        }

        // L0a：精确匹配
        match MESSAGE_FAST_DB.get_async(&key).await {
            Ok(Some(v)) => {
                L0A_HITS.fetch_add(1, Ordering::Relaxed);
                debug!(message_abstract = ?key, "L0a: 精确命中热回复");
                return Some(v);
            }
            Ok(None) => {}
            Err(e) => error!(error = ?e, "查询热回复数据库失败，返回 None"),
        }

        // L0b：trigram + Jaccard 近似匹配（字符数 >= 3 且 L0b 未降级时启用）
        let text_len = key.normalized_text.chars().count();
        if text_len >= 3 && should_use_l0b() {
            let query_tris = char_trigrams(&key.normalized_text);
            let group_keys = get_group_keys(key.group_id).await;
            let best = group_keys
                .iter()
                .filter(|k| k.normalized_text.chars().count() >= 3)
                .map(|k| {
                    (
                        k,
                        jaccard_similarity(&query_tris, &char_trigrams(&k.normalized_text)),
                    )
                })
                .filter(|(_, sim)| *sim >= JACCARD_THRESHOLD)
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            if let Some((best_key, sim)) = best {
                debug!(
                    group_id = key.group_id,
                    query = %key.normalized_text,
                    best_match = %best_key.normalized_text,
                    sim = %sim,
                    "L0b: trigram 近似命中"
                );
                if let Ok(Some(v)) = MESSAGE_FAST_DB.get_async(best_key).await {
                    L0B_HITS.fetch_add(1, Ordering::Relaxed);
                    return Some(v);
                }
            }
            L0B_MISSES.fetch_add(1, Ordering::Relaxed);
        }

        None
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
