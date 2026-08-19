use async_trait::async_trait;
use reqwest::Response;
use serde::de::DeserializeOwned;

#[async_trait]
pub trait SmartJsonExt {
    async fn json_smart<T: DeserializeOwned>(self) -> anyhow::Result<T>;
}

#[async_trait]
impl SmartJsonExt for Response {
    // --- RELEASE 模式：极致性能 ---
    #[cfg(not(debug_assertions))]
    async fn json_smart<T: DeserializeOwned>(self) -> anyhow::Result<T> {
        // 直接流式解析，不经过中间 String，性能最优
        Ok(self.json::<T>().await?)
    }

    // --- DEBUG 模式：详尽诊断 ---
    #[cfg(debug_assertions)]
    async fn json_smart<T: DeserializeOwned>(self) -> anyhow::Result<T> {
        let full_body = self.text().await?;

        match serde_json::from_str::<T>(&full_body) {
            Ok(val) => Ok(val),
            Err(e) => {
                // 打印极其详尽的调试面板
                eprintln!("\n--- [DEBUG] JSON DECODE ERROR ---");
                eprintln!("Reason: {}", e);
                eprintln!("At: Line {}, Column {}", e.line(), e.column());
                eprintln!("Raw Body:\n{}", full_body);
                eprintln!("---------------------------------\n");

                // 包装错误并返回，保留原始 serde 错误以便进一步处理
                Err(anyhow::anyhow!(e).context("JSON decoding failed in debug mode"))
            }
        }
    }
}

/// 把显式的 `null` 当作 `Default::default()` 处理。
///
/// 教务（jw）系列接口在“写操作成功”时会返回 `{"code":"0","datas":{"addJdjssq":null}}`，
/// 即 `datas` 下的动态字段是 `null` 而不是对象；配合 `#[serde(default)]` 才能同时覆盖
/// “字段缺失”（错误响应里根本没有 `datas`）和“字段为 null”两种情况。
pub fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + Default,
{
    Ok(<Option<T> as serde::Deserialize>::deserialize(deserializer)?.unwrap_or_default())
}
