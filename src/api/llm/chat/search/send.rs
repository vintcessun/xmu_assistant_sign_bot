use crate::abi::message::Target;
use crate::api::llm::chat::archive::bridge::llm_msg_from_message_without_archive;
use crate::api::llm::chat::archive::file_embedding::search_llm_file;
use crate::api::llm::chat::archive::memo_fragment::MemoFragment;
use crate::api::llm::chat::audit::audit_test_search;
use crate::api::llm::chat::audit::backlist::Backlist;
use crate::api::llm::chat::llm::ask_llm;
use crate::api::llm::chat::message::bridge::IntoMessageSend;
use crate::api::llm::tool::{LlmBool, LlmPrompt, ask_as};
use crate::{
    abi::{Context, logic_import::Message, network::BotClient, websocket::BotHandler},
    api::llm::chat::{llm::get_chat_embedding, search::store::MessageSearchStore},
};
use anyhow::{Result, anyhow};
use genai::chat::{Binary, ChatMessage, ContentPart};
use helper::LlmPrompt;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, trace, warn};

#[derive(Debug, Serialize, Deserialize, LlmPrompt, Clone)]
pub struct SearchMessageReply {
    #[prompt("当前是否需要对用户进行回复，如果搜索结果非常相关则回复 true，否则回复 false")]
    is_match: LlmBool,
    #[prompt("基于搜索的结果生成一个简短的回复，回复要简洁明了")]
    reply: String,
}

pub async fn send_message_from_store<T>(ctx: &mut Context<T, Message>) -> Result<()>
where
    T: BotClient + BotHandler + std::fmt::Debug + 'static,
{
    let message = ctx.get_message();

    let group_id = match ctx.get_target() {
        Target::Group(id) => id,
        Target::Private(id) => -id,
    };
    debug!(group_id = ?group_id, "开始处理搜索消息请求");

    let msg_src = llm_msg_from_message_without_archive(ctx.client.clone(), &message).await;
    let msg = get_chat_embedding(msg_src.clone()).await.map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "获取消息嵌入向量失败");
        e
    })?;
    debug!(group_id = ?group_id, "消息嵌入向量计算完成");

    let result = MessageSearchStore::search(&msg, 5).await.map_err(|e| {
        warn!(group_id = ?group_id, error = ?e, "在消息搜索存储中搜索失败");
        e
    })?;
    let files = search_llm_file(&msg, 5).await.map_err(|e| {
        warn!(group_id = ?group_id, error = ?e, "在文件搜索存储中搜索失败");
        e
    })?;

    let files_len = files.len();

    let file_chat = {
        let mut ret = Vec::with_capacity(files.len() * 3 + 3);
        for (_, file) in files {
            let mut parts = Vec::with_capacity(3);
            parts.push(ContentPart::Text(format!(
                "文件名称: {} 文件ID: {}",
                file.alias, file.id
            )));
            let binary_file = Binary::from_file(&file.file.path).map_err(|e| {
                error!(group_id = ?group_id, file_path = %file.file.path.display(), error = ?e, "将文件转换为 Binary 失败");
                e
            })?;
            parts.push(ContentPart::Binary(binary_file));
            ret.push(ChatMessage::assistant(parts));
            debug!(group_id = ?group_id, file_id = %file.id, file_alias = %file.alias, "附加搜索到的文件到 ChatMessage");
        }
        ret
    };

    let memo_segment = MemoFragment::search(&msg, 5).await.map_err(|e| {
        warn!(group_id = ?group_id, error = ?e, "在记忆片段存储中搜索失败");
        e
    })?;

    debug!(
        group_id = ?group_id,
        memo_segments = ?memo_segment.len(),
        files = ?files_len,
        messages = ?result.len(),
        "搜索结果收集完成"
    );

    let chat_message = [
    vec![ChatMessage::system(
        "你是一个智能的理解用户回复的助手，请根据 embedding 的结果和用户的提问进行回复的生成",
    ),
    ChatMessage::system(format!(
        "以下是根据用户提问搜索到的相关内容: {:?}",
        result
            .iter()
            .map(|r| format!("内容: {:?}", r.1))
            .collect::<Vec<String>>()
    ))],
    vec![ChatMessage::system(
        "以下是根据用户提问搜索到的相关文件: ")],
    file_chat,
    vec![
    ChatMessage::system(format!(
        "以下是根据用户提问搜索到的相关聊天记录片段: {:?}",
        memo_segment
            .iter()
            .map(|r| format!("聊天片段内容: {:?}", r.1))
            .collect::<Vec<String>>()
    )),
    ChatMessage::system("用户的提问:")],
    msg_src.clone()].concat();

    let message = ask_as::<SearchMessageReply>(chat_message)
        .await
        .map_err(|e| {
            error!(group_id = ?group_id, error = ?e, "LLM 分析搜索匹配度失败");
            e
        })?;

    trace!(reply_analysis = ?message, "LLM 搜索回复匹配分析完成");

    if !*message.is_match {
        debug!(group_id = ?group_id, "LLM 分析认为搜索结果匹配度低，不回复");
        return Err(anyhow!("未命中搜索回复"));
    }

    info!(group_id = ?group_id, "LLM 分析认为搜索结果匹配度高，继续生成回复");

    let backlist = Backlist::search(&msg, 5)
        .await
        .map_err(|e| {
            warn!(group_id = ?group_id, error = ?e, "在黑名单搜索存储中搜索失败");
            e
        })
        .unwrap_or_default();

    debug!(group_id = ?group_id, backlist_count = ?backlist.len(), "黑名单搜索完成");

    let chat_message = [
        vec![
            ChatMessage::system(
                "你是一个智能的助手，请根据用户的提问和搜索到的相关内容，完成回复文本的生成。",
            ),
            ChatMessage::system(format!(
                "以下是根据用户提问搜索到的相关内容: {:?}",
                result
                    .iter()
                    .map(|r| format!("内容: {:?}", r.1))
                    .collect::<Vec<String>>()
            )),
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

    info!(group_id = ?group_id, "开始调用 LLM 生成搜索回复");
    let resp = ask_llm(chat_message).await.map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "LLM 生成搜索回复失败");
        e
    })?;

    trace!(llm_response = ?resp, "LLM 搜索回复源数据");

    let msg = match IntoMessageSend::get_message_send(resp).await {
        Ok(m) => m,
        Err(e) => {
            error!(group_id = ?group_id, error = ?e, "LLM 搜索回复转写消息失败");
            return Err(anyhow!("搜索回复消息转换失败"));
        }
    };
    debug!(group_id = ?group_id, "LLM 搜索回复消息转写成功");

    audit_test_search(
        &msg,
        group_id,
        result.iter().map(|x| x.0.to_owned()).collect::<Vec<_>>(),
    )
    .await
    .map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "发送搜索审计任务失败");
        e
    })?;

    debug!(group_id = ?group_id, "搜索审计任务发送成功");

    ctx.send_message(msg).await.map_err(|e| {
        error!(group_id = ?group_id, error = ?e, "发送搜索回复消息失败");
        e
    })?;

    info!(group_id = ?group_id, "LLM 搜索回复发送成功");

    Ok(())
}
