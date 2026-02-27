use crate::logic::rollcall;
use axum::{
    Json, Router,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/web_data.rs"));

pub fn task_router(router: Router) -> Router {
    router
        // /rollcall/info - 获取API信息接口
        .route("/info", get(info_handler))
        // /rollcall/qr - 扫码传入二维码信息接口
        .route("/qr", post(qr_handler))
        // /rollcall/sign - 获取签到信息接口
        .route("/sign", post(content_handler))
        // /rollcall/auto_sign - 获取自动签到信息接口
        .route("/auto_sign", post(auto_sign_handler))
        // /rollcall/push_sign - 获取推送签到信息接口
        .route("/push_sign", post(push_sign_handler))
        // /rollcall/spec_sign - 获取特定签到信息接口
        .route("/spec_sign", post(spec_sign_handler))
}

async fn info_handler() -> Html<&'static str> {
    Html(ROLLCALL_HTML)
}

#[derive(Deserialize)]
struct QrRequest {
    content: String,
}

#[derive(Deserialize)]
struct UserRequest {
    user: String,
}

#[derive(Deserialize)]
struct PushRequest {
    rollcall_id: String,
}

#[derive(Deserialize)]
struct SpecRequest {
    user: String,
    rollcall_id: String,
}

#[derive(Serialize)]
struct MessageResponse<T> {
    message: T,
}

async fn qr_handler(Json(payload): Json<QrRequest>) -> impl IntoResponse {
    match rollcall::qr_sign_request(&payload.content).await {
        Ok(result) => (StatusCode::OK, Json(MessageResponse { message: result })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "detail": format!("处理失败: {}", e) })),
        )
            .into_response(),
    }
}

async fn content_handler(Json(payload): Json<UserRequest>) -> impl IntoResponse {
    let qq = match payload.user.parse::<i64>() {
        Ok(qq) => qq,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "detail": "无效的QQ号" })),
            )
                .into_response();
        }
    };
    match rollcall::sign_request(qq).await {
        Ok(result) => (StatusCode::OK, Json(MessageResponse { message: result })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "detail": format!("处理失败: {}", e) })),
        )
            .into_response(),
    }
}

async fn auto_sign_handler(Json(payload): Json<UserRequest>) -> impl IntoResponse {
    let qq = match payload.user.parse::<i64>() {
        Ok(qq) => qq,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "detail": "无效的QQ号" })),
            )
                .into_response();
        }
    };
    match rollcall::auto_sign_request(qq).await {
        Ok(result) => (StatusCode::OK, Json(MessageResponse { message: result })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "detail": format!("处理失败: {}", e) })),
        )
            .into_response(),
    }
}

async fn push_sign_handler(Json(payload): Json<PushRequest>) -> impl IntoResponse {
    let rollcall_id = match payload.rollcall_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "detail": "无效的签到ID" })),
            )
                .into_response();
        }
    };
    match rollcall::push_sign_request(rollcall_id).await {
        Ok(result) => (StatusCode::OK, Json(MessageResponse { message: result })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "detail": format!("处理失败: {}", e) })),
        )
            .into_response(),
    }
}

async fn spec_sign_handler(Json(payload): Json<SpecRequest>) -> impl IntoResponse {
    let qq = match payload.user.parse::<i64>() {
        Ok(qq) => qq,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "detail": "无效的QQ号" })),
            )
                .into_response();
        }
    };
    let rollcall_id = match payload.rollcall_id.parse::<i64>() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "detail": "无效的签到ID" })),
            )
                .into_response();
        }
    };
    match rollcall::spec_sign_request(qq, rollcall_id).await {
        Ok(result) => (StatusCode::OK, Json(MessageResponse { message: result })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "detail": format!("处理失败: {}", e) })),
        )
            .into_response(),
    }
}
