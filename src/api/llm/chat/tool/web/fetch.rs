use crate::api::{llm::tool::ask_str, network::SessionClient};
use anyhow::Result;
use genai::chat::ChatMessage;
use helper::tool;
use std::sync::LazyLock;
//写一个过程宏让我传入每个参数的含义然后和descrption自动生成Tool调用，
static CLIENT: LazyLock<SessionClient> = LazyLock::new(SessionClient::new);

#[tool(
    name = "web_fetch",
    description = "从指定URL获取网页内容，并提取主要文本信息，去除广告、导航等无关内容，转写成 markdown 格式。"
)]
pub async fn fetch(
    /// 要请求的网页 URL
    url: String,
) -> Result<String> {
    let response = CLIENT.get(url).await?;
    let text = response.text().await?;
    let chat_message = vec![
        ChatMessage::system(
            "你是一个网络分析专家，请把以下HTML内容提取出主要文本信息，去除广告、导航等无关内容，并转写成 markdown 格式",
        ),
        ChatMessage::user(text),
    ];
    let md = ask_str(chat_message).await?;
    Ok(md)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_tool() -> Result<()> {
        let tool = FetchTool::tool();
        println!("Tool Definition: {:?}", tool);
        let name = FetchTool::FN_NAME;
        println!("Tool Function Name: {}", name);
        let args = FetchArgs {
            url: "https://www.rust-lang.org/".to_string(),
        };
        println!("Tool Call Arguments: {:?}", args);
        let call_ret = FetchTool::call(args).await?;
        println!("Tool Call Result: {}", call_ret);
        Ok(())
    }
}
