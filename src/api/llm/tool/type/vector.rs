use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};

use crate::api::llm::tool::{Cache, LlmPrompt};

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
        // 1. 定义一个内部包装类
        #[derive(Deserialize)]
        struct ItemWrapper<T> {
            // $value 表示：取当前标签内部的“值”或“子标签”进行解析
            #[serde(rename = "$value")]
            content: T,
        }

        #[derive(Deserialize)]
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

impl<T: LlmPrompt + 'static> LlmPrompt for LlmVec<T> {
    fn get_prompt_schema() -> &'static str {
        let sub_schema = T::get_prompt_schema();
        let cache = Cache::<T>::get();
        cache.prompt_schema.get_or_init(|| {
            format!(
                "一个由零个或多个元素组成的列表，每个元素的格式为: <item>{}</item>，请注意即使是单个item也**必须**用<item></item>标签包括内容",
                sub_schema
            )
        })
    }
    fn root_name() -> &'static str {
        let sub_root_name = T::root_name();
        let cache = Cache::<T>::get();
        cache
            .root_name
            .get_or_init(|| format!("Vec<{}>", sub_root_name))
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

    const CORRECT_DATA_SINGLE: &str = r#"<FilesChoiceResponseLlm>
  <all>false</all>
  <files><item>3052935</item></files>
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

    const CORRECT_COMPLEX_DATA: &str = r#"<MessageSendLlmResponse>
  <message>
    <item><Text><text>这个文本的核心意义在于快速回顾和总结了多个学科的关键知识点、历史事件和哲学原理，同时融入了当代流行文化和政治口号，形成了一种独特的、高度信息化的知识口诀体系。</text></Text></item>
    <item><Text><text>这份文本是一份高度压缩、多学科交织的知识地图。它将复杂的知识点通过韵律和口诀的形式串联起来，展现了知识分子对广阔世界认知和学习的热情，以及在信息时代背景下对知识进行快速整合和传递的尝试。其结构反映了“万物皆可连接”的互联网思维，将历史、科学和流行文化置于同一框架内进行探讨。</text></Text></item>
    <item><Text><text>主要包含了化学、物理、数学和生物学的核心公式、定义和实验操作规范。</text></Text></item>
    <item><Text><text>包含了针对英语学习和语法规则的助记词。</text></Text></item>
    <item><Text><text>涵盖了中国近代史、世界史以及重要的政治、经济和哲学概念。</text></Text></item>
    <item><Text><text>包含了信息技术、互联网概念和对特定游戏的引用。</text></Text></item>
    <item><Text><text>化学与实验操作：</text></Text></item>
    <item><Text><text>制氧：提到“高锰酸钾制氧气”，指实验室常用方法。</text></Text></item>
    <item><Text><text>氧化还原：明确了氧化剂和还原剂的判断口诀：“失升氧化还原剂，得降还原氧化剂”（失电子/化合价升高者为还原剂；得电子/化合价降低者为氧化剂）。</text></Text></item>
    <item><Text><text>惰性气体：强调了“氦不能作还原剂，氦气稳定不参与”，体现其化学性质。</text></Text></item>
    <item><Text><text>实验安全：强调“试管口需要略低”，避免冷凝水回流炸裂试管。</text></Text></item>
    <item><Text><text>物理与数学：</text></Text></item>
    <item><Text><text>基础公式：提及热量计算  $Q=cm\Delta t$  和速度计算  $v=s/t$。</text></Text></item>
    <item><Text><text>概念：提及“平面直角坐标系”。</text></Text></item>
    <item><Text><text>物理学成就：“麦克斯韦很给力，电磁统一创世纪”。</text></Text></item>
    <item><Text><text>宇宙学原理：提到“熵增定律是真理，宇宙终局热寂里”。</text></Text></item>
    <item><Text><text>生物学与生物化学：</text></Text></item>
    <item><Text><text>细胞能量：强调“线粒体产ATP，ATP的循环急”，以及驱动力“三羧循环不停息”。</text></Text></item>
    <item><Text><text>ATP性质的澄清：特别指出“ATP是还原剂，不是典型还原剂，不靠电子来发力，只是能量的载体”，准确描述了ATP在能量代谢中的角色（非典型氧化还原反应中的电子供体，而是能量携带者）。</text></Text></item>
    <item><Text><text>词汇与责任：提及单词“responsibility”。</text></Text></item>
    <item><Text><text>语法规则：“介词后加-ing”（介词后接动名词）。</text></Text></item>
    <item><Text><text>数字表达口诀：详细描述了英语序数词和基数词的用法（如1、2、3特殊记；几十以-y结尾需改ie；几十几的序数词用法等）。</text></Text></item>
    <item><Text><text>中国历史进程：</text></Text></item>
    <item><Text><text>古代文学：“巴山楚水凄凉地”。</text></Text></item>
    <item><Text><text>近代屈辱史：明确列出了一系列关键节点：“虎门销烟”、“南京条约”、“甲午风云”、“马关条约”、“辛丑之约”、“辛亥枪响”。</text></Text></item>
    <item><Text><text>历史运动与失败：提及“金田起义太平军”、“洋务运动终成泥”、“戊戌变法百日熄”。</text></Text></item>
    <item><Text><text>政治与经济指导思想：</text></Text></item>
    <item><Text><text>核心口号：强调“解放发展生产力”、“改革春风吹满地”、“实事求是是真理”、“创新驱动生产力”。</text></Text></item>
    <item><Text><text>治理思想：提及“兄死社稷由弟继”（古代继承制度）。</text></Text></item>
    <item><Text><text>哲学与认知心理学：</text></Text></item>
    <item><Text><text>进化论：“自然选择显威力”、“丛林法则适者立”。</text></Text></item>
    <item><Text><text>量子力学：“薛定谔的猫诡异，量子叠加真神奇”。</text></Text></item>
    <item><Text><text>社会心理学：“费斯廷格揭奥秘，认知失调生焦虑”。</text></Text></item>
    <item><Text><text>计算机科学：提到了网络通信的核心协议：“TCP/IP传数据，三次握手建联系”（TCP三次握手是建立连接的关键步骤）。</text></Text></item>
    <item><Text><text>新兴技术：“元宇宙与新虚拟，数字身份可转移”。</text></Text></item>
    <item><Text><text>人工智能：对“黑箱效应”进行了辩证：“黑箱效应太狭义，AI也有理解力”。</text></Text></item>
    <item><Text><text>流行文化（《明日方舟》系列）：直接引用了游戏名称和设定：“公测新游宣发密，明日方舟终末地”、“源石造就矿石病”。</text></Text></item>
  </message>
