use super::llm::CLIENT;
use crate::api::storage::ImageFile;
use anyhow::Result;
use anyhow::bail;
use genai::chat::BinarySource;
use genai::chat::ChatMessage;
use tracing::{debug, error, info};

const MODEL_NAME: &str = "gpt-image-2-all";

pub async fn generate_image(chat_message: Vec<ChatMessage>) -> Result<ImageFile> {
    info!(model = MODEL_NAME, "开始调用 LLM 进行图片生成");
    let chat_message = [
        chat_message,
        vec![ChatMessage::system(
            "请根据用户的描述生成一张图片，图片应符合描述内容。",
        )],
    ]
    .concat();

    let chat_req = genai::chat::ChatRequest::new(chat_message);

    let res = CLIENT
        .exec_chat(MODEL_NAME, chat_req, None)
        .await
        .map_err(|e| {
            error!(model = MODEL_NAME, error = ?e, "LLM 调用图片生成接口失败");
            e
        })?;

    #[cfg(test)]
    println!("LLM 图片生成原始响应: {:#?}", res);

    debug!(response = ?res, "LLM 图片生成原始响应");

    for part in &res.content {
        if let Some(binary) = part.as_binary() {
            {
                if let BinarySource::Base64(base64_str) = &binary.source {
                    let file = ImageFile::create_from_base64(base64_str).await?;
                    return Ok(file);
                }
            }
        }
        // 按 Markdown 格式 ](...) 提取括号内容
        if let Some(text) = part.as_text() {
            let mut search_from = 0;
            while let Some(rel_pos) = text[search_from..].find("](") {
                let content_start = search_from + rel_pos + 2;
                if let Some(content) = text[content_start..].split(')').next() {
                    let content = content.trim();
                    if content.starts_with("data:image/") {
                        // Markdown 嵌入的 base64 data URL
                        if let Some(base64_str) = content.split(',').nth(1) {
                            #[cfg(test)]
                            println!("从文本 base64 提取图片数据");
                            let file = ImageFile::create_from_base64(base64_str).await?;
                            return Ok(file);
                        }
                    } else if content.starts_with("https://") {
                        #[cfg(test)]
                        println!("等待文件系统准备好文件: {}", content);
                        let file = ImageFile::create_from_url(&content.to_string()).await?;
                        return Ok(file);
                    }
                }
                search_from += rel_pos + 2;
            }
        }
    }
    bail!("LLM 响应中未找到有效的 Base64 图片数据");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_api() -> Result<()> {
        let chat_req = generate_image(vec![ChatMessage::user(
            "生成一张猫咪的图片，要求是卡通风格，分辨率512x512",
        )])
        .await?;

        println!("chat_req: {:?}", chat_req);

        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_download() -> Result<()> {
        let url = "https://pro.filesystem.site/cdn/20260423/656d621610191c3ffb42ac14fe788d.webp";
        let file = ImageFile::create_from_url(&url.to_string()).await?;
        println!("文件已下载，路径: {:?}", file.path);
        Ok(())
    }
}
