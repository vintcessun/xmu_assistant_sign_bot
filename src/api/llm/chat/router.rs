use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use tracing::{debug, info};

use crate::{
    abi::{
        Context,
        logic_import::{Message, Notice},
        message::{MessageReceive, Target, event_body::MessageType, event_message, message_body::SegmentReceive},
        network::BotClient,
        websocket::BotHandler,
    },
    api::llm::chat::{
        archive::{
            identity_group_archive, identity_person_archive, message_archive, notice_archive,
        },
        deep::send_message_from_llm,
        repeat::send_message_from_hot,
        search::send::send_message_from_store,
    },
    config::get_self_qq,
};

/// group_id -> (window_start_secs, reply_count)
static GROUP_RATE_LIMITER: LazyLock<DashMap<i64, (u64, u32)>> = LazyLock::new(DashMap::new);

/// (group_id, user_id) -> last_trigger_secs，用于最后发言保护
static LAST_SPEAKER: LazyLock<DashMap<(i64, i64), u64>> = LazyLock::new(DashMap::new);

// ── 可观测性计数器 ──────────────────────────────────────────────
static ROUTE_TOTAL: AtomicU64 = AtomicU64::new(0);
static L0_HIT: AtomicU64 = AtomicU64::new(0);
static L1_HIT: AtomicU64 = AtomicU64::new(0);
static L2_HIT: AtomicU64 = AtomicU64::new(0);

const RATE_LIMIT_WINDOW_SECS: u64 = 60;
const RATE_LIMIT_MAX_REPLIES: u32 = 10;

/// 同一用户在同一群组内连续触发时的保护窗口（秒）
const LAST_SPEAKER_PROTECT_SECS: u64 = 30;

/// 寒暄/水消息词组，字数较少时命中则视为低质量消息
const GREETING_PATTERNS: &[&str] = &[
    "哈哈", "嗯嗯", "呵呵", "哈哈哈", "嗯", "啊", "哦", "好的", "好", "是", "ok", "OK",
    "okay", "嗯嗯嗯", "哈", "嗯哼", "哦哦", "哦哦哦", "好好",
];

fn current_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 检查消息中是否有 AT 机器人的消息段。
fn is_at_bot(message: &event_message::Message) -> bool {
    let self_qq = get_self_qq().to_string();
    let segs = match message {
        event_message::Message::Group(g) => match &g.message {
            MessageReceive::Array(arr) => arr.as_slice(),
            _ => return false,
        },
        _ => return false,
    };
    segs.iter()
        .any(|s| matches!(s, SegmentReceive::At(a) if a.qq == self_qq))
}

/// 判断消息是否为低质量消息（纯表情、超短文字、寒暄词）。
/// AT 豁免应在调用方判断。
fn is_low_quality_message(message: &event_message::Message, text: &str) -> bool {
    // 纯表情段（全为 Face 类型）
    let segs: Option<&[SegmentReceive]> = match message {
        event_message::Message::Group(g) => match &g.message {
            MessageReceive::Array(arr) => Some(arr.as_slice()),
            _ => None,
        },
        event_message::Message::Private(p) => match &p.message {
            MessageReceive::Array(arr) => Some(arr.as_slice()),
            _ => None,
        },
    };
    if let Some(segs) = segs {
        if !segs.is_empty() && segs.iter().all(|s| matches!(s, SegmentReceive::Face(_))) {
            return true;
        }
    }
    // 去除空白后字符数
    let char_count: usize = text.chars().filter(|c| !c.is_whitespace()).count();
    // 极短消息（≤3 个有效字符）
    if char_count <= 3 {
        return true;
    }
    // 寒暄词匹配（只在较短消息中检测，避免误伤正常对话）
    if char_count <= 8 {
        let trimmed = text.trim();
        for pat in GREETING_PATTERNS {
            if trimmed == *pat || text.contains(pat) {
                return true;
            }
        }
    }
    false
}

/// 更新用户触发时间戳并检查是否仍处于最后发言保护窗口内。
/// 返回 `true` 表示应跳过本次 L2 深度回复。
fn check_last_speaker_protection(group_id: i64, user_id: i64) -> bool {
    let now = current_secs();
    let mut entry = LAST_SPEAKER.entry((group_id, user_id)).or_insert(0);
    let last = *entry;
    *entry = now;
    last > 0 && now.saturating_sub(last) < LAST_SPEAKER_PROTECT_SECS
}

