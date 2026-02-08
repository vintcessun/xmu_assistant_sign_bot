use anyhow::Result;
use axum::{Router, routing::get};

pub mod file;
pub mod md;

use file::file_router;
use md::md_router;

const URL: &str = "https://zzy.vintces.icu";
const LOCAL: &str = "0.0.0.0:3080";

pub async fn start() -> Result<()> {
    let app = router();

    let listener = tokio::net::TcpListener::bind(LOCAL).await?;

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    Ok(())
}

fn router() -> Router {
    let router = Router::new();
    let router = router.nest("/file", file_router());
    let router = router.nest("/md", md_router()); // 嵌套 md 模块路由
    main_router(router)
}

fn main_router(router: Router) -> Router {
    router.route("/status", get(status_handler))
}

async fn status_handler() -> &'static str {
    "Web API is running"
}
