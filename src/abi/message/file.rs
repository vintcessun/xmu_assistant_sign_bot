use std::{any::Any, fmt, path::Path, sync::Arc};

use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use url::Url;

#[derive(Serialize, Deserialize, Debug)]
pub struct File {
    pub id: String,
    pub name: String,
    pub size: i64,
    pub busid: i64,
}

#[derive(Debug)]
pub enum FileUrl {
    Temp {
        url: String,
        _handle: Arc<dyn Any + Send + Sync>,
    },
    Raw(String),
}

impl FileUrl {
    pub fn new(url: String) -> Self {
        FileUrl::Raw(url)
    }
}

impl Clone for FileUrl {
    fn clone(&self) -> Self {
        match self {
            FileUrl::Temp { url, _handle } => FileUrl::Temp {
                url: url.clone(),
                _handle: _handle.clone(),
            },
            FileUrl::Raw(url) => FileUrl::Raw(url.clone()),
        }
    }
}

impl FileUrl {
    pub fn get_url(&self) -> &str {
        match self {
            FileUrl::Temp { url, .. } => url,
            FileUrl::Raw(url) => url,
        }
    }
}

impl Serialize for FileUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            // 无论哪种情况，都只序列化内部的字符串部分
            FileUrl::Temp { url, .. } => serializer.serialize_str(url),
            FileUrl::Raw(url) => serializer.serialize_str(url),
        }
    }
}

impl<'de> Deserialize<'de> for FileUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // 定义一个访问者来处理字符串数据
        struct FileUrlVisitor(std::marker::PhantomData<()>);

        impl<'de> de::Visitor<'de> for FileUrlVisitor {
            type Value = FileUrl;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string representing a file URL")
            }

            // 当 serde 发现数据是一个字符串（owned string）时调用
            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FileUrl::Raw(v))
            }

            // 当 serde 发现数据是一个字符串切片（borrowed str）时调用
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(FileUrl::Raw(v.to_string()))
            }
        }

        // 告诉 deserializer 我们期待一个字符串
        deserializer.deserialize_str(FileUrlVisitor(std::marker::PhantomData))
    }
}

impl FileUrl {
    pub fn from_path(path: &Path) -> Result<Self> {
        let absolute_path = std::fs::canonicalize(path)?;

        let ret: String = Url::from_file_path(absolute_path)
            .map(|url| url.into())
            .map_err(|_| anyhow::anyhow!("Failed to convert path to file URL"))?;

        Ok(Self::new(ret))
    }
}
