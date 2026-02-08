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
        Arc, LazyLock,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{io::AsyncReadExt, sync::watch};
use tracing::error;
use url::Url;

pub static DATA_DIR: LazyLock<&'static str> = LazyLock::new(|| {
    let path = concatcp!(BASE_DATA_DIR, "/", BASE);
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
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    cache.insert(name);
                }
            }
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
            return self.dir.join(filename);
        }
        loop {
            let id = self.counter.fetch_add(1, Ordering::Relaxed);
            let new_name = format!("{}_{}{}", stem, id, ext);
            if self.cache.insert(new_name.clone()) {
                return self.dir.join(new_name);
            }
        }
    }
}

// --- 核心文件结构 ---
#[derive(Debug, Serialize)]
pub struct File {
    pub path: PathBuf,
    // 异步读取状态同步
    #[serde(skip)]
    read_rx: Option<watch::Receiver<Option<Arc<Vec<u8>>>>>,
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
            read_rx: None,
        }
    }

    /// 下载完成后调用，开启后台预读
    pub fn freeze(&mut self) {
        let (tx, rx) = watch::channel(None);
        let p = self.path.clone();

        tokio::spawn(async move {
            let res: Result<Arc<Vec<u8>>> = async {
                let mut f = tokio::fs::File::open(p).await?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).await?;
                Ok(Arc::new(buf))
            }
            .await;

            match res {
                Ok(d) => {
                    let _ = tx.send(Some(d));
                }
                Err(e) => {
                    error!("预读文件失败: {}", e);
                }
            }
        });
        self.read_rx = Some(rx);
    }

    pub async fn wait_for_data(&self) -> Result<Arc<Vec<u8>>> {
        // 文件必须已经调用 freeze()，否则是逻辑错误
        let mut rx = self.read_rx.as_ref().unwrap().clone();

        if rx.borrow().is_none() {
            rx.changed()
                .await
                .map_err(|_| anyhow!("读取协程意外关闭"))?;
        }
        rx.borrow()
            .as_ref()
            .cloned()
            .ok_or_else(|| anyhow!("数据读取失败"))
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
        let mut file = Self {
            path: f.path,
            read_rx: None,
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
    pub async fn finish(&mut self) -> Result<()> {
        let p = self.path.clone();

        // 1. 修改权限为只读 (使用 spawn_blocking 避免阻塞)
        tokio::task::spawn_blocking(move || -> Result<()> {
            // 确保文件存在
            let metadata = fs::metadata(&p)
                .map_err(|e| anyhow!("获取文件元数据失败: {}, path: {:?}", e, p))?;
            let mut perms = metadata.permissions();

            if !perms.readonly() {
                perms.set_readonly(true);
                fs::set_permissions(&p, perms)?;
            }
            Ok(())
        })
        .await??;

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
            error!("spawn_blocking for canonicalize failed: {}", e);
            // 任务失败时，返回原始路径的克隆
            self.path.clone()
        });

        Url::from_file_path(absolute_path)
            .map(|url| url.into())
            .unwrap_or_default()
    }
}

impl File {
    pub async fn to_fileurl(mut self) -> Result<FileUrl> {
        self.finish().await?;
        Ok(FileUrl::Raw(self.get_url().await))
    }
}
