const BASE: &str = "file";

use super::BASE_DATA_DIR;
use crate::{abi::message::file::FileUrl, config::ensure_dir};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use const_format::concatcp;
use dashmap::DashSet;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{io::AsyncReadExt, sync::watch};
use tracing::{debug, error, info, trace, warn};
use url::Url;

pub static DATA_DIR: LazyLock<&'static str> = LazyLock::new(|| {
    let path = concatcp!(BASE_DATA_DIR, "/", BASE);
    info!(path = path, "初始化文件存储目录");
    ensure_dir(path);
    path
});

static MANAGER: LazyLock<FileManager> = LazyLock::new(FileManager::new);

// --- 路径管理器 ---
pub struct FileManager {
    dir: PathBuf,
    cache: DashSet<String>,
    counter: AtomicUsize,
}

impl FileManager {
    pub fn new() -> Self {
        let dir = Path::new(*DATA_DIR).to_path_buf();
        let cache = DashSet::new();
        info!(path = ?dir, "初始化文件管理器");
        if let Ok(entries) = fs::read_dir(&dir) {
            let mut count = 0;
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    cache.insert(name);
                    count += 1;
                } else {
                    warn!(path = ?entry.path(), "无法将文件名转换为 UTF-8 字符串，忽略缓存");
                }
            }
            info!(count = count, "从文件系统缓存中加载现有文件");
        } else {
            error!(path = ?dir, "读取文件存储目录失败");
        }
        Self {
            dir,
            cache,
            counter: AtomicUsize::new(0),
        }
    }

    pub fn alloc_path(&self, filename: &str) -> PathBuf {
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("f");
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e))
            .unwrap_or_default();

        if self.cache.insert(filename.to_string()) {
            debug!(filename = filename, "分配文件路径成功 (无冲突)");
            return self.dir.join(filename);
        }
        // 文件名冲突，需要重试
        debug!(filename = filename, "文件路径冲突，开始重试分配");
        loop {
            let id = self.counter.fetch_add(1, Ordering::Relaxed);
            let new_name = format!("{}_{}{}", stem, id, ext);
            if self.cache.insert(new_name.clone()) {
                debug!(
                    original_name = filename,
                    new_name = new_name,
                    "分配文件路径成功 (重试)"
                );
                return self.dir.join(new_name);
            }
            trace!(new_name = new_name, "路径冲突，继续重试");
        }
    }
}

// --- 核心文件结构 ---
#[derive(Debug, Serialize)]
pub struct File {
    pub path: PathBuf,
    // 异步读取状态同步
    #[serde(skip)]
    read_rx: OnceLock<watch::Receiver<Option<Arc<Vec<u8>>>>>,
}

impl File {
    /// 仅内部使用的构造，用于准备占坑
    fn prepare(filename: &str) -> Self {
        let path = MANAGER.alloc_path(filename);
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        Self {
            path,
            read_rx: OnceLock::new(),
        }
    }

    /// 下载完成后调用，开启后台预读
    pub fn freeze(&self) {
        if self.read_rx.get().is_some() {
            // 已经启动预读，直接返回
            debug!(path = ?self.path, "文件预读任务已启动，跳过重复调用");
            return;
        }
        debug!(path = ?self.path, "启动文件后台预读任务");
        let (tx, rx) = watch::channel(None);
        let p = self.path.clone();

        tokio::spawn(async move {
            let res: Result<Arc<Vec<u8>>> = async {
                let mut f = tokio::fs::File::open(&p).await.map_err(|e| {
                    error!(path = ?p, error = ?e, "打开文件失败");
                    e
                })?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).await.map_err(|e| {
                    error!(path = ?p, error = ?e, "读取文件内容失败");
                    e
                })?;
                Ok(Arc::new(buf))
            }
            .await;

            match res {
                Ok(d) => {
                    debug!(path = ?p, "文件预读完成");
                    let _ = tx.send(Some(d));
                }
                Err(e) => {
                    error!(path = ?p, error = ?e, "文件预读任务失败");
                }
            }
        });
        let _ = self.read_rx.set(rx);
    }

    pub async fn wait_for_data(&self) -> Result<Arc<Vec<u8>>> {
        // 文件必须已经调用 freeze()，否则是逻辑错误
        let mut rx = self.read_rx.get().unwrap().clone();

        if rx.borrow().is_none() {
            trace!(path = ?self.path, "等待文件预读完成...");
            rx.changed()
                .await
                .map_err(|_| anyhow!("读取协程意外关闭"))?;
        }
        rx.borrow().as_ref().cloned().ok_or_else(|| {
            error!(path = ?self.path, "文件预读数据为空");
            anyhow!("数据读取失败")
        })
    }
}

