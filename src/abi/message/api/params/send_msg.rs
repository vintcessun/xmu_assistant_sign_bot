use super::Params;
use crate::{
    abi::{
        echo::Echo,
        logic_import::Message,
        message::{
            self, MessageSend, Sender, Target,
            api::data,
            event_body,
            message_body::{self, SegmentSend},
        },
    },
    config::get_self_qq,
};
use core::panic;
use helper::{api, box_new};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, trace};

#[derive(Serialize, Debug)]
pub struct ApiSend<T: Params + Serialize> {
    pub action: &'static str,
    pub params: T,
    pub echo: Echo,
}

#[api("/send_group_msg", data::SendMsgResponse)]
pub struct SendGroupMessageParams {
    pub group_id: i64,
    pub message: MessageSend,
}

impl SendGroupMessageParams {
    pub const fn new(group_id: i64, message: MessageSend) -> Self {
        Self { group_id, message }
    }
}

#[api("/send_group_forward_msg", data::GetForwardMsgResponse)]
pub struct SendGroupForwardMessageParams {
    pub group_id: i64,
    pub messages: MessageSend,
}

impl SendGroupForwardMessageParams {
    pub fn new(
        is_echo: bool,
        message_list: Vec<MessageSend>,
        sender: Arc<Sender>,
        msg: Arc<Message>,
        target: Target,
    ) -> Self {
        let group_id = match target {
            Target::Group(group_id) => group_id,
            _ => panic!("SendGroupForwardMessageParams 只能用于群聊消息"),
        };
        let message = get_msg(is_echo, message_list, sender, msg, target);

        Self {
            group_id,
            messages: MessageSend::Array(message),
        }
    }
}

#[api("/send_private_msg", data::SendMsgResponse)]
pub struct SendPrivateMessageParams {
    pub user_id: i64,
    pub message: MessageSend,
}

impl SendPrivateMessageParams {
    pub const fn new(user_id: i64, message: MessageSend) -> Self {
        Self { user_id, message }
    }
}

#[api("/send_private_forward_msg", data::GetForwardMsgResponse)]
pub struct SendPrivateForwardMessageParams {
    pub user_id: i64,
    pub messages: MessageSend,
}

impl SendPrivateForwardMessageParams {
    pub fn new(
        is_echo: bool,
        message_list: Vec<MessageSend>,
        sender: Arc<Sender>,
        msg: Arc<Message>,
        target: Target,
    ) -> Self {
        let user_id = match target {
            Target::Private(user_id) => user_id,
            _ => panic!("SendPrivateForwardMessageParams 只能用于私聊消息"),
        };
        let msg = get_msg(is_echo, message_list, sender, msg, target);
        Self {
            user_id,
            messages: MessageSend::Array(msg),
        }
    }
}

fn get_msg(
    is_echo: bool,
    message_list: Vec<MessageSend>,
    sender: Arc<Sender>,
    msg: Arc<Message>,
    target: Target,
) -> Vec<SegmentSend> {
    let mut message = Vec::with_capacity(message_list.len() + 3);
    if is_echo {
        let msg_content = match &*msg {
            event_body::message::Message::Private(p) => &p.message,
            event_body::message::Message::Group(g) => &g.message,
        };
        let msg_add = message::receive2send_add_prefix(
            msg_content,
            match target {
                Target::Group(group_id) => format!(
                    "来自群({group_id})的{}({} {})命令: ",
                    sender.card.as_deref().unwrap_or("未知群昵称"),
                    sender.nickname.as_deref().unwrap_or("未知昵称"),
                    sender.user_id.unwrap_or(0),
                ),
                Target::Private(user_id) => {
                    format!(
                        "用户{user_id}({})的命令: ",
                        sender.nickname.as_deref().unwrap_or("未知昵称")
                    )
                }
            },
        );
        message.push(message_body::SegmentSend::Node(
            message_body::node::DataSend::Content(message_body::node::DataSend2 {
                user_id: format!("{}", sender.user_id.unwrap_or(114514)),
                nickname: sender
                    .nickname
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| "用户指令".to_string()),
                content: box_new!(MessageSend, msg_add),
            }),
        ))
    };

    let messages = message_list;
    debug!("发送转发消息共{}条", messages.len());
    trace!(?messages);
    for msg in messages {
        message.push(message_body::SegmentSend::Node(
            message_body::node::DataSend::Content(message_body::node::DataSend2 {
                user_id: get_self_qq().to_string(),
                nickname: "指令回复".to_string(),
                content: box_new!(MessageSend, msg.clone()),
            }),
        ))
    }
    message
}
