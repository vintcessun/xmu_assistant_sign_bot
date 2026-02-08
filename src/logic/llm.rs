use crate::{
    abi::logic_import::*,
    api::llm::chat::router::{handle_llm_message, handle_llm_notice},
};

use tracing::trace;

#[handler(msg_type=Message)]
pub async fn llm_message(ctx: Context) -> Result<()> {
    let msg = ctx.get_message_text().replace("\n", "").replace(" ", "");
    if msg.starts_with(config::get_command_prefix()) {
        trace!("跳过命令消息");
        return Ok(());
    }

    handle_llm_message(&mut ctx).await;
    Ok(())
}

#[handler(msg_type=Notice)]
pub async fn llm_notice(ctx: Context) -> Result<()> {
    handle_llm_notice(&mut ctx).await;
    Ok(())
}
