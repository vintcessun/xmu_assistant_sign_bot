use serde::{Deserialize, Deserializer, Serialize};
use std::ops::{Deref, DerefMut};

use crate::api::llm::tool::LlmPrompt;

#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq, Eq)]
#[serde(transparent)]
pub struct LlmBool(pub bool);

impl Deref for LlmBool {
    type Target = bool;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LlmBool {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<bool> for LlmBool {
    fn from(b: bool) -> Self {
        LlmBool(b)
    }
}

impl From<LlmBool> for bool {
    fn from(lb: LlmBool) -> Self {
        lb.0
    }
}

impl PartialEq<bool> for LlmBool {
    fn eq(&self, other: &bool) -> bool {
        self.0 == *other
    }
}

impl PartialEq<LlmBool> for bool {
    fn eq(&self, other: &LlmBool) -> bool {
        *self == other.0
    }
}

impl std::ops::Not for LlmBool {
    type Output = bool;
    fn not(self) -> Self::Output {
        !self.0
    }
}

impl std::ops::Not for &LlmBool {
    type Output = bool;
    fn not(self) -> Self::Output {
        !self.0
    }
}

impl<'de> Deserialize<'de> for LlmBool {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        match s.trim().to_lowercase().as_str() {
            // 真值全家桶
            "true" | "1" | "yes" | "y" | "t" | "on" | "真" | "checked" | "selected" => {
                Ok(LlmBool(true))
            }
            // 假值全家桶
            "false" | "0" | "no" | "n" | "f" | "off" | "假" | "null" | "none" | "" => {
                Ok(LlmBool(false))
            }
            // 如果 LLM 输出了其他胡言乱语，默认报错
            _ => Err(serde::de::Error::custom(format!(
                "无法将字符串 '{}' 解析为布尔值",
                s
            ))),
        }
    }
}

impl LlmPrompt for LlmBool {
    fn get_prompt_schema() -> &'static str {
        "布尔值，取值为 true 或 false"
    }
    fn root_name() -> &'static str {
        "bool"
    }
}
