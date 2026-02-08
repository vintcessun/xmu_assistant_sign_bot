use anyhow::Result;
use uuid::Uuid;

use crate::api::{
    llm::chat::{file::LlmFile, llm::get_single_file_embedding},
    storage::{HasEmbedding, VectorSearchEngine},
};
use std::sync::{Arc, LazyLock};

static FILE_EMBEDDING_DB: LazyLock<VectorSearchEngine<LlmFile>> =
    LazyLock::new(|| VectorSearchEngine::new("llm_chat_file_embedding_database"));

impl HasEmbedding for LlmFile {
    fn get_embedding(&self) -> &[f32] {
        self.embedding.as_ref().unwrap().as_slice()
    }
}

pub async fn embedding_llm_file(mut file: LlmFile) -> Result<Arc<LlmFile>> {
    let embedding = get_single_file_embedding(&file).await?;
    file.embedding = Some(embedding);
    let file = Arc::new(file);
    FILE_EMBEDDING_DB.insert(file.clone()).await?;
    Ok(file)
}

pub async fn search_llm_file(key: Vec<f32>, top_k: usize) -> Result<Vec<(Uuid, Arc<LlmFile>)>> {
    FILE_EMBEDDING_DB.search(key, top_k).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_file_embedding() -> Result<()> {
        let mut file = LlmFile::from_url("https://multimedia.nt.qq.com.cn/download?appid=1407&fileid=EhSxkb2veUyr4m_-dy2fsPv9hIl4NRiPuwIg_woojYKrkv3EkgMyBHByb2RQgL2jAVoQ1a6ygbvSFAzdVDPPqyDFsXoC5LKCAQJneg&rkey=CAESMIEslqobKMl_19QcqkL8Buyx96vGvI3WxtwpRlDFl9TXj0BNUjA9kXdVpfgaKfuxkw", "A1A1EA9F31371A1935416E6746F4212A.jpg".to_string()).await?;

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
