use crate::api::llm::chat::archive::bridge::llm_msg_from_message_without_archive;
use crate::api::llm::chat::audit::backlist::Backlist;
use crate::api::llm::chat::llm::ask_llm;
use crate::api::llm::chat::message::bridge::IntoMessageSend;
use crate::api::llm::tool::{LlmBool, LlmPrompt, ask_as};
use crate::{
    abi::{Context, logic_import::Message, network::BotClient, websocket::BotHandler},
    api::llm::chat::{llm::get_chat_embedding, search::store::MessageSearchStore},
};
use anyhow::{Result, anyhow};
use genai::chat::ChatMessage;
use helper::LlmPrompt;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, trace};

#[derive(Debug, Serialize, Deserialize, LlmPrompt, Clone)]
struct SearchMessageReply {
    #[prompt(
        "当前是否需要对用户进行回复，如果用户提示@我要回复或者搜索结果非常相关则回复 true，否则回复 false"
    )]
    is_match: LlmBool,
    #[prompt("基于搜索的结果生成一个简短的回复，回复要简洁明了")]
    reply: String,
}

pub async fn send_message_from_store<T>(ctx: &mut Context<T, Message>) -> Result<()>
where
    T: BotClient + BotHandler + std::fmt::Debug + 'static,
{
    let message = ctx.get_message();

    let msg_src = llm_msg_from_message_without_archive(&message).await;
    let msg = get_chat_embedding(msg_src.clone()).await?;

    let result = MessageSearchStore::search(msg.clone(), 5).await?;

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
    )),
    ChatMessage::system("用户的提问:")],
    msg_src.clone()].concat();

    let message = ask_as::<SearchMessageReply>(chat_message).await?;

    trace!("Search Reply Analysis: {:?}", message);

    if !*message.is_match {
        return Err(anyhow!("未命中搜索回复"));
    }

    let backlist = Backlist::search(msg.clone(), 5).await.unwrap_or_default();

    let chat_message = [
        vec![
            ChatMessage::system(
                "你是一个智能的助手，请根据用户的提问和搜索到的相关内容，阐述回复的要点等。",
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

    let resp = ask_llm(chat_message).await?;

    trace!("LLM Search Reply Source: {:?}", resp);

    let msg = match IntoMessageSend::get_message_send(resp).await {
        Ok(m) => m,
        Err(e) => {
            error!("转写消息失败: {:?}", e);
            return Err(anyhow!("搜索回复消息转换失败"));
        }
    };

    debug!("LLM Search Reply: {:?}", msg);

    ctx.send_message(msg).await?;

    Ok(())
}
