use super::config::MODEL_MAP;
use crate::api::llm::tool::choose::{all_num, router};
use anyhow::Result;
use genai::{
    Client, ModelIden, ServiceTarget,
    chat::{ChatMessage, ChatResponse},
    resolver::{AuthData, AuthResolver, Endpoint, ServiceTargetResolver},
};
use llm_xml_caster::{LlmPrompt, generate_as_with_retries};
use serde::de::DeserializeOwned;
use std::sync::LazyLock;
use tracing::{debug, error, info, trace, warn};

const LOW_MODEL: &str = "gemini-3.1-flash-lite";
const HIGH_MODEL: &str = "gemini-3.1-flash-lite";

pub static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    info!(
        low_model = LOW_MODEL,
        high_model = HIGH_MODEL,
        "初始化 LLM 客户端"
    );
    // 1. AuthResolver
    let auth_resolver = AuthResolver::from_resolver_fn(|model_id: ModelIden| {
        trace!(
            model_name = %model_id.model_name,
            adapter_kind = ?model_id.adapter_kind,
            "尝试寻找模型的认证配置"
        );
        // 逻辑：根据模型名从 HashMap 中查找
        let config = MODEL_MAP.get(&*model_id.model_name);

        if let Some(cfg) = config {
            // 尝试从环境变量读取
            // 尝试从环境变量读取
            if let Ok(key) = std::env::var(cfg.api_key_env) {
                debug!(
                    api_key_env = %cfg.api_key_env,
                    "成功从环境变量加载 API 密钥"
                );
                return Ok(Some(AuthData::from_single(key)));
            }
            // 如果环境变量不存在，直接把 api_key_env 字符串本身当作 Key (兼容你目前的写法)
            debug!(
                api_key_env = %cfg.api_key_env,
                "成功从硬编码配置加载 API 密钥"
            );
            #[cfg(test)]
            println!("成功从硬编码配置加载 API 密钥: {}", cfg.api_key_env);

            return Ok(Some(AuthData::from_single(cfg.api_key_env.to_string())));
        }
        warn!(adapter_kind = ?model_id.adapter_kind, "未找到有效的 API 密钥");
        Ok(None)
    });

    // 2. ServiceTargetResolver (修正逻辑)
    let target_resolver = ServiceTargetResolver::from_resolver_fn(|mut target: ServiceTarget| {
        // 注意：genai 的 model_name 可能是全称，这里用 get 匹配
        if let Some(cfg) = MODEL_MAP.get(&*target.model.model_name) {
            debug!(
                model_name = %target.model.model_name,
                base_url = %cfg.base_url,
                "为模型设置自定义 Base URL 并修正适配器类型"
            );
            target.endpoint = Endpoint::from_static(cfg.base_url);
            // 关键：在这里修正适配器类型，因为它会影响后续的请求格式
            target.model.adapter_kind = cfg.kind;
        } else {
            debug!(
                model_name = %target.model.model_name,
                "使用默认 Base URL"
            );
        }
        Ok(target)
    });

    Client::builder()
        .with_auth_resolver(auth_resolver)
        .with_service_target_resolver(target_resolver)
        .build()
});

pub async fn ask(message: Vec<ChatMessage>) -> Result<ChatResponse> {
    trace!("开始调用 LLM: {}", HIGH_MODEL);
    let chat_req = genai::chat::ChatRequest::new(message);
    let res = CLIENT
        .exec_chat(HIGH_MODEL, chat_req, None)
        .await
        .map_err(|e| {
            error!(model_name = HIGH_MODEL, error = ?e, "LLM 调用失败");
            e
        })?;
    trace!(response = ?res, "LLM 调用成功");
    Ok(res)
}