/// 对群组令牌桶进行消耗，返回 `true` 表示允许回复。
fn consume_group_token(group_id: i64) -> bool {
    let now = current_secs();
    let mut entry = GROUP_RATE_LIMITER.entry(group_id).or_insert((now, 0));
    let (window_start, count) = entry.value_mut();
    if now.saturating_sub(*window_start) >= RATE_LIMIT_WINDOW_SECS {
        *window_start = now;
        *count = 1;
        true
    } else if *count < RATE_LIMIT_MAX_REPLIES {
        *count += 1;
        true
    } else {
        false
    }
}

/// 每 N 次路由打印一次统计摘要
fn maybe_log_stats(total: u64) {
    const LOG_INTERVAL: u64 = 100;
    if total % LOG_INTERVAL == 0 {
        let l0 = L0_HIT.load(Ordering::Relaxed);
        let l1 = L1_HIT.load(Ordering::Relaxed);
        let l2 = L2_HIT.load(Ordering::Relaxed);
        let miss = total.saturating_sub(l0 + l1 + l2);
        info!(
            total_routes = total,
            l0_hits = l0,
            l1_hits = l1,
            l2_hits = l2,
            no_reply = miss,
            l0_rate_pct = if total > 0 { l0 * 100 / total } else { 0 },
            "路由统计摘要"
        );
    }
}

pub async fn handle_llm_message<T>(ctx: &mut Context<T, Message>)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    message_archive(ctx).await;
    identity_person_archive(ctx).await;
    identity_group_archive(ctx).await;

    let total = ROUTE_TOTAL.fetch_add(1, Ordering::Relaxed) + 1;
    maybe_log_stats(total);

    let message = ctx.message.as_ref();
    let at_bot = is_at_bot(message);
    let text = ctx.get_message_text().to_owned();

    //L0: 命中回复
    match send_message_from_hot(ctx).await {
        Ok(_) => {
            L0_HIT.fetch_add(1, Ordering::Relaxed);
            info!("L0: 命中回复成功，结束路由");
            return;
        }
        Err(e) => debug!(error = ?e, "L0: 命中回复处理失败，继续路由"),
    }

    // 快速门控：群组非 AT 低质量消息直接跳过后续 LLM 路由
    if let Target::Group(group_id) = ctx.get_target() {
        if !at_bot && is_low_quality_message(ctx.message.as_ref(), &text) {
            debug!(group_id = ?group_id, text = %text, "快速门控：低质量消息，跳过 L1/L2");
            return;
        }
    }

    //L1: 搜索回复
    match send_message_from_store(ctx).await {
        Ok(_) => {
            L1_HIT.fetch_add(1, Ordering::Relaxed);
            info!("L1: 搜索回复成功，结束路由");
            return;
        }
        Err(e) => debug!(error = ?e, "L1: 搜索回复处理失败，继续路由"),
    }

    //L2: 深度回复
    // 群组限流检查 + 最后发言保护（AT 消息豁免，私聊不受限）
    if let Target::Group(group_id) = ctx.get_target() {
        if !at_bot {
            let user_id = ctx.get_message().get_sender().user_id.unwrap_or_default();
            if check_last_speaker_protection(group_id, user_id) {
                debug!(group_id = ?group_id, user_id = ?user_id, "L2: 最后发言保护触发，跳过深度回复");
                return;
            }
            if !consume_group_token(group_id) {
                debug!(group_id = ?group_id, "L2: 群组回复频率超限，跳过深度回复");
                return;
            }
        }
    }

    match send_message_from_llm(ctx).await {
        Ok(_) => {
            L2_HIT.fetch_add(1, Ordering::Relaxed);
            info!("L2: 深度回复成功，结束路由");
            return;
        }
        Err(e) => debug!(error = ?e, "L2: 深度回复处理失败"),
    }

    info!(
        message = ?ctx.get_message(),
        "未生成 LLM 回复，消息路由结束"
    );
}

pub async fn handle_llm_notice<T>(ctx: &mut Context<T, Notice>)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    notice_archive(ctx).await;
}
