use super::super::BuildHelp;
use super::cache::write_client_cache;
use crate::abi::message::MessageSend;
use crate::api::network::SessionClient;
use crate::api::storage::HotTable;
use crate::logic::login::LOGIN_DATA;
use crate::logic::login::process::login_base_data;
use crate::web::login::LoginTask;
use crate::{abi::logic_import::*, api::xmu_service::login::login_password};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::LazyLock;
use tracing::error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginPwd {
    pub username: String,
    pub password: String,
}

pub static PWD_DATA: LazyLock<HotTable<i64, LoginPwd>> =
    LazyLock::new(|| HotTable::new("login_pwd_data"));

#[handler(msg_type=Message,command="loginpwd",echo_cmd=true,
help_msg=r#"用法:/loginpwd
功能:发起账号密码登录"#)]
pub async fn login_pwd(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let id = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;

    match PWD_DATA.get(&id) {
        Some(_) => {
            ctx.send_message_async(message::from_str("账号密码已录入"));
        }
        None => {
            let task = LoginTask::new(id);
            let client = SessionClient::new();
            ctx.send_message(
                MessageSend::new_message()
                    .at(id.to_string())
                    .text(format!("请点击为{id}用户登录:{}", task.get_url()))
                    .build(),
            )
            .await?;
            let (usr, pwd) = task.wait_result().await?;
            let login_data = login_password(&client, usr.clone(), &pwd).await?;
            PWD_DATA
                .insert(
                    id,
                    Arc::new(LoginPwd {
                        username: usr,
                        password: pwd,
                    }),
                )
                .map_err(|e| {
                    error!("存储用户账密失败: {:?}", e);
                    e
                })?;

            let login_data = Arc::new(login_data);

            login_base_data(&mut ctx, id, login_data.clone()).await?;

            LOGIN_DATA.insert(id, login_data).map_err(|e| {
                error!(user_id = id, error = ?e, "存储用户登录数据失败");
                e
            })?;

            write_client_cache(id, client.clone(), "login_pwd_cmd");

            ctx.send_message_async(message::from_str("账密保存成功"));
        }
    }

    Ok(())
}

#[handler(msg_type=Message,command="logoutpwd",echo_cmd=true,
help_msg=r#"用法:/logoutpwd
功能:删除账号密码登录数据"#)]
pub async fn logout_pwd(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let id = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;

    PWD_DATA.remove(&id)?;

    ctx.send_message_async(message::from_str("已删除账号密码登录数据"));

    Ok(())
}
