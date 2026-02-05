use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};
use std::sync::OnceLock;

use crate::api::llm::tool::LlmPrompt;

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(transparent)]
pub struct LlmVec<T>(pub Vec<T>);

impl<T> Deref for LlmVec<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for LlmVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<Vec<T>> for LlmVec<T> {
    fn from(v: Vec<T>) -> Self {
        LlmVec(v)
    }
}

impl<T> From<LlmVec<T>> for Vec<T> {
    fn from(lv: LlmVec<T>) -> Self {
        lv.0
    }
}

impl<'a, T> IntoIterator for &'a LlmVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T> IntoIterator for LlmVec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T> LlmVec<T> {
    pub fn to_vec(self) -> Vec<T> {
        self.0
    }
}

impl<'de, T> Deserialize<'de> for LlmVec<T>
where
    T: Deserialize<'de> + Debug,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // 1. 定义一个代理结构体

        // 1. 定义一个内部包装类
        #[derive(Deserialize)]
        struct ItemWrapper<T> {
            // $value 表示：取当前标签内部的“值”或“子标签”进行解析
            #[serde(rename = "$value")]
            content: T,
        }

        #[derive(Deserialize)]
        // 关键：拒绝未知字段。这样 <file> 标签就会触发报错
        #[serde(deny_unknown_fields)]
        struct XmlSeq<T> {
            // 关键：rename 捕获子标签
            #[serde(rename = "item", default = "Vec::new")]
            items: Vec<ItemWrapper<T>>,
        }

        // 2. 直接解析，不再使用 untagged enum
        // quick-xml 会自动处理单/多标签
        match XmlSeq::<T>::deserialize(deserializer) {
            Ok(wrapper) => Ok(LlmVec(
                wrapper.items.into_iter().map(|w| w.content).collect(),
            )),
            Err(e) => {
                // 如果解析失败，说明不是 item 标签，也不是空
                Err(serde::de::Error::custom(format!(
                    "【XML 结构非法】必须是 <item> 序列。详情: {}",
                    e
                )))
            }
        }
    }
}

impl<T: LlmPrompt> LlmPrompt for LlmVec<T> {
    fn get_prompt_schema() -> &'static str {
        let sub_schema = T::get_prompt_schema();
        static SCHEMA_CACHE: OnceLock<String> = OnceLock::new();
        SCHEMA_CACHE.get_or_init(|| {
            format!(
                "一个由零个或多个元素组成的列表，每个元素的格式为: <item>{}</item>",
                sub_schema
            )
        })
    }
    fn root_name() -> &'static str {
        let sub_root_name = T::root_name();
        static SCHEMA_CACHE: OnceLock<String> = OnceLock::new();
        SCHEMA_CACHE.get_or_init(|| format!("Vec<{}>", sub_root_name))
    }
}

#[cfg(test)]
mod tests {
    const CORRECT_DATA: &str = r#"<FilesChoiceResponseLlm>
  <all>false</all>
  <files>
    <item>3052935</item>
    <item>3036828</item>
    <item>3036831</item>
    <item>3036837</item>
    <item>3036834</item>
    <item>3036825</item>
    <item>3036843</item>
    <item>3036840</item>
  </files>
</FilesChoiceResponseLlm>"#;
    const CORRECT_EMPTY_DATA: &str = r#"<FilesChoiceResponseLlm>
  <all>true</all>
  <files />
</FilesChoiceResponseLlm>"#;
    const WRONG_DATA: &str = r#"<FilesChoiceResponseLlm>
  <all>false</all>
  <files>
    <file>我是错误返回的字符串</file>
  </files>
  </FilesChoiceResponseLlm>"#;

    use super::*;
    use crate::api::llm::tool::{LlmBool, LlmI64, LlmOption, LlmPrompt, LlmVec};
    use helper::LlmPrompt;
    use quick_xml::de::from_str;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, LlmPrompt, Serialize, Deserialize)]
    pub struct FilesChoiceResponseLlm {
        #[prompt("如果目的是选择所有的内容则设置为 true，否则为 false")]
        pub all: LlmBool,
        #[prompt("请注意这里对应的是提供的内容的reference_id字段")]
        pub files: LlmOption<LlmVec<LlmI64>>,
    }

    #[test]
    fn test() {
        let data = from_str::<FilesChoiceResponseLlm>(CORRECT_DATA);
        println!("Parsed data: {:?}", data);
        let data = from_str::<FilesChoiceResponseLlm>(CORRECT_EMPTY_DATA);
        println!("Parsed data: {:?}", data);
        let data = from_str::<FilesChoiceResponseLlm>(WRONG_DATA);
        println!("Parsed data: {:?}", data);
        println!()
    }
}
