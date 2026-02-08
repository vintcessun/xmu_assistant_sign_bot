use anyhow::Result;
use axum::{Router, routing::get};
use tracing::{error, info};

pub mod file;
pub mod md;

use file::file_router;
use md::md_router;

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
    let router = router.nest("/md", md_router()); // 嵌套 md 模块路由
    main_router(router)
}

fn main_router(router: Router) -> Router {
    router.route("/status", get(status_handler))
}

async fn status_handler() -> &'static str {
    "Web API is running"
}
