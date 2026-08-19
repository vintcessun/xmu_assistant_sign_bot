use crate::web::guard::task::{
    Guard, MIN_PASSWORD_LEN, get_guard, get_setup, secret_updated_at, verify,
};
use axum::{
    Json, Router,
    extract::{FromRequestParts, Path},
    http::{StatusCode, request::Parts},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::trace;

include!(concat!(env!("OUT_DIR"), "/web_data.rs"));

/// 从 `Authorization: Bearer <token>` 或 `?token=<token>` 里取出访问令牌。
///
/// 页面上的 XHR 走请求头；`<a href>` 直接下载 PDF 这种没法加头的场景走查询串。
pub struct GuardToken(pub String);

impl<S: Send + Sync> FromRequestParts<S> for GuardToken {
    type Rejection = (StatusCode, Json<serde_json::Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let from_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .map(str::to_owned);

        let token = from_header.or_else(|| {
            let query = parts.uri.query()?;
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "token")
                .map(|(_, value)| value.into_owned())
                .filter(|token| !token.is_empty())
        });

        match token {
            Some(token) => Ok(Self(token)),
            None => Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "detail": "缺少访问令牌，请先输入口令解锁" })),
            )),
        }
    }
}

#[derive(Deserialize)]
struct IdPath {
    id: String,
}

#[derive(Deserialize)]
struct PasswordRequest {
    password: String,
}

#[derive(Serialize)]
struct SetupStateResponse {
    qq: i64,
    /// true = 这次是刷新（该 QQ 之前设过口令），false = 首次设置。
    refresh: bool,
    seconds_left: u64,
    min_password_len: usize,
}

#[derive(Serialize)]
struct GuardStateResponse {
    qq: i64,
    unlocked: bool,
    locked: bool,
    seconds_left: u64,
    lock_seconds_left: u64,
    /// 口令最近一次设置时间；页面用它提示"口令是什么时候设的"。
    password_updated_at: Option<u64>,
}

#[derive(Serialize)]
struct UnlockResponse {
    ok: bool,
    message: String,
    token: String,
    seconds_left: u64,
}

fn gone(detail: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::GONE,
        Json(serde_json::json!({ "detail": detail })),
    )
}

// ------------------------------------------------------------ 设置口令页面

async fn setup_page_handler(Path(params): Path<IdPath>) -> impl IntoResponse {
    match get_setup(&params.id) {
        Some(task) => Html(
            SETPWD_HTML
                .replace("__QQ_ID__", &task.qq.to_string())
                .replace(
                    "__SETUP_ID_JS__",
                    &serde_json::to_string(&task.id).unwrap_or_else(|_| "\"\"".into()),
                ),
        )
        .into_response(),
        None => (StatusCode::NOT_FOUND, Html(NOT_FOUND_HTML)).into_response(),
    }
}

async fn setup_state_handler(Path(params): Path<IdPath>) -> impl IntoResponse {
    let Some(task) = get_setup(&params.id) else {
        return gone("这个设置链接不存在、已用过或已过期，请重新发送 /setpwd").into_response();
    };

    Json(SetupStateResponse {
        qq: task.qq,
        refresh: task.refresh,
        seconds_left: task.seconds_left(),
        min_password_len: MIN_PASSWORD_LEN,
    })
    .into_response()
}

/// 保存永久口令。链接一次性，保存成功即失效。
async fn setup_save_handler(
    Path(params): Path<IdPath>,
    Json(payload): Json<PasswordRequest>,
) -> impl IntoResponse {
    let Some(task) = get_setup(&params.id) else {
        return gone("这个设置链接不存在、已用过或已过期，请重新发送 /setpwd").into_response();
    };

    match task.save(&payload.password).await {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "message": "口令已保存，长期有效。以后所有需要口令的页面都用它解锁；要更换请重新发送 /setpwd。"
        }))
        .into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "detail": e.to_string() })),
        )
            .into_response(),
    }
}

// -------------------------------------------------------------- 受保护页面

/// 锁屏状态：只暴露"是否已解锁 / 是否被锁定 / 还剩多久"，不泄漏任何口令信息。
async fn state_handler(Path(params): Path<IdPath>) -> impl IntoResponse {
    trace!(guard_id = params.id, "查询页面口令状态");
    let Some(guard) = get_guard(&params.id) else {
        return gone("页面不存在或已过期，请重新申请").into_response();
    };

    let status = guard.status();
    Json(GuardStateResponse {
        qq: guard.qq,
        unlocked: status.unlocked,
        locked: status.locked,
        seconds_left: status.seconds_left,
        lock_seconds_left: status.lock_seconds_left,
        password_updated_at: secret_updated_at(guard.qq),
    })
    .into_response()
}

/// 校验口令并签发访问令牌；成功会顶掉上一位访问者手里的令牌。
async fn unlock_handler(
    Path(params): Path<IdPath>,
    Json(payload): Json<PasswordRequest>,
) -> impl IntoResponse {
    let Some(guard) = get_guard(&params.id) else {
        return gone("页面不存在或已过期，请重新申请").into_response();
    };

    match guard.unlock(&payload.password).await {
        Ok(token) => Json(UnlockResponse {
            ok: true,
            message: "口令正确，已开启本次访问".to_owned(),
            token: token.to_string(),
            seconds_left: guard.seconds_left(),
        })
        .into_response(),
        Err(e) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "detail": e.to_string() })),
        )
            .into_response(),
    }
}

/// 主动退出：注销当前令牌，页面回到锁屏。
async fn lock_handler(Path(params): Path<IdPath>, token: GuardToken) -> impl IntoResponse {
    let Some(guard) = verify(&params.id, &token.0) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "detail": "访问令牌无效或已被顶下线" })),
        )
            .into_response();
    };

    guard.revoke_token();
    StatusCode::NO_CONTENT.into_response()
}

/// 供被保护模块复用：校验令牌，失败时给出统一的 401 响应。
pub fn require(id: &str, token: &str) -> Result<Arc<Guard>, (StatusCode, Json<serde_json::Value>)> {
    verify(id, token).ok_or((
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "detail": "访问令牌无效、已过期或已被他人顶下线，请重新输入口令" })),
    ))
}

pub fn task_router(router: Router) -> Router {
    router
        // 设置 / 刷新永久口令（一次性链接）
        .route("/setup/{id}", get(setup_page_handler))
        .route("/setup/{id}/state", get(setup_state_handler))
        .route("/setup/{id}/save", post(setup_save_handler))
        // 受保护页面的锁屏
        .route("/{id}/state", get(state_handler))
        .route("/{id}/unlock", post(unlock_handler))
        .route("/{id}/lock", post(lock_handler))
}
