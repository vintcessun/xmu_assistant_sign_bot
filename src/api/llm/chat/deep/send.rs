use crate::abi::message::Target;
use crate::api::llm::chat::archive::bridge::llm_msg_from_message_without_archive;
use crate::api::llm::chat::archive::message_storage::MessageStorage;
use crate::api::llm::chat::audit::audit_test_deep;
use crate::api::llm::chat::audit::backlist::Backlist;
use crate::api::llm::chat::llm::ask_llm;
use crate::api::llm::chat::message::bridge::IntoMessageSend;
use crate::api::llm::tool::{LlmBool, LlmPrompt, ask_as};
use crate::{
    abi::{Context, logic_import::Message, network::BotClient, websocket::BotHandler},
    api::llm::chat::llm::get_chat_embedding,
};
use anyhow::{Result, anyhow};
use genai::chat::ChatMessage;
use helper::LlmPrompt;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, trace};

#[derive(Debug, Serialize, Deserialize, LlmPrompt, Clone)]
struct LlmMessageReply {
    #[prompt(
        "当前是否需要对用户进行回复，如果用户聊的话题需要你进行回复就设置为 true 其他时候别人在聊天的时候设置为 false"
    )]
    is_match: LlmBool,
    #[prompt("基于当前结果生成一个简短的回复，回复要简洁明了")]
    reply: String,
}

pub async fn send_message_from_llm<T>(ctx: &mut Context<T, Message>) -> Result<()>
where
    T: BotClient + BotHandler + std::fmt::Debug + 'static,
{
    let message = ctx.get_message();

    let group_id = match ctx.get_target() {
        Target::Group(id) => id,
        Target::Private(id) => -id,
    };

    let msg_src = llm_msg_from_message_without_archive(&message).await;
    let msg = get_chat_embedding(msg_src.clone()).await?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let context_message = MessageStorage::get_range(ts - 120, ts)
        .await
        .into_iter()
        .map(|m| m.1)
        .collect::<Vec<_>>();

    let chat_message = [
        vec![
            ChatMessage::system(
                "你是一个智能的理解用户回复的助手，请根据用户的提问和上下文进行回复的生成",
            ),
            ChatMessage::system("上下文:"),
        ],
        context_message.clone(),
        vec![ChatMessage::system("用户的提问:")],
        msg_src.clone(),
    ]
    .concat();

    trace!(?chat_message, "LLM Deep 分析提示词");

    let message = ask_as::<LlmMessageReply>(chat_message).await?;

    trace!("Llm Reply Analysis: {:?}", message);

    if !*message.is_match {
        return Err(anyhow!("AI决定不回复"));
    }

    let backlist = Backlist::search(msg.clone(), 5).await.unwrap_or_default();

    let chat_message = [
        vec![
            ChatMessage::system(
                "你是一个智能的助手，请根据用户的提问和上下文，完成回复文本的书写。",
            ),
            ChatMessage::system("上下文:"),
        ],
        context_message,
        vec![
            ChatMessage::system(format!(
                "以下是根据用户提问搜索到的不良回答案例: {:?}",
                backlist
                    .iter()
                    .map(|r| format!(
                        "不良内容详情: {}\n不良内容原因: {}\n改进建议: {:?}",
                        r.1.entry.bad_detail, r.1.entry.bad_reason, r.1.entry.suggestions
                    ))
                    .collect::<Vec<String>>()
            )),
            ChatMessage::system("用户的提问:"),
        ],
        msg_src,
    ]
    .concat();

    trace!(?chat_message, "LLM Deep 提示词");

    let resp = ask_llm(chat_message).await?;

    trace!("LLM Deep Reply Source: {:?}", resp);

    let msg = match IntoMessageSend::get_message_send(resp).await {
        Ok(m) => m,
        Err(e) => {
            error!("转写消息失败: {:?}", e);
            return Err(anyhow!("搜索回复消息转换失败"));
        }
    };

    audit_test_deep(&msg, group_id).await?;

    debug!("LLM Deep Reply: {:?}", msg);

    ctx.send_message(msg).await?;

    Ok(())
}
