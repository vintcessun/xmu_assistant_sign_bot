use anyhow::Result;
use chrono::NaiveDateTime;
use helper::tool;
use searxng_client::{ResponseFormat, SearXNGClient};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const SEARXNG_BASE: &str = "http://localhost:8089/";
static CLIENT: LazyLock<SearXNGClient> =
    LazyLock::new(|| SearXNGClient::new(SEARXNG_BASE, ResponseFormat::Json));

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResult {
    pub title: String,
    pub content: String,
    pub url: String,
    pub source: String, // 来源引擎
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
}

#[tool(
    name = "web_search",
    description = "使用联网搜索工具从互联网获取信息，返回相关结果的标题、摘要和链接。"
)]
pub async fn search(
    /// 搜索查询的关键词
    query: String,
    /// 返回结果的数量
    num: usize,
) -> Result<String> {
    let response = CLIENT.search(query).send_get_num(num).await?;
    let ret = response
        .into_iter()
        .map(|x| match x {
            searxng_client::response::SearchResult::LegacyResult(d) => SearchResult {
                title: d.title,
                content: d.content,
                url: d.url.unwrap_or_default(),
                source: d.engine,
                date: d.published_date,
            },
            searxng_client::response::SearchResult::MainResult(d) => SearchResult {
                title: d.title,
                content: d.content,
                url: d.url.unwrap_or_default(),
                source: d.engine.unwrap_or_default(),
                date: d.published_date,
            },
        })
        .collect::<Vec<_>>();
    let ret = SearchResponse { results: ret };
    let ret_str = quick_xml::se::to_string(&ret)?;
    Ok(ret_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_tool() -> Result<()> {
        let tool = SearchTool::tool();
        println!("Tool Definition: {:?}", tool);
        let name = SearchTool::FN_NAME;
        println!("Tool Function Name: {}", name);
        let args = SearchArgs {
            query: "rust programming".to_string(),
            num: 5,
        };
        println!("Tool Call Arguments: {:?}", args);
        let call_ret = SearchTool::call(args).await?;
        println!("Tool Call Result: {}", call_ret);
        Ok(())
    }
}
