use crate::api::{
    llm::chat::archive::file_embedding::embedding_llm_file,
    network::{SessionClient, download_to_file},
    storage::{ColdTable, File},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{
    fmt::Display,
    sync::{Arc, LazyLock},
};
use tracing::{debug, error, info, trace, warn};

static FILE_DB: LazyLock<ColdTable<FileShortId, Arc<LlmFile>>> =
    LazyLock::new(|| ColdTable::new("llm_chat_file_storage"));
#[derive(Hash, Eq, PartialEq, Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileShortId(u32);

impl FileShortId {
    // 从完整 SHA-256 字符串生成
    pub fn from_hex(hex: &str) -> Result<Self> {
        let val = u32::from_str_radix(&hex[..8], 16)?;
        Ok(Self(val))
    }

    //前八位也可以从 LLM 传过来的字符串解析
    pub fn from_llm(hex: &str) -> Result<Self> {
        Self::from_hex(hex)
    }

    // 转回给 LLM 看的字符串
    pub fn to_hex(&self) -> String {
        format!("{:08x}", self.0)
    }
}

impl Display for FileShortId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

#[derive(Debug, Serialize, Clone, Deserialize)]
pub struct LlmFile {
    pub id: FileShortId, // 8位 SHA-256
    pub file: Arc<File>, // 你原有的文件抽象
    #[serde(default)]
    pub alias: String, // LLM 容易理解的文件别名（如“大笑.gif”）
    pub embedding: Option<Vec<f32>>, // 可选的向量嵌入
}

impl LlmFile {
    /// 从现有的 File 对象创建一个 LlmFile
    pub async fn attach(file: File, alias: String) -> Result<Self> {
        debug!(path = %file.path.display(), alias = %alias, "开始新建文件并生成短 ID");
        let p = file.path.clone();
        let short_id = tokio::task::spawn_blocking(move || {
            let mut hasher = sha2::Sha256::new();
            let mut f = std::fs::File::open(&p).map_err(|e| {
                error!(path = %p.display(), error = ?e, "打开文件失败");
                e
            })?;
            std::io::copy(&mut f, &mut hasher).map_err(|e| {
                error!(path = %p.display(), error = ?e, "读取文件内容计算 SHA-256 失败");
                e
            })?;
            let hash = format!("{:x}", hasher.finalize());
            let short = FileShortId::from_hex(&hash).map_err(|e| {
                error!(hash = %hash, error = ?e, "将 SHA-256 转换为 FileShortId 失败");
                e
            })?;
            Ok::<FileShortId, anyhow::Error>(short)
        })
        .await
        .map_err(|e| {
            error!(error = ?e, "文件短 ID 生成阻塞任务失败");
            e
        })??;

        debug!(file_id = %short_id, "文件短 ID 生成成功，开始完成文件设置");

        // 3. 完成物理文件的 finish (只读设置、预读)
        file.finish().await.map_err(|e| {
            error!(file_id = %short_id, error = ?e, "完成物理文件设置失败");
            e
        })?;

        debug!(file_id = %short_id, "物理文件设置完成");

        let ret = Self {
            id: short_id,
            file: Arc::new(file),
            alias,
            embedding: None,
        };

        debug!(file_id = %short_id, "LlmFile 新建成功");
        Ok(ret)
    }

    pub fn insert_embedding(&mut self, embedding: Vec<f32>) {
        self.embedding = Some(embedding);
    }
}

static FILE_URL_FILTER_DB: LazyLock<ColdTable<String, FileShortId>> =
    LazyLock::new(|| ColdTable::new("llm_chat_file_url_filter"));

impl LlmFile {
    pub async fn from_url(url: &String, alias: String) -> Result<Self> {
        debug!(url = %url, alias = %alias, "尝试从 URL 获取 LlmFile");

        // 1. 先检查 URL 是否已经下载过（通过 URL 过滤）
        let id_result = FILE_URL_FILTER_DB.get_async(url).await.map_err(|e| {
            error!(url = %url, error = ?e, "查询 URL 过滤数据库失败");
            e
        })?;

        if let Some(id) = id_result
            && let Some(file) = Self::get_by_id(id).map_err(|e| {
                error!(file_id = %id, error = ?e, "从文件数据库中获取文件失败");
                e
            })?
        {
            debug!(url = %url, file_id = %id, "文件已存在于数据库中，直接返回");
            return Ok((*file).clone());
        }

        debug!(url = %url, "文件在过滤数据库中不存在或查找失败，开始下载");

        let file = download_to_file(Arc::new(SessionClient::new()), url, &alias)
            .await
            .map_err(|e| {
                error!(url = %url, error = ?e, "下载文件失败");
                e
            })?;

        let file = Self::attach(file, alias).await.map_err(|e| {
            error!(url = %url, error = ?e, "附加下载文件失败");
            e
        })?;

        FILE_URL_FILTER_DB
            .insert(url, &file.id)
            .await
            .map_err(|e| {
                warn!(url = %url, file_id = %file.id, error = ?e, "插入 URL 过滤数据库失败");
                e
            })?;

        debug!(url = %url, file_id = %file.id, "文件下载、附加和记录成功");
        #[cfg(test)]
        {
            println!(
                "Downloaded and attached file from URL: {}, assigned ID: {}",
                url, file.id
            );
            println!("文件已写入");
        }
        Ok(file)
    }

    pub async fn insert(file: Arc<Self>) -> Result<()> {
        debug!(file_id = %file.id, alias = %file.alias, "插入 LlmFile 到数据库");
        FILE_DB.insert(&file.id, &file).await.map_err(|e| {
            error!(file_id = %file.id, error = ?e, "插入 LlmFile 到数据库失败");
            e
        })?;
        trace!(file_id = %file.id, "LlmFile 插入成功");
        Ok(())
    }

    pub fn get_by_id(id: FileShortId) -> Result<Option<Arc<Self>>> {
        trace!(file_id = %id, "尝试从数据库获取 LlmFile");
        FILE_DB.get(&id).map_err(|e| {
            error!(file_id = %id, error = ?e, "从数据库获取 LlmFile 失败");
            e
        })
    }

    pub async fn embedded(self) -> Result<Arc<Self>> {
        if self.embedding.is_none() {
            debug!(file_id = %self.id, alias = %self.alias, "文件尚未嵌入，开始生成嵌入向量");
            let id = self.id;
            let file = embedding_llm_file(self).await.map_err(|e| {
                error!(file_id = %id, error = ?e, "生成文件嵌入向量失败");
                e
            })?;
            Self::insert(file.clone()).await.map_err(|e| {
                error!(file_id = %file.id, error = ?e, "嵌入后插入数据库失败");
                e
            })?;
            info!(file_id = %file.id, "文件嵌入和存储完成");
            Ok(file)
        } else {
            debug!(file_id = %self.id, "文件已包含嵌入向量，跳过嵌入步骤");
            Ok(Arc::new(self))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = r#"https://samplelib.com/lib/preview/png/sample-hut-400x300.png"#;
    const ALIAS: &str = "sample-hut-400x300.png";

    #[tokio::test(flavor = "multi_thread")]
    async fn test_llm_file_from_url() -> Result<()> {
        let file = LlmFile::from_url(&URL.to_string(), ALIAS.to_string()).await?;
        let file = file.embedded().await?;
        println!("Downloaded LlmFile: {:?}", file.alias);
        let file = FILE_URL_FILTER_DB
            .get(&URL.to_string())?
            .and_then(|id| LlmFile::get_by_id(id).ok()?)
            .unwrap();

        println!("Retrieved from DB: {:?}", file.alias);
        Ok(())
    }
}
