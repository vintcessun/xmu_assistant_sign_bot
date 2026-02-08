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
    pub async fn attach(mut file: File, alias: String) -> Result<Self> {
        let p = file.path.clone();
        let short_id = tokio::task::spawn_blocking(move || {
            let mut hasher = sha2::Sha256::new();
            let mut f = std::fs::File::open(&p)?;
            std::io::copy(&mut f, &mut hasher)?;
            let hash = format!("{:x}", hasher.finalize());
            let short = FileShortId::from_hex(&hash)?;
            Ok::<FileShortId, anyhow::Error>(short)
        })
        .await??;

        // 3. 完成物理文件的 finish (只读设置、预读)
        file.finish().await?;

        let ret = Self {
            id: short_id,
            file: Arc::new(file),
            alias,
            embedding: None,
        };

        Ok(ret)
    }

    pub fn insert_embedding(&mut self, embedding: Vec<f32>) {
        self.embedding = Some(embedding);
    }
}

static FILE_URL_FILTER_DB: LazyLock<ColdTable<String, FileShortId>> =
    LazyLock::new(|| ColdTable::new("llm_chat_file_url_filter"));

impl LlmFile {
    pub async fn from_url(url: &str, alias: String) -> Result<Self> {
        // 1. 先检查 URL 是否已经下载过（通过 URL 过滤）
        if let Some(id) = FILE_URL_FILTER_DB.get_async(url.to_string()).await?
            && let Some(file) = Self::get_by_id(id)?
        {
            return Ok((*file).clone());
        }
        let file = download_to_file(Arc::new(SessionClient::new()), url, &alias).await?;
        let file = Self::attach(file, alias).await?;
        FILE_URL_FILTER_DB.insert(url.to_string(), file.id).await?;
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
        FILE_DB.insert(file.id, file.clone()).await?;
        Ok(())
    }

    pub fn get_by_id(id: FileShortId) -> Result<Option<Arc<Self>>> {
        FILE_DB.get(id)
    }

    pub async fn embedded(self) -> Result<Arc<Self>> {
        if self.embedding.is_none() {
            let file = embedding_llm_file(self).await?;
            Self::insert(file.clone()).await?;
            Ok(file)
        } else {
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
        let file = LlmFile::from_url(URL, ALIAS.to_string()).await?;
        let file = file.embedded().await?;
        println!("Downloaded LlmFile: {:?}", file.alias);
        let file = FILE_URL_FILTER_DB
            .get(URL.to_string())?
            .and_then(|id| LlmFile::get_by_id(id).ok()?)
            .unwrap();

        println!("Retrieved from DB: {:?}", file.alias);
        Ok(())
    }
}
