use super::BuildHelp;
use crate::abi::logic_import::*;
use tracing::{debug, trace};

#[handler(msg_type=Message,command="echo",echo_cmd=true,
help_msg=r#"用法:/echo <内容>
<内容>:你想让我重复的话
功能:用于测试系统可用性"#)]
pub async fn echo(ctx: Context) -> Result<()> {
    let msg = ctx.get_message();
    trace!(message = ?msg, "收到 /echo 命令");
    let raw_message = match &*msg {
        Message::Group(g) => {
            debug!(group_id = g.group_id, "收到群聊 echo 消息");
            g.raw_message.clone()
        }
        Message::Private(p) => {
            debug!(user_id = p.user_id, "收到私聊 echo 消息");
            p.raw_message.clone()
        }
    };
    let content = format!("你说的是: {}", raw_message);

    trace!(content = content, "发送回显消息");
    let message = message::from_str(content);
    ctx.send_message_async(message);

    Ok(())
}
