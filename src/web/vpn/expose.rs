use crate::api::xmu_service::securelink::{extract_code, vpn_remotes};
use crate::web::vpn::task::{VpnFlow, get_flow};
use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, warn};

include!(concat!(env!("OUT_DIR"), "/web_data.rs"));

#[derive(Deserialize)]
struct FlowPath {
    id: String,
}

#[derive(Deserialize)]
struct SubmitRequest {
    id: String,
    callback_url: String,
}

#[derive(Deserialize)]
struct RefreshRequest {
    id: String,
}

#[derive(Serialize)]
struct SubmitResponse {
    ok: bool,
    message: String,
    exit_ip: Option<String>,
    remotes: Vec<String>,
}

async fn vpn_page_handler(Path(params): Path<FlowPath>) -> impl IntoResponse {
    match get_flow(&params.id) {
        Some(flow) => Html(render_page(&flow)).into_response(),
        None => (StatusCode::NOT_FOUND, Html(NOT_FOUND_HTML)).into_response(),
    }
}

/// 提交登录后浏览器最终跳转到的 callback URL，服务端从中取 code 完成 SSO。
async fn submit_handler(Json(req): Json<SubmitRequest>) -> impl IntoResponse {
    let Some(flow) = get_flow(&req.id) else {
        return Json(SubmitResponse {
            ok: false,
            message: "链接已失效，请重新 /flushvpn".into(),
            exit_ip: None,
            remotes: Vec::new(),
        });
    };

    let Some(code) = extract_code(&req.callback_url) else {
        return Json(SubmitResponse {
            ok: false,
            message: "callback URL 中没有 code 参数，请粘贴完整的最终跳转地址".into(),
            exit_ip: None,
            remotes: Vec::new(),
        });
    };

    let mut api = flow.api.lock().await;
    if let Err(e) = api.complete_sso(&flow.auth_name, &code).await {
        warn!(qq = flow.qq, error = ?e, "SecureLink SSO 完成失败");
        return Json(SubmitResponse {
            ok: false,
            message: format!("登录失败: {e}"),
            exit_ip: None,
            remotes: Vec::new(),
        });
    }

    // 登录成功后顺手拉一次 VPN 配置与出口 IP 作为“VPN 内容”。
    let remotes = match api.get_vpn_config().await {
        Ok(cfg) => vpn_remotes(&cfg)
            .into_iter()
            .map(|(h, p)| format!("{h}:{p}"))
            .collect(),
        Err(e) => {
            debug!(error = ?e, "拉取 VPN 配置失败（不影响登录）");
            Vec::new()
        }
    };
    drop(api);

    // 数据面已移除；隧道出口 IP 暂不可得（将来接入用户态 OpenVPN/SOCKS5 后再填）。
    let exit_ip: Option<String> = None;

    Json(SubmitResponse {
        ok: true,
        message: "SecureLink 登录成功，会话已刷新并保存".into(),
        exit_ip,
        remotes,
    })
}

/// 刷新展示当前 VPN 内容（远端列表）与出口 IP。
async fn refresh_ip_handler(Json(req): Json<RefreshRequest>) -> impl IntoResponse {
    let Some(flow) = get_flow(&req.id) else {
        return Json(SubmitResponse {
            ok: false,
            message: "链接已失效，请重新 /flushvpn".into(),
            exit_ip: None,
            remotes: Vec::new(),
        });
    };

    let mut api = flow.api.lock().await;
    let remotes = match api.get_vpn_config().await {
        Ok(cfg) => vpn_remotes(&cfg)
            .into_iter()
            .map(|(h, p)| format!("{h}:{p}"))
            .collect(),
        Err(e) => {
            debug!(error = ?e, "刷新 VPN 配置失败");
            Vec::new()
        }
    };
    drop(api);

    // 数据面已移除；隧道出口 IP 暂不可得（将来接入用户态 OpenVPN/SOCKS5 后再填）。
    let exit_ip: Option<String> = None;
    Json(SubmitResponse {
        ok: true,
        message: "已刷新".into(),
        exit_ip,
        remotes,
    })
}

fn render_page(flow: &Arc<VpnFlow>) -> String {
    VPN_HTML
        .replace("__LOGIN_QR__", &qr_svg(&flow.login_url))
        .replace("__QQ_ID__", &flow.qq.to_string())
        .replace(
            "__VPN_ID_JS__",
            &serde_json::to_string(&flow.id).unwrap_or_else(|_| "\"\"".into()),
        )
        .replace(
            "__LOGIN_URL_JS__",
            &serde_json::to_string(&flow.login_url).unwrap_or_else(|_| "\"\"".into()),
        )
}

fn qr_svg(data: &str) -> String {
    use qrcode::render::svg;
    match qrcode::QrCode::new(data.as_bytes()) {
        Ok(code) => code
            .render()
            .min_dimensions(220, 220)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
        Err(_) => "<p>二维码生成失败，请使用下方链接</p>".to_string(),
    }
}

pub fn vpn_router() -> Router {
    Router::new()
        .route("/{id}", get(vpn_page_handler))
        .route("/submit", post(submit_handler))
        .route("/refresh_ip", post(refresh_ip_handler))
}
