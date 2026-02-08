const BASE: &str = "temp";

use super::BASE_DATA_DIR;
use crate::abi::message::file::FileUrl;
use crate::api::storage::file::{FileBackend, FileStorage};
use crate::config::ensure_dir;
use anyhow::Result;
use async_trait::async_trait;
use const_format::concatcp;
use dashmap::DashSet;
use std::time::Duration;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        LazyLock,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, error, info, trace, warn};
use url::Url;

pub static TEMP_DATA_DIR: LazyLock<&'static str> = LazyLock::new(|| {
    let path = concatcp!(BASE_DATA_DIR, "/", BASE);
    info!(path = path, "初始化临时文件存储目录");
    ensure_dir(path);
    path
});

static MANAGER: LazyLock<TempFileManager> = LazyLock::new(TempFileManager::new);

// --- 临时文件管理器 ---
pub struct TempFileManager {
    dir: PathBuf,
    cache: DashSet<String>,
    counter: AtomicUsize,
    tx: mpsc::UnboundedSender<PathBuf>,
}

impl TempFileManager {
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();

        let dir = Path::new(*TEMP_DATA_DIR).to_path_buf();
        let cache = DashSet::new();
        info!(path = ?dir, "初始化临时文件管理器");
        if let Ok(entries) = fs::read_dir(&dir) {
            let mut count = 0;
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    cache.insert(name);
                    count += 1;
                }
            }
            if count > 0 {
                warn!(count = count, "检测到未清理的临时文件，可能上次异常退出");
            } else {
                debug!("临时文件目录为空");
            }
        } else {
            error!(path = ?dir, "读取临时文件存储目录失败");
        }

        tokio::spawn(async move {
            info!("临时文件清理协程已启动");
            while let Some(path) = rx.recv().await {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    debug!(path = ?path, "接收到临时文件清理请求，等待 60 秒...");
                    sleep(Duration::from_secs(60)).await; // 延迟删除，防止文件正在被使用

                    let name_string = name.to_string();

                    if let Err(e) = fs::remove_file(&path) {
                        error!(path = ?path, error = ?e, "删除临时文件失败");
                    } else {
                        debug!(path = ?path, "临时文件已清理");
                    }

                    MANAGER.cache.remove(&name_string);
                }
            }
            warn!("临时文件清理协程退出");
        });

        Self {
            dir,
            cache,
            counter: AtomicUsize::new(0),
            tx,
        }
    }

    pub fn alloc_path(&self, filename: &str) -> PathBuf {
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("t");
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e))
            .unwrap_or_default();

        if self.cache.insert(filename.to_string()) {
            debug!(filename = filename, "分配临时文件路径成功 (无冲突)");
            return self.dir.join(filename);
        }
        // 文件名冲突，需要重试
        debug!(filename = filename, "临时文件路径冲突，开始重试分配");
        loop {
            let id = self.counter.fetch_add(1, Ordering::Relaxed);
            let new_name = format!("{}_{}{}", stem, id, ext);
            if self.cache.insert(new_name.clone()) {
                debug!(
                    original_name = filename,
                    new_name = new_name,
                    "分配临时文件路径成功 (重试)"
                );
                return self.dir.join(new_name);
            }
            trace!(new_name = new_name, "路径冲突，继续重试");
        }
    }

    pub fn release(&self, path: PathBuf, remove_disk: bool) {
        if remove_disk {
            debug!(path = ?path, "临时文件标记为待删除");
            if let Err(e) = self.tx.send(path) {
                error!(error = ?e, "发送临时文件到清理队列失败");
            }
        } else {
            debug!(path = ?path, "临时文件被标记为不自动删除，跳过清理");
        }
    }
}

// --- TempFile 结构重构 ---
#[derive(Debug)]
pub struct TempFile {
    pub path: PathBuf,
    pub remove_on_drop: bool,
}

impl TempFile {
    /// 仅内部使用的物理占坑构造
    fn prepare_internal(filename: &str, remove: bool) -> Self {
        let path = MANAGER.alloc_path(filename);

        Self {
            path,
            remove_on_drop: remove,
        }
    }
}

/// RAII: 当作用域结束时，TempFile 会自动从管理器中释放并删除磁盘文件
impl Drop for TempFile {
    fn drop(&mut self) {
        trace!(path = ?self.path, "TempFile 结构体被销毁 (Drop)");
        MANAGER.release(self.path.clone(), self.remove_on_drop);
    }
}

// --- 实现协议，支持 from_url 自动路由 ---

#[async_trait]
impl FileStorage for TempFile {
    fn get_path(&self) -> &PathBuf {
        &self.path
    }
    fn is_temp(&self) -> bool {
        true
    }
}

#[async_trait]
impl FileBackend for TempFile {
    /// 统一构造器：由 FileFromUrl 调用
    fn prepare(filename: &str) -> Self {
        // 默认 TempFile 在 from_url 场景下开启自动删除
        Self::prepare_internal(filename, true)
    }

    /// 下载完成钩子
    async fn on_complete(&mut self) -> Result<()> {
        Ok(())
    }
}

impl TempFile {
    pub async fn get_url(&self) -> String {
        let path_for_blocking = self.path.clone();
        debug!(path = ?path_for_blocking, "开始获取临时文件的 File URL");

        let absolute_path = tokio::task::spawn_blocking(move || {
            // 阻塞 I/O 运行在单独的线程上
            std::fs::canonicalize(&path_for_blocking).unwrap_or_else(|e| {
                error!(path = ?path_for_blocking, error = ?e, "获取文件规范路径失败");
                path_for_blocking
            })
        })
        .await
        .unwrap_or_else(|e| {
            error!(error = ?e, "文件规范路径获取任务执行失败");
            // 任务失败时，返回原始路径的克隆
            self.path.clone()
        });

        Url::from_file_path(absolute_path.clone())
            .map(|url| {
                debug!(path = ?absolute_path, url = url.as_str(), "成功将文件路径转换为 URL");
                url.into()
            })
            .unwrap_or_else(|_| {
                error!(path = ?absolute_path, "无法将文件路径转换为 URL");
                String::new()
            })
    }

    pub async fn to_fileurl(self) -> FileUrl {
        let url = self.get_url().await;
        debug!(url = url, "临时文件获取 FileUrl 成功");
        FileUrl::Raw(url)
    }
}
