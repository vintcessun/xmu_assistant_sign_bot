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
需要先用 /setpwd 设好长期口令，打开网页后输入该口令解锁。
选一个成绩范围点查询，会一次性给出绩点、加权/算术平均分，
并自动生成绩点证明 PDF、从正文里提取专业排名（教务 JSON 接口只返回 *，
排名只有证明 PDF 里才有），同时展示完整的证明正文
注:网页链接 30 分钟内有效；重新输入口令会把上一位访问者顶下线"#)]
pub async fn jdpm(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let qq = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;

    let client = process_login_castgc(&mut ctx, qq).await?;
    let session = create_session(qq, client)?;
    let url = page_url(&session.id);
    info!(user_id = qq, session_id = %session.id, "已创建绩点查询页面");

    ctx.send_message_async(
        MessageSend::new_message()
            .at(qq.to_string())
            .text(format!(
                "\n绩点/排名查询网页（30 分钟内有效）：\n{url}\n\
                 打开后输入你用 /setpwd 设的口令即可查询。"
            ))
            .build(),
    );

    Ok(())
}
