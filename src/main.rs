#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use xmu_assistant_bot::*;

use anyhow::Result;
use tracing::level_filters::LevelFilter;
use xmu_assistant_bot::abi::router::handler::Router;

const LOG_PATH: &str = "logs";

#[tokio::main]
async fn main() -> Result<()> {
    config::ensure_dir(LOG_PATH);
    config::ensure_dir(config::DATA_DIR);

    let _guard = logger::init_logger(LOG_PATH, LevelFilter::TRACE);

    let mut router = abi::run(config::get_napcat_config())
        .await
        .expect("Failed to initialize ABI and connect to Napcat");

    web::start().await?;

    router.run().await;

    Ok(())
}
