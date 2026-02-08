use ahash::RandomState;
use anyhow::Result;
use dashmap::DashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::time;
use tokio_tungstenite::tungstenite::Utf8Bytes;
use tracing::{debug, error, trace, warn};

static COUNTER: AtomicU64 = AtomicU64::new(1);
static TIMEOUT: Duration = Duration::from_secs(600);

static RESPONSE_REGISTRY: LazyLock<DashMap<u64, oneshot::Sender<Utf8Bytes>, RandomState>> =
    LazyLock::new(|| DashMap::with_hasher(RandomState::default()));

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct Echo(u64);

impl Default for Echo {
    fn default() -> Self {
        Self::new()
    }
}

impl Echo {
    pub fn new() -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        trace!(echo_id = ?id, "生成新的 Echo ID");
        // 依赖 COUNTER 递增保证唯一性，移除 DashSet 操作和 loop
        Self(id)
    }
}

impl Serialize for Echo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Echo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct EchoVisitor;

        impl<'v> de::Visitor<'v> for EchoVisitor {
            type Value = Echo;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string representing a u64 echo id")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                v.parse::<u64>().map(Echo).map_err(|e| {
                    warn!(input_str = ?v, error = ?e, "无法解析 Echo ID 字符串");
                    de::Error::custom(format!("invalid echo format: {}", v))
                })
            }
        }

        deserializer.deserialize_str(EchoVisitor)
    }
}

pub fn echo_send_result(echo: &str, response: Utf8Bytes) {
    match echo.parse::<u64>() {
        Ok(echo_id) => {
            if let Some(entry) = RESPONSE_REGISTRY.remove(&echo_id) {
                let sender = entry.1;
                if let Err(_response) = sender.send(response) {
                    warn!(echo_id = ?echo_id, "无法发送 Echo 响应，接收端可能已关闭或超时");
                } else {
                    trace!(echo_id = ?echo_id, "Echo 响应已成功发送");
                }
            } else {
                trace!(echo_id = ?echo_id, "未找到对应的 Echo 注册信息，可能已超时或已处理");
            }
        }
        Err(e) => {
            warn!(echo_str = ?echo, error = ?e, "无法解析 Echo ID 字符串");
        }
    }
}

pub struct EchoPending {
    pub echo: Echo,
    receiver: oneshot::Receiver<Utf8Bytes>,
}

impl EchoPending {
    pub fn new(echo: Echo) -> Self {
        let (tx, rx) = oneshot::channel();
        RESPONSE_REGISTRY.insert(echo.0, tx);
        trace!(echo = ?echo, "注册新的 Echo 待处理请求");
        Self { echo, receiver: rx }
    }

    pub async fn wait(self) -> Result<Utf8Bytes> {
        let ret = match time::timeout(TIMEOUT, self.receiver).await {
            Ok(Ok(response)) => {
                debug!(echo = ?self.echo, "成功接收到 Echo 响应");
                Ok(response)
            }
            Ok(Err(e)) => {
                error!(echo = ?self.echo, error = ?e, "收到的 Echo 响应通道已关闭");
                Err(anyhow::anyhow!("收到的响应通道已关闭"))
            }
            Err(e) => {
                error!(echo = ?self.echo, error = ?e, "等待 Echo 响应超时");
                Err(anyhow::anyhow!("等待 Echo 响应超时"))
            }
        };

        // 确保 RESPONSE_REGISTRY 被清理，修复可能的内存泄漏。
        RESPONSE_REGISTRY.remove(&self.echo.0);
        debug!(echo = ?self.echo, "清理 Echo 注册信息");

        ret
    }
}
