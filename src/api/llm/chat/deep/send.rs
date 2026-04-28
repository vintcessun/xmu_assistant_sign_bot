use crate::abi::message::{MessageType, Target};
use crate::api::llm::chat::archive::bridge::{
    get_face_reference_message, llm_msg_from_message_without_archive,
};
use crate::api::llm::chat::archive::memo_fragment::MemoFragment;
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

/// 字符粗估阈值：超过此字符数触发上下文压缩（约 3000~4000 token for CJK）
const CTX_COMPRESS_CHAR_THRESHOLD: usize = 8000;
/// 压缩后保留的最近消息数
const CTX_KEEP_RECENT_COUNT: usize = 15;
/// MemoFragment 召回的摘要条数
const MEMO_TOP_K: usize = 3;

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

    let context_message_pairs = MessageStorage::get_recent_by_group(group_id, 30).await;

    // Phase D: 上下文 token 估算与压缩
    // 粗估字符总量（Debug 格式长度作为代理）
    let total_chars: usize = context_message_pairs
        .iter()
        .map(|(_, m)| format!("{:?}", m).len())
        .sum();

    // 搜索 MemoFragment 获取历史对话摘要（向量检索回填）
    let memo_summaries: Vec<ChatMessage> = MemoFragment::search(msg.as_ref(), MEMO_TOP_K)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, seg)| seg.group_id == group_id)
        .map(|(_, seg)| {
            ChatMessage::system(format!(
                "[历史摘要] {}\n关键词: {:?}",
                seg.summary, seg.keywords
            ))
        })
        .collect();

    let context_message: Vec<ChatMessage> = if total_chars > CTX_COMPRESS_CHAR_THRESHOLD {
        // 触发压缩：将较旧的消息提交给 MemoFragment 异步归档，仅保留最近 N 条
        let split_at = context_message_pairs.len().saturating_sub(CTX_KEEP_RECENT_COUNT);
        let (old_pairs, recent_pairs) = context_message_pairs.split_at(split_at);

        if !old_pairs.is_empty() {
            let old_ids: Vec<String> = old_pairs.iter().map(|(id, _)| id.clone()).collect();
            let gid = group_id;
            tokio::spawn(async move {
                if let Err(e) =
                    MemoFragment::insert(gid, old_ids, "请压缩并摘要以下对话内容".to_string())
                        .await
                {
                    warn!(group_id = ?gid, error = ?e, "MemoFragment 异步压缩失败");
                }
            });
        }
        debug!(
            group_id = ?group_id,
            total_chars = total_chars,
            kept = recent_pairs.len(),
            "上下文超过压缩阈值，已截断并触发异步摘要归档"
        );
        recent_pairs.iter().map(|(_, m)| m.clone()).collect()
    } else {
        context_message_pairs.into_iter().map(|(_, m)| m).collect()
    };

    let image = get_impression(user_id).await;

    let chat_message = [
        vec![
            ChatMessage::system(
                "你是李老师，一个来自厦门大学的智能助手。你话风諲谐幽默但不油腻，不卧不冒，能用简短自然的句子回应追问。请根据用户的提问和上下文进行回复的判断，判断是否和你相关。",
            ),
        ],
        if memo_summaries.is_empty() {
            vec![]
        } else {
            let mut v = vec![ChatMessage::system("以下是历史对话摘要（仅供参考）：")];
            v.extend(memo_summaries.clone());
            v
        },
        vec![ChatMessage::system("上下文:")],
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
        ],
        if memo_summaries.is_empty() {
            vec![]
        } else {
            let mut v = vec![ChatMessage::system("以下是历史对话摘要（仅供参考）：")];
            v.extend(memo_summaries);
            v
        },
        vec![ChatMessage::system("上下文:")],
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
