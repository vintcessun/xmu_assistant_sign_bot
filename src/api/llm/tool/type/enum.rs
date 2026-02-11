use crate::api::llm::tool::LlmPrompt;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize)]
pub struct LlmEnum<T>(pub T);

// 实现 Deref 方便直接当做 T 使用
impl<T> std::ops::Deref for LlmEnum<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de, T: Deserialize<'de> + LlmPrompt + 'static> Deserialize<'de> for LlmEnum<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct EnumWrapper<T> {
            #[serde(rename = "$value")]
            content: T,
        }
        EnumWrapper::<T>::deserialize(deserializer).map(|w| LlmEnum(w.content))
    }
}

impl<T: LlmPrompt> LlmPrompt for LlmEnum<T> {
    fn get_prompt_schema() -> &'static str {
        T::get_prompt_schema()
    }

    fn root_name() -> &'static str {
        T::root_name()
    }
}
