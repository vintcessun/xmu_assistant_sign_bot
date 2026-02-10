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
use tracing::{debug, error, info, trace, warn};

const MODEL_NAME: &str = "gemini-2.0-flash";

pub static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    info!(model = MODEL_NAME, "初始化 LLM 客户端");
    // 1. AuthResolver
    let auth_resolver = AuthResolver::from_resolver_fn(|model_id: ModelIden| {
        trace!(
            adapter_kind = ?model_id.adapter_kind,
            "尝试为模型适配器寻找认证配置"
        );
        // 关键：我们要找的是匹配当前 adapter_kind 的配置
        let config = MODEL_MAP
            .values()
            .find(|cfg| cfg.kind == model_id.adapter_kind);

        if let Some(cfg) = config {
            // 尝试从环境变量读取
            if let Ok(key) = std::env::var(cfg.api_key_env) {
                info!(
                    api_key_env = %cfg.api_key_env,
                    "成功从环境变量加载 API 密钥"
                );
                return Ok(Some(AuthData::from_single(key)));
            }
            // 如果环境变量不存在，直接把 api_key_env 字符串本身当作 Key (兼容你目前的写法)
            if cfg.api_key_env.starts_with("sk-") {
                info!(
                    api_key_env = %cfg.api_key_env,
                    "成功从硬编码配置加载 API 密钥"
                );
                return Ok(Some(AuthData::from_single(cfg.api_key_env.to_string())));
            }
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
                "为模型设置自定义 Base URL"
            );
            target.endpoint = Endpoint::from_static(cfg.base_url);
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

pub trait LlmPrompt {
    fn get_prompt_schema() -> &'static str;
    fn root_name() -> &'static str;
}

pub async fn ask(message: Vec<ChatMessage>) -> Result<ChatResponse> {
    trace!("开始调用 LLM: {}", MODEL_NAME);
    let chat_req = genai::chat::ChatRequest::new(message);
    let res = CLIENT
        .exec_chat(MODEL_NAME, chat_req, None)
        .await
        .map_err(|e| {
            error!(model_name = MODEL_NAME, error = ?e, "LLM 调用失败");
            e
        })?;
    trace!(response = ?res, "LLM 调用成功");
    Ok(res)
}

pub async fn ask_as<T>(message: Vec<ChatMessage>) -> Result<T>
where
    T: DeserializeOwned + LlmPrompt,
{
    // 1. 自动注入 Schema 指令
    let mut chat_message = [
        message,
        vec![
            ChatMessage::system(format!(
                "你必须直接返回 XML 格式的数据，禁止任何开场白。当前root_name为 {}，格式规范如下：",
                T::root_name()
            )),
            ChatMessage::system(T::get_prompt_schema()),
        ],
    ]
    .concat();

    let mut err = anyhow!("未知原因解析失败");

    for attempt in 1..=3 {
        debug!(attempt = attempt, "尝试调用 LLM 获取结构化回复");
        // 2. 调用 genai
        let chat_req = genai::chat::ChatRequest::new(chat_message.clone());
        let res = CLIENT
            .exec_chat(MODEL_NAME, chat_req, None)
            .await
            .map_err(|e| {
                error!(model_name = MODEL_NAME, error = ?e, "LLM 结构化调用失败");
                e
            })?;
        let text = res.first_text().ok_or_else(|| {
            error!(model_name = MODEL_NAME, "LLM 返回空响应，无法获取文本内容");
            anyhow::anyhow!("No response")
        })?;

        // 3. XML 清洗与反序列化 (根据 root_name 精确截取)
        let root_name = T::root_name();
        let start_tag = format!("<{}>", root_name);
        let end_tag = format!("</{}>", root_name);

        let xml_content: &str;
        let data: Result<T>;

        if let (Some(xml_start), Some(xml_end_tag_start)) =
            (text.find(&start_tag), text.rfind(&end_tag))
        {
            let xml_end = xml_end_tag_start + end_tag.len();
            xml_content = &text[xml_start..xml_end];
            data = from_str(xml_content)
                .map_err(|e| anyhow!("XML反序列化错误：{e}\n模型返回的内容为：\n{xml_content}"));
        } else {
            // 无法找到根标签，直接进入错误处理逻辑
            let tag_error = anyhow!(
                "XML提取失败：无法找到完整的根标签 <{}>...</{}>。模型返回的内容为：\n{}",
                root_name,
                root_name,
                text
            );
            warn!(error = ?tag_error, root_name = root_name, "LLM XML 标签解析失败，准备重试");

            // 构造重试消息，这里使用完整的文本作为 '之前回复'
            chat_message.push(ChatMessage::assistant(format!(
                "之前回复:{}\n报错:{}\n",
                text, tag_error
            )));
            chat_message.push(ChatMessage::system(
                "你上次返回的内容格式有误，请严格按照要求的 XML 格式返回。",
            ));

            err = tag_error;
            continue;
        };

        match data {
            Ok(v) => {
                info!(root_name = T::root_name(), "LLM 结构化回复解析成功");
                return Ok(v);
            }
            Err(e) => {
                warn!(error = ?e, root_name = T::root_name(), "LLM XML 数据反序列化失败，准备重试");

                chat_message.push(ChatMessage::assistant(format!(
                    "之前回复:{}\n报错:{}\n",
                    xml_content, e
                )));
                chat_message.push(ChatMessage::system(format!(
                    "你上次返回的内容格式有误，请严格按照要求的 XML 格式返回。格式如下: {}",
                    T::get_prompt_schema(),
                )));

                err = e;
            }
        }
    }
    error!(error = ?err, "LLM 结构化回复多次重试解析失败");
    Err(anyhow!("LLM 解析多次失败{err}"))
}

pub async fn ask_str(chat_message: Vec<ChatMessage>) -> Result<String> {
    trace!("开始调用 LLM (字符串模式): {}", MODEL_NAME);
    let chat_req = genai::chat::ChatRequest::new(chat_message);
    let res = CLIENT
        .exec_chat(MODEL_NAME, chat_req, None)
        .await
        .map_err(|e| {
            error!(model_name = MODEL_NAME, error = ?e, "LLM 字符串调用失败");
            e
        })?;
    let text = res.first_text().ok_or_else(|| {
        error!(
            model_name = MODEL_NAME,
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
