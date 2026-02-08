use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking;
use tracing_subscriber::{Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_logger(path: &str, level: LevelFilter) -> non_blocking::WorkerGuard {
    let file_appender = tracing_appender::rolling::daily(path, "xmu_assistant_bot");
    let (file_writer, guard) = non_blocking(file_appender);

    let stdout_layer = fmt::layer()
        .with_ansi(true)
        .with_thread_ids(true)
        .with_target(true)
        .with_filter(level);

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_writer(file_writer)
        .with_filter(LevelFilter::TRACE);

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    guard
}
