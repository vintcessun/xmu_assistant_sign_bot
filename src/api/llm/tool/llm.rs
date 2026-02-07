use std::sync::LazyLock;

use super::config::MODEL_MAP;
use anyhow::{Result, anyhow};
use genai::{
    Client, ModelIden, ServiceTarget,
    chat::{ChatMessage, ChatResponse},
    resolver::{AuthData, AuthResolver, Endpoint, ServiceTargetResolver},
};
use quick_xml::de::from_str;
use serde::de::DeserializeOwned;
use tracing::error;

const MODEL_NAME: &str = "gemini-2.0-flash";

pub static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    // 1. AuthResolver
    let auth_resolver = AuthResolver::from_resolver_fn(|model_id: ModelIden| {
        // 关键：我们要找的是匹配当前 adapter_kind 的配置
        let config = MODEL_MAP
            .values()
            .find(|cfg| cfg.kind == model_id.adapter_kind);

        if let Some(cfg) = config {
            // 尝试从环境变量读取
            if let Ok(key) = std::env::var(cfg.api_key_env) {
                return Ok(Some(AuthData::from_single(key)));
            }
            // 如果环境变量不存在，直接把 api_key_env 字符串本身当作 Key (兼容你目前的写法)
            if cfg.api_key_env.starts_with("sk-") {
                return Ok(Some(AuthData::from_single(cfg.api_key_env.to_string())));
            }
        }
        Ok(None)
    });

    // 2. ServiceTargetResolver (修正逻辑)
    let target_resolver = ServiceTargetResolver::from_resolver_fn(|mut target: ServiceTarget| {
        // 注意：genai 的 model_name 可能是全称，这里用 get 匹配
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

pub trait LlmPrompt {
    fn get_prompt_schema() -> &'static str;
    fn root_name() -> &'static str;
}

pub async fn ask(message: Vec<ChatMessage>) -> Result<ChatResponse> {
    let chat_req = genai::chat::ChatRequest::new(message);
    let res = CLIENT.exec_chat(MODEL_NAME, chat_req, None).await?;
    Ok(res)
}

pub async fn ask_as<T>(message: Vec<ChatMessage>) -> Result<T>
where
    T: DeserializeOwned + LlmPrompt,
{
    // 1. 自动注入 Schema 指令
    let schema = T::get_prompt_schema();

    let mut chat_message = [
        message,
        vec![
            ChatMessage::system("你必须直接返回 XML 格式的数据，禁止任何开场白。格式规范如下："),
            ChatMessage::system(schema),
        ],
    ]
    .concat();

    let mut err = anyhow!("未知原因解析失败");

    for _ in 0..3 {
        // 2. 调用 genai
        let chat_req = genai::chat::ChatRequest::new(chat_message.clone());
        let res = CLIENT.exec_chat(MODEL_NAME, chat_req, None).await?;
        let text = res
            .first_text()
            .ok_or_else(|| anyhow::anyhow!("No response"))?;

        // 3. XML 清洗与反序列化
        let xml_start = text.find('<').unwrap_or(0);
        let xml_end = text.rfind('>').map(|i| i + 1).unwrap_or(text.len());
        let xml_content = &text[xml_start..xml_end];
        let data: Result<T> = from_str(xml_content)
            .map_err(|e| anyhow!("错误为：{e}\n模型返回的内容为：\n{xml_content}"));

        match data {
            Ok(v) => return Ok(v),
            Err(e) => {
                error!("LLM 解析失败，准备重试：{}", e);

                chat_message.push(ChatMessage::assistant(format!(
                    "之前回复:{}\n报错:{}\n",
                    xml_content, e
                )));
                chat_message.push(ChatMessage::system(
                    "你上次返回的内容格式有误，请严格按照要求的 XML 格式返回。",
                ));

                err = e;
            }
        }
    }
    Err(anyhow!("LLM 解析多次失败{err}"))
}

pub async fn ask_str(chat_message: Vec<ChatMessage>) -> Result<String> {
    let chat_req = genai::chat::ChatRequest::new(chat_message);
    let res = CLIENT.exec_chat(MODEL_NAME, chat_req, None).await?;
    let text = res
        .first_text()
        .ok_or_else(|| anyhow::anyhow!("No response"))?;
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
            archive::bridge::llm_msg_from_message_without_archive, search::send::SearchMessageReply,
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
  "raw_message": "[CQ:image,file=AF0239D1AA177A18E979D76F303C9225.jpg,sub_type=0,url=https://multimedia.nt.qq.com.cn/download?appid=1407&amp;fileid=EhTqSnOJqWbLUDdYWr1_hfl0AheunhjJ9Q4g_woo_9fOl_nGkgMyBHByb2RQgL2jAVoQH-_JaMCwsME5qzTQc3RR8HoC5vaCAQJneg&amp;rkey=CAMSMHAIqkgX9guztt4pjMZnAuDmQAsRPlgBx6ehaite6o85Ua1MOar_FdV7_YiZLkksbQ,file_size=244425]",
  "font": 14,
  "sub_type": "normal",
  "message": [
    {
      "type": "image",
      "data": {
        "summary": "",
        "file": "AF0239D1AA177A18E979D76F303C9225.jpg",
        "sub_type": 0,
        "url": "https://multimedia.nt.qq.com.cn/download?appid=1407&fileid=EhTqSnOJqWbLUDdYWr1_hfl0AheunhjJ9Q4g_woo_9fOl_nGkgMyBHByb2RQgL2jAVoQH-_JaMCwsME5qzTQc3RR8HoC5vaCAQJneg&rkey=CAMSMHAIqkgX9guztt4pjMZnAuDmQAsRPlgBx6ehaite6o85Ua1MOar_FdV7_YiZLkksbQ",
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

        println!("原消息: {:?}", msg);

        let msg_chat = llm_msg_from_message_without_archive(client, &msg).await;

        let chat_messages = [
            vec![ChatMessage::system(
                "你是一个智能的理解用户回复的助手，请根据用户的提问和上下文进行回复的生成",
            )],
            msg_chat,
        ]
        .concat();

        println!("请求的 ChatMessage: {:?}", chat_messages);

        let message = ask_as::<SearchMessageReply>(chat_messages).await?;

        println!("解析后的结构化数据: {:?}", message);

        Ok(())
    }
}
