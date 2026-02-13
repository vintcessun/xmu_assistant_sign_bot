use super::config::MODEL_MAP;
use crate::api::llm::{
    chat::{
        file::LlmFile,
        tool::{get_tools, handle_tool},
    },
    tool::ask_as,
};
use anyhow::{Result, anyhow, bail};
use genai::{
    Client, ModelIden, ServiceTarget,
    chat::{Binary, ChatMessage, ChatOptions, ChatRequest, ChatResponse, ContentPart},
    resolver::{AuthData, AuthResolver, Endpoint, ServiceTargetResolver},
};
use llm_xml_caster::llm_prompt;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tracing::{debug, error, info};

// 定义常量
const CHAT_MODEL: &str = "gemini-3-flash-preview";
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
                debug!(
                    model_kind = %model_id.adapter_kind,
                    env_var = %cfg.api_key_env,
                    "已通过环境变量加载 LLM API Key"
                );
                return Ok(Some(AuthData::from_single(key)));
            }
            // 兼容明文 sk- 写入
            if cfg.api_key_env.starts_with("sk-") {
                debug!(
                    api_key_env = %cfg.api_key_env,
                    "成功从硬编码配置加载 API 密钥"
                );
                return Ok(Some(AuthData::from_single(cfg.api_key_env.to_string())));
            }
        }
        debug!(model_kind = %model_id.adapter_kind, "未找到 LLM 模型的 API Key");
        Ok(None)
    });

    // 2. 统一路由解析器：根据模型名从 MODEL_MAP 映射 Base URL
    let target_resolver = ServiceTargetResolver::from_resolver_fn(|mut target: ServiceTarget| {
        if let Some(cfg) = MODEL_MAP.get(&*target.model.model_name) {
            info!(
                model_name = %target.model.model_name,
                base_url = %cfg.base_url,
                "LLM 服务目标已路由到配置的 Base URL"
            );
            target.endpoint = Endpoint::from_static(cfg.base_url);
        } else {
            debug!(model_name = %target.model.model_name, "未找到 LLM 模型的 Base URL 配置，使用默认路由");
        }
        Ok(target)
    });

    let client = Client::builder()
        .with_auth_resolver(auth_resolver)
        .with_service_target_resolver(target_resolver)
        .build();

    info!("LLM 客户端初始化完成");
    client
});

pub async fn get_single_text_embedding(input: String) -> Result<Vec<f32>> {
    debug!(
        model = %EMBED_MODEL,
        input_len = ?input.len(),
        "开始获取文本嵌入向量"
    );
    let embed_req = genai::embed::EmbedRequest::new(input);
    let res = CLIENT
        .exec_embed(EMBED_MODEL, embed_req, None)
        .await
        .map_err(|e| {
            error!(error = ?e, "获取文本嵌入向量时 API 调用失败");
            e
        })?;

    res.embeddings
        .into_iter()
        .next()
        .map(|e| {
            info!(
                model = %EMBED_MODEL,
                "文本嵌入向量获取成功"
            );
            e.vector
        })
        .ok_or_else(|| {
            error!("LLM 返回了空嵌入向量列表");
            anyhow!("No embedding returned")
        })
}

#[llm_prompt]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSemanticSnapshot {
    #[prompt("这是这个文件的整体的摘要")]
    pub summary: String,
    #[prompt("这是这个文件的详细描述或者关键信息点")]
    pub details: String,
    #[prompt("这是这个文件的关键词列表")]
    pub keywords: Vec<String>,
}

const CHAT_FILE_SEMANTIC_SNAPSHOT_VALID_EXAMPLE: &str = r#"
<FileSemanticSnapshot>
  <summary><![CDATA[这是这个文件的整体的摘要]]></summary>
  <details><![CDATA[这是这个文件的详细描述或者关键信息点]]></details>
  <keywords>
    <item><![CDATA[关键词1]]></item>
    <item><![CDATA[关键词2]]></item>
  </keywords>
</FileSemanticSnapshot>"#;

#[cfg(test)]
#[test]
fn test_file_semantic_snapshot_parsing() {
    let example_response =
        quick_xml::de::from_str::<FileSemanticSnapshot>(CHAT_FILE_SEMANTIC_SNAPSHOT_VALID_EXAMPLE)
            .expect("解析示例 XML 失败");
    assert_eq!(
        example_response,
        FileSemanticSnapshot {
            summary: "这是这个文件的整体的摘要".to_string(),
            details: "这是这个文件的详细描述或者关键信息点".to_string(),
            keywords: vec!["关键词1".to_string(), "关键词2".to_string()],
        }
    );
}

pub async fn get_single_file_embedding(file: &LlmFile) -> Result<Vec<f32>> {
    info!(file_name = %file.alias, "开始为文件生成嵌入向量");
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

    let response =
        ask_as::<FileSemanticSnapshot>(prompt, CHAT_FILE_SEMANTIC_SNAPSHOT_VALID_EXAMPLE)
            .await
            .map_err(|e| {
                error!(file_name = %file.alias, error = ?e, "文件语义分析失败");
                e
            })?;

    #[cfg(test)]
    println!("文件分析结果: {:?}", response);

    let result = get_single_text_embedding(format!(
        "文件名: {}\n文件摘要: {}\n详细信息: {}\n关键词: {:?}",
        filename, response.summary, response.details, response.keywords
    ))
    .await
    .map_err(|e| {
        error!(file_name = %file.alias, error = ?e, "从文件分析结果生成嵌入向量失败");
        e
    })?;

    info!(file_name = %file.alias, "文件嵌入向量生成成功");
    Ok(result)
}

