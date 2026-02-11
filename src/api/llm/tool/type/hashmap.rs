use crate::api::llm::tool::{CachePair, LlmPrompt};
use serde::{Deserialize, Deserializer, Serialize};
use std::cmp::Ord;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(transparent)]
pub struct LlmHashMap<K: Eq + Hash + Ord, V: Eq + Hash>(pub BTreeMap<K, V>);

impl<K, V> Deref for LlmHashMap<K, V>
where
    K: Eq + Hash + Ord,
    V: Eq + Hash,
{
    type Target = BTreeMap<K, V>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> DerefMut for LlmHashMap<K, V>
where
    K: Eq + Hash + Ord,
    V: Eq + Hash,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<K, V> From<BTreeMap<K, V>> for LlmHashMap<K, V>
where
    K: Eq + Hash + Ord,
    V: Eq + Hash,
{
    fn from(m: BTreeMap<K, V>) -> Self {
        LlmHashMap(m)
    }
}

impl<'de, K, V> Deserialize<'de> for LlmHashMap<K, V>
where
    K: Deserialize<'de> + Debug + Eq + Hash + Ord,
    V: Deserialize<'de> + Debug + Eq + Hash,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // 1. 定义 Entry 结构，模拟 XML 中的 <entry><key>...</key><value>...</value></entry>
        #[derive(Deserialize, Debug)]
        struct Entry<K, V> {
            key: K,
            value: V,
        }

        // 2. 定义包装层，捕获所有的 <entry> 标签
        #[derive(Deserialize)]
        struct XmlMap<K, V> {
            #[serde(rename = "entry", default = "Vec::new")]
            entries: Vec<Entry<K, V>>,
        }

        // 3. 解析并转换成 HashMap
        match XmlMap::<K, V>::deserialize(deserializer) {
            Ok(wrapper) => {
                let map: BTreeMap<K, V> = wrapper
                    .entries
                    .into_iter()
                    .map(|e| (e.key, e.value))
                    .collect();
                Ok(LlmHashMap(map))
            }
            Err(e) => Err(serde::de::Error::custom(format!(
                "【XML 结构非法】必须是 <entry> 序列，且包含 <key> 和 <value>。详情: {}",
                e
            ))),
        }
    }
}

impl<K, V> LlmPrompt for LlmHashMap<K, V>
where
    K: LlmPrompt + Eq + Hash + Ord + 'static,
    V: LlmPrompt + Eq + Hash + 'static,
{
    fn get_prompt_schema() -> &'static str {
        let key_schema = K::get_prompt_schema();
        let val_schema = V::get_prompt_schema();
        let cache = CachePair::<K, V>::get();
        cache.prompt_schema.get_or_init(|| {
            format!(
                "一个键值对集合，格式为: <entry><key>{}</key><value>{}</value></entry>，可重复多次。",
                key_schema, val_schema
            )
        })
    }

    fn root_name() -> &'static str {
        let key_name = K::root_name();
        let val_name = V::root_name();
        let cache = CachePair::<K, V>::get();
        cache
            .root_name
            .get_or_init(|| format!("HashMap<{}, {}>", key_name, val_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quick_xml::de::from_str;

    // 模拟 LLM 可能返回的印象标签权重
    const MAP_DATA: &str = r#"
    <UserImpression>
        <tags>
            <entry><key>humor</key><value>high</value></entry>
            <entry><key>trust</key><value>medium</value></entry>
        </tags>
    </UserImpression>"#;

    #[derive(Debug, Deserialize, Serialize)]
    struct UserImpression {
        tags: LlmHashMap<String, String>,
    }

    #[test]
    fn test_map_parse() {
        let parsed: Result<UserImpression, _> = from_str(MAP_DATA);
        match parsed {
            Ok(data) => {
                println!("Parsed Map: {:?}", data.tags);
                assert_eq!(data.tags.get("humor").unwrap(), "high");
            }
            Err(e) => panic!("Failed to parse: {}", e),
        }
    }
}
