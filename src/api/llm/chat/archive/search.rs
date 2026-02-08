use crate::api::llm::chat::archive::file_embedding::search_llm_file;
use crate::api::llm::chat::archive::memo_fragment::{ChatSegment, MemoFragment};
use crate::api::llm::chat::file::LlmFile;
use crate::api::llm::tool::LlmUsize;
use crate::api::llm::{
    chat::llm::get_single_text_embedding,
    tool::{LlmPrompt, ask_as},
};
use anyhow::Result;
use genai::chat::{ChatMessage, ChatResponse};
use helper::LlmPrompt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, trace};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, LlmPrompt)]
pub struct SearchRequest {
    #[prompt("请提供搜索的概要内容")]
    pub query: String,
    #[prompt("返回的搜索结果数量")]
    pub top_k: LlmUsize,
}

pub async fn search_file(query_response: ChatResponse) -> Result<Vec<(Uuid, Arc<LlmFile>)>> {
    info!("开始解析文件搜索请求");
    let request = ask_as::<SearchRequest>(vec![
        ChatMessage::system("你是一个专业的文件搜索助手，请根据用户提供的搜索请求进行文件搜索"),
        ChatMessage::user(query_response.content),
    ])
    .await
    .map_err(|e| {
        error!(error = ?e, "LLM 解析文件搜索请求失败");
        e
    })?;

    info!(query = %request.query, top_k = ?request.top_k, "文件搜索请求解析成功，开始生成查询嵌入");
    let query_embedding = get_single_text_embedding(request.query)
        .await
        .map_err(|e| {
            error!(error = ?e, "生成文件查询嵌入失败");
            e
        })?;

    trace!("开始文件向量搜索");
    let results = search_llm_file(&query_embedding, *request.top_k)
        .await
        .map_err(|e| {
            error!(error = ?e, "执行文件向量搜索失败");
            e
        })?;
    info!(result_count = ?results.len(), "文件搜索完成");
    Ok(results)
}

pub async fn search_memo(query_response: ChatResponse) -> Result<Vec<(Uuid, Arc<ChatSegment>)>> {
    info!("开始解析记忆片段搜索请求");
    let request = ask_as::<SearchRequest>(vec![
        ChatMessage::system(
            "你是一个专业的聊天记录搜索助手，请根据用户提供的搜索请求进行聊天记录搜索",
        ),
        ChatMessage::user(query_response.content),
    ])
    .await
    .map_err(|e| {
        error!(error = ?e, "LLM 解析记忆片段搜索请求失败");
        e
    })?;

    info!(query = %request.query, top_k = ?request.top_k, "记忆片段搜索请求解析成功，开始生成查询嵌入");
    let query_embedding = get_single_text_embedding(request.query)
        .await
        .map_err(|e| {
            error!(error = ?e, "生成记忆片段查询嵌入失败");
            e
        })?;

    trace!("开始记忆片段向量搜索");
    let results = MemoFragment::search(&query_embedding, *request.top_k)
        .await
        .map_err(|e| {
            error!(error = ?e, "执行记忆片段向量搜索失败");
            e
        })?;
    info!(result_count = ?results.len(), "记忆片段搜索完成");
    Ok(results)
}
