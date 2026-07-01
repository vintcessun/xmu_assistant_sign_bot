#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use anyhow::{Context, Result};
use std::time::Duration;
use tracing::{info, level_filters::LevelFilter};
use xmu_assistant_bot::abi::client::client_init;
use xmu_assistant_bot::abi::router::handler::Router;
use xmu_assistant_bot::*;
// const LOG_PATH: &str = "logs";

// mimalloc option 序号（libmimalloc-sys 0.1.47 未导出具名常量；mimalloc v2/v3 的
// `mi_option_e` 枚举中这些序号一致，见 mimalloc.h）。
const MI_OPTION_PURGE_DECOMMITS: i32 = 5; // purge 时 decommit，真正把内存还给 OS（默认 1）
const MI_OPTION_PURGE_DELAY: i32 = 15; // purge 延迟(ms)，默认 10；设 0 立即归还
const MI_OPTION_ARENA_PURGE_MULT: i32 = 24; // arena purge 延迟倍数，默认 ×10；设 1 让大块内存也尽快归还

/// 让 mimalloc 更积极地把空闲内存归还操作系统，避免 RSS 持续增长到数 GB。
fn tune_allocator() {
    // SAFETY: 仅设置 mimalloc 全局选项，运行期可安全调用。
    unsafe {
        libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DECOMMITS, 1);
        libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DELAY, 0);
        libmimalloc_sys::mi_option_set(MI_OPTION_ARENA_PURGE_MULT, 1);
    }
}

/// 周期性强制回收，迫使 mimalloc 将抽象层缓存/废弃段的空闲内存归还 OS。
fn spawn_memory_reclaimer() {
    tokio::spawn(async {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            // SAFETY: mi_collect 是线程安全的全局回收调用。
            unsafe { libmimalloc_sys::mi_collect(true) };
        }
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    // 尽早配置分配器，使后续运行期更积极地回收内存。
    tune_allocator();

    info!("正在启动 xmu_assistant_bot...");

    config::ensure_dir(config::DATA_DIR);

    //config::ensure_dir(LOG_PATH);
    //let _guard = logger::init_logger_with_file(LOG_PATH, LevelFilter::INFO);
    logger::init_logger_without_file(LevelFilter::INFO);

    // 启动周期性内存回收任务。
    spawn_memory_reclaimer();

    let napcat_config = config::get_napcat_config();
    info!(napcat_host = ?napcat_config.host, napcat_port = ?napcat_config.port, "尝试初始化 ABI 并连接到 Napcat 服务...");

    let mut router = abi::run(napcat_config)
        .await
        .context("初始化 ABI 并连接到 Napcat 失败")?;

    let client = router.get_client();

    client_init(client);

    info!("Napcat ABI 初始化成功，等待消息...");

    web::start().await.context("启动 Web 服务失败")?;
    info!("Web 服务启动成功");

    router.run().await;

    info!("程序已正常退出");
    Ok(())
}
