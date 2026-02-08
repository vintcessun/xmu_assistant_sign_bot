use serde::{Deserialize, Deserializer, Serialize};
use std::{
    fmt,
    ops::{Deref, DerefMut},
    sync::OnceLock,
};

use crate::api::llm::tool::LlmPrompt;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(transparent)]
pub struct LlmOption<T>(pub Option<T>);

impl<T> Deref for LlmOption<T> {
    type Target = Option<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for LlmOption<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<Option<T>> for LlmOption<T> {
    fn from(opt: Option<T>) -> Self {
        LlmOption(opt)
    }
}

impl<T> From<LlmOption<T>> for Option<T> {
    fn from(lo: LlmOption<T>) -> Self {
        lo.0
    }
}

impl<T: PartialEq> PartialEq<Option<T>> for LlmOption<T> {
    fn eq(&self, other: &Option<T>) -> bool {
        &self.0 == other
    }
}

impl<T> LlmOption<T> {
    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }

    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    pub fn unwrap_or(self, default: T) -> T {
        self.0.unwrap_or(default)
    }
}

impl<T: Copy> LlmOption<T> {
    pub fn get(&self) -> Option<T> {
        self.0
    }
}

impl<'de, T> Deserialize<'de> for LlmOption<T>
where
    T: Deserialize<'de> + fmt::Debug,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // 1. 定义一个代理结构体
        #[derive(Deserialize)]
        #[serde(transparent)]
        struct XmlOption<T>(Option<T>);
        match XmlOption::<T>::deserialize(deserializer) {
            Ok(wrapper) => Ok(LlmOption(wrapper.0)),
            Err(e) => {
                // 如果解析失败，说明不是 item 标签，也不是空
                Err(serde::de::Error::custom(format!(
                    "【XML 结构非法】 详情: {}",
                    e
                )))
            }
        }
    }
}

impl<T: LlmPrompt> LlmPrompt for LlmOption<T> {
    fn get_prompt_schema() -> &'static str {
        let sub_schema = T::get_prompt_schema();
        static SCHEMA_CACHE: OnceLock<String> = OnceLock::new();
        SCHEMA_CACHE.get_or_init(|| {
            format!(
                "可选，如果不提供就不要出现任何标签，如果提供则格式为: {}",
                sub_schema
            )
        })
    }
    fn root_name() -> &'static str {
        let sub_root_name = T::root_name();
        static SCHEMA_CACHE: OnceLock<String> = OnceLock::new();
        SCHEMA_CACHE.get_or_init(|| format!("Option<{}>", sub_root_name))
    }
}

impl<T: Default> LlmOption<T> {
    pub fn unwrap_or_default(self) -> T {
        self.0.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use crate::api::llm::tool::LlmI64;

    use super::*;
    use helper::LlmPrompt;
    use quick_xml::de::from_str;
    use serde::{Deserialize, Serialize};

    const CORRECT_SOME: &str = r#"<CourseChoiceResponse>
  <course_id>
  71211
</course_id>
</CourseChoiceResponse>"#;

    const CORRECT_NONE: &str = r#"<CourseChoiceResponse>
</CourseChoiceResponse>"#;

    const WRONG_DATA: &str = r#"<CourseChoiceResponse>
  <course_id>
  fuck you and wrong
</course_id>
</CourseChoiceResponse>"#;

    #[derive(Debug, LlmPrompt, Serialize, Deserialize)]
    pub struct CourseChoiceResponse {
        #[prompt("如果找到符合要求的课程就返回课程ID; 如果没找到指定的课程就是 null")]
        pub course_id: LlmOption<LlmI64>,
    }

    #[test]
    fn test() {
        let data = from_str::<CourseChoiceResponse>(CORRECT_SOME);
        println!("Parsed data: {:?}", data);
        let data = from_str::<CourseChoiceResponse>(CORRECT_NONE);
        println!("Parsed data: {:?}", data);
        let data = from_str::<CourseChoiceResponse>(WRONG_DATA);
        println!("Parsed data: {:?}", data);
        println!()
    }
}
