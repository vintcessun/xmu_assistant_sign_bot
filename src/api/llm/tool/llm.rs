use std::sync::LazyLock;

use super::config::MODEL_MAP;
use anyhow::{Result, anyhow};
use genai::{
    Client, ModelIden, ServiceTarget,
    chat::ChatMessage,
    resolver::{AuthData, AuthResolver, Endpoint, ServiceTargetResolver},
};
use quick_xml::de::from_str;
use serde::de::DeserializeOwned; // 重新导出宏

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

pub async fn ask_as<T>(mut chat_message: Vec<ChatMessage>) -> Result<T>
where
    T: DeserializeOwned + LlmPrompt,
{
    // 1. 自动注入 Schema 指令
    let schema = T::get_prompt_schema();

    chat_message.push(ChatMessage::system(
        "你必须直接返回 XML 格式的数据，禁止任何开场白。格式规范如下：",
    ));
    chat_message.push(ChatMessage::system(schema));

    // 2. 调用 genai
    let chat_req = genai::chat::ChatRequest::new(chat_message);
    let res = CLIENT.exec_chat(MODEL_NAME, chat_req, None).await?;
    let text = res
        .first_text()
        .ok_or_else(|| anyhow::anyhow!("No response"))?;

    // 3. XML 清洗与反序列化
    let xml_start = text.find('<').unwrap_or(0);
    let xml_end = text.rfind('>').map(|i| i + 1).unwrap_or(text.len());
    let xml_content = &text[xml_start..xml_end];

    let data: T = from_str(xml_content)
        .map_err(|e| anyhow!("错误为：{e}\n模型返回的内容为：\n{xml_content}"))?;
    Ok(data)
}

pub async fn ask_str(chat_message: Vec<ChatMessage>) -> Result<String> {
    let chat_req = genai::chat::ChatRequest::new(chat_message);
    let res = CLIENT.exec_chat(MODEL_NAME, chat_req, None).await?;
    let text = res
        .first_text()
        .ok_or_else(|| anyhow::anyhow!("No response"))?;
    Ok(text.to_string())
}
