use super::r#trait::TimeTask;
use anyhow::Result;
use arc_swap::ArcSwap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

/// 泛型管理类：负责存储结果、重试和定时调度
pub struct TaskRunner<T: TimeTask> {
    task: T,
    // 存储最新的成功结果
    value: ArcSwap<Option<T::Output>>,
}

impl<T: TimeTask> TaskRunner<T> {
    pub fn new(task: T) -> Arc<Self> {
        let runner = Arc::new(Self {
            task,
            value: ArcSwap::new(Arc::new(None)),
        });

        // 在创建时直接启动后台协程
        let runner_clone = runner.clone();
        tokio::spawn(async move {
            runner_clone.maintain().await;
        });

        runner
    }

    /// 获取当前最新的数据（如果从未成功则尝试运行一次）。
    ///
    /// 兜底那次也会写回，否则启动窗口里每个调用方都会各跑一遍任务，
    /// 而且拿到的还是彼此独立的副本——对「就地增量维护」的结果（如选课索引）
    /// 意味着改动写进了一个马上被丢掉的对象里。
    pub async fn get_latest(&self) -> Result<T::Output> {
        if let Some(val) = self.value.load().as_ref() {
            return Ok(val.clone());
        }
        let val = self.task.run().await?;
        self.value.store(Arc::new(Some(val.clone())));
        Ok(val)
    }

    pub async fn force_update(&self) -> Result<T::Output> {
        let new_val = self.task.run().await?;
        self.value.store(Arc::new(Some(new_val.clone())));
        Ok(new_val)
    }

    /// 后台维护逻辑：定时执行 + 错误重试。
    ///
    /// 先跑一次再睡，所以启动时立即有一份结果；每轮重新取一次 `interval()`，
    /// 让实现方能返回随机间隔（`tokio::time::interval` 只会在创建时取一次，
    /// 而且任务跑超时后会连续补齐 tick，对出网任务来说是雪上加霜）。
    async fn maintain(&self) {
        loop {
            // 尝试执行任务，如果失败则进入重试逻辑
            match self.task.run().await {
                Ok(new_val) => {
                    self.value.store(Arc::new(Some(new_val)));
                    info!(task = self.task.name(), "任务更新成功");
                }
                Err(e) => {
                    error!(task = self.task.name(), error = ?e, "任务运行失败，准备重试...");
                    self.retry_logic().await;
                }
            }

            tokio::time::sleep(self.task.interval()).await;
        }
    }

    /// 简单的线性重试逻辑
    async fn retry_logic(&self) {
        let retry_interval = Duration::from_secs(5); // 失败后 5 秒重试
        loop {
            tokio::time::sleep(retry_interval).await;
            match self.task.run().await {
                Ok(new_val) => {
                    self.value.store(Arc::new(Some(new_val)));
                    info!(task = self.task.name(), "重试后更新成功");
                    break; // 成功后跳出重试循环，回到正常定时
                }
                Err(e) => {
                    warn!(task = self.task.name(), error = ?e, "重试仍然失败，等待下次重试");
                }
            }
        }
    }
}
