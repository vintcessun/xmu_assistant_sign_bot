use anyhow::{Result, anyhow};

use crate::{
    abi::{Context, message::MessageType, network::BotClient, websocket::BotHandler},
    api::{network::SessionClient, xmu_service::lnt::get_session_client},
    logic::login::DATA,
};
use std::fmt;

pub async fn get_client_or_err<T, M>(ctx: &Context<T, M>) -> Result<SessionClient>
where
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
    M: MessageType + fmt::Debug + Send + Sync + 'static,
{
    let sender = ctx.message.get_sender();
    let id = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;
    let session = DATA
        .get(&id)
        .ok_or(anyhow!("未登录，请使用“/login”登录后使用"))?;

    Ok(get_session_client(&session.lnt))
}
