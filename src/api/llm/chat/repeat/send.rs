use crate::{
    abi::{
        Context,
        message::{MessageType, Target},
        network::BotClient,
        websocket::BotHandler,
    },
    api::llm::chat::{
        audit::audit_test_fast,
        repeat::reply::{MessageAbstract, RepeatReply},
    },
};
use anyhow::{Result, anyhow};
use tracing::{debug, error, info, warn};

pub async fn send_message_from_hot<T, M>(ctx: &mut Context<T, M>) -> Result<()>
where
    T: BotClient + BotHandler + std::fmt::Debug + 'static,
    M: MessageType + std::fmt::Debug + Send + Sync + 'static,
{
    let msg = ctx.get_message_text().to_string();
    let sender = ctx.get_message().get_sender().user_id.ok_or_else(|| {
        error!("获取消息发送者 QQ 号失败");
        anyhow!("获取消息发送者 QQ 号失败")
    })?;
    let message = MessageAbstract {
        qq: sender,
        msg_text: msg,
    };

    debug!(message = ?message, "尝试生成重复消息热回复");

    let message_send = RepeatReply::get(message.clone()).await.ok_or_else(|| {
        debug!(message = ?message, "未命中热回复");
        anyhow!("未命中热回复")
    })?;

    let group_id = match ctx.get_target() {
        Target::Group(id) => id,
        Target::Private(id) => -id,
    };

    audit_test_fast(&message_send, message.clone(), group_id)
        .await
        .map_err(|e| {
            warn!(group_id = ?group_id, error = ?e, "发送快速审计任务失败");
            e
        })?;

    info!(group_id = ?group_id, reply_segment = ?message_send, "已生成重复消息热回复，准备发送");

    ctx.send_message_async(message_send);

    info!(group_id = ?group_id, "热回复发送流程完成");

    Ok(())
}
