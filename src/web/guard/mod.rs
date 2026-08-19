//! 访问口令模块：给需要保护的网页加一道“申请口令 → 输入口令 → 换取访问令牌”的门。
//!
//! 口令有两种来源：调用方自己指定，或由本模块随机生成（[`PasswordMode`]）。
//! 口令只以「随机盐 + 多轮 SHA-256」的摘要形式留在内存里，明文只在创建时返回一次。
//! 验证成功会轮换访问令牌，因此同一时刻只有最后一次输入口令的人能继续访问页面。

mod expose;
pub mod task;

use axum::{Router, routing::get};

pub use expose::{GuardToken, require};
pub use task::*;

pub fn guard_router() -> Router {
    let router = Router::new();
    let router = expose::task_router(router);
    main_router(router)
}

fn main_router(router: Router) -> Router {
    router.route("/status", get(status_handler))
}

async fn status_handler() -> &'static str {
    "Web API Guard Module Is Running"
}

#[cfg(test)]
mod tests {
    use super::{guard_router, task::Guard};
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    async fn call(
        router: &Router,
        method: &str,
        uri: String,
        token: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        let request = match body {
            Some(json) => builder
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };

        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// 走真实路由把「设置口令 → 解锁 → 顶下线 → 登出」跑一遍，
    /// 覆盖网页实际依赖的那几个 HTTP 契约。
    #[tokio::test]
    async fn password_setup_and_unlock_over_http() {
        let router = Router::new().nest("/guard", guard_router());
        let guard = Guard::create(10086);
        let id = guard.id.clone();
        let state_uri = format!("/guard/{id}/state");

        // 初始：还没设过口令
        let (status, body) = call(&router, "GET", state_uri.clone(), None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["configured"], false);
        assert_eq!(body["unlocked"], false);
        assert_eq!(body["qq"], 10086);

        // 太短的口令要被拒
        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/{id}/setup"),
            None,
            Some(serde_json::json!({ "password": "abc" })),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);

        // 设置口令，当场拿到令牌
        let (status, body) = call(
            &router,
            "POST",
            format!("/guard/{id}/setup"),
            None,
            Some(serde_json::json!({ "password": "correct-horse" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let first_token = body["token"].as_str().unwrap().to_owned();
        assert!(!first_token.is_empty());

        let (_, body) = call(&router, "GET", state_uri.clone(), None, None).await;
        assert_eq!(body["configured"], true);
        assert_eq!(body["unlocked"], true);

        // 口令只能设置一次
        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/{id}/setup"),
            None,
            Some(serde_json::json!({ "password": "someone-else" })),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);

        // 错口令
        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/{id}/unlock"),
            None,
            Some(serde_json::json!({ "password": "wrong-password" })),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // 对口令 -> 换发新令牌，旧令牌立刻作废
        let (status, body) = call(
            &router,
            "POST",
            format!("/guard/{id}/unlock"),
            None,
            Some(serde_json::json!({ "password": "correct-horse" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let second_token = body["token"].as_str().unwrap().to_owned();
        assert_ne!(first_token, second_token);

        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/{id}/lock"),
            Some(&first_token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "旧令牌应已被顶下线");

        // 新令牌可以正常登出
        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/{id}/lock"),
            Some(&second_token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (_, body) = call(&router, "GET", state_uri, None, None).await;
        assert_eq!(body["unlocked"], false);

        super::task::remove_guard(&id);
    }
}
