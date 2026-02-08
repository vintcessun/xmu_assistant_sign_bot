use helper::define_default_type;
use serde::{Deserialize, Deserializer, Serialize, de};
use tracing::trace;
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CQ {
    pub user_id: u64,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SegmentReceive {
    Text(text::DataReceive),
    Face(face::DataReceive),
    Image(Box<image::DataReceive>),
    Record(Box<record::DataReceive>),
    Video(Box<video::DataReceive>),
    At(at::DataReceive),
    Rps(rps::Data),
    Dice(dice::Data),
    Poke(Box<poke::DataReceive>),
    Share(Box<share::DataReceive>),
    Contact(contact::DataReceive),
    Location(Box<location::DataReceive>),
    Reply(reply::DataReceive),
    Forward(forward::DataReceive),
    Xml(xml::DataReceive),
    Json(json::DataReceive),
    File(Box<file::DataReceive>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SegmentSend {
    Text(text::DataSend),
    Face(face::DataSend),
    Image(Box<image::DataSend>),
    Record(record::DataSend),
    Video(video::DataSend),
    At(at::DataSend),
    Rps(rps::Data),
    Dice(dice::Data),
    Shake(shake::Data),
    Poke(poke::DataSend),
    Anonymous(anonymous::DataSend),
    Music(Box<music::DataSend>),
    Share(Box<share::DataSend>),
    Contact(contact::DataSend),
    Location(Box<location::DataSend>),
    Reply(reply::DataSend),
    Node(node::DataSend),
    Xml(xml::DataSend),
    Json(json::DataSend),
    File(file::DataSend),
}

type ArraySend = Vec<SegmentSend>;
type ArrayReceive = Vec<SegmentReceive>;

#[derive(Serialize, Debug, Clone)]
pub enum MessageReceive {
    Array(ArrayReceive),
    Single(SegmentReceive),
    //Cq(CQ),
}

impl<'de> de::Deserialize<'de> for MessageReceive {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MessageVisitor;

        impl<'de> de::Visitor<'de> for MessageVisitor {
            type Value = MessageReceive;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("array or map")
            }

            // 当探测到 JSON 以 '[' 开头时
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut vec = Vec::new();
                while let Some(elem) = seq.next_element()? {
                    vec.push(elem);
                }
                trace!(segment_count = ?vec.len(), "消息段数组反序列化成功");
                Ok(MessageReceive::Array(vec))
            }

            // 当探测到 JSON 以 '{' 开头时
            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                // 直接反序列化为单条 Segment，跳过缓冲逻辑
                let seg = SegmentReceive::deserialize(de::value::MapAccessDeserializer::new(map))?;
                trace!("单个消息段反序列化成功");
                Ok(MessageReceive::Single(seg))
            }
        }

        // 使用 any 会根据 JSON 的第一个字符自动分发到 visit_seq 或 visit_map
        deserializer.deserialize_any(MessageVisitor)
    }
}

