use anyhow::Result;
use dashmap::DashSet;
use uuid::Uuid;

use crate::api::{
    llm::chat::{file::{LlmFile, FileShortId}, llm::get_single_file_embedding},
    storage::{HasEmbedding, VectorSearchEngine},
};
use std::sync::{Arc, LazyLock};
use tracing::{debug, error, info};

static FILE_EMBEDDING_DB: LazyLock<VectorSearchEngine<LlmFile>> =
    LazyLock::new(|| VectorSearchEngine::new("llm_chat_file_embedding_dataset"));

/// 会话内已嵌入的文件 ID 集合，防止重复插入向量库
static EMBEDDED_FILE_IDS: LazyLock<DashSet<FileShortId>> = LazyLock::new(DashSet::new);

impl HasEmbedding for LlmFile {
    fn get_embedding(&self) -> &[f32] {
        self.embedding.as_ref().unwrap().as_slice()
    }
}

pub async fn embedding_llm_file(mut file: LlmFile) -> Result<Arc<LlmFile>> {
    // Phase E: 幂等检查——同 file_id 跳过重复插入
    if EMBEDDED_FILE_IDS.contains(&file.id) {
        debug!(file_id = %file.id, file_name = %file.alias, "文件嵌入已存在，跳过重复插入");
        return Ok(Arc::new(file));
    }

    info!(file_name = %file.alias, "开始文件嵌入和存储");
    let embedding = get_single_file_embedding(&file).await.map_err(|e| {
        error!(file_name = %file.alias, error = ?e, "获取文件嵌入失败");
        e
    })?;
    file.embedding = Some(embedding);
    let file = Arc::new(file);
    FILE_EMBEDDING_DB.insert(file.clone()).await.map_err(|e| {
        error!(file_name = %file.alias, error = ?e, "插入向量数据库失败");
        e
    })?;
    EMBEDDED_FILE_IDS.insert(file.id);
    info!(file_name = %file.alias, "文件嵌入和存储成功");
    Ok(file)
}

pub async fn search_llm_file(key: &[f32], top_k: usize) -> Result<Vec<(Uuid, Arc<LlmFile>)>> {
    info!(key_len = ?key.len(), top_k = ?top_k, "开始在向量数据库中搜索文件");
    let result = FILE_EMBEDDING_DB.search(key, top_k).await.map_err(|e| {
        error!(error = ?e, "向量数据库搜索文件失败");
        e
    })?;
    info!(result_count = ?result.len(), "文件向量搜索完成");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_file_embedding() -> Result<()> {
        let mut file = LlmFile::from_url(&"https://multimedia.nt.qq.com.cn/download?appid=1407&fileid=EhSxkb2veUyr4m_-dy2fsPv9hIl4NRiPuwIg_woojYKrkv3EkgMyBHByb2RQgL2jAVoQ1a6ygbvSFAzdVDPPqyDFsXoC5LKCAQJneg&rkey=CAESMIEslqobKMl_19QcqkL8Buyx96vGvI3WxtwpRlDFl9TXj0BNUjA9kXdVpfgaKfuxkw".to_string(), "A1A1EA9F31371A1935416E6746F4212A.jpg".to_string()).await?;

        //let embedded_file = embedding_llm_file(file).await?;

        let embedding = get_single_file_embedding(&file).await?;
        println!("File is embedded");
        file.embedding = Some(embedding);
        println!("Embedding is set");
        let file = Arc::new(file);
        FILE_EMBEDDING_DB.insert(file.clone()).await?;
        println!("File is inserted into embedding DB");
        println!("Embedded file: {:?}", file);
        Ok(())
    }
}
