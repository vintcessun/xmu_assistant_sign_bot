use crate::{
    abi::logic_import::*,
    api::llm::chat::router::{handle_llm_message, handle_llm_notice},
};

use tracing::debug;

#[handler(msg_type=Message)]
pub async fn llm_message(ctx: Context) -> Result<()> {
    let msg = ctx.get_message_text().replace("\n", "").replace(" ", "");
    if msg.starts_with(config::get_command_prefix()) {
        debug!(
            msg = msg,
            "消息包含命令前缀，跳过 LLM 聊天处理，交给命令处理器"
        );
        return Ok(());
    }

    debug!(msg = msg, "开始处理 LLM 消息");
    handle_llm_message(&mut ctx).await;
    debug!("LLM 消息处理完成");
    Ok(())
}

#[handler(msg_type=Notice)]
pub async fn llm_notice(ctx: Context) -> Result<()> {
    handle_llm_notice(&mut ctx).await;
    debug!("LLM 通知处理完成");
    Ok(())
}