pub async fn ask_as<T>(message: Vec<ChatMessage>, valid_example: &str) -> Result<T>
where
    T: DeserializeOwned + LlmPrompt,
{
    let mut i = 1;
    loop {
        if i > 100 {
            return Err(anyhow::anyhow!("LLM 结构化调用失败，已达到最大重试次数"));
        }
        let model_name = if i <= all_num() * 2
            && let Some(_model) = router(i).await
        {
            #[cfg(not(test))]
            {
                _model
            }
            #[cfg(test)]
            {
                LOW_MODEL
            }
        } else {
            LOW_MODEL
        };

        #[cfg(test)]
        println!("使用的模型: {}", model_name);

        trace!("开始调用 LLM (结构化模式): {} 第 {} 次", model_name, i);
        let result =
            generate_as_with_retries(&CLIENT, model_name, message.clone(), valid_example, 10).await;
        match result {
            Ok(res) => return Ok(res),
            Err(e) => {
                let err_str = format!("{:?}", e);
                if err_str.contains("unknown format") && err_str.contains("ResponseFailedStatus") {
                    return Err(anyhow::anyhow!(
                        "错误的消息:\n{}",
                        serde_json::to_string(&message)?
                    ));
                }
                #[cfg(test)]
                println!("LLM 结构化调用失败错误信息: {:?} 第 {} 次", e, i);
                debug!(model_name = model_name, error = ?e, "LLM 结构化调用失败 第 {} 次", i);
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
        i += 1;
        // 本地随便造不设置重试次数了
    }
}

pub async fn ask_as_high<T>(message: Vec<ChatMessage>, valid_example: &str) -> Result<T>
where
    T: DeserializeOwned + LlmPrompt,
{
    //ask_as(message, valid_example).await
    Ok(generate_as_with_retries(&CLIENT, HIGH_MODEL, message.clone(), valid_example, 3).await?)
}

pub async fn ask_str(chat_message: Vec<ChatMessage>) -> Result<String> {
    trace!("开始调用 LLM (字符串模式): {}", LOW_MODEL);
    let chat_req = genai::chat::ChatRequest::new(chat_message);
    let res = CLIENT
        .exec_chat(LOW_MODEL, chat_req, None)
        .await
        .map_err(|e| {
            error!(model_name = LOW_MODEL, error = ?e, "LLM 字符串调用失败");
            e
        })?;
    let text = res.first_text().ok_or_else(|| {
        error!(
            model_name = LOW_MODEL,
            "LLM 返回空响应，无法获取文本内容 (字符串模式)"
        );
        anyhow::anyhow!("No response")
    })?;
    trace!(response = %text, "LLM 字符串调用成功");
    Ok(text.to_string())
}

#[cfg(test)]
pub mod mock_client {

    use anyhow::Result;
    use async_trait::async_trait;
    use serde::Serialize;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Utf8Bytes;

    use crate::abi::{
        echo::Echo,
        message::{Params, api::ApiResponsePending},
        network::BotClient,
        websocket::BotHandler,
    };
    #[derive(Debug)]
    pub struct MockClient;

    #[async_trait]
    impl BotClient for MockClient {
        async fn call_api<'a, R: Params + Serialize + std::fmt::Debug>(
            &'a self,
            _request: &'a R,
            _echo: Echo,
        ) -> Result<ApiResponsePending<R::Response>> {
            // 模拟异步操作的开销，使其更符合实际分发工作中的 I/O 等待
            tokio::task::yield_now().await;
            // 返回一个 ApiResponsePending 实例
            Ok(ApiResponsePending::new(Echo::new()))
        }
    }

    #[async_trait]
    impl BotHandler for MockClient {
        async fn on_connect(&self) {
            // do nothing
        }
        async fn on_disconnect(&self) {
            // do nothing
        }
        async fn init(
            &self,
            _event: mpsc::UnboundedSender<String>,
            _api: mpsc::UnboundedSender<String>,
        ) -> Result<()> {
            Ok(())
        }
        async fn handle_api(&self, _message: Utf8Bytes) {
            // This is a Mock, no-op
        }
        async fn handle_event(&self, _event: Utf8Bytes) {
            // This is a Mock, no-op
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        abi::logic_import::Message,
        api::llm::chat::{
            archive::bridge::llm_msg_from_message_without_archive,
            search::send::{SEARCH_MESSAGE_REPLY_VALID_EXAMPLE, SearchMessageReply},
        },
    };

    use super::*;

    const MSG_SRC_JSON: &str = r#"
{
  "self_id": 1363408373,
  "user_id": 2218870695,
  "time": 1770452410,
  "message_id": 1253893250,
  "message_seq": 1253893250,
  "real_id": 1253893250,
  "real_seq": "10833",
  "message_type": "group",
  "sender": {
    "user_id": 2218870695,
    "nickname": "恒星",
    "card": "主人",
    "role": "owner"
  },
  "raw_message": "[CQ:image,file=AF0239D1AA177A18E979D76F303C9225.jpg,sub_type=0,url=https://disk.sample.cat/samples/jpg/monalisa-100x100.jpg]",
  "font": 14,
  "sub_type": "normal",
  "message": [
    {
      "type": "image",
      "data": {
        "summary": "",
        "file": "AF0239D1AA177A18E979D76F303C9225.jpg",
        "sub_type": 0,
        "url": "https://disk.sample.cat/samples/jpg/monalisa-100x100.jpg",
        "file_size": "244425"
      }
    }
  ],
  "message_format": "array",
  "post_type": "message",
  "group_id": 536405397,
  "group_name": "测试"
}
"#;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_ask_as() -> Result<()> {
        let client = Arc::new(mock_client::MockClient);

        let msg = serde_json::from_str::<Message>(MSG_SRC_JSON)?;

        //println!("原消息: {:?}", msg);

        let msg_chat = llm_msg_from_message_without_archive(client, &msg).await;

        let chat_messages = [
            vec![ChatMessage::system(
                "你是一个智能的理解用户回复的助手，请根据用户的提问和上下文进行回复的生成",
            )],
            msg_chat,
        ]
        .concat();

        //println!("请求的 ChatMessage: {:?}", chat_messages);

        let message =
            ask_as::<SearchMessageReply>(chat_messages, SEARCH_MESSAGE_REPLY_VALID_EXAMPLE).await?;

        println!("解析后的结构化数据: {:?}", message);

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_ask_as_high() -> Result<()> {
        let client = Arc::new(mock_client::MockClient);

        let msg = serde_json::from_str::<Message>(MSG_SRC_JSON)?;

        //println!("原消息: {:?}", msg);

        let msg_chat = llm_msg_from_message_without_archive(client, &msg).await;

        let chat_messages = [
            vec![ChatMessage::system(
                "你是一个智能的理解用户回复的助手，请根据用户的提问和上下文进行回复的生成",
            )],
            msg_chat,
        ]
        .concat();

        //println!("请求的 ChatMessage: {:?}", chat_messages);

        let message =
            ask_as_high::<SearchMessageReply>(chat_messages, SEARCH_MESSAGE_REPLY_VALID_EXAMPLE)
                .await?;

        println!("解析后的结构化数据: {:?}", message);

        Ok(())
    }
}
