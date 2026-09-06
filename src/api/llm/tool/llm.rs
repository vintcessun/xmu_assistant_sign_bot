use super::config::MODEL_MAP;
use anyhow::Result;
use genai::{
    Client, ModelIden, ServiceTarget,
    chat::ChatMessage,
    resolver::{AuthData, AuthResolver, Endpoint, ServiceTargetResolver},
};
use llm_xml_caster::{LlmPrompt, generate_as_with_retries};
use serde::de::DeserializeOwned;
use std::sync::LazyLock;
use tracing::{debug, info, trace, warn};

/// 用于必要的 LLM 选择（课程/文件/课表等结构化选择）的模型。
/// 固定为 llmux 网关的 `auto`：由网关自己跑整条链选后端，客户端不钉死某一家。
const MODEL: &str = "auto";

pub static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    info!(model = MODEL, "初始化 LLM 客户端");
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
            if let Ok(key) = std::env::var(cfg.api_key_env) {
                debug!(
                    api_key_env = %cfg.api_key_env,
                    "成功从环境变量加载 API 密钥"
                );
                return Ok(Some(AuthData::from_single(key)));
            }
            // 如果环境变量不存在，直接把 api_key_env 字符串本身当作 Key
            debug!(
                api_key_env = %cfg.api_key_env,
                "成功从硬编码配置加载 API 密钥"
            );
            return Ok(Some(AuthData::from_single(cfg.api_key_env.to_string())));
        }
        warn!(adapter_kind = ?model_id.adapter_kind, "未找到有效的 API 密钥");
        Ok(None)
    });

    // 2. ServiceTargetResolver
    let target_resolver = ServiceTargetResolver::from_resolver_fn(|mut target: ServiceTarget| {
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

/// 结构化 LLM 调用：用于必要的 LLM 选择。
pub async fn ask_as<T>(message: Vec<ChatMessage>, valid_example: &str) -> Result<T>
where
    T: DeserializeOwned + LlmPrompt,
{
    let mut i = 1;
    loop {
        if i > 100 {
            return Err(anyhow::anyhow!("LLM 结构化调用失败，已达到最大重试次数"));
        }

        trace!("开始调用 LLM (结构化模式): {} 第 {} 次", MODEL, i);
        let result =
            generate_as_with_retries(&CLIENT, MODEL, message.clone(), valid_example, 10).await;
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
                debug!(model_name = MODEL, error = ?e, "LLM 结构化调用失败 第 {} 次", i);
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::xmu_service::llm::choose_timetable::TimetableChoiceResponseLlm;
    use crate::api::xmu_service::testenv;

    const EXAMPLE: &str = r#"
<TimetableChoiceResponseLlm>
    <semester>20261</semester>
</TimetableChoiceResponseLlm>"#;

    /// genai -> llmux 网关 -> `auto` 这条链的最小实跑验证。
    ///
    /// `auto` 不是 genai 认得的模型名前缀，端点和 adapter 全靠 `ServiceTargetResolver`
    /// 强行改写，通不通只能真发一次请求才知道。网关只监听部署机的 127.0.0.1:8787，
    /// 本机跑之前先开隧道：`ssh -N -L 8787:127.0.0.1:8787 root@vintces.icu`。
    #[tokio::test]
    async fn test_ask_as_via_llmux() -> Result<()> {
        if !testenv::network_enabled() {
            return testenv::skipped_network(module_path!());
        }

        let messages = vec![
            ChatMessage::system("根据用户需求，从下面的学期里选一个，按要求返回学年学期代码"),
            ChatMessage::system(
                "<data>学期名称: 2025-2026学年第三学期, 学年学期代码: 20253</data>",
            ),
            ChatMessage::system(
                "<data>学期名称: 2026-2027学年第一学期, 学年学期代码: 20261</data>",
            ),
            ChatMessage::user("最新学期的课表"),
        ];

        // ask_as 内部会一直重试，连不上时不设上限会挂死，这里给整体加个闸。
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            ask_as::<TimetableChoiceResponseLlm>(messages, EXAMPLE),
        )
        .await??;

        println!("llmux 返回: {:?}", response);
        assert_eq!(response.semester.trim(), "20261");
        Ok(())
    }
}
