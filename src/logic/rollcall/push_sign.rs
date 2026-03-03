use super::super::BuildHelp;
use super::data::LOGIN_DATA;
use crate::{
    abi::{logic_import::*, message::from_str},
    logic::rollcall::{auto_sign_data::AutoSignResponse, spec_sign_request},
};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

#[handler(msg_type=Message,command="pushsign",echo_cmd=true,
help_msg=r#"用法:/pushsign <ID>
<ID>: 签到ID，可以通过/sign命令查看
注: 签到功能和/autosign相同
功能:自动对所有已登录用户指定课程签到数字和雷达"#)]
pub async fn push_sign(ctx: Context) -> Result<()> {
    let rollcall_id = ctx
        .get_message_text()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<i64>()
        .map_err(|e| anyhow!("无效的签到ID {e}"))?;

    let ret = push_sign_request(rollcall_id).await?;
    for r in ret {
        ctx.send_message_async(from_str(format!(
            "QQ: {}\n{}",
            r.qq,
            r.response
                .iter()
                .map(|r| format!("{}\n", r))
                .collect::<Vec<_>>()
                .join("\n")
        )));
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSignResponse {
    pub qq: i64,
    pub response: Vec<AutoSignResponse>,
}

pub async fn push_sign_request(rollcall_id: i64) -> Result<Vec<PushSignResponse>> {
    let mut task = Vec::new();
    for val in &*LOGIN_DATA {
        let qq = *val.key();
        task.push(async move { (qq, spec_sign_request(qq, rollcall_id).await) });
    }

    let ret = futures::future::join_all(task)
        .await
        .into_iter()
        .filter(|x| x.1.is_ok())
        .map(|x| {
            let (qq, response) = x;
            PushSignResponse {
                qq,
                response: response.unwrap(),
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .filter(|x| !x.response.is_empty())
        .collect();

    Ok(ret)
}
