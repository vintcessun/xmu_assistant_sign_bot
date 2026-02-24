const BASE: &str = "image";

use super::BASE_DATA_DIR;
use crate::{
    abi::message::file::FileUrl,
    api::storage::{FileBackend, FileStorage},
    config::ensure_dir,
};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
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
use tokio::{io::AsyncReadExt, io::AsyncWriteExt, sync::OnceCell};
use tracing::{debug, error, info, trace, warn};
use url::Url;

pub static DATA_DIR: LazyLock<&'static str> = LazyLock::new(|| {
    let path = concatcp!(BASE_DATA_DIR, "/", BASE);
    info!(path = path, "初始化图片存储目录");
    ensure_dir(path);
    path
});

static MANAGER: LazyLock<ImageManager> = LazyLock::new(ImageManager::new);

// --- 路径管理器 ---
pub struct ImageManager {
    dir: PathBuf,
    cache: DashSet<String>,
    counter: AtomicUsize,
}

impl ImageManager {
    pub fn new() -> Self {
        let dir = Path::new(*DATA_DIR).to_path_buf();
        let cache = DashSet::new();
        info!(path = ?dir, "初始化图片文件管理器");
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
            info!(count = count, "从文件系统缓存中加载现有图片文件");
        } else {
            error!(path = ?dir, "读取图片存储目录失败");
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
            debug!(filename = filename, "分配图片文件路径成功 (无冲突)");
            return self.dir.join(filename);
        }
        // 文件名冲突，需要重试
        debug!(filename = filename, "图片文件路径冲突，开始重试分配");
        loop {
            let id = self.counter.fetch_add(1, Ordering::Relaxed);
            let new_name = format!("{}_{}{}", stem, id, ext);
            if self.cache.insert(new_name.clone()) {
                debug!(
                    original_name = filename,
                    new_name = new_name,
                    "分配图片文件路径成功 (重试)"
                );
                return self.dir.join(new_name);
            }
            trace!(new_name = new_name, "路径冲突，继续重试");
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ImageFile {
    pub path: PathBuf,

    // 使用 OnceCell 实现懒加载
    // 1. 它是线程安全的异步原语
    // 2. 只有第一次调用 get() 时才会初始化
    // 3. 支持并发：如果多个请求同时到来，只有一个会执行加载，其他的等待
    #[serde(skip)]
    cache: Arc<OnceCell<Arc<str>>>,
}

impl ImageFile {
    /// 从 Base64 字符串创建新文件
    /// 逻辑：写入磁盘
    pub async fn create_from_base64(base64_str: &str) -> Result<Self> {
        let filename = format!("img_{}.png", uuid::Uuid::new_v4());
        debug!(filename = filename, "开始从 Base64 创建图片文件");

        // 1. 分配路径
        let file = Self::prepare(&filename);

        // 2. 解码数据
        let bytes = general_purpose::STANDARD.decode(base64_str).map_err(|e| {
            error!(error = ?e, "Base64 解码失败");
            anyhow!("Base64 解码失败: {}", e)
        })?;
        debug!(
            size = bytes.len(),
            "Base64 解码成功，获取 {} 字节数据",
            bytes.len()
        );

        // 3. 异步写入磁盘
        let path = file.path.clone();
        let mut fs_file = tokio::fs::File::create(&path)
            .await
            .with_context(|| format!("创建图片文件失败: {:?}", path))?;

        if let Err(e) = fs_file.write_all(&bytes).await {
            error!(path = ?path, error = ?e, "写入图片数据失败");
            return Err(e).context(format!("写入图片数据到文件失败: {:?}", path));
        }
        if let Err(e) = fs_file.flush().await {
            warn!(path = ?path, error = ?e, "写入图片数据 Flush 失败");
        }

        // 4. 设置只读权限
        file.set_readonly().await?;

        debug!(path = ?file.path, "图片文件创建并写入磁盘成功");
        Ok(file)
    }

    /// 核心懒加载方法：获取 Base64
    /// 如果是第一次调用，会触发磁盘读取；否则直接返回内存数据
    pub async fn get_base64(&self) -> Result<Arc<str>> {
        let path = self.path.clone();
        debug!(path = ?path, "调用 get_base64，尝试获取缓存或进行懒加载");

        // get_or_try_init: 如果未初始化，执行闭包；如果已初始化，直接返回。
        // 闭包中的逻辑只会执行一次。
        let data = self
            .cache
            .get_or_try_init(|| async move {
                debug!(path = ?path, "图片文件 Base64 懒加载已触发 (首次加载)");

                // 1. 异步读取文件
                let mut f = tokio::fs::File::open(&path)
                    .await
                    .with_context(|| format!("无法打开图片文件: {:?}", path))?;

                let mut buf = Vec::new();
                f.read_to_end(&mut buf).await.map_err(|e| {
                    error!(path = ?path, error = ?e, "读取文件内容失败");
                    e
                })?;

                // 2. 转码为 Base64
                let b64 = general_purpose::STANDARD.encode(&buf);
                debug!(path = ?path, "图片文件 Base64 懒加载完成");
                Ok::<Arc<str>, anyhow::Error>(Arc::from(b64))
            })
            .await?;

        trace!(path = ?self.path, "返回 Base64 缓存数据");
        Ok(data.clone())
    }

    /// 设置文件为只读 (辅助方法)
    async fn set_readonly(&self) -> Result<()> {
        let p = self.path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            if !p.exists() {
                warn!(path = ?p, "文件不存在，跳过设置只读权限");
                return Ok(());
            }
            let metadata = fs::metadata(&p).map_err(|e| {
                error!(path = ?p, error = ?e, "获取文件元数据失败");
                e
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
                trace!(path = ?p, "文件已是只读");
            }
            Ok(())
        })
        .await
        .map_err(|e| {
            error!(path = ?self.path, error = ?e, "设置只读权限任务执行失败");
            e
        })??;
        Ok(())
    }

    /// 获取 URL 对象
    pub async fn get_url_obj(&self) -> Url {
        let path_for_blocking = self.path.clone();
        debug!(path = ?path_for_blocking, "开始获取图片的 File URL");

        // 规范化路径以获取绝对路径 (用于 file://)
        let absolute_path = tokio::task::spawn_blocking(move || {
            std::fs::canonicalize(&path_for_blocking).unwrap_or_else(|e| {
                error!(path = ?path_for_blocking, error = ?e, "获取文件规范路径失败");
                path_for_blocking
            })
        })
        .await
        .unwrap_or_else(|e| {
            error!(error = ?e, "文件规范路径获取任务执行失败");
            self.path.clone()
        });

        Url::from_file_path(absolute_path.clone())
            .inspect(|url| {
                debug!(path = ?absolute_path, url = url.as_str(), "成功将文件路径转换为 URL");
            })
            .unwrap_or_else(|_| {
                error!(path = ?absolute_path, "无法将文件路径转换为 URL");
                Url::parse("file:///unknown").unwrap()
            })
    }

    /// 转换为 FileUrl (外部接口)
    pub async fn to_fileurl(&self) -> Result<FileUrl> {
        if !self.path.exists() {
            error!(path = ?self.path, "图片文件不存在");
            return Err(anyhow!("图片文件不存在: {:?}", self.path));
        }
        trace!(path = ?self.path, "获取 FileUrl 成功");
        Ok(FileUrl::Raw(self.get_url_obj().await.into()))
    }
}

// --- Trait 实现: FileStorage ---
impl FileStorage for ImageFile {
    fn get_path(&self) -> &PathBuf {
        &self.path
    }
    fn is_temp(&self) -> bool {
        false
    }
}

// --- Trait 实现: FileBackend ---
#[async_trait]
impl FileBackend for ImageFile {
    fn prepare(filename: &str) -> Self {
        // 使用 MANAGER 分配路径
        let path = MANAGER.alloc_path(filename);
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        Self {
            path,
            cache: Arc::new(OnceCell::new()), // 初始化为空，等待懒加载
        }
    }

    async fn on_complete(&mut self) -> Result<()> {
        // 即使没有加载 Base64，也要确保文件权限正确
        self.set_readonly().await?;
        Ok(())
    }
}

// --- 自动反序列化 ---
// 这里的关键是：反序列化时只恢复路径，不加载内容
impl<'de> Deserialize<'de> for ImageFile {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Field {
            path: PathBuf,
        }

        let f = Field::deserialize(deserializer)?;

        Ok(Self {
            path: f.path,
            // 关键点：创建一个空的 OnceCell
            // 数据将在第一次调用 get_base64() 时自动从 path 读取
            cache: Arc::new(OnceCell::new()),
        })
    }
}
