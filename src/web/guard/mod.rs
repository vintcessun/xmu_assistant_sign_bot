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

use crate::web::URL;

/// 设置/刷新口令的一次性链接地址。
pub fn setup_url(id: &str) -> String {
    format!("{URL}/guard/setup/{id}")
}

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
    use super::{
        guard_router,
        task::{Guard, create_setup},
    };
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
    };
    use rand::RngExt;
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

    /// 走真实路由把「一次性链接设长期口令 → 用它解锁受保护页面 → 顶下线 → 登出」
    /// 跑一遍，覆盖网页实际依赖的那几个 HTTP 契约。
    // 用到落盘的 HotTable，其懒初始化会阻塞，单线程 runtime 上会把自己堵死。
    #[tokio::test(flavor = "multi_thread")]
    async fn permanent_password_setup_and_unlock_over_http() {
        let router = Router::new().nest("/guard", guard_router());
        // 口令落盘，固定 QQ 会被上一次 cargo test 的残留污染。
        let qq = -rand::rng().random_range(1..1_000_000_000i64);

        // --- 设置长期口令（一次性链接）---
        let setup = create_setup(qq);
        let setup_id = setup.id.clone();

        let (status, body) = call(
            &router,
            "GET",
            format!("/guard/setup/{setup_id}/state"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["qq"], qq);
        assert_eq!(body["refresh"], false);

        // 太短的口令要被拒，且不消耗链接
        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/setup/{setup_id}/save"),
            None,
            Some(serde_json::json!({ "password": "abc" })),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);

        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/setup/{setup_id}/save"),
            None,
            Some(serde_json::json!({ "password": "correct-horse" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        // 一次性：同一条链接不能再用
        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/setup/{setup_id}/save"),
            None,
            Some(serde_json::json!({ "password": "someone-else" })),
        )
        .await;
        assert_eq!(status, StatusCode::GONE);

        // --- 用这个长期口令解锁受保护页面 ---
        let guard = Guard::create(qq).expect("已设口令，应能建页面");
        let id = guard.id.clone();

        let (status, body) = call(&router, "GET", format!("/guard/{id}/state"), None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["unlocked"], false);
        assert!(body["password_updated_at"].is_number(), "应能看到口令设置时间");

        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/{id}/unlock"),
            None,
            Some(serde_json::json!({ "password": "wrong-password" })),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, body) = call(
            &router,
            "POST",
            format!("/guard/{id}/unlock"),
            None,
            Some(serde_json::json!({ "password": "correct-horse" })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let first_token = body["token"].as_str().unwrap().to_owned();

        // 再解锁一次 -> 换发新令牌，旧的立刻作废
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

        let (status, _) = call(
            &router,
            "POST",
            format!("/guard/{id}/lock"),
            Some(&second_token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        super::task::remove_guard(&id);
        super::task::forget_secret(qq).await.ok();
    }
}
