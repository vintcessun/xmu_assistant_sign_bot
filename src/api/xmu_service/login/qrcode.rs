use crate::api::xmu_service::IDS_URL;
use crate::api::xmu_service::lnt::LNT_URL;
use crate::api::xmu_service::login::{LOGIN_URL, LoginData, extract_execution_fast};
use crate::api::{network::SessionClient, xmu_service::login::LoginRequest};
use anyhow::{Result, anyhow, bail};
use std::time;
use tracing::{debug, error, info, trace};

impl LoginRequest {
    pub fn qrcode(url: String, qrcode_id: String, execution: String) -> Self {
        LoginRequest {
            url,
            body: super::LoginApiBody {
                token: "",
                qrcode_id: Some(qrcode_id),
                client_type: "qrLogin",
                login_type: "generalLogin",
                execution,
                event_id: "submit",
                remember_me: Some("1"),
                username: None,
                password: None,
                captcha: None,
            },
        }
    }
}

pub async fn get_qrcode(session: &SessionClient) -> Result<LoginRequest> {
    info!("开始获取厦大 IDS 登录二维码所需数据");
    let login_page = session.get(LOGIN_URL.clone()).await?;
    let base_url = login_page.url().to_string();
    let login_page_text = login_page.text().await?;
    if login_page_text.contains("IP冻结提示") {
        error!("登录服务被冻结，请联系管理员解决。");
        return Err(anyhow!("登录服务被冻结，请联系管理员解决。".to_string(),));
    }
    trace!(url = base_url, "成功获取登录页面");

    let pos = match login_page_text.find("qrLoginForm") {
        Some(e) => {
            debug!(position = e, "成功定位 qrLoginForm");
            e
        }
        None => {
            error!("登录页面结构发生变化，无法定位 qrLoginForm");
            bail!("登录错误，可能是登录页面结构发生了变化。");
        }
    };

    let login_form_data = &login_page_text[pos..];
    //找到第一个符合要求的
    let execution = extract_execution_fast(login_form_data).ok_or_else(|| {
        error!("获取 execution 失败");
        anyhow!("获取 execution 失败")
    })?;
    debug!(execution = execution, "成功获取 execution");

    let token_url = format!(
        "https://ids.xmu.edu.cn/authserver/qrCode/getToken?ts={}",
        time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)?
            .as_millis()
    );
    trace!(url = token_url, "请求二维码 ID");
    let resp = session.get(token_url.as_str()).await?;

    let qrcode_id = resp.text().await?.trim().to_string();
    debug!(qrcode_id = qrcode_id, "成功获取二维码 ID");

    Ok(LoginRequest::qrcode(base_url, qrcode_id, execution))
}

pub async fn wait_qrcode(session: &SessionClient, qrcode_id: &str) -> Result<()> {
    info!(qrcode_id = qrcode_id, "开始等待二维码扫码和确认");
    loop {
        let status_url = format!(
            "https://ids.xmu.edu.cn/authserver/qrCode/getStatus.htl?ts={}&uuid={}",
            time::SystemTime::now()
                .duration_since(time::UNIX_EPOCH)?
                .as_millis(),
            qrcode_id
        );
        let status = session.get(status_url).await?.text().await?;

        match status.as_str() {
            "0" => {
                trace!(status = "未扫码", "二维码未扫码，继续等待");
            }
            "1" => {
                info!(status = "确认登录", "二维码状态请求成功，已确认登录");
                break;
            }
            "2" => {
                trace!(status = "已扫码", "二维码已扫码，等待确认");
            }
            "3" => {
                error!(status = "已失效", "二维码已失效，请重新登录。");
                return Err(anyhow!("二维码已失效，请重新登录。"));
            }
            s => {
                error!(status = s, "收到未知的二维码状态码");
                return Err(anyhow!("未知的二维码状态码。"));
            }
        }
        tokio::time::sleep(time::Duration::from_secs(10)).await;
    }
    Ok(())
}

