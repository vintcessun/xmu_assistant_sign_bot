use super::config::MODEL_MAP;
use crate::api::llm::{
    chat::file::LlmFile,
    tool::{LlmPrompt, LlmVec, ask_as},
};
use anyhow::{Result, anyhow};
use genai::{
    Client, ModelIden, ServiceTarget,
    chat::{Binary, ChatMessage, ChatResponse, ContentPart},
    resolver::{AuthData, AuthResolver, Endpoint, ServiceTargetResolver},
};
use helper::LlmPrompt;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

// 定义常量
const CHAT_MODEL: &str = "gemini-flash-latest";
const EMBED_MODEL: &str = "text-embedding-3-large";

pub static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    // 1. 统一鉴权解析器：从 MODEL_MAP 匹配 API Key
    let auth_resolver = AuthResolver::from_resolver_fn(|model_id: ModelIden| {
        let config = MODEL_MAP
            .values()
            .find(|cfg| cfg.kind == model_id.adapter_kind);

        if let Some(cfg) = config {
            // 优先读取环境变量
            if let Ok(key) = std::env::var(cfg.api_key_env) {
                return Ok(Some(AuthData::from_single(key)));
            }
            // 兼容明文 sk- 写入
            if cfg.api_key_env.starts_with("sk-") {
                return Ok(Some(AuthData::from_single(cfg.api_key_env.to_string())));
            }
        }
        Ok(None)
    });

    // 2. 统一路由解析器：根据模型名从 MODEL_MAP 映射 Base URL
    let target_resolver = ServiceTargetResolver::from_resolver_fn(|mut target: ServiceTarget| {
        if let Some(cfg) = MODEL_MAP.get(&*target.model.model_name) {
            target.endpoint = Endpoint::from_static(cfg.base_url);
        }
        Ok(target)
    });

    Client::builder()
        .with_auth_resolver(auth_resolver)
        .with_service_target_resolver(target_resolver)
        .build()
});

pub async fn get_single_text_embedding(input: String) -> Result<Vec<f32>> {
    let embed_req = genai::embed::EmbedRequest::new(input);
    let res = CLIENT.exec_embed(EMBED_MODEL, embed_req, None).await?;

    res.embeddings
        .into_iter()
        .next()
        .map(|e| e.vector)
        .ok_or_else(|| anyhow!("No embedding returned"))
}

#[derive(Debug, LlmPrompt, Clone, Serialize, Deserialize)]
pub struct FileSemanticSnapshot {
    #[prompt("这是这个文件的整体的摘要")]
    pub summary: String,
    #[prompt("这是这个文件的详细描述或者关键信息点")]
    pub details: String,
    #[prompt("这是这个文件的关键词列表")]
    pub keywords: LlmVec<String>,
}

pub async fn get_single_file_embedding(file: &LlmFile) -> Result<Vec<f32>> {
    let filename = &file.alias;
    let prompt = vec![
        ChatMessage::system("你是一个文件分析专家。请分析以下文件内容并提取关键信息。"),
        ChatMessage::user(format!("文件名为: {}\n内容如下:\n", filename)),
        ChatMessage::user(vec![ContentPart::Binary(Binary::from_file(
            &file.file.path,
        )?)]),
    ];

    #[cfg(test)]
    println!("文件分析提示词: {:?}", prompt);

    let response = ask_as::<FileSemanticSnapshot>(prompt).await?;

    #[cfg(test)]
    println!("文件分析结果: {:?}", response);

    get_single_text_embedding(format!(
        "文件名: {}\n文件摘要: {}\n详细信息: {}\n关键词: {:?}",
        filename, response.summary, response.details, response.keywords
    ))
    .await
}

#[derive(Debug, LlmPrompt, Clone, Serialize, Deserialize)]
pub struct ChatSemanticSnapshot {
    #[prompt("这是这些消息的整体的摘要")]
    pub summary: String,
    #[prompt("这是这些消息的详细描述或者关键信息点")]
    pub details: String,
    #[prompt("这是这些消息的关键词列表")]
    pub keywords: LlmVec<String>,
}

pub async fn get_chat_embedding(messages: Vec<ChatMessage>) -> Result<Vec<f32>> {
    let msgs = [
        vec![ChatMessage::system(
            "你是一个聊天消息分析专家。请分析以下聊天内容并提取关键信息。",
        )],
        messages,
    ]
    .concat();

    let response = ask_as::<ChatSemanticSnapshot>(msgs).await?;
    let combined_text = format!(
        "消息摘要: {}\n详细信息: {}\n关键词: {:?}",
        response.summary, response.details, response.keywords
    );

    get_single_text_embedding(combined_text).await
}

pub async fn ask_llm(chat_message: Vec<ChatMessage>) -> Result<ChatResponse> {
    let chat_req = genai::chat::ChatRequest::new(chat_message);
    let res = CLIENT.exec_chat(CHAT_MODEL, chat_req, None).await?;
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    pub async fn test_text_embedding() -> Result<()> {
        let embedding = get_single_text_embedding("Hello, world!".to_string()).await?;
        println!("Embedding: {:?}", embedding);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_file_embedding() -> Result<()> {
        let file = LlmFile::from_url(
            "https://samplelib.com/lib/preview/png/sample-boat-400x300.png",
            "sample-boat-400x300.png".to_string(),
        )
        .await?;
        let file = file.embedded().await?;
        println!("File embedding result: {:?}", file);
        Ok(())
    }
}
