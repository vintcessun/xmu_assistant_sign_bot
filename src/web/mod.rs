use anyhow::Result;
use axum::{Router, routing::get};
use tracing::{error, info};

pub mod file;
pub mod md;
pub mod rollcall;

use file::file_router;
use md::md_router;
use rollcall::rollcall_router;

const URL: &str = "https://zzy.vintces.icu";
const LOCAL: &str = "0.0.0.0:3080";

pub async fn start() -> Result<()> {
    info!(local = %LOCAL, "正在启动 Web API 服务器");
    let app = router();

    let listener = tokio::net::TcpListener::bind(LOCAL).await?;
    info!(local = %LOCAL, "Web API 监听地址绑定成功");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!(error = ?e, "Web API 服务器运行失败");
        }
        info!("Web API 服务器已停止运行");
    });

    Ok(())
}

fn router() -> Router {
    let router = Router::new();
    let router = router.nest("/file", file_router());
    let router = router.nest("/md", md_router());
    let router = router.nest("/rollcall", rollcall_router());
    main_router(router)
}

fn main_router(router: Router) -> Router {
    router
        .route("/status", get(status_handler))
        .route("/index", get(index_handler))
}

async fn status_handler() -> &'static str {
    "Web API is running"
}

include!(concat!(env!("OUT_DIR"), "/web_data.rs"));

async fn index_handler() -> &'static str {
    INDEX_HTML
}
