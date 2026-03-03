use crate::abi::client::get_client;
use crate::abi::echo::Echo;
use crate::abi::logic_import::*;
use crate::abi::message::MessageSend;
use crate::abi::message::api::SendGroupMessageParams;
use crate::abi::network::BotClient;
use crate::logic::rollcall::auto_sign_data::AutoSignResponse;
use crate::logic::rollcall::auto_sign_data::auto_sign_response::{NumberSign, QRSign, RadarSign};
use crate::logic::rollcall::data::TIMETABLE_GROUP;
use crate::{
    api::{
        scheduler::{TaskRunner, TimeTask},
        xmu_service::jw::ClockTime,
    },
    logic::rollcall::{
        auto_sign_request, data::TIMETABLE_DATA, time::TIME_SIGN_TASK, utils::uniform,
    },
};
use anyhow::Result;
use async_trait::async_trait;
use helper::handler;
use std::sync::Arc;
use std::{sync::LazyLock, time::Duration};
use tracing::{error, info, trace};

pub struct TimeSignTask;

#[async_trait]
impl TimeTask for TimeSignTask {
    type Output = ();

    fn interval(&self) -> Duration {
        Duration::from_secs(uniform(20..40))
    }

    fn name(&self) -> &'static str {
        "TimeSignTask"
    }

    async fn run(&self) -> Result<Self::Output> {
        time_sign_task().await?;
        Ok(())
    }
}

pub struct TimeSignUpdateResponse {
    pub qq: i64,
    pub group_id: i64,
    pub response: Vec<AutoSignResponse>,
}

async fn time_sign_task() -> Result<()> {
    let mut tasks = vec![];
    let course_time = TIME_SIGN_TASK.get_latest().await?;
    for val in &*TIMETABLE_DATA {
        let qq = *val.key();
        if let Some(entry) = course_time.get(&qq)
            && let Some(group_id) = TIMETABLE_GROUP.get(&qq)
        {
            let time_val = entry.value();
            if time_val.is_active(ClockTime::now()) {
                tasks.push(async move {
                    let response = auto_sign_request(qq).await?;
                    let response = response
                        .into_iter()
                        .filter(|x| match x {
                            AutoSignResponse::Qr(data) => matches!(data, QRSign::Success(_)),
                            AutoSignResponse::Number(data) => {
                                matches!(data, NumberSign::Success(_))
                            }
                            AutoSignResponse::Radar(data) => {
                                matches!(data, RadarSign::Success(_))
                            }
                        })
                        .collect::<Vec<_>>();
                    Ok::<TimeSignUpdateResponse, anyhow::Error>(TimeSignUpdateResponse {
                        qq,
                        group_id: *group_id,
                        response,
                    })
                });
            }
        }
    }
    let ret = futures::future::join_all(tasks)
        .await
        .into_iter()
        .filter(|x| x.is_ok())
        .flatten()
        .collect::<Vec<_>>();
    let client = get_client();
    let mut tasks = Vec::with_capacity(ret.len());

    for r in ret {
        if r.response.is_empty() {
            continue;
        }

        let params = SendGroupMessageParams {
            group_id: r.group_id,
            message: Arc::new(
                MessageSend::new_message()
                    .at(r.qq.to_string())
                    .text(format!(
                        "定时签到结果:\n{}",
                        r.response
                            .iter()
                            .map(|r| format!("{}\n", r))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ))
                    .build(),
            ),
        };
        trace!(qq=r.qq,group_id=r.group_id, params=?params, "准备发送定时签到消息");
        tasks.push(async move {
            let echo = client.call_api(&params, Echo::new()).await?;
            let res = echo.wait_echo().await;
            match res {
                Ok(e) => {
                    info!("定时签到消息发送成功: {:?}", e);
                }
                Err(e) => {
                    error!("定时签到消息发送失败: {:?}", e);
                }
            };
            Ok::<(), anyhow::Error>(())
        });
    }

    futures::future::join_all(tasks).await;

    Ok(())
}

pub static TIME_SIGN_TASK_RUNNER: LazyLock<Arc<TaskRunner<TimeSignTask>>> =
    LazyLock::new(|| TaskRunner::new(TimeSignTask));

#[handler(msg_type=Message)]
pub async fn time_sign(ctx: Context) -> Result<()> {
    TIME_SIGN_TASK_RUNNER.get_latest().await?;

    Ok(())
}
