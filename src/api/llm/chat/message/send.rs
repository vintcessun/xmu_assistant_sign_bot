use crate::{
    abi::{Context, message::MessageType, network::BotClient, websocket::BotHandler},
    api::llm::chat::message::bridge::IntoMessageSend,
};
use anyhow::Result;
use genai::chat::ChatResponse;

pub async fn send_message_from_response<T, M>(
    ctx: &mut Context<T, M>,
    detail_msg: ChatResponse,
) -> Result<()>
where
    T: BotClient + BotHandler + std::fmt::Debug + 'static,
    M: MessageType + std::fmt::Debug + Send + Sync + 'static,
{
    let msg = IntoMessageSend::get_message_send(detail_msg).await?;
    ctx.send_message(msg).await?;
    Ok(())
}
