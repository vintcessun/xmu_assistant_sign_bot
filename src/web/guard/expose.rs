use crate::web::guard::task::{Guard, MIN_PASSWORD_LEN, get_guard, verify};
use axum::{
    Json, Router,
    extract::{FromRequestParts, Path},
    http::{StatusCode, request::Parts},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::trace;

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
struct GuardPath {
    id: String,
}

#[derive(Deserialize)]
struct PasswordRequest {
    password: String,
}

#[derive(Serialize)]
struct StateResponse {
    qq: i64,
    /// 口令是否已经被设置过：false 时前端渲染「设置口令」，true 时渲染「输入口令」。
    configured: bool,
    unlocked: bool,
    locked: bool,
    seconds_left: u64,
    /// 还剩多久必须完成口令设置。
    setup_seconds_left: u64,
    lock_seconds_left: u64,
    /// 口令最短长度，供前端做即时校验。
    min_password_len: usize,
}

#[derive(Serialize)]
struct UnlockResponse {
    ok: bool,
    message: String,
    token: String,
    seconds_left: u64,
}

fn gone() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::GONE,
        Json(serde_json::json!({ "detail": "页面口令不存在或已过期，请重新申请" })),
    )
}

/// 锁屏状态：只暴露“是否已解锁 / 是否被锁定 / 还剩多久”，不泄漏任何口令信息。
async fn state_handler(Path(params): Path<GuardPath>) -> impl IntoResponse {
    trace!(guard_id = params.id, "查询页面口令状态");
    let Some(guard) = get_guard(&params.id) else {
        return gone().into_response();
    };

    let status = guard.status();
    Json(StateResponse {
        qq: guard.qq,
        configured: status.configured,
        unlocked: status.unlocked,
        locked: status.locked,
        seconds_left: status.seconds_left,
        setup_seconds_left: status.setup_seconds_left,
        lock_seconds_left: status.lock_seconds_left,
        min_password_len: MIN_PASSWORD_LEN,
    })
    .into_response()
}

/// 首次设置访问口令。只能成功一次，设置者当场拿到令牌。
async fn setup_handler(
    Path(params): Path<GuardPath>,
    Json(payload): Json<PasswordRequest>,
) -> impl IntoResponse {
    let Some(guard) = get_guard(&params.id) else {
        return gone().into_response();
    };

    match guard.setup(&payload.password).await {
        Ok(token) => Json(UnlockResponse {
            ok: true,
            message: "口令已设置，本页面现在归你".to_owned(),
            token: token.to_string(),
            seconds_left: guard.seconds_left(),
        })
        .into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "detail": e.to_string() })),
        )
            .into_response(),
    }
}

/// 校验口令并签发访问令牌；成功会顶掉上一位访问者手里的令牌。
async fn unlock_handler(
    Path(params): Path<GuardPath>,
    Json(payload): Json<PasswordRequest>,
) -> impl IntoResponse {
    let Some(guard) = get_guard(&params.id) else {
        return gone().into_response();
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
async fn lock_handler(Path(params): Path<GuardPath>, token: GuardToken) -> impl IntoResponse {
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
        // 锁屏状态查询
        .route("/{id}/state", get(state_handler))
        // 首次设置访问口令
        .route("/{id}/setup", post(setup_handler))
        // 口令校验并换取访问令牌
        .route("/{id}/unlock", post(unlock_handler))
        // 主动注销访问令牌
        .route("/{id}/lock", post(lock_handler))
}
