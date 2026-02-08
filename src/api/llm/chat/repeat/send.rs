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
use tracing::debug;

pub async fn send_message_from_hot<T, M>(ctx: &mut Context<T, M>) -> Result<()>
where
    T: BotClient + BotHandler + std::fmt::Debug + 'static,
    M: MessageType + std::fmt::Debug + Send + Sync + 'static,
{
    let msg = ctx.get_message_text().to_string();
    let sender = ctx
        .get_message()
        .get_sender()
        .user_id
        .ok_or(anyhow!("查询用户失败"))?;
    let message = MessageAbstract {
        qq: sender,
        msg_text: msg,
    };

    let message_send = RepeatReply::get(message.clone())
        .await
        .ok_or(anyhow!("未命中热回复"))?;

    let group_id = match ctx.get_target() {
        Target::Group(id) => id,
        Target::Private(id) => -id,
    };
    audit_test_fast(&message_send, message, group_id).await?;

    debug!("Hot reply generated: {:?}", message_send);

    ctx.send_message(message_send).await?;

    Ok(())
}
