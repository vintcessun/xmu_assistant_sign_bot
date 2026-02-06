use super::BuildHelp;
use crate::abi::{logic_import::*, message::from_str};

#[handler(msg_type=Message,command="github",echo_cmd=true,
help_msg=r#"用法:/github
功能:获取项目github地址 https://github.com/vintcessun/xmu_assistant_bot"#)]
pub async fn echo(ctx: Context) -> Result<()> {
    ctx.send_message_async(from_str("https://github.com/vintcessun/xmu_assistant_bot"));
    Ok(())
}
