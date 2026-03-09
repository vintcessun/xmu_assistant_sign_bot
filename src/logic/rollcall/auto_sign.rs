use super::super::BuildHelp;
use super::data::LOGIN_DATA;
use crate::{
    abi::{logic_import::*, message::from_str},
    api::{
        network::SessionClient,
        xmu_service::lnt::{Rollcalls, get_session_client, rollcalls::RollcallStatus},
    },
    logic::{
        helper::get_client_or_err,
        rollcall::{auto_sign_data::AutoSignResponse, auto_sign_request::AutoSignRequest},
    },
};
use anyhow::{Result, anyhow};
use std::sync::Arc;
use tracing::trace;

#[handler(msg_type=Message,command="autosign",echo_cmd=true,
help_msg=r#"用法:/autosign
注:推荐使用/signtime命令预先存储课程时间表以提升签到命中率
注:对于数字签到第一个签到的人会存储签到码
注:对于雷达签到，会先查询用户的课程时间表，如果没有课程时间表就会查询签到缓存，如果都失败就会逐个尝试
功能:自动签到数字和雷达"#)]
pub async fn auto_sign(ctx: Context) -> Result<()> {
    let client = Arc::new(get_client_or_err(&ctx).await?);
    let qq = ctx
        .message
        .get_sender()
        .user_id
        .ok_or(anyhow!("获取用户ID失败"))?;

    let ret = auto_sign_request_inner(qq, client).await?;

    ctx.send_message_async(from_str(format!("签到完成，{} 门课程", ret.len())));

    for e in ret {
        ctx.send_message_async(from_str(format!("{}", e)));
    }

    Ok(())
}

pub async fn auto_sign_request(qq: i64) -> Result<Vec<AutoSignResponse>> {
    let login_data = LOGIN_DATA
        .get(&qq)
        .ok_or(anyhow!("未找到登录数据，请先登录"))?;
    let client = Arc::new(get_session_client(&login_data.lnt));
    auto_sign_request_inner(qq, client).await
}

pub async fn auto_sign_request_inner(
    qq: i64,
    client: Arc<SessionClient>,
) -> Result<Vec<AutoSignResponse>> {
    let rollcall_data = Rollcalls::get_from_client(&client)
        .await
        .map_err(|e| anyhow!("错误: {e} 登录状态可能失效"))?;
    let mut responses = Vec::with_capacity(rollcall_data.rollcalls.len());
    for rollcall in rollcall_data.rollcalls {
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