// --- 自动加载反序列化 ---
impl<'de> Deserialize<'de> for File {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Field {
            path: PathBuf,
        }
        let f = Field::deserialize(deserializer)?;
        let file = Self {
            path: f.path,
            read_rx: OnceLock::new(),
        };
        file.freeze(); // 恢复即加载
        Ok(file)
    }
}

// --- 后端抽象 Trait ---
pub trait FileStorage: Send + Sync {
    fn get_path(&self) -> &PathBuf;
    fn is_temp(&self) -> bool;
}

#[async_trait]
pub trait FileBackend: FileStorage + Sized {
    fn prepare(filename: &str) -> Self;
    async fn on_complete(&mut self) -> Result<()>;
}

impl FileStorage for File {
    fn get_path(&self) -> &PathBuf {
        &self.path
    }
    fn is_temp(&self) -> bool {
        false
    }
}

#[async_trait]
impl FileBackend for File {
    fn prepare(filename: &str) -> Self {
        Self::prepare(filename)
    }
    async fn on_complete(&mut self) -> Result<()> {
        self.finish().await
    }
}

impl File {
    /// 业务层调用的终结方法：将文件锁定为只读并预读
    pub async fn finish(&self) -> Result<()> {
        debug!(path = ?self.path, "开始终结文件下载/创建过程");
        let p = self.path.clone();

        // 1. 修改权限为只读 (使用 spawn_blocking 避免阻塞)
        tokio::task::spawn_blocking(move || -> Result<()> {
            // 确保文件存在
            let metadata = fs::metadata(&p).map_err(|e| {
                error!(path = ?p, error = ?e, "获取文件元数据失败");
                anyhow!("获取文件元数据失败: {}, path: {:?}", e, p)
            })?;
            let mut perms = metadata.permissions();

            if !perms.readonly() {
                perms.set_readonly(true);
                fs::set_permissions(&p, perms).map_err(|e| {
                    error!(path = ?p, error = ?e, "设置文件权限失败");
                    e
                })?;
                debug!(path = ?p, "设置文件为只读");
            } else {
                debug!(path = ?p, "文件已经是只读");
            }
            Ok(())
        })
        .await
        .map_err(|e| {
            error!(path = ?self.path, error = ?e, "文件权限设置任务执行失败");
            anyhow!("文件权限设置任务执行失败: {}", e)
        })??;

        // 2. 触发预读逻辑 (此时文件已是只读，freeze 内部 open 将以只读方式打开)
        self.freeze();

        Ok(())
    }
}

impl File {
    pub async fn get_url(&self) -> String {
        let path_for_blocking = self.path.clone();

        let absolute_path = tokio::task::spawn_blocking(move || {
            // 阻塞 I/O 运行在单独的线程上
            std::fs::canonicalize(&path_for_blocking).unwrap_or(path_for_blocking)
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
}

impl File {
    pub async fn to_fileurl(self) -> Result<FileUrl> {
        self.finish().await?;
        let url = self.get_url().await;
        debug!(url = url, "文件终结并获取 FileUrl 成功");
        Ok(FileUrl::Raw(url))
    }
}
