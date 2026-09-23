use super::super::BuildHelp;
use super::data::LOGIN_DATA;
use super::data::TIMETABLE_DATA as DATA;
use super::data::TIMETABLE_DATA_V3;
use super::data::TIMETABLE_GROUP;
use super::time::{TIME_SIGN_TASK, get_today_courses};
use super::utils::uniform;
use crate::logic::login::process::{login_castgc_for_id, process_login_castgc};
use crate::{
    abi::{logic_import::*, message::from_str},
    api::xmu_service::{
        jw::{ClockTime, ScheduleCourseTime},
        llm::choose_timetable::ChooseTimetable,
    },
};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use tracing::{info, warn};

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

    write_sign_time(id, course_time.clone())?;
    TIMETABLE_GROUP.insert(id, Arc::new(group_id))?;
    TIME_SIGN_TASK.force_update().await?;
    // 更新课表往往意味着刚改过选课，顺手刷一次选课索引：
    // 定时签到的同课蹲守要靠它把人分组，索引落后就会盯不到新加的课。
    if let Some(login) = LOGIN_DATA.get(&id) {
        crate::logic::rollcall::spawn_upsert(id, login.lnt.clone());
    }
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
    TIMETABLE_DATA_V3.remove(&qq).ok();
    Ok(())
}

/// 读课表的**唯一入口**：先看 v4，没有再回落到 v3 并就地转换。
///
/// 所有读课表的地方都必须走这里，否则还没重新跑过 `/signtime` 的用户会凭空消失。
/// v3 空了之后，这个函数缩回 `DATA.get(&qq)` 即可。
pub fn query_sign_time(qq: i64) -> Option<Arc<ScheduleCourseTime>> {
    if let Some(v4) = DATA.get(&qq) {
        return Some(v4);
    }
    let v3 = TIMETABLE_DATA_V3.get(&qq)?;
    Some(Arc::new(ScheduleCourseTime::from((*v3).clone())))
}

/// 所有存了课表的用户（v4 与 v3 的并集）。遍历课表的地方都必须用它，
/// 直接遍历 `DATA` 会把还没重新跑过 `/signtime` 的用户整个漏掉。
/// v3 空了之后，这个函数缩回只遍历 `DATA` 即可。
pub fn all_timetable_users() -> Vec<i64> {
    let mut users: Vec<i64> = Vec::new();
    for entry in &*DATA {
        users.push(*entry.key());
    }
    for entry in &*TIMETABLE_DATA_V3 {
        users.push(*entry.key());
    }
    users.sort_unstable();
    users.dedup();
    users
}

/// 该用户的课表还停留在 v3（没有 class_code）。状态卡靠它显示"待迁移"标记；
/// v3 表排空后连同标记一起删。
pub fn is_legacy_timetable(qq: i64) -> bool {
    DATA.get(&qq).is_none() && TIMETABLE_DATA_V3.get(&qq).is_some()
}

/// 还留在 v3 表上的用户数。归零就说明存量已经刷干净，可以删掉 v3 的表和代码了。
pub fn legacy_v3_count() -> usize {
    let mut n = 0;
    for _ in &*TIMETABLE_DATA_V3 {
        n += 1;
    }
    n
}

/// 还留在 v3 表上的用户 QQ（升序），随人数播报一起打进日志，方便判断这些人还用不用。
/// v3 空了之后连同 `legacy_v3_count` 一起删。
pub fn legacy_v3_users() -> Vec<i64> {
    let mut users: Vec<i64> = Vec::new();
    for entry in &*TIMETABLE_DATA_V3 {
        users.push(*entry.key());
    }
    users.sort_unstable();
    users
}

/// 写 v4 的同时把 v3 的旧行删掉，让 v3 表真的能被排空。
fn write_sign_time(qq: i64, course_time: Arc<ScheduleCourseTime>) -> Result<()> {
    DATA.insert(qq, course_time)?;
    // v3 里没有这个人时 remove 也是无害的。
    TIMETABLE_DATA_V3.remove(&qq).ok();
    Ok(())
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
    write_sign_time(qq, Arc::new(course_time))?;
    TIME_SIGN_TASK.force_update().await?;
    Ok(())
}

/// 替还停在 v3 的用户在后台重跑一次 `/signtime`（默认学期、不问 LLM），
/// 从教务拿回真实的班级代码写进 v4，同时删掉 v3 行。
///
/// 登不上（CASTGC 失效且没有账号密码）、拉课表失败、或拉回来是空课表的，一律跳过，
/// 让他继续留在 v3——绝不拿 v3 转一份空 class_code 的 v4 去顶替。
/// 启动时跑一次；每次重启都会重试剩下的人。v3 空了之后连同 legacy 代码一起删。
pub async fn migrate_legacy_v3() {
    let users = legacy_v3_users();
    if users.is_empty() {
        return;
    }
    info!(users = ?users, "开始自动迁移 v3 课表");
    for (i, qq) in users.into_iter().enumerate() {
        // 别在同一秒打一排 CAS 登录。
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(uniform(5..15))).await;
        }
        match migrate_one_legacy(qq).await {
            Ok((total, with_code)) => info!(
                qq,
                courses = total,
                with_class_code = with_code,
                "v3 课表已自动迁移到 v4"
            ),
            Err(e) => warn!(qq, error = %e, "v3 课表自动迁移跳过，保留原课表"),
        }
    }
    info!(
        legacy_v3_users = legacy_v3_count(),
        remaining = ?legacy_v3_users(),
        "v3 课表自动迁移结束"
    );
}

async fn migrate_one_legacy(qq: i64) -> Result<(usize, usize)> {
    let client = login_castgc_for_id(qq)
        .await
        .map_err(|e| anyhow!("教务登录失败: {e}"))?;
    let (schedule, _) = ChooseTimetable::get_from_client(&client, "").await?;
    let course_time = ScheduleCourseTime::new(schedule)?;
    if course_time.times.is_empty() {
        return Err(anyhow!("教务返回的默认学期课表是空的"));
    }
    let total = course_time.times.len();
    let with_code = course_time
        .times
        .iter()
        .filter(|c| !c.class_code.is_empty())
        .count();
    update_sign_time(qq, course_time).await?;
    Ok((total, with_code))
}
