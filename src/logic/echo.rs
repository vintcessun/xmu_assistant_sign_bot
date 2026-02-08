use super::BuildHelp;
use crate::abi::logic_import::*;

#[handler(msg_type=Message,command="echo",echo_cmd=true,
help_msg=r#"用法:/echo <内容>
<内容>:你想让我重复的话
功能:用于测试系统可用性"#)]
pub async fn echo(ctx: Context) -> Result<()> {
    let msg = ctx.get_message();
    let raw_message = match &*msg {
        Message::Group(g) => g.raw_message.clone(),
        Message::Private(p) => p.raw_message.clone(),
    };
    let content = format!("你说的是: {}", raw_message);

    let message = message::from_str(content);

    ctx.send_message_async(message);

    Ok(())
}
