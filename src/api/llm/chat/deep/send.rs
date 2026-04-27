use crate::abi::message::{MessageType, Target};
use crate::api::llm::chat::archive::bridge::{
    get_face_reference_message, llm_msg_from_message_without_archive,
};
use crate::api::llm::chat::archive::message_storage::MessageStorage;
use crate::api::llm::chat::audit::audit_test_deep;
use crate::api::llm::chat::audit::backlist::Backlist;
use crate::api::llm::chat::impression::get_impression;
use crate::api::llm::chat::message::bridge::{
    IntoMessageSend, MESSAGE_SEND_LLM_RESPONSE_VALID_EXAMPLE, MessageSendLlmResponse,
};
use crate::api::llm::tool::ask_as_high;
use crate::config::get_self_qq;
use crate::{
    abi::{Context, logic_import::Message, network::BotClient, websocket::BotHandler},
    api::llm::chat::llm::get_chat_embedding,
};
use anyhow::{Result, anyhow};
use genai::chat::ChatMessage;
use llm_xml_caster::llm_prompt;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

#[llm_prompt]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct LlmMessageReply {
    #[prompt("用户是否有 at 机器人")]
    is_at: bool,
    #[prompt("用户是否聊的和机器人有关")]
    is_match: bool,
    #[prompt("基于当前结果生成一个不回复的原因或者回复的原因")]
    reason: String,
}

const AUDIT_LLM_MESSAGE_REPLY_VALID_EXAMPLE: &str = r#"
<LlmMessageReply>
    <is_at>false</is_at>
    <is_match>false</is_match>
    <reason>用户没有特别要求回复，因此不进行回复。</reason>
</LlmMessageReply>"#;

#[test]
fn test_llm_message_reply_valid_example() {
    let parsed: LlmMessageReply =
        quick_xml::de::from_str(AUDIT_LLM_MESSAGE_REPLY_VALID_EXAMPLE).unwrap();
    assert_eq!(
        parsed,
        LlmMessageReply {
            is_at: false,
            is_match: false,
            reason: "用户没有特别要求回复，因此不进行回复。".into()
        }
    );
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
    let user_id = message.get_sender().user_id.unwrap_or_default();
    info!(group_id = ?group_id, "开始处理 LLM 深度回复请求");

    let msg_src = llm_msg_from_message_without_archive(ctx.client.clone(), &message).await;

    trace!("开始生成聊天消息嵌入");
    let msg = get_chat_embedding(msg_src.clone()).await.map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "生成聊天消息嵌入失败");
        e
    })?;
    trace!("聊天消息嵌入生成成功");

    let context_message = MessageStorage::get_recent_by_group(group_id, 30)
        .await
        .into_iter()
        .map(|m| m.1)
        .collect::<Vec<_>>();

    let image = get_impression(user_id).await;

    let chat_message = [
        vec![
            ChatMessage::system(
                "你是李老师，一个来自厦门大学的智能助手。你话风諲谐幽默但不油腻，不卧不冒，能用简短自然的句子回应追问。请根据用户的提问和上下文进行回复的判断，判断是否和你相关。",
            ),
            ChatMessage::system("上下文:"),
        ],
        context_message.clone(),
        vec![ChatMessage::system("用户的提问:")],
        msg_src.clone(),
        vec![
            ChatMessage::system(format!(
                "你的 QQ 号是: {}，如果有人@你，你必须做出回复",
                get_self_qq()
            )),
            ChatMessage::system(format!("以下是用户的印象: {:?}", image)),
        ],
    ]
    .concat();

    trace!(prompt = ?chat_message, "LLM 深度回复分析提示词");

    let message =
        ask_as_high::<LlmMessageReply>(chat_message, AUDIT_LLM_MESSAGE_REPLY_VALID_EXAMPLE)
            .await
            .map_err(|e| {
                error!(group_id = ?group_id, error = ?e, "LLM 分析回复匹配度失败");
                e
            })?;

    trace!(reply_analysis = ?message, "LLM 深度回复匹配分析完成");

    info!(group_id=?group_id,message_reply_analysis=?message, "LLM 深度回复匹配分析结果");

    if !message.is_match {
        return Err(anyhow!("AI决定不回复: {}", message.reason));
    }

    let backlist_result = Backlist::search(&msg, 5).await;
    let backlist = backlist_result.unwrap_or_else(|e| {
        warn!(group_id = ?group_id, error = ?e, "黑名单搜索失败，使用空列表");
        vec![]
    });

    debug!(backlist_count = ?backlist.len(), "黑名单搜索完成");

    let chat_message = [
        vec![
            ChatMessage::system(
                "你是李老师，一个来自厦门大学的智能助手，请根据用户的提问和上下文，直接生成格式化的回复。\
### 回复格式规则：\n\
1. 严禁直接在 <item> 标签下书写任何文字。\n\
2. 所有的文本内容必须包裹在 <Text><text>...</text></Text> 结构中。\n\
3. 即使只有一段话，也要拆分为 <item><Text><text>...</text></Text></item>。\n\
4. 严格遵守提供的符号体系，不要输出 XML 以外的文字。\n\
5. 如果需要表达表情，请使用 <item><Face><id>表情ID</id></Face></item>，表情ID必须来自参考列表。\n\
6. 每个消息段后会自动加上换行符，无需在文本内容中添加换行符。\n\
7. 如果需要提及某人，请使用 <item><At><qq>QQ号</qq></At></item>。\n\
8. 不需要使用markdown语法。\n\
9. 在回复文件时务必发送回文件的ID，格式：[文件,file_id=aaaaaaaa]。",
            ),
            get_face_reference_message(),
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

    let response = ask_as_high::<MessageSendLlmResponse>(
        chat_message,
        MESSAGE_SEND_LLM_RESPONSE_VALID_EXAMPLE,
    )
    .await
    .map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "LLM 生成结构化回复失败");
        e
    })?;

    trace!(llm_response = ?response, "LLM 深度回复源数据");

    let msg = IntoMessageSend::from_response(response)
        .await
        .map_err(|e| {
            error!(group_id = ?group_id, error = ?e, "转换结构化回复为消息失败");
            anyhow!("转换结构化回复为消息失败: {:?}", e)
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
