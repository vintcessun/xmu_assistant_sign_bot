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
use tracing::{debug, error, info, trace, warn};

#[derive(Debug, Serialize, Deserialize, LlmPrompt, Clone)]
struct LlmMessageReply {
    #[prompt(
        "当前是否需要对用户进行回复，如果用户特别想你回答就 true 大部分情况下都是不需要进行回答的，除非用户的问题非常明确并且你有非常相关的上下文可以用来回答，否则请回复 false"
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
    info!(group_id = ?group_id, "开始处理 LLM 深度回复请求");

    let msg_src = llm_msg_from_message_without_archive(ctx.client.clone(), &message).await;

    trace!("开始生成聊天消息嵌入");
    let msg = get_chat_embedding(msg_src.clone()).await.map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "生成聊天消息嵌入失败");
        e
    })?;
    trace!("聊天消息嵌入生成成功");

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

    trace!(prompt = ?chat_message, "LLM 深度回复分析提示词");

    let message = ask_as::<LlmMessageReply>(chat_message).await.map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "LLM 分析回复匹配度失败");
        e
    })?;

    trace!(reply_analysis = ?message, "LLM 深度回复匹配分析完成");

    if !*message.is_match {
        info!(group_id = ?group_id, reply = ?message.reply, "LLM 决定不回复");
        return Err(anyhow!("AI决定不回复"));
    }

    info!(group_id = ?group_id, reply = ?message.reply, "LLM 决定回复，开始搜索黑名单");

    let backlist_result = Backlist::search(&msg, 5).await;
    let backlist = backlist_result.unwrap_or_else(|e| {
        warn!(group_id = ?group_id, error = ?e, "黑名单搜索失败，使用空列表");
        vec![]
    });

    debug!(backlist_count = ?backlist.len(), "黑名单搜索完成");

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

    trace!(prompt = ?chat_message, "LLM 深度回复生成提示词");

    let resp = ask_llm(chat_message).await.map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "LLM 生成回复失败");
        e
    })?;

    trace!(llm_response = ?resp, "LLM 深度回复源数据");

    let msg = IntoMessageSend::get_message_send(resp).await.map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "LLM 深度回复转写消息失败");
        anyhow!("LLM 深度回复转写消息失败: {:?}", e)
    })?;

    audit_test_deep(&msg, group_id).await.map_err(|e| {
        warn!(group_id = ?group_id, error = ?e, "发送深度审计任务失败");
        e
    })?;

    debug!(reply_message = ?msg, "LLM 深度回复消息已准备发送");

    ctx.send_message_async(msg);

    info!(group_id = ?group_id, "LLM 深度回复流程完成");

    Ok(())
}
