use super::super::llm::CLIENT;
use super::data::GeminiResponse;
use crate::api::storage::ImageFile;
use anyhow::Result;
use anyhow::bail;
use genai::chat::ChatMessage;
use genai::chat::ChatOptions;
use tracing::{debug, error, info, trace};

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
        .exec_chat(
            MODEL_NAME,
            chat_req,
            Some(&ChatOptions {
                capture_raw_body: Some(true),
                ..Default::default()
            }),
        )
        .await
        .map_err(|e| {
            error!(model = MODEL_NAME, error = ?e, "LLM 调用图片生成接口失败");
            e
        })?;

    debug!(response = ?res, "LLM 图片生成原始响应");

    if let Some(res_raw) = res.captured_raw_body {
        let data = serde_json::from_value::<GeminiResponse>(res_raw).map_err(|e| {
            error!(error = ?e, "解析 LLM 原始响应 JSON 失败");
            e
        })?;
        //println!("data: {:?}\n\n\n", data);
        for candidate in data.candidates.into_iter() {
            trace!(candidate = ?candidate, "处理图片生成候选结果");
            for part in candidate.content.parts.into_iter() {
                trace!(part = ?part, "处理候选结果部件");
                if let Some(image_data) = part.inline_data {
                    let image_file = ImageFile::create_from_base64(image_data.data)
                        .await
                        .map_err(|e| {
                            error!(error = ?e, "将 Base64 数据转换为 ImageFile 失败");
                            e
                        })?;
                    info!(file_path = %image_file.path.display(), "图片生成成功并保存");
                    return Ok(image_file);
                }
            }
        }
        error!("LLM 返回的响应中未包含 Base64 图片数据");
        bail!("Generate image failed, please check the model is actually support image generation");
    } else {
        error!("LLM 响应未捕获原始正文（captured_raw_body 为 None）");
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
