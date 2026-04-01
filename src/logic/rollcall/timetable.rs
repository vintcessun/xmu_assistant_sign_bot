use super::super::BuildHelp;
use super::data::TIMETABLE_DATA as DATA;
use super::data::TIMETABLE_GROUP;
use super::time::{TIME_SIGN_TASK, get_today_courses};
use crate::logic::login::process::process_login_castgc;
use crate::{
    abi::{logic_import::*, message::from_str},
    api::xmu_service::{
        jw::{ClockTime, ScheduleCourseTime},
        llm::choose_timetable::ChooseTimetable,
    },
};
use anyhow::{Result, anyhow};
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
    let client = process_login_castgc(&mut ctx, id).await?;

    let (schedule, _) = ChooseTimetable::get_from_client(&client, ctx.get_message_text()).await?;

    let course_time = ScheduleCourseTime::new(schedule)?;
    let course_time = Arc::new(course_time);

    DATA.insert(id, course_time.clone())?;
    TIMETABLE_GROUP.insert(id, Arc::new(group_id))?;
    TIME_SIGN_TASK.force_update().await?;
    let edit_url = crate::web::timetable::task::create_edit_task_url(id, &course_time);

    ctx.send_message_async(from_str(format!(
        "课程时间表已更新，包含 {} 段时间",
        course_time.times.len()
    )));

    ctx.send_message_async(from_str(format!(
        "可在 20 分钟内通过以下链接编辑课表，超时将自动使用当前保存结果:\n{}",
        edit_url
    )));

    ctx.send_message_async(from_str("定时签到已开启"));

    Ok(())
}

#[handler(msg_type=Message,command="delsigntime",echo_cmd=true,
help_msg=r#"用法:/delsigntime
功能:删除指定用户存储的课程时间表"#)]
pub async fn del_sign_time(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let id = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;

    remove_sign_time(id).await?;

    ctx.send_message_async(from_str("课程时间表已删除"));

    Ok(())
}

pub async fn remove_sign_time(qq: i64) -> Result<()> {
    TIMETABLE_GROUP.remove(&qq)?;
    TIME_SIGN_TASK.force_update().await?;
    DATA.remove(&qq)?;
    Ok(())
}

pub fn query_sign_time(qq: i64) -> Option<Arc<ScheduleCourseTime>> {
    DATA.get(&qq)
}

pub fn query_sign_group(qq: i64) -> Option<i64> {
    TIMETABLE_GROUP.get(&qq).map(|x| *x)
}

pub fn is_sign_time_active_now(qq: i64) -> bool {
    get_today_courses(qq)
        .map(|m| m.is_active(ClockTime::now()))
        .unwrap_or(false)
}

pub async fn update_sign_time(qq: i64, course_time: ScheduleCourseTime) -> Result<()> {
    DATA.insert(qq, Arc::new(course_time))?;
    TIME_SIGN_TASK.force_update().await?;
    Ok(())
}