#[llm_prompt]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatSemanticSnapshot {
    #[prompt("这是这些消息的整体的摘要")]
    pub summary: String,
    #[prompt("这是这些消息的详细描述或者关键信息点")]
    pub details: String,
    #[prompt("这是这些消息的关键词列表")]
    pub keywords: Vec<String>,
}

const CHAT_SEGMENT_VALID_EXAMPLE: &str = r#"
<ChatSemanticSnapshot>
    <summary><![CDATA[这是这些消息的整体的摘要]]></summary>
    <details><![CDATA[这是这些消息的详细描述或者关键信息点]]></details>
    <keywords>
        <item><![CDATA[关键词1]]></item>
        <item><![CDATA[关键词2]]></item>
    </keywords>
</ChatSemanticSnapshot>"#;

#[cfg(test)]
#[test]
fn test_chat_semantic_snapshot_parsing() {
    let example_response =
        quick_xml::de::from_str::<ChatSemanticSnapshot>(CHAT_SEGMENT_VALID_EXAMPLE)
            .expect("解析示例 XML 失败");
    assert_eq!(
        example_response,
        ChatSemanticSnapshot {
            summary: "这是这些消息的整体的摘要".to_string(),
            details: "这是这些消息的详细描述或者关键信息点".to_string(),
            keywords: vec!["关键词1".to_string(), "关键词2".to_string()],
        }
    );
}

pub async fn get_chat_embedding(messages: Vec<ChatMessage>) -> Result<Vec<f32>> {
    info!(messages_count = ?messages.len(), "开始为聊天记录生成嵌入向量");
    let msgs = [
        vec![ChatMessage::system(
            "你是一个聊天消息分析专家。请分析以下聊天内容并提取关键信息。",
        )],
        messages,
    ]
    .concat();

    let response = ask_as::<ChatSemanticSnapshot>(msgs, CHAT_SEGMENT_VALID_EXAMPLE)
        .await
        .map_err(|e| {
            error!(error = ?e, "聊天记录语义分析失败");
            e
        })?;
    let combined_text = format!(
        "消息摘要: {}\n详细信息: {}\n关键词: {:?}",
        response.summary, response.details, response.keywords
    );

    get_single_text_embedding(combined_text).await.map_err(|e| {
        error!(error = ?e, "从聊天记录分析结果生成嵌入向量失败");
        e
    })
}

pub async fn ask_llm(chat_message: Vec<ChatMessage>) -> Result<ChatResponse> {
    debug!(messages_count = ?chat_message.len(), model = %CHAT_MODEL, "向 LLM 发送聊天请求");
    let tools = get_tools();
    let options = ChatOptions::default().with_capture_tool_calls(true);
    let mut chat_req = ChatRequest::new(chat_message).with_tools(tools);
    for _ in 0..10 {
        #[cfg(test)]
        println!("发送聊天请求: {:?}", chat_req);
        let res = CLIENT
            .exec_chat(CHAT_MODEL, chat_req.clone(), Some(&options))
            .await
            .map_err(|e| {
                error!(error = ?e, "LLM 聊天请求失败");
                e
            })?;
        debug!(model = %CHAT_MODEL, "LLM 聊天请求成功");
        if res.tool_calls().is_empty() {
            return Ok(res);
        } else {
            let tool_calls = res.into_tool_calls();
            #[cfg(test)]
            println!("工具调用请求: {:?}", tool_calls);

            info!(tool_calls_count = ?tool_calls.len(), "LLM 返回了工具调用请求，准备处理工具调用");
            chat_req = chat_req.append_message(ChatMessage::assistant(
                tool_calls
                    .iter()
                    .map(|t| ContentPart::ToolCall(t.clone()))
                    .collect::<Vec<_>>(),
            ));
            for tool in tool_calls {
                debug!(fn_name = %tool.fn_name, "LLM 请求调用工具");
                let res_tool = handle_tool(tool).await;
                #[cfg(test)]
                println!("工具调用结果: {:?}", res_tool);
                chat_req = chat_req.append_message(res_tool);
            }
        }
    }
    bail!("调用次数太多，可能存在死循环");
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
            &"https://samplelib.com/lib/preview/png/sample-boat-400x300.png".to_string(),
            "sample-boat-400x300.png".to_string(),
        )
        .await?;
        let file = file.embedded().await?;
        println!("File embedding result: {:?}", file);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_chat_tool() -> Result<()> {
        let messages = vec![ChatMessage::user(
            "调用工具并使用 Python 计算斐波那契数列的第10个数字",
        )];
        let ret = ask_llm(messages).await?;
        println!("Chat tool test result: {:?}", ret);
        Ok(())
    }
}
