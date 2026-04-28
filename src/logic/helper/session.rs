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
    logic::login::{
        LOGIN_DATA, PWD_DATA,
        cache::{CLIENT_CACHE, write_client_cache},
        process::process_login,
    },
};
use std::fmt;
use std::sync::Arc;
use tracing::{info, warn};

pub use crate::logic::login::cache::get_client_from_cache;

pub async fn get_client_or_err_for_id(id: i64) -> Result<SessionClient> {
    let cached_client = CLIENT_CACHE.get(&id).map(|entry| entry.value().clone());
    if let Some(client) = cached_client
        && ProfileWithoutCache::get_from_client(&client).await.is_ok()
    {
        return Ok(client);
    }
    let login_lnt = LOGIN_DATA.get(&id).map(|entry| entry.lnt.clone());
    if let Some(lnt) = login_lnt {
        let client = get_session_client(&lnt);
        if ProfileWithoutCache::get_from_client(&client).await.is_ok() {
            write_client_cache(id, client.clone(), "recover_lnt");
            info!(user_id = id, "recover_success_cache_written");
            return Ok(client);
        } else {
            warn!(
                user_id = id,
                "recover_failed: lnt 恢复会话失败，尝试密码重登录"
            );
        }
    }
    let login_credential = PWD_DATA
        .get(&id)
        .map(|entry| (entry.username.clone(), entry.password.clone()));
    if let Some((username, password)) = login_credential {
        let client = SessionClient::new();
        match login_password(&client, username, &password).await {
            Ok(login_data) => {
                LOGIN_DATA.insert(id, Arc::new(login_data))?;
                write_client_cache(id, client.clone(), "recover_pwd");
                info!(user_id = id, "recover_success_cache_written");
                return Ok(client);
            }
            Err(e) => {
                warn!(user_id = id, error = ?e, "recover_failed: 密码重登录失败");
                return Err(e);
            }
        }
    }
    warn!(user_id = id, "recover_failed: 无可用登录凭证");
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
    info!(user_id = id, "未登录发起登录");
    let login_data = process_login(ctx, id).await?;
    let client = get_session_client(&login_data.lnt);
    // process_login 内部路径已写入缓存；此处为 qr 登录返回后经 lnt 重建 session 的补充写入
    write_client_cache(id, client.clone(), "login_qr_rehydrate");
    Ok(client)
}
