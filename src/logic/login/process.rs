use super::main::DATA;
use crate::abi::message::MessageSend;
use crate::api::xmu_service::jw::{UserInfo, Zzy, ZzyProfile};
use crate::api::xmu_service::lnt::Profile;
use crate::api::xmu_service::login::{
    LoginData, LoginRequest, get_qrcode_id, request_qrcode, wait_qrcode,
};
use crate::{abi::logic_import::*, api::network::SessionClient};
use anyhow::Result;
use std::sync::Arc;
use tracing::warn;

pub async fn update_and_login(
    session: &SessionClient,
    data: LoginRequest,
    id: i64,
) -> Result<Arc<LoginData>> {
    let login_data = Arc::new(request_qrcode(session, data).await?);

    let login_data_insert = login_data.clone();

    DATA.insert(id, login_data_insert)?;

    Ok(login_data)
}

#[inline(never)]
pub async fn login_base(login_data: Arc<LoginData>) -> Result<ZzyProfile> {
    let user_id = match Profile::get(&login_data.lnt).await {
        Ok(p) => p.user_no.clone(),
        Err(e) => {
            warn!(
                "获取 LNT 用户信息失败，尝试使用 JW 用户信息登录，错误信息: {}",
                e
            );
            let userinfo = UserInfo::get(&login_data.castgc).await?;
            userinfo.user_id
        }
    };

    let data = Zzy::get(&login_data.castgc, &user_id).await?;

    let zzy_profile = data.get_profile()?;

    Ok(zzy_profile)
}

#[inline(never)]
pub async fn send_msg_and_wait<T: BotClient + BotHandler + fmt::Debug>(
    ctx: &mut Context<T, Message>,
    session: &SessionClient,
    id: i64,
) -> Result<LoginRequest> {
    let (qrcode_id, data) = get_qrcode_id(session).await?;

    {
        let qrcode_url =
            format!("https://ids.xmu.edu.cn/authserver/qrCode/getCode?uuid={qrcode_id}");

        let qrcode_login =
            format!("https://ids.xmu.edu.cn/authserver/qrCode/qrCodeLogin.do?uuid={qrcode_id}");

        ctx.send_message(
            MessageSend::new_message()
                .at(id.to_string())
                .text(format!("将为{id}登录：\n"))
                .text("请使用企业微信扫码登录")
                .image_url(qrcode_url)
                .text("\n或者直接点击链接登录：")
                .text(qrcode_login)
                .build(),
        )
        .await?;
    }

    wait_qrcode(session, &qrcode_id).await?;

    Ok(data)
}

pub async fn process_login<T: BotClient + BotHandler + fmt::Debug>(
    ctx: &mut Context<T, Message>,
    id: i64,
) -> Result<Arc<LoginData>> {
    let session = SessionClient::new();

    let data = send_msg_and_wait(ctx, &session, id).await?;

    ctx.send_message_async(message::from_str("登录成功！"));

    let login_data = update_and_login(&session, data, id).await?;

    let zzy_profile = login_base(login_data.clone()).await?;

    ctx.send_message_async(message::from_str(format!(
        "信息:{} 转入学院:{:?}",
        zzy_profile.entry_year, zzy_profile.trans_dept
    )));

    // 假设 entry_year 总是 "YYYY" 格式，长度至少为 4，使用 unsafe 切片消除运行时边界检查。
    let year = unsafe { zzy_profile.entry_year.get_unchecked(2..4).to_string() };

    let dept = zzy_profile.trans_dept.join(",");

    ctx.set_title(format!("{}转{}", year, dept)).await?;

    Ok(login_data)
}
