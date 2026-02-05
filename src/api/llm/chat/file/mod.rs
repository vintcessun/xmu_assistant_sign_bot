use crate::api::{
    llm::tool::LlmPrompt,
    network::{SessionClient, download_to_file},
    storage::{ColdTable, File},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::sync::{Arc, LazyLock};

static FILE_DB: LazyLock<ColdTable<FileShortId, Arc<LlmFile>>> =
    LazyLock::new(|| ColdTable::new("llm_chat_file_storage"));

static CLIENT: LazyLock<Arc<SessionClient>> = LazyLock::new(|| Arc::new(SessionClient::new()));

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

#[derive(Debug, Serialize, Clone)]
pub struct LlmFile {
    pub id: FileShortId, // 8位 SHA-256
    pub file: Arc<File>, // 你原有的文件抽象
    #[serde(default)]
    pub alias: String, // LLM 容易理解的文件别名（如“大笑.gif”）
    pub embedding: Option<Vec<f32>>, // 可选的向量嵌入
}

impl LlmPrompt for LlmFile {
    fn get_prompt_schema() -> &'static str {
        // 给 LLM 的 Schema 只展示 ID 和 别名，隐藏复杂的物理路径
        "<id> 文件的8位 SHA-256短ID</id> \n<alias> 文件的别名（如“大笑.gif”）</alias>"
    }
    fn root_name() -> &'static str {
        "file"
    }
}

impl<'de> Deserialize<'de> for LlmFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct LlmFileHelper {
            id: String,
            //alias: String,
        }

        let helper = LlmFileHelper::deserialize(deserializer)?;
        let id = FileShortId::from_llm(&helper.id).map_err(serde::de::Error::custom)?;

        let file = Self::get_by_id(id)
            .map_err(|e| serde::de::Error::custom(format!("Get File by id error {e}")))?
            .ok_or(serde::de::Error::custom("The file is not found"))?;
        let file = (*file).clone();

        Ok(file)
    }
}

impl LlmFile {
    /// 从现有的 File 对象创建一个 LlmFile
    pub async fn attach(mut file: File, alias: String) -> Result<Self> {
        let p: std::path::PathBuf = file.path.clone();
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

impl LlmFile {
    pub async fn from_url(url: &str, alias: String) -> Result<Self> {
        let file = download_to_file(CLIENT.clone(), url, &alias).await?;
        let file = Self::attach(file, alias).await?;
        Ok(file)
    }

    pub async fn insert(file: Arc<Self>) -> Result<()> {
        FILE_DB.insert(file.id, file.clone()).await?;
        Ok(())
    }

    pub fn get_by_id(id: FileShortId) -> Result<Option<Arc<Self>>> {
        FILE_DB.get(id)
    }
}
