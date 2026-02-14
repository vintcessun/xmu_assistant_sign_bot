mod main;
pub mod python;
pub mod time;
pub mod web;
use schemars::{JsonSchema, generate::SchemaSettings};

use anyhow::Result;
use async_trait::async_trait;
use genai::chat::Tool;
pub use main::*;

#[async_trait]
pub trait ToolCallback {
    const FN_NAME: &'static str;
    type Output;
    async fn call(args: Self::Output) -> Result<String>;
    fn tool() -> Tool;
}

pub trait ToolStruct: JsonSchema {
    /// 获取不带 $schema 头部的纯粹 JSON Schema Value
    fn tool_schema() -> serde_json::Value {
        // 配置 schemars 以生成内联的、平铺的 Schema（LLM 更喜欢这种）
        let settings = SchemaSettings::draft07().with(|s| {
            s.inline_subschemas = true; // 关键：内联所有定义，不使用 $ref
        });
        let generator = settings.into_generator();
        let schema = generator.into_root_schema_for::<Self>();

        // 只提取其中的 schema 部分，忽略 root 包装
        let mut ret = serde_json::to_value(schema).expect("Failed to serialize schema");
        if let Some(obj) = ret.as_object_mut() {
            obj.remove("$schema"); // 移除 $schema 字段
            obj.remove("title"); // 移除 title 字段
        }
        ret
    }

    /// 合并描述的统一接口
    fn schema_with_override(desc: Option<&str>) -> serde_json::Value {
        let mut s = Self::tool_schema();
        if let Some(obj) = s.as_object_mut()
            && let Some(d) = desc
            && !d.is_empty()
        {
            let d = d.trim();
            obj.insert("description".into(), serde_json::json!(d));
        }
        s
    }
}

// 为所有实现了 JsonSchema 的类型自动实现 ToolStruct
impl<T: JsonSchema> ToolStruct for T {}
