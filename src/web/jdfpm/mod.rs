//! 绩点分排名查询页：受口令保护地列出成绩范围、申请绩点计算、
//! 下载绩点证明 PDF 并把正文回显到网页。
//!
//! 口令由 [`crate::web::guard`] 模块负责，本模块只在每个接口入口校验访问令牌。

mod expose;
pub mod task;

use axum::{Router, routing::get};

pub fn jdfpm_router() -> Router {
    let router = Router::new();
    let router = expose::task_router(router);
    main_router(router)
}

fn main_router(router: Router) -> Router {
    router.route("/status", get(status_handler))
}

async fn status_handler() -> &'static str {
    "Web API Jdfpm Module Is Running"
}

#[cfg(test)]
mod tests {
    use super::jdfpm_router;
    use crate::api::xmu_service::testenv;
    use crate::web::guard::guard_router;
    use anyhow::Result;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
    };
    use rand::RngExt;
    use tower::ServiceExt;

    /// 路由冲突（如 `/{id}` 与 `/status` 撞车）在 axum 里是构造期 panic，
    /// 会直接打挂整个 Web 服务，所以在测试里先把路由装配一遍。
    #[test]
    fn routers_assemble_without_conflict() {
        let _router: Router = Router::new()
            .nest("/jdfpm", jdfpm_router())
            .nest("/guard", guard_router());
    }

    async fn call(
        router: &Router,
        method: &str,
        uri: String,
        token: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, Vec<u8>) {
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
        let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap();
        (status, bytes.to_vec())
    }

    async fn call_json(
        router: &Router,
        method: &str,
        uri: String,
        token: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let (status, bytes) = call(router, method, uri, token, body).await;
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// 全链路：设长期口令 -> 建查询页 -> 解锁 -> 列成绩范围 -> 查询 ->
    /// 断言**排名**跟着查询一起回来了 -> 下载证明 PDF。
    ///
    /// 排名是这条链路的重点：教务的 JSON 接口一律返回 `*`，只有证明 PDF 里才写明，
    /// 所以查询响应里必须带上 certificate.facts.rank，否则网页上就是没有排名。
    #[tokio::test(flavor = "multi_thread")]
    async fn full_flow_shows_rank() -> Result<()> {
        let Some(castgc) = testenv::castgc() else {
            return testenv::skipped(module_path!());
        };

        let router = Router::new()
            .nest("/guard", guard_router())
            .nest("/jdfpm", jdfpm_router());
        let qq = -rand::rng().random_range(1..1_000_000_000i64);
        let password = "full-test-password";

        // 1. 用一次性链接设置长期口令
        let setup = crate::web::guard::create_setup(qq);
        let (status, _) = call_json(
            &router,
            "POST",
            format!("/guard/setup/{}/save", setup.id),
            None,
            Some(serde_json::json!({ "password": password })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "设置口令失败");

        // 2. 用真实教务会话建查询页
        let client = crate::api::xmu_service::jw::get_castgc_client(castgc);
        let session = super::task::create_session(qq, client)?;
        let id = session.id.clone();

        // 3. 没有令牌一律 401
        let (status, _) = call_json(
            &router,
            "GET",
            format!("/jdfpm/api/{id}/ranges"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // 4. 用口令解锁
        let (status, body) = call_json(
            &router,
            "POST",
            format!("/guard/{id}/unlock"),
            None,
            Some(serde_json::json!({ "password": password })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "解锁失败: {body}");
        let token = body["token"].as_str().unwrap().to_owned();

        // 5. 成绩范围
        let (status, ranges) = call_json(
            &router,
            "GET",
            format!("/jdfpm/api/{id}/ranges"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "取成绩范围失败: {ranges}");
        let ranges = ranges.as_array().cloned().unwrap_or_default();
        assert!(!ranges.is_empty(), "教务没有返回任何成绩范围");
        println!("成绩范围 {} 个", ranges.len());

        // 6. 查询第一个范围 —— 证明与排名应当随查询一起回来
        let range = &ranges[0];
        let (status, result) = call_json(
            &router,
            "POST",
            format!("/jdfpm/api/{id}/query"),
            Some(&token),
            Some(serde_json::json!({
                "range_wid": range["wid"],
                "range_name": range["name"],
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "查询失败: {result}");
        println!("查询结果: {}", serde_json::to_string_pretty(&result)?);

        let certificate = &result["certificate"];
        assert!(
            !certificate.is_null(),
            "查询没有带回绩点证明，certificate_error={}",
            result["certificate_error"]
        );

        let rank = certificate["facts"]["rank"].as_str();
        assert!(
            rank.is_some_and(|r| !r.is_empty()),
            "证明正文里没提取到排名，正文如下:
{}",
            certificate["text"].as_str().unwrap_or_default()
        );
        println!(
            "==> 专业绩点排名 {} / {}",
            rank.unwrap(),
            certificate["facts"]["major_size"].as_str().unwrap_or("?")
        );
        assert!(
            certificate["text"]
                .as_str()
                .is_some_and(|text| text.contains("厦门大学")),
            "证明正文不完整"
        );

        // 7. PDF 能下载
        let wid = certificate["wid"].as_str().unwrap();
        let (status, pdf) = call(
            &router,
            "GET",
            format!(
                "/jdfpm/{id}/certificate.pdf?wid={}&token={}",
                urlencoding::encode(wid),
                urlencoding::encode(&token)
            ),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "下载证明 PDF 失败");
        assert!(pdf.starts_with(b"%PDF"), "下载到的不是 PDF");
        println!("证明 PDF {} 字节", pdf.len());

        crate::web::guard::forget_secret(qq).await.ok();
        Ok(())
    }
}
