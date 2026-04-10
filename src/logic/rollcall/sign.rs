use super::super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::from_str},
    api::{
        network::SessionClient,
        xmu_service::lnt::{CourseData, Rollcalls},
    },
    logic::{
        helper::{get_client_or_err, get_client_or_err_for_id},
        rollcall::sign_data::{SignData, SignResponse, get_on_call_total_num},
    },
};
use anyhow::{Result, anyhow};

#[handler(msg_type=Message,command="sign",echo_cmd=true,
help_msg=r#"用法:/sign
功能:查询签到"#)]
pub async fn sign(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&mut ctx).await?;
    let ret = sign_request_inner(&client).await?;

    ctx.send_message_async(from_str(format!("查询完成，{} 门课程", ret.len())));

    for e in ret {
        ctx.send_message_async(from_str(format!("{}", e)));
    }

    Ok(())
}

pub async fn sign_request(qq: i64) -> Result<Vec<SignResponse>> {
    let client = get_client_or_err_for_id(qq).await?;
    sign_request_inner(&client).await
}

pub async fn sign_request_inner(client: &SessionClient) -> Result<Vec<SignResponse>> {
    let all_courses = Rollcalls::get_from_client(client)
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

        let course_info = CourseData::get_from_client(client, course_id).await?;
        let course_code = course_info.course_code.clone();
        let instructors = course_info
            .instructors
            .iter()
            .map(|x| x.name.clone())
            .collect::<Vec<_>>();

        let (sign_num, student_num) = get_on_call_total_num(client, activity_id).await?;

        if is_number {
            let number_code = SignData::number(client, activity_id).await?;

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
