use super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::MessageSend},
    logic::login::process::process_login_castgc,
    web::jdfpm::task::{create_session, page_url},
};
use anyhow::anyhow;
use tracing::info;

#[handler(msg_type=Message,command="jdpm",echo_cmd=true,
help_msg=r#"用法:/jdpm
功能:创建一个绩点/排名查询网页并发送链接。
口令不经过聊天窗口：打开链接后在网页上设置访问口令（可手输，也可点“随机生成”），
保存即生效，此后所有查询都要先用这个口令解锁。
网页上可以查看成绩范围、申请绩点计算、生成绩点证明 PDF，
并把证明正文里的排名（教务接口只给 *）直接显示出来
注:链接是发在群里的，谁先设置口令谁就拿到这个页面，请自己尽快设置。
口令 10 分钟内必须设置完成，网页整体 30 分钟内有效"#)]
pub async fn jdpm(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let qq = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;

    let client = process_login_castgc(&mut ctx, qq).await?;
    let session = create_session(qq, client);
    let url = page_url(&session.id);
    info!(user_id = qq, session_id = %session.id, "已创建绩点查询页面");

    ctx.send_message_async(
        MessageSend::new_message()
            .at(qq.to_string())
            .text(format!(
                "\n绩点/排名查询网页：\n{url}\n\
                 请立刻打开并设置访问口令——链接在群里是公开的，\
                 谁先设置口令谁就拿到这个页面。\n\
                 口令请在 10 分钟内设置完成，网页 30 分钟后整体失效。"
            ))
            .build(),
    );

    Ok(())
}