</MessageSendLlmResponse>"#;

    use super::*;
    use crate::api::llm::{
        chat::message::bridge::MessageSendLlmResponse,
        tool::{LlmBool, LlmI64, LlmOption, LlmPrompt, LlmVec},
    };
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
        let data = from_str::<FilesChoiceResponseLlm>(CORRECT_DATA_SINGLE);
        println!("Parsed data: {:?}", data);
        let data = from_str::<FilesChoiceResponseLlm>(CORRECT_EMPTY_DATA);
        println!("Parsed data: {:?}", data);
        let data = from_str::<FilesChoiceResponseLlm>(WRONG_DATA);
        println!("Parsed data: {:?}", data);
        let data = from_str::<MessageSendLlmResponse>(CORRECT_COMPLEX_DATA);
        println!("Parsed data: {:?}", data);
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd)]
    struct TypeA;
    impl LlmPrompt for TypeA {
        fn get_prompt_schema() -> &'static str {
            "这是TypeA的提示词schema"
        }
        fn root_name() -> &'static str {
            "TypeA"
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd)]
    struct TypeB;
    impl LlmPrompt for TypeB {
        fn get_prompt_schema() -> &'static str {
            "这是TypeB的提示词schema"
        }
        fn root_name() -> &'static str {
            "TypeB"
        }
    }

    #[derive(
        Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd, LlmPrompt,
    )]
    struct LlmVecTypeA {
        #[prompt("A的值")]
        val: LlmVec<TypeA>,
    }

    #[derive(
        Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd, LlmPrompt,
    )]
    struct LlmVecTypeB {
        #[prompt("B的值")]
        val: LlmVec<TypeB>,
    }

    #[test]
    fn test_generic_vec() {
        println!("LlmVecTypeA schema: {}", LlmVecTypeA::get_prompt_schema());
        println!("LlmVecTypeB schema: {}", LlmVecTypeB::get_prompt_schema());
    }
}
