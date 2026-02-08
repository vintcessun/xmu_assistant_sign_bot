use super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::MessageSend},
    api::{
        network::SessionClient,
        xmu_service::{
            jw::ScheduleRenderer, llm::choose_timetable::ChooseTimetable,
            login::request_qrcode_castgc,
        },
    },
    logic::login::process::send_msg_and_wait,
};
use anyhow::anyhow;

#[handler(msg_type=Message,command="timetable",echo_cmd=true,
help_msg=r#"用法:/timetable <描述>
<描述>:你想查看的课程表及其周数等信息
注:查看课表一定会伴随着一次登录
功能:查看指定周数的课程表"#)]
pub async fn timetable(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let id = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;
    let client = SessionClient::new();
    let login_data = send_msg_and_wait(&mut ctx, &client, id).await?;

    request_qrcode_castgc(&client, login_data).await?;

    let (schedule, week) =
        ChooseTimetable::get_from_client(&client, ctx.get_message_text()).await?;

    let image = ScheduleRenderer::render_to_file(&schedule, week).await?;

    ctx.send_message_async(
        MessageSend::new_message()
            .text(format!("你的第{}周课程表如下", week))
            .image(image.to_fileurl().await)
            .build(),
    );

    Ok(())
}
