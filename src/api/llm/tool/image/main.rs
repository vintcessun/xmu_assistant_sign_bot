use super::super::llm::CLIENT;
use super::data::GeminiResponse;
use crate::api::storage::ImageFile;
use anyhow::Result;
use anyhow::bail;
use genai::chat::ChatMessage;
use genai::chat::ChatOptions;
use tracing::trace;

const MODEL_NAME: &str = "gemini-3-pro-image-preview";

pub async fn generate_image(chat_message: Vec<ChatMessage>) -> Result<ImageFile> {
    let chat_message = [
        chat_message,
        vec![ChatMessage::system(
            "请根据用户的描述生成一张图片，图片应符合描述内容。",
        )],
    ]
    .concat();

    let chat_req = genai::chat::ChatRequest::new(chat_message);

    let res = CLIENT
        .exec_chat(
            MODEL_NAME,
            chat_req,
            Some(&ChatOptions {
                capture_raw_body: Some(true),
                ..Default::default()
            }),
        )
        .await?;

    if let Some(res_raw) = res.captured_raw_body {
        let data = serde_json::from_value::<GeminiResponse>(res_raw)?;
        //println!("data: {:?}\n\n\n", data);
        for candidate in data.candidates.into_iter() {
            trace!(?candidate);
            for part in candidate.content.parts.into_iter() {
                trace!(?part);
                if let Some(image_data) = part.inline_data {
                    let image_file = ImageFile::create_from_base64(image_data.data).await?;
                    return Ok(image_file);
                }
            }
        }
        bail!("Generate image failed, please check the model is actually support image generation");
    } else {
        bail!("The response does not capture raw body, please check the state of the model type");
    }
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