pub async fn request_qrcode(session: &SessionClient, data: LoginRequest) -> Result<LoginData> {
    info!(url = data.url, "发送二维码登录请求");
    session
        .post(&data.url, &data.body)
        .await?
        .error_for_status_ref()
        .map_err(|e| {
            error!(url = data.url, error = ?e, "二维码登录请求返回非成功状态码");
            e
        })?;

    let castgc = session.get_cookie("CASTGC", &IDS_URL).ok_or_else(|| {
        error!("登录失败，未获取到 CASTGC Cookie");
        anyhow!("登录失败，未获取到CASTGC Cookie")
    })?;
    debug!("成功获取 CASTGC Cookie");

    // 访问 LNT URL 获取 session cookie
    let lnt_url = LNT_URL.clone();
    let lnt_resp = session.get(lnt_url).await?;
    lnt_resp.error_for_status().map_err(|e| {
        error!(url = ?LNT_URL, error = ?e, "访问 LNT URL 返回非成功状态码");
        e
    })?;

    let lnt = session.get_cookie("session", &LNT_URL).ok_or_else(|| {
        error!("登录失败，未获取到 LNT session Cookie");
        anyhow!("登录失败，未获取到session")
    })?;
    debug!("成功获取 LNT session Cookie");

    info!("二维码登录流程完成，成功获取登录数据");
    Ok(LoginData {
        castgc: castgc.to_string(),
        lnt: lnt.to_string(),
    })
}

pub async fn request_qrcode_castgc(session: &SessionClient, data: LoginRequest) -> Result<String> {
    info!(url = data.url, "发送二维码登录请求以获取 CASTGC");
    session
        .post(&data.url, &data.body)
        .await?
        .error_for_status_ref()
        .map_err(|e| {
            error!(url = data.url, error = ?e, "二维码登录请求返回非成功状态码");
            e
        })?;

    let castgc = session.get_cookie("CASTGC", &IDS_URL).ok_or_else(|| {
        error!("登录失败，未获取到 CASTGC Cookie");
        anyhow!("登录失败，未获取到CASTGC Cookie")
    })?;

    info!("成功通过二维码登录流程获取 CASTGC");
    Ok(castgc.to_string())
}

pub async fn get_qrcode_id(session: &SessionClient) -> Result<(String, LoginRequest)> {
    let data = get_qrcode(session).await?;

    let qrcode_id = data.body.qrcode_id.clone().ok_or_else(|| {
        error!("从 LoginRequest 中获取 qrcode_id 失败");
        anyhow!("二维码生成失败")
    })?;

    Ok((qrcode_id, data))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::api::network::SessionClient;
    use crate::api::xmu_service::jw::Zzy;
    use crate::api::xmu_service::lnt::Profile;

    use super::*;
    use anyhow::Result;
    use anyhow::anyhow;

    #[tokio::test]
    async fn test_qrcode() -> Result<()> {
        let session = SessionClient::new();

        let data = get_qrcode(&session).await?;

        println!("数据：{}", serde_json::to_string(&data)?);

        let qrcode_id = data
            .body
            .qrcode_id
            .clone()
            .ok_or(anyhow!("二维码生成失败"))?;

        let qrcode_url = format!(
            "https://ids.xmu.edu.cn/authserver/qrCode/getCode?uuid={}",
            qrcode_id
        );

        println!("请使用企业微信扫码登录：{}", qrcode_url);

        wait_qrcode(&session, &qrcode_id).await?;

        let login_data = Arc::new(request_qrcode(&session, data).await?);

        println!("登录成功！");

        let profile = Profile::get(&login_data.lnt).await?;

        println!("用户信息：{:?}", profile);

        let data = Zzy::get(&login_data.castgc, &profile.user_no).await?;

        let zzy_profile = data.get_profile()?;

        println!(
            "信息:{} 转入学院:{:?}",
            zzy_profile.entry_year, zzy_profile.trans_dept
        );

        Ok(())
    }
}
