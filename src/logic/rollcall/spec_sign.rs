use super::super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::from_str},
    api::{
        network::SessionClient,
        xmu_service::lnt::{Rollcalls, rollcalls::RollcallStatus},
    },
    logic::{
        helper::{get_client_or_err, get_client_or_err_for_id},
        rollcall::{auto_sign_data::AutoSignResponse, auto_sign_request::AutoSignRequest},
    },
};
use anyhow::{Result, anyhow};
use tracing::trace;

#[handler(msg_type=Message,command="specsign",echo_cmd=true,
help_msg=r#"用法:/specsign <ID>
<ID>: 签到ID，可以通过/sign命令查看
注: 签到功能和/autosign相同
功能:自动对指定课程签到数字和雷达"#)]
pub async fn spec_sign(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&mut ctx).await?;
    let qq = ctx
        .message
        .get_sender()
        .user_id
        .ok_or(anyhow!("获取用户ID失败"))?;

    let rollcall_id = ctx.get_message_number::<i64>()?;

    let ret = spec_sign_request_inner(qq, client, rollcall_id).await?;

    ctx.send_message_async(from_str(format!("签到完成，{} 门课程", ret.len())));

    for e in ret {
        ctx.send_message_async(from_str(format!("{}", e)));
    }

    Ok(())
}

pub async fn spec_sign_request(qq: i64, rollcall_id: i64) -> Result<Vec<AutoSignResponse>> {
    let client = get_client_or_err_for_id(qq).await?;
    spec_sign_request_inner(qq, client, rollcall_id).await
}

pub async fn spec_sign_request_inner(
    qq: i64,
    client: SessionClient,
    rollcall_id: i64,
) -> Result<Vec<AutoSignResponse>> {
    let rollcall_data = Rollcalls::get_from_client(&client)
        .await
        .map_err(|e| anyhow!("错误: {e} 登录状态可能失效"))?;
    let mut responses = Vec::with_capacity(rollcall_data.rollcalls.len());
    for rollcall in rollcall_data.rollcalls {
        if rollcall.rollcall_id != rollcall_id {
            continue;
        }

        let auto_sign_request =
            AutoSignRequest::get(rollcall.course_id, qq, client.clone()).await?;
        if rollcall.is_number {
            match rollcall.status {
                RollcallStatus::Absent => {
                    trace!(rollcall = ?rollcall, "处理当前数字签到信息");
                    responses.push(auto_sign_request.number(rollcall.rollcall_id).await?);
                }
                RollcallStatus::OnCallFine => {
                    trace!(rollcall=?rollcall,"当前数字签到状态已签到");
                    responses.push(AutoSignResponse::number_already_signed(
                        rollcall.course_title.clone(),
                    ));
                }
            }
        } else if rollcall.is_radar {
            match rollcall.status {
                RollcallStatus::Absent => {
                    trace!(rollcall = ?rollcall, "处理当前雷达签到信息");
                    responses.push(auto_sign_request.radar(rollcall.rollcall_id).await?);
                }
                RollcallStatus::OnCallFine => {
                    trace!(rollcall=?rollcall,"当前雷达签到状态已签到");
                    responses.push(AutoSignResponse::radar_already_signed(
                        rollcall.course_title,
                    ));
                }
            }
        } else {
            match rollcall.status {
                RollcallStatus::Absent => {
                    trace!(rollcall = ?rollcall, "处理当前二维码签到信息");

                    responses.push(AutoSignResponse::qr_pending(rollcall.course_title));
                }
                RollcallStatus::OnCallFine => {
                    trace!(rollcall=?rollcall,"当前二维码签到状态已签到");
                    responses.push(AutoSignResponse::qr_already_signed(rollcall.course_title));
                }
            }
        }
    }
    Ok(responses)
}
