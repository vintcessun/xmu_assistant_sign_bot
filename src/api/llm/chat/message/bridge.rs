use std::sync::Arc;

use crate::{
    abi::message::{
        MessageSend,
        file::FileUrl,
        message_body::{Cache, Proxy, SegmentSend, at, face, image, text},
    },
    api::{
        llm::{
            chat::{
                archive::bridge::get_face_reference_message,
                file::{FileShortId, LlmFile},
            },
            tool::{LlmPrompt, LlmVec, ask_as},
        },
        storage::FileStorage,
    },
};
use anyhow::{Result, anyhow};
use genai::chat::{ChatMessage, ChatResponse};
use helper::{LlmPrompt, box_new};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

#[derive(Debug, LlmPrompt, Serialize, Deserialize)]
pub struct LlmFileWithIdOrOptionAlias {
    #[prompt("文件ID，8位SHA-256短ID")]
    pub id: String,
}

impl LlmFileWithIdOrOptionAlias {
    pub async fn to_llm_file(&self) -> Result<Arc<LlmFile>> {
        let id = FileShortId::from_llm(&self.id).map_err(|e| {
            debug!(file_id = %self.id, error = ?e, "从 LLM ID 解析 FileShortId 失败");
            e
        })?;
        let file = LlmFile::get_by_id(id)
            .map_err(|e| {
                debug!(file_short_id = ?id, error = ?e, "获取 LlmFile 失败");
                e
            })?
            .ok_or_else(|| {
                debug!(file_short_id = ?id, "LlmFile 未找到");
                anyhow!("文件ID:{}未找到", id)
            })?;
        debug!(file_short_id = ?id, "成功获取 LlmFile");
        Ok(file)
    }
}

#[derive(Debug, LlmPrompt, Serialize, Deserialize)]
pub enum SegmentSendLlmResponse {
    #[prompt("纯文本内容")]
    Text {
        #[prompt("文本内容")]
        text: String,
    },

    #[prompt("图片内容")]
    Image {
        #[prompt("图片文件")]
        file: LlmFileWithIdOrOptionAlias,
    },

    #[prompt("QQ表情")]
    Face {
        #[prompt("表情ID")]
        id: String,
    },

    #[prompt("提及某人")]
    At {
        #[prompt("提及对象的QQ号")]
        qq: String,
    },
}

#[derive(Debug, LlmPrompt, Serialize, Deserialize)]
pub struct MessageSendLlmResponse {
    #[prompt("请根据提供的回复改写并运用提供的符号体系进行回应")]
    pub message: LlmVec<SegmentSendLlmResponse>,
}

pub struct IntoMessageSend;

impl IntoMessageSend {
    pub async fn get(msg: ChatResponse) -> Result<MessageSendLlmResponse> {
        let messages: Vec<ChatMessage> = vec![
            ChatMessage::system(
                "你是一个专业的将消息进行转写的助手，请根据用户提供的信息和所有上下文进行转写为规范格式\
            ### 核心规则：\n\
             1. 严禁直接在 <item> 标签下书写任何文字。\n\
             2. 所有的文本内容必须包裹在 <Text><text>...</text></Text> 结构中。\n\
             3. 即使只有一段话，也要拆分为 <item><Text><text>...</text></Text></item>。\n\
             4. 严格遵守提供的符号体系，不要发挥，不要输出 XML 以外的文字。\
             5. 如果需要表达表情，请使用 <item><Face><id>表情ID</id></Face></item>，其中表情ID必须是提供的参考图中的ID。\n\
             6. 每个消息段后会自动加上换行符，无需在文本内容中添加换行符。
             7. 如果需要提及某人，请使用 <item><At><qq>QQ号</qq></At></item>。
             8. 不需要使用markdown语法进行转写。",
            ),
            get_face_reference_message(),
            ChatMessage::assistant(msg.texts().join("\n")),
            ChatMessage::user("请将上述消息转写为规范的消息格式，不要添加任何额外的说明。"),
        ];

        let response = ask_as::<MessageSendLlmResponse>(messages)
            .await
            .map_err(|e| {
                error!(error = ?e, "LLM 转写结构化消息失败");
                e
            })?;
        info!("LLM 成功将回复转写为结构化消息");
        Ok(response)
    }

    pub async fn get_message_send(msg: ChatResponse) -> Result<MessageSend> {
        let msg: MessageSendLlmResponse = Self::get(msg).await?;
        let mut ret = Vec::new();
        for segment in msg.message {
            ret.push(match segment {
                SegmentSendLlmResponse::At { qq } => {
                    trace!(qq = %qq, "转写消息段: @用户");
                    SegmentSend::At(at::DataSend { qq })
                }
                SegmentSendLlmResponse::Face { id } => {
                    trace!(face_id = %id, "转写消息段: 表情");
                    SegmentSend::Face(face::DataSend { id })
                }
                SegmentSendLlmResponse::Image { file } => {
                    trace!(file_id = %file.id, "转写消息段: 图片");
                    let llm_file = file.to_llm_file().await?;
                    let file_path = llm_file.file.get_path();
                    let file_url = FileUrl::from_path(file_path).map_err(|e| {
                        warn!(path = %file_path.display(), error = ?e, "从文件路径创建 FileUrl 失败");
                        e
                    })?;
                    SegmentSend::Image(box_new!(image::DataSend, {
                        file: file_url,
                        r#type: None,
                        cache: Cache::default(),
                        proxy: Proxy::default(),
                        timeout: None,
                    }))
                }
                SegmentSendLlmResponse::Text { text } => {
                    trace!(content = %text, "转写消息段: 文本");
                    SegmentSend::Text(text::DataSend { text })
                }
            });
            ret.push(SegmentSend::Text(text::DataSend {
                text: "\n".to_string(),
            }));
        }
        info!(segment_count = ?ret.len(), "消息转写完成");
        Ok(MessageSend::Array(ret))
    }
}

#[cfg(test)]
mod tests {
    use genai::chat::ChatRequest;

    use crate::api::llm::chat::llm::CLIENT;

    use super::*;

    #[tokio::test]
    pub async fn test_message_into() -> Result<()> {
        println!("{}", SegmentSendLlmResponse::get_prompt_schema());

        let msg = CLIENT
            .exec_chat(
                "gemini-flash-latest",
                ChatRequest::default().append_message(ChatMessage::user("请你写一句诗")),
                None,
            )
            .await?;
        println!("LLM 原始回复: {:?}", msg);
        let msg = IntoMessageSend::get_message_send(msg).await?;
        println!("转写结果: {:?}", msg);
        Ok(())
    }

    #[tokio::test]
    pub async fn test_message_face() -> Result<()> {
        let msg = CLIENT
            .exec_chat(
                "gemini-flash-latest",
                ChatRequest::default().append_message(ChatMessage::user(
                    "请描述大笑表情并让AI选择表情库中的大小的表情ID进行回复，要求AI必须从face参考图中选择一个表情ID进行回复",
                )),
                None,
            )
            .await?;
        println!("LLM 原始回复: {:?}", msg);
        let msg = IntoMessageSend::get_message_send(msg).await?;
        println!("转写结果: {}", serde_json::to_string(&msg)?);
        Ok(())
    }
}
