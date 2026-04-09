use anyhow::{Result, anyhow};

use crate::{
    abi::{
        Context,
        message::{MessageType, event_body::message::Message},
        network::BotClient,
        websocket::BotHandler,
    },
    api::{
        network::SessionClient,
        xmu_service::{
            lnt::{ProfileWithoutCache, get_session_client},
            login::login_password,
        },
    },
    logic::login::{LOGIN_DATA, PWD_DATA, process::process_login},
};
use dashmap::DashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::LazyLock;
use tracing::info;

static CLIENT_CACHE: LazyLock<DashMap<i64, SessionClient>> = LazyLock::new(DashMap::new);

pub fn get_client_from_cache(id: i64) -> Option<SessionClient> {
    CLIENT_CACHE.get(&id).map(|entry| entry.value().clone())
}

pub async fn get_client_or_err_for_id(id: i64) -> Result<SessionClient> {
    if let Some(client) = CLIENT_CACHE.get(&id) {
        let client = client.value().clone();
        if ProfileWithoutCache::get_from_client(&client).await.is_ok() {
            return Ok(client.clone());
        }
    }
    if let Some(e) = LOGIN_DATA.get(&id) {
        let client = get_session_client(&e.lnt);
        if ProfileWithoutCache::get_from_client(&client).await.is_ok() {
            CLIENT_CACHE.insert(id, client.clone());
            return Ok(client);
        }
    }
    if let Some(e) = PWD_DATA.get(&id) {
        let client = SessionClient::new();
        let login_data = login_password(&client, e.username.clone(), &e.password).await?;
        LOGIN_DATA.insert(id, Arc::new(login_data))?;
        CLIENT_CACHE.insert(id, client.clone());
        return Ok(client);
    }
    Err(anyhow!("未登录"))
}

pub async fn get_client_or_err<T>(ctx: &mut Context<T, Message>) -> Result<SessionClient>
where
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
{
    let sender = ctx.message.get_sender();
    let id = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;
    if let Ok(client) = get_client_or_err_for_id(id).await {
        return Ok(client);
    }
    info!("未登录发起登录");
    let login_data = process_login(ctx, id).await?;
    let client = get_session_client(&login_data.lnt);
    CLIENT_CACHE.insert(id, client.clone());
    Ok(client)
}
