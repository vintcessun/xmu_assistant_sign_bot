use anyhow::Result;
use std::time::Duration;

/// 定义任务接口
#[async_trait::async_trait]
pub trait TimeTask: Send + Sync + 'static {
    type Output: Send + Sync + Clone + 'static;

    /// 任务运行间隔
    fn interval(&self) -> Duration;

    /// 任务名称（用于日志）
    fn name(&self) -> &'static str;

    /// 执行任务的具体逻辑
    async fn run(&self) -> Result<Self::Output>;
}
