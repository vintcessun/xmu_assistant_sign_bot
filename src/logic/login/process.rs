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
use tracing::{debug, error, info, trace, warn};

pub async fn update_and_login(
    session: &SessionClient,
    data: LoginRequest,
    id: i64,
) -> Result<Arc<LoginData>> {
    debug!(user_id = id, "开始请求二维码登录凭证");
    let login_data = Arc::new(request_qrcode(session, data).await?);
    info!(user_id = id, "成功获取二维码登录凭证");

    let login_data_insert = login_data.clone();

    DATA.insert(id, login_data_insert).map_err(|e| {
        error!(user_id = id, error = ?e, "存储用户登录数据失败");
        e
    })?;
    info!(user_id = id, "用户登录数据存储成功");

    Ok(login_data)
}

#[inline(never)]
pub async fn login_base(login_data: Arc<LoginData>) -> Result<ZzyProfile> {
    debug!("开始获取用户基础信息");
    let user_id = match Profile::get(&login_data.lnt).await {
        Ok(p) => {
            debug!(user_no = p.user_no, "通过 LNT 成功获取用户学号");
            p.user_no.clone()
        }
        Err(e) => {
            warn!(error = ?e, "获取 LNT 用户信息失败，尝试使用 JW 用户信息登录");
            let user_info = UserInfo::get(&login_data.castgc).await?;
            debug!(user_id = user_info.user_id, "通过 JW 成功获取用户学号");
            user_info.user_id
        }
    };

    trace!(user_id = user_id, "开始获取转专业用户信息");
    let data = Zzy::get(&login_data.castgc, &user_id).await?;

    let zzy_profile = data.get_profile()?;
    debug!("成功解析正方系统用户信息");

    Ok(zzy_profile)
}

#[inline(never)]
pub async fn send_msg_and_wait<T: BotClient + BotHandler + fmt::Debug>(
    ctx: &mut Context<T, Message>,
    session: &SessionClient,
    id: i64,
) -> Result<LoginRequest> {
    info!(user_id = id, "开始获取二维码 ID 和登录请求数据");
    let (qrcode_id, data) = get_qrcode_id(session).await?;
    debug!(qrcode_id = qrcode_id, "成功获取二维码 ID");

    {
        let qrcode_url =
            format!("https://ids.xmu.edu.cn/authserver/qrCode/getCode?uuid={qrcode_id}");

        let qrcode_login =
            format!("https://ids.xmu.edu.cn/authserver/qrCode/qrCodeLogin.do?uuid={qrcode_id}");

        info!(user_id = id, "向用户发送扫码登录信息");
        ctx.send_message(
            MessageSend::new_message()
                .at(id.to_string())
                .text(format!("将为{id}登录：\n"))
                .text("请使用企业微信扫码登录")
                .image_url(qrcode_url)
                .text("\n或者移动端直接点击链接登录：")
                .text(qrcode_login)
                .build(),
        )
        .await
        .map_err(|e| {
            error!(user_id = id, error = ?e, "发送二维码消息失败");
            e
        })?;
    }

    debug!(qrcode_id = qrcode_id, "等待用户扫码确认");
    wait_qrcode(session, &qrcode_id).await?;
    info!(user_id = id, "用户已扫码并确认登录");

    Ok(data)
}

pub async fn process_login<T: BotClient + BotHandler + fmt::Debug>(
    ctx: &mut Context<T, Message>,
    id: i64,
) -> Result<Arc<LoginData>> {
    info!(user_id = id, "开始执行登录流程");
    let session = SessionClient::new();
    debug!(user_id = id, "创建新的 SessionClient");

    let data = send_msg_and_wait(ctx, &session, id).await?;

    ctx.send_message_async(message::from_str("登录成功！"));

    let login_data = update_and_login(&session, data, id).await?;

    info!(user_id = id, "开始获取并处理用户身份信息");

    match login_base(login_data.clone()).await {
        Ok(zzy_profile) => {
            info!(
                user_id = id,
                entry_year = zzy_profile.entry_year,
                trans_dept = ?zzy_profile.trans_dept,
                "成功获取用户身份信息"
            );

            ctx.send_message_async(message::from_str(format!(
                "信息:{} 转入学院:{:?}",
                zzy_profile.entry_year, zzy_profile.trans_dept
            )));

            // 假设 entry_year 总是 "YYYY" 格式，长度至少为 4，使用 unsafe 切片消除运行时边界检查。
            let year = unsafe { zzy_profile.entry_year.get_unchecked(2..4).to_string() };

            let dept = zzy_profile.trans_dept.join(",");

            info!(user_id = id, year = year, dept = dept, "更新群头衔");
            ctx.set_title(format!("{}转{}", year, dept)).await?;
            info!(user_id = id, "登录流程执行完毕");
        }
        Err(e) => {
            warn!(user_id = id, error = ?e, "获取用户转专业信息失败，登录流程执行完毕");
            ctx.send_message_async(message::from_str("获取用户转专业信息失败"));
        }
    };

    Ok(login_data)
}

pub async fn process_login_castgc<T: BotClient + BotHandler + fmt::Debug>(
    ctx: &mut Context<T, Message>,
    id: i64,
) -> Result<SessionClient> {
    if let Some(data) = DATA.get(&id)
        && UserInfo::get(&data.castgc).await.is_ok()
    {
        info!(user_id = id, "用户已登录，直接使用现有登录数据");
        let session = SessionClient::new();
        session.set_cookie(
            "CASTGC",
            &data.castgc,
            &url::Url::parse("https://ids.xmu.edu.cn").unwrap(),
        );
        return Ok(session);
    }

    let client = SessionClient::new();

    let data = send_msg_and_wait(ctx, &client, id).await?;

    update_and_login(&client, data, id).await?;

    Ok(client)
}
