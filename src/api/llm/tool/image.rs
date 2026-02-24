use super::llm::CLIENT;
use crate::api::storage::ImageFile;
use anyhow::Result;
use anyhow::bail;
use genai::chat::BinarySource;
use genai::chat::ChatMessage;
use tracing::{debug, error, info};

const MODEL_NAME: &str = "gemini-3-pro-image-preview";

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
    }
    bail!("LLM 响应中未找到有效的 Base64 图片数据");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_api() -> Result<()> {
        let chat_req = generate_image(vec![ChatMessage::user(
            "生成一张猫咪的图片，要求是卡通风格，分辨率512x512",
        )])
        .await?;

        println!("chat_req: {:?}", chat_req);

        Ok(())
    }
}
