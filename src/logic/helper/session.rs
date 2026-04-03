use anyhow::{Result, anyhow};

use crate::{
    abi::{
        Context,
        message::{MessageType, event_body::message::Message},
        network::BotClient,
        websocket::BotHandler,
    },
    api::{network::SessionClient, xmu_service::lnt::get_session_client},
    logic::login::{LOGIN_DATA, process::process_login},
};
use std::fmt;
use tracing::info;

pub async fn get_client_or_err<T>(ctx: &mut Context<T, Message>) -> Result<SessionClient>
where
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
{
    let sender = ctx.message.get_sender();
    let id = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;
    match async move {
        let session = LOGIN_DATA
            .get(&id)
            .ok_or(anyhow!("未登录，请使用“/login”登录后使用"))?;

        Ok::<SessionClient, anyhow::Error>(get_session_client(&session.lnt))
    }
    .await
    {
        Ok(e) => Ok(e),
        Err(e) => {
            info!("未登录({e})发起登录");
            let login_data = process_login(ctx, id).await?;
            Ok(get_session_client(&login_data.lnt))
        }
    }
}
