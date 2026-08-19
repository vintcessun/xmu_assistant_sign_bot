use super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::MessageSend},
    web::guard::{create_setup, setup_url},
};
use anyhow::anyhow;

#[handler(msg_type=Message,command="setpwd",echo_cmd=true,
help_msg=r#"用法:/setpwd
功能:设置或刷新你的访问口令。发送后会给出一个一次性链接，
在网页上输入口令（也可以点“随机生成”）保存即可。
口令**长期有效**，设一次就一直用，所有需要口令的页面（如 /jdpm 的绩点查询）
都用这一个口令解锁；想换口令就再发一次 /setpwd，旧口令立即失效。
注:口令服务端只存摘要（随机盐 + 10 万轮 SHA-256），明文不落盘不进日志。
设置链接 15 分钟内有效且只能用一次，请自己尽快打开"#)]
pub async fn setpwd(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let qq = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;

    let task = create_setup(qq);
    let url = setup_url(&task.id);
    let action = if task.refresh { "刷新" } else { "设置" };

    ctx.send_message_async(
        MessageSend::new_message()
            .at(qq.to_string())
            .text(format!(
                "\n{action}访问口令：\n{url}\n\
                 打开后自己输入或随机生成一个口令，保存即长期生效。\n\
                 链接 15 分钟内有效、只能用一次；谁先打开谁就设上了，请自己尽快打开。"
            ))
            .build(),
    );

    Ok(())
}
