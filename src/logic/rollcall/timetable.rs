use super::super::BuildHelp;
use super::data::TIMETABLE_DATA as DATA;
use super::data::TIMETABLE_GROUP;
use super::time::TIME_SIGN_TASK;
use crate::{
    abi::{logic_import::*, message::from_str},
    api::{
        network::SessionClient,
        xmu_service::{
            jw::ScheduleCourseTime, llm::choose_timetable::ChooseTimetable,
            login::request_qrcode_castgc,
        },
    },
    logic::login::process::send_msg_and_wait,
};
use anyhow::anyhow;
use std::sync::Arc;

#[handler(msg_type=Message,command="signtime",echo_cmd=true,
help_msg=r#"用法:/signtime <描述>
<描述>:存储签到课程的学期的描述，用于短路命中定位签到
注:查看课表一定会伴随着一次登录
注:存储后会自动开启定时签到
功能:存储刷新指定用户存储的课程位置时间表"#)]
pub async fn sign_time(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let id = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;
    let group_id = match &*ctx.message {
        Message::Group(msg) => msg.group_id,
        Message::Private(_) => return Err(anyhow!("请在群聊中使用此命令")),
    };

    let client = SessionClient::new();
    let login_data = send_msg_and_wait(&mut ctx, &client, id).await?;

    request_qrcode_castgc(&client, login_data).await?;

    let (schedule, _) = ChooseTimetable::get_from_client(&client, ctx.get_message_text()).await?;

    let course_time = ScheduleCourseTime::new(schedule)?;
    let course_time = Arc::new(course_time);

    DATA.insert(id, course_time.clone())?;
    TIMETABLE_GROUP.insert(id, Arc::new(group_id))?;
    TIME_SIGN_TASK.force_update().await?;

    ctx.send_message_async(from_str(format!(
        "课程时间表已更新，包含 {} 段时间",
        course_time.times.len()
    )));

    Ok(())
}
