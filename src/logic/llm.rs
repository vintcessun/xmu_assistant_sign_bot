use crate::{
    abi::logic_import::*,
    api::llm::chat::{
        broker,
        router::{handle_llm_message, handle_llm_notice},
    },
};

use tracing::debug;

#[handler(msg_type=Message)]
pub async fn llm_message(mut ctx: Context) -> Result<()> {
    let raw_msg = ctx.get_message_text().to_string();
    let compact_msg = raw_msg.replace("\n", "").replace(" ", "");
    let prefix = config::get_command_prefix();

    if let Some(raw_cmd_part) = raw_msg.strip_prefix(prefix) {
        let trimmed_cmd_part = raw_cmd_part.trim_start();

        // 兼容 "前缀后带空白" 的命令输入：通过 Broker 统一注入前缀并分发
        if !trimmed_cmd_part.is_empty() && !raw_cmd_part.starts_with(trimmed_cmd_part) {
            let mut it = trimmed_cmd_part.splitn(2, char::is_whitespace);
            let command_name = it.next().unwrap_or_default();
            let args = it.next().unwrap_or_default().trim();

            if broker::is_registered(command_name) {
                debug!(
                    command = command_name,
                    args = args,
                    "检测到前缀后空白命令，使用 LogicCommandBroker 统一分发"
                );
                broker::dispatch(&mut ctx, command_name, args);
                return Ok(());
            }
        }

        debug!(
            msg = compact_msg,
            "消息包含命令前缀，跳过 LLM 聊天处理，交给命令处理器"
        );
        return Ok(());
    }

    debug!(msg = compact_msg, "开始处理 LLM 消息");
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
