use crate::api::qrcode::bridge::QrCodeDetector;
use anyhow::Result;
use crossbeam_channel::{Sender, unbounded};
use std::{sync::LazyLock, thread};
use tokio::sync::oneshot;
use tracing::{info, warn};

// 定义任务
struct QrTask {
    image_data: Vec<u8>,
    reply: oneshot::Sender<Result<Vec<String>>>,
}

#[derive(Debug)]
pub struct QrProcessor {
    tx: Sender<QrTask>,
}

impl Default for QrProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl QrProcessor {
    pub fn new() -> Self {
        let total_cores = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        // 计算逻辑：核心数 - 2，但使用 saturating_sub 避免溢出，再用 max(1) 兜底
        let worker_threads = (total_cores.saturating_sub(2)).max(1);

        let (tx, rx) = unbounded::<QrTask>();

        for i in 0..worker_threads {
            let thread_rx = rx.clone();

            // 启动原生线程，不占 tokio 阻塞池名额
            thread::spawn(move || {
                // 绑定 CPU 核心 (在 macOS 上跳过 affinity)
                #[cfg(not(target_os = "macos"))]
                {
                    let core_ids = vec![i];
                    if let Err(e) = affinity::set_thread_affinity(&core_ids) {
                        warn!(thread_index = i, error = ?e, "Failed to set thread affinity");
                    } else {
                        info!(thread_index = i, core_id = i, "Thread bound to core");
                    }
                }

                // --- 1. 预热阶段 (Warm-up) ---
                // 这里的初始化只在 Bot 启动时发生一次！
                let mut detector = QrCodeDetector::new().expect("OpenCV Init Failed");
                detector.preload().expect("Failed to preload QR code model");

                // --- 2. 循环处理阶段 ---
                // 使用 blocking_recv 监听来自 Tokio 的信号
                while let Ok(task) = thread_rx.recv() {
                    let result = detector.decode_from_bytes(&task.image_data);
                    let _ = task.reply.send(result);
                }
            });
        }
        Self { tx }
    }
}

static QR_CORE: LazyLock<QrProcessor> = LazyLock::new(QrProcessor::new);

// 提供给 Bot 插件调用的异步接口
pub async fn process_image(img: Vec<u8>) -> Result<Vec<String>> {
    let (reply_tx, reply_rx) = oneshot::channel();
    QR_CORE.tx.send(QrTask {
        image_data: img,
        reply: reply_tx,
    })?;
    reply_rx.await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::qrcode::bridge::PRELOAD_QRCODE_JPG;

    #[tokio::test]
    async fn test_preload_qrcode() {
        let results = process_image(PRELOAD_QRCODE_JPG.to_vec())
            .await
            .expect("Should be able to run decoding");

        println!("Decoded results: {:?}", results);
        assert!(!results.is_empty(), "Should detect at least one QR code");
        assert!(
            !results[0].is_empty(),
            "Decoded content should not be empty"
        );
    }
}
