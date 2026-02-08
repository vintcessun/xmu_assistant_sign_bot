#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use xmu_assistant_bot::*;

use anyhow::{Context, Result};
use tracing::{info, level_filters::LevelFilter};
use xmu_assistant_bot::abi::router::handler::Router;

const LOG_PATH: &str = "logs";

#[tokio::main]
async fn main() -> Result<()> {
    info!("正在启动 xmu_assistant_bot...");

    config::ensure_dir(LOG_PATH);
    config::ensure_dir(config::DATA_DIR);

    let _guard = logger::init_logger(LOG_PATH, LevelFilter::INFO);

    let napcat_config = config::get_napcat_config();
    info!(napcat_host = ?napcat_config.host, napcat_port = ?napcat_config.port, "尝试初始化 ABI 并连接到 Napcat 服务...");

    let mut router = abi::run(napcat_config)
        .await
        .context("初始化 ABI 并连接到 Napcat 失败")?;

    info!("Napcat ABI 初始化成功，等待消息...");

    web::start().await.context("启动 Web 服务失败")?;
    info!("Web 服务启动成功");

    router.run().await;

    info!("程序已正常退出");
    Ok(())
}
