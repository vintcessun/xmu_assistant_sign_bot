use crate::abi::logic_import::*;
use crate::abi::message::{MessageReceive, message_body::SegmentReceive};
use crate::logic::Notice;
use tracing::debug;

/// 别人给消息贴了表情，bot 跟着贴同一个。
#[handler(msg_type=Notice)]
pub async fn emoji_follow(ctx: Context) -> Result<()> {
    if let Notice::GroupMsgEmojiLike(event) = ctx.get_message().as_ref()
        && event.is_add
    {
        for like in &event.likes {
            like_silently(&ctx, event.message_id, like.emoji_id.clone()).await;
        }
    }
    Ok(())
}

/// 消息里带表情（小黄脸、超级表情、商城大表情、emoji）就贴同样 ID 的表情。
#[handler(msg_type=Message)]
pub async fn emoji_mirror(ctx: Context) -> Result<()> {
    let msg = ctx.get_message();
    let (message_id, message) = match &*msg {
        Message::Group(m) => (m.message_id, &m.message),
        Message::Private(m) => (m.message_id, &m.message),
    };

    let segments = match message {
        MessageReceive::Array(arr) => arr.iter().collect::<Vec<_>>(),
        MessageReceive::Single(seg) => vec![seg],
    };

    let mut emoji_ids: Vec<String> = Vec::new();
    for seg in segments {
        match seg {
            SegmentReceive::Face(face) => emoji_ids.push(face.id.clone()),
            SegmentReceive::Image(img) => {
                if let Some(id) = &img.emoji_id {
                    emoji_ids.push(id.clone());
                }
            }
            SegmentReceive::Text(text) => emoji_ids.extend(
                text.text
                    .chars()
                    .filter(|&c| is_emoji(c))
                    .map(|c| (c as u32).to_string()),
            ),
            _ => {}
        }
    }
    emoji_ids.dedup();

    for emoji_id in emoji_ids {
        like_silently(&ctx, message_id as i64, emoji_id).await;
    }
    Ok(())
}

/// 贴失败（已贴过、表情不受支持等）静默放弃。
async fn like_silently<T, M>(ctx: &Context<T, M>, message_id: i64, emoji_id: String)
where
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
    M: MessageType + fmt::Debug + Send + Sync + 'static,
{
    if let Err(e) = ctx.set_msg_emoji_like(message_id, emoji_id.clone()).await {
        debug!(message_id, emoji_id, error = ?e, "贴表情失败，放弃");
    }
}

/// 粗判一个字符是不是 emoji：覆盖 Unicode 里 emoji 集中的几个区块。
fn is_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1FAFF
            | 0x2600..=0x27BF
            | 0x2300..=0x23FF
            | 0x2B00..=0x2BFF
            | 0x2190..=0x21FF
            | 0x25A0..=0x25FF
            | 0x2934
            | 0x2935
            | 0x203C
            | 0x2049
            | 0x2122
            | 0x2139
            | 0x24C2
            | 0x3030
            | 0x303D
            | 0x3297
            | 0x3299
            | 0x00A9
            | 0x00AE
    )
}

#[cfg(test)]
mod tests {
    use super::is_emoji;

    #[test]
    fn picks_emoji_not_cjk() {
        let ids: Vec<u32> = "哈哈😂好❓ok👍"
            .chars()
            .filter(|&c| is_emoji(c))
            .map(|c| c as u32)
            .collect();
        assert_eq!(ids, vec![128514, 10067, 128077]);
    }
}
