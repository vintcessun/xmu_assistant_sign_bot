use crate::api::llm::tool::config::{AK, ENDPOINT, REGION, SK};
use crate::api::scheduler::{TaskRunner, TimeTask};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tracing::{debug, trace};
use volcengine_rs::send_request;

pub struct UsageTask;

#[async_trait]
impl TimeTask for UsageTask {
    type Output = Vec<&'static str>;

    fn interval(&self) -> Duration {
        Duration::from_secs(20)
    }

    fn name(&self) -> &'static str {
        "UsageTask"
    }

    async fn run(&self) -> Result<Self::Output> {
        let ret = get_top_k_model().await?;
        Ok(ret.into_iter().map(|info| info.name).collect())
    }
}

const ALL_MODEL: [&str; 20] = [
    "ep-20260225003643-2w6k5",
    "ep-20260225232137-4rlsz",
    "ep-20260227171140-2c4bc",
    "ep-20260227174622-h4kf4",
    "ep-20260227174722-zqdw2",
    "ep-20260227174817-xm7wq",
    "ep-20260227174956-dwmtd",
    "ep-20260227175127-ms226",
    "ep-20260227175746-z4rcs",
    "ep-20260227175835-ltsn2",
    "ep-20260227175918-ss5zl",
    "ep-20260227175955-4pmnf",
    "ep-20260227180029-7dbz4",
    "ep-20260227180130-l9bs6",
    "ep-20260227180228-4m625",
    "ep-20260227181008-5kd4s",
    "ep-20260310195544-5c4d6",
    "ep-20260310195826-75hfq",
    "ep-20260310195926-b54k5",
    "ep-20260310200006-x4mdw",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UsageResponse {
    result: UsageResponseInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UsageResponseInner {
    pub usage_results: Vec<UsageResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UsageResult {
    pub metric_items: Vec<UsageMetricItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UsageMetricItem {
    pub values: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: &'static str,
    pub usage: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Value {
    pub value: usize,
}

pub async fn get_top_k_model() -> Result<Vec<ModelInfo>> {
    let today_zero = chrono::Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap_or_default()
        .and_utc()
        .timestamp();
    let time = chrono::Utc::now().timestamp();
    let mut ret = Vec::with_capacity(ALL_MODEL.len());
    let mut query_params = BTreeMap::new();
    query_params.insert("Version", "2024-01-01");
    query_params.insert("Action", "GetUsage");

    for model in ALL_MODEL {
        let result = send_request::<UsageResponse>(
            AK,
            SK,
            ENDPOINT,
            REGION,
            "ark",
            "POST",
            "application/json",
            query_params.clone(),
            json!({
                "EndpointIds": [model],
                "StartTime": today_zero,
                "EndTime": time,
                "Interval": 3600,
            }),
        )
        .await
        .map_err(|e| anyhow!("请求模型使用量接口失败: {:?}", e))?;
        let mut total = 0;
        for usage_result in result.result.usage_results {
            for metric_item in usage_result.metric_items {
                for value in metric_item.values {
                    total += value.value;
                }
            }
        }
        trace!(model = model, usage = total, "模型使用量");
        ret.push(ModelInfo {
            name: model,
            usage: total,
        });
    }
    ret.sort_by_key(|info| info.usage);
    debug!(models = ?ret, "所有模型使用量");
    let ret = ret
        .into_iter()
        .filter(|x| x.usage < 1500000)
        .collect::<Vec<_>>();
    Ok(ret)
}

static TASK: LazyLock<Arc<TaskRunner<UsageTask>>> = LazyLock::new(|| TaskRunner::new(UsageTask));

pub async fn router(num: usize) -> Option<&'static str> {
    let list = TASK.get_latest().await.ok()?;
    list.get((num - 1) % list.len()).copied()
}

pub fn all_num() -> usize {
    ALL_MODEL.len()
}

#[cfg(test)]
mod tests {
    use super::super::config::MODEL_MAP;
    use super::*;

    #[test]
    fn test_model_in_config() {
        for model in ALL_MODEL {
            assert!(MODEL_MAP.contains_key(model), "模型 {} 不在配置中", model);
        }
    }

    #[tokio::test]
    async fn get_model_usage() -> Result<()> {
        let ret = get_top_k_model().await?;
        println!("模型使用情况: ");
        for info in ret {
            println!("模型: {}, 使用量: {}", info.name, info.usage);
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_get_model_usage() -> Result<()> {
        let today_zero = chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default()
            .and_utc()
            .timestamp();
        let time = chrono::Utc::now().timestamp();
        let model = "ep-20260225003643-2w6k5";
        let mut query_params = BTreeMap::new();
        query_params.insert("Version", "2024-01-01");
        query_params.insert("Action", "GetUsage");

        let result = send_request::<serde_json::Value>(
            AK,
            SK,
            ENDPOINT,
            REGION,
            "ark",
            "POST",
            "application/json",
            query_params,
            json!({
                "EndpointIds": [model],
                "StartTime": today_zero,
                "EndTime": time,
                "Interval": 3600,
            }),
        )
        .await
        .unwrap();

        println!("原始响应: {:?}", result);

        let result = serde_json::from_value::<UsageResponse>(result)?;

        println!("解析结果: {:?}", result);

        Ok(())
    }
}