impl MessageReceive {
    pub fn get_text(&self) -> String {
        match self {
            // 1. 极速路径：单条文本直接 Clone
            MessageReceive::Single(SegmentReceive::Text(data)) => data.text.clone(),

            // 2. 数组路径：利用 Extend 内部优化
            MessageReceive::Array(arr) => {
                let mut result = String::new();
                result.extend(arr.iter().filter_map(|seg| {
                    if let SegmentReceive::Text(data) = seg {
                        Some(data.text.clone())
                    } else {
                        None
                    }
                }));
                result
            }

            // 3. 其他情况：统一返回空 String (Inline 处理)
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn test_size() {
        println!("\n========================================");
        println!("MessageSend size: {} bytes", size_of::<MessageSend>());
        println!("MessageReceive size: {} bytes", size_of::<MessageReceive>());
        println!("========================================\n\n");
    }

    #[test]
    fn locate_large_variants_send() {
        let mut sizes = vec![
            ("Text", size_of::<text::DataSend>()),
            ("Face", size_of::<face::DataSend>()),
            ("Image", size_of::<image::DataSend>()),
            ("Record", size_of::<record::DataSend>()),
            ("Video", size_of::<video::DataSend>()),
            ("At", size_of::<at::DataSend>()),
            ("Rps", size_of::<rps::Data>()),
            ("Dice", size_of::<dice::Data>()),
            ("Shake", size_of::<shake::Data>()),
            ("Poke", size_of::<poke::DataSend>()),
            ("Anonymous", size_of::<anonymous::DataSend>()),
            ("Music", size_of::<music::DataSend>()),
            ("Share", size_of::<share::DataSend>()),
            ("Contact", size_of::<contact::DataSend>()),
            ("Location", size_of::<location::DataSend>()),
            ("Reply", size_of::<reply::DataSend>()),
            ("Xml", size_of::<xml::DataSend>()),
            ("Json", size_of::<json::DataSend>()),
            ("Node", size_of::<node::DataSend>()),
            ("File", size_of::<file::DataSend>()),
        ];

        // 按字节大小降序排列
        sizes.sort_by(|a, b| b.1.cmp(&a.1));

        println!("\n========================================");
        println!("   SEGMENT VARIANT SIZE ANALYSIS FOR SEND    ");
        println!("========================================");
        println!("Total SegmentSend size: {} bytes", size_of::<SegmentSend>());
        println!("Total MessageSend size: {} bytes", size_of::<MessageSend>());
        println!("----------------------------------------");

        for (name, size) in sizes {
            let status = if size > 64 {
                "!! [REALLY LARGE]"
            } else if size > 32 {
                "!  [LARGE]"
            } else {
                "   [OK]"
            };
            println!("{:<20} : {:>3} bytes  {}", name, size, status);
        }
        println!("========================================\n");
    }

    #[test]
    fn locate_large_variants_receive() {
        let mut sizes = vec![
            ("Text", size_of::<text::DataReceive>()),
            ("Face", size_of::<face::DataReceive>()),
            ("Image", size_of::<image::DataReceive>()),
            ("Record", size_of::<record::DataReceive>()),
            ("Video", size_of::<video::DataReceive>()),
            ("At", size_of::<at::DataReceive>()),
            ("Rps", size_of::<rps::Data>()),
            ("Dice", size_of::<dice::Data>()),
            ("Shake", size_of::<shake::Data>()),
            ("Poke", size_of::<poke::DataReceive>()),
            ("Anonymous", size_of::<anonymous::DataReceive>()),
            ("Music", size_of::<music::DataReceive>()),
            ("Share", size_of::<share::DataReceive>()),
            ("Contact", size_of::<contact::DataReceive>()),
            ("Location", size_of::<location::DataReceive>()),
            ("Reply", size_of::<reply::DataReceive>()),
            ("Xml", size_of::<xml::DataReceive>()),
            ("Json", size_of::<json::DataReceive>()),
            ("Forward", size_of::<forward::DataReceive>()),
            ("File", size_of::<file::DataReceive>()),
        ];

        // 按字节大小降序排列
        sizes.sort_by(|a, b| b.1.cmp(&a.1));

        println!("\n========================================");
        println!("  SEGMENT VARIANT SIZE ANALYSIS FOR RECEIVE    ");
        println!("========================================");
        println!(
            "Total SegmentReceive size: {} bytes",
            size_of::<SegmentReceive>()
        );
        println!(
            "Total MessageReceive size: {} bytes",
            size_of::<MessageReceive>()
        );
        println!("----------------------------------------");

        for (name, size) in sizes {
            let status = if size > 64 {
                "!! [REALLY LARGE]"
            } else if size > 32 {
                "!  [LARGE]"
            } else {
                "   [OK]"
            };
            println!("{:<20} : {:>3} bytes  {}", name, size, status);
        }
        println!("========================================\n");
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum MessageSend {
    Array(ArraySend),
    Single(SegmentSend),
}

pub mod text {

    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub text: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub text: String,
    }
}

pub mod face {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub id: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub id: String,
    }
}

define_default_type!(Cache, u8, 1);
define_default_type!(Proxy, u8, 1);

pub mod image {
    use crate::abi::message::file::FileUrl;

    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub file: String,
        pub r#type: Option<String>,
        pub url: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub file: FileUrl,
        pub r#type: Option<String>,
        pub cache: Cache,
        pub proxy: Proxy,
        pub timeout: Option<u64>,
    }
}

define_default_type!(Magic, u8, 0);

pub mod record {
    use crate::abi::message::file::FileUrl;

    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub file: String,
        pub magic: Magic,
        pub url: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub file: FileUrl,
        pub magic: Magic,
        pub cache: Cache,
        pub proxy: Proxy,
        pub timeout: Option<u64>,
    }
}

pub mod video {
    use crate::abi::message::file::FileUrl;

    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub file: String,
        pub url: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub file: FileUrl,
        pub cache: Cache,
        pub proxy: Proxy,
        pub timeout: Option<u64>,
    }
}

pub mod at {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub qq: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub qq: String,
    }
}

pub mod rps {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, Copy)]
    pub struct Data {}
}

pub mod dice {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, Copy)]
    pub struct Data {}
}

pub mod shake {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone, Copy)]
    pub struct Data {}
}

pub mod poke {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub r#type: String,
        pub id: String,
        pub name: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub r#type: String,
        pub id: String,
    }
}

pub mod anonymous {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {}

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub ignore: Option<u8>,
    }
}

pub mod share {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub url: String,
        pub title: String,
        pub content: String,
        pub image: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub url: String,
        pub title: String,
        pub content: Option<String>,
        pub image: Option<String>,
    }
}

pub mod contact {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum DataSend {
        Qq(QqSend),
        Group(GroupSend),
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct QqSend {
        pub id: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct GroupSend {
        pub id: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum DataReceive {
        Qq(QqReceive),
        Group(GroupReceive),
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct QqReceive {
        pub id: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct GroupReceive {
        pub id: String,
    }
}

pub mod location {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub lat: String,
        pub lon: String,
        pub title: String,
        pub content: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub lat: String,
        pub lon: String,
        pub title: Option<String>,
        pub content: Option<String>,
    }
}

pub mod music {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum DataSend {
        Qq {
            id: String,
        },
        #[serde(rename = "163")]
        NetEase163 {
            id: String,
        },
        Xm {
            id: String,
        },
        Custom {
            url: String,
            audio: String,
            title: String,
            content: Option<String>,
            image: Option<String>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum DataReceive {
        Qq {
            id: String,
        },
        #[serde(rename = "163")]
        NetEase163 {
            id: String,
        },
        Xm {
            id: String,
        },
        Custom {
            url: String,
            audio: String,
            title: String,
            content: Option<String>,
            image: Option<String>,
        },
    }
}

pub mod reply {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub id: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub id: String,
    }
}

pub mod forward {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub id: String,
    }
}

pub mod node {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(untagged)]
    pub enum DataSend {
        Id(DataSend1),
        Content(DataSend2),
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend1 {
        pub id: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend2 {
        pub user_id: String,
        pub nickname: String,
        pub content: Box<MessageSend>,
    }
}

pub mod xml {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub data: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub data: String,
    }
}

pub mod json {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub data: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub data: String,
    }
}

pub mod file {
    use crate::abi::message::file::FileUrl;

    use super::*;
    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataSend {
        pub file: FileUrl,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct DataReceive {
        pub file: String,
        pub file_id: String,
        pub file_size: String,
        pub url: String,
    }
}
