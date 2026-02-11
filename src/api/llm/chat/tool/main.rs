use super::{python, web};
use crate::api::llm::chat::tool::ToolCallback;
use anyhow::Result;
use genai::chat::{Tool, ToolCall, ToolResponse};
use serde_json::Value;
use tracing::error;

async fn handle_tool_inner(fn_name: &str, fn_arguments: Value) -> Result<String> {
    match fn_name {
        web::FetchTool::FN_NAME => {
            let args: web::FetchArgs = serde_json::from_value(fn_arguments)?;
            web::FetchTool::call(args).await
        }
        web::SearchTool::FN_NAME => {
            let args: web::SearchArgs = serde_json::from_value(fn_arguments)?;
            web::SearchTool::call(args).await
        }
        python::PythonExecTool::FN_NAME => {
            let args: python::PythonExecArgs = serde_json::from_value(fn_arguments)?;
            python::PythonExecTool::call(args).await
        }
        _ => Err(anyhow::anyhow!("未知工具调用: {}", fn_name)),
    }
}

pub async fn handle_tool(tool: ToolCall) -> ToolResponse {
    let ToolCall {
        fn_name,
        fn_arguments,
        call_id,
        ..
    } = tool;
    match handle_tool_inner(&fn_name, fn_arguments).await {
        Ok(result) => ToolResponse::new(call_id, result),
        Err(e) => {
            error!(error = ?e, "工具调用失败: {}", fn_name);
            ToolResponse::new(call_id, format!("工具调用失败: {}", e))
        }
    }
}

pub fn get_tools() -> Vec<Tool> {
    vec![
        web::FetchTool::tool(),
        web::SearchTool::tool(),
        python::PythonExecTool::tool(),
    ]
}
