use super::super::BuildHelp;
use super::data::LOGIN_DATA;
use crate::{
    abi::{logic_import::*, message::from_str},
    api::{
        network::SessionClient,
        xmu_service::lnt::{CourseData, Rollcalls, get_session_client},
    },
    logic::{
        helper::get_client_or_err,
        rollcall::sign_data::{SignData, SignResponse, get_on_call_total_num},
    },
};
use anyhow::{Result, anyhow};
use std::sync::Arc;

#[handler(msg_type=Message,command="sign",echo_cmd=true,
help_msg=r#"用法:/sign
功能:查询签到"#)]
pub async fn sign(ctx: Context) -> Result<()> {
    let client = Arc::new(get_client_or_err(&mut ctx).await?);
    let qq = ctx
        .message
        .get_sender()
        .user_id
        .ok_or(anyhow!("获取用户ID失败"))?;

    let ret = sign_request_inner(qq, client).await?;

    ctx.send_message_async(from_str(format!("查询完成，{} 门课程", ret.len())));

    for e in ret {
        ctx.send_message_async(from_str(format!("{}", e)));
    }

    Ok(())
}

pub async fn sign_request(qq: i64) -> Result<Vec<SignResponse>> {
    let login_data = LOGIN_DATA
        .get(&qq)
        .ok_or(anyhow!("未找到登录数据，请先登录"))?;
    let client = Arc::new(get_session_client(&login_data.lnt));
    sign_request_inner(qq, client).await
}

pub async fn sign_request_inner(qq: i64, client: Arc<SessionClient>) -> Result<Vec<SignResponse>> {
    let session_cookie = &LOGIN_DATA.get(&qq).ok_or(anyhow!("没找到登录信息"))?.lnt;

    let all_courses = Rollcalls::get_from_client(&client)
        .await
        .map_err(|e| anyhow!("错误: {e} 登录状态可能失效"))?;

    let mut ret = Vec::with_capacity(all_courses.rollcalls.len());

    for per_course in all_courses.rollcalls {
        let course_id = per_course.course_id;
        let activity_id = per_course.rollcall_id;
        let status = per_course.status;
        let is_number = per_course.is_number;
        let is_radar = per_course.is_radar;
        let course_title = per_course.course_title;

        let course_info = CourseData::get(session_cookie, course_id).await?;
        let course_code = course_info.course_code.clone();
        let instructors = course_info
            .instructors
            .iter()
            .map(|x| x.name.clone())
            .collect::<Vec<_>>();

        let (sign_num, student_num) = get_on_call_total_num(&client, activity_id).await?;

        if is_number {
            let number_code = SignData::number(&client, activity_id).await?;

            ret.push(
                SignResponse::create(
                    course_title,
                    course_code,
                    activity_id,
                    instructors,
                    sign_num,
                    student_num,
                    status,
                )
                .number(number_code),
            );
        } else if is_radar {
            ret.push(
                SignResponse::create(
                    course_title,
                    course_code,
                    activity_id,
                    instructors,
                    sign_num,
                    student_num,
                    status,
                )
                .radar(),
            );
        } else {
            ret.push(
                SignResponse::create(
                    course_title,
                    course_code,
                    activity_id,
                    instructors,
                    sign_num,
                    student_num,
                    status,
                )
                .qr(),
            );
        }
    }

    Ok(ret)
}
