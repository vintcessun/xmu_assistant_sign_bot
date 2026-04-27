use genai::chat::ChatMessage;
use tracing::{error, info, trace, warn};

use crate::{
    abi::{
        Context,
        echo::Echo,
        logic_import::{Message, Notice},
        message::{
            MessageType,
            api::{self, GetGroupInfo, GroupMemberInfo},
            event_notice::Notify,
        },
        network::BotClient,
        websocket::BotHandler,
    },
    api::llm::chat::{
        archive::{
            bridge::{llm_msg_from_message, llm_msg_from_notice},
            identity::{IdentityGroupUpdateSend, IdentityPersonUpdateSend, IdentityUpdate},
            message_storage::{MessageStorage, NoticeStorage},
        },
        impression::push_message,
    },
};

pub async fn message_archive<T>(ctx: &mut Context<T, Message>)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    let message = ctx.get_message();
    let id = match &*message {
        Message::Group(g) => g.message_id,
        Message::Private(p) => p.message_id,
    };
    let user_id = message.get_sender().user_id.unwrap_or_default();
    let group_id = match &*message {
        Message::Group(g) => g.group_id,
        Message::Private(p) => -p.user_id,
    };

    let msg_content = llm_msg_from_message(ctx.client.clone(), &message).await;
    let msg_single = msg_content
        .iter()
        .map(|x| x.content.parts().clone())
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    //消息记录
    MessageStorage::save_with_group(&id.to_string(), msg_content, group_id).await;
    trace!(message_id = ?id, "消息内容存储完成");

    //印象记录
    if let Err(e) = push_message(user_id, ChatMessage::user(msg_single)).await {
        warn!(message_id = ?id, error = ?e, "消息印象推送失败");
    }
    trace!(message_id = ?id, "消息印象推送处理完成");
}

pub async fn notice_archive<T>(ctx: &mut Context<T, Notice>)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    trace!(notice = ?ctx.get_message(), "开始归档通知");
    let notice = ctx.get_message();
    let time = match &*notice {
        Notice::GroupUpload(e) => e.time,
        Notice::GroupAdmin(e) => e.time,
        Notice::GroupDecrease(e) => e.time,
        Notice::GroupIncrease(e) => e.time,
        Notice::GroupBan(e) => e.time,
        Notice::FriendAdd(e) => e.time,
        Notice::GroupRecall(e) => e.time,
        Notice::FriendRecall(e) => e.time,
        Notice::GroupMsgEmojiLike(e) => e.time,
        Notice::Notify(e) => match e {
            Notify::Poke(e) => e.time,
            Notify::LuckyKing(e) => e.time,
            Notify::Honor(e) => e.time,
            Notify::Title(e) => e.time,
        },
        Notice::GroupCard(e) => e.time,
    };

    let notice_content = llm_msg_from_notice(&notice).await;
    NoticeStorage::save(time, notice_content).await;
    trace!(time = ?time, "通知内容存储完成");
}

pub async fn identity_person_archive<T>(ctx: &mut Context<T, Message>)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    let msg = ctx.get_message();
    match &*msg {
        Message::Private(p) => {
            warn!(message = ?p, "私人消息不进行身份归档");
        }
        Message::Group(p) => {
            let params = GroupMemberInfo::new(p.group_id, p.user_id, false);
            let call = ctx.client.call_api(&params, Echo::new()).await;
            match call {
                Ok(call) => {
                    let res = call.wait_echo().await;
                    trace!(?res);
                    match res {
                        Ok(res) => match res.status {
                            api::Status::Ok => {
                                if let Some(data) = res.data {
                                    let person = IdentityPersonUpdateSend {
                                        qq: data.user_id,
                                        group_id: Some(data.group_id),
                                        now_nickname: data.nickname,
                                        now_group_nickname: Some(data.card),
                                    };
                                    IdentityUpdate::person_update(person);
                                }
                            }
                            api::Status::Failed => {
                                error!(
                                    message = ?res.message,
                                    "获取群聊个人信息失败"
                                );
                            }
                            api::Status::Async => {
                                info!("正在异步处理获取群聊个人信息");
                            }
                        },
                        Err(e) => {
                            error!(error = ?e, "等待群聊个人信息回声时出错");
                        }
                    }
                }
                Err(e) => {
                    error!(error = ?e, "获取群聊个人信息 API 调用失败");
                }
            }
        }
    }
}

pub async fn identity_group_archive<T>(ctx: &mut Context<T, Message>)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    match &*ctx.get_message() {
        Message::Private(p) => {
            warn!(message = ?p, "私人消息不进行群身份归档");
        }
        Message::Group(p) => {
            let params = GetGroupInfo::new(p.group_id, false);
            let call = ctx.client.call_api(&params, Echo::new()).await;
            match call {
                Ok(call) => {
                    let res = call.wait_echo().await;
                    trace!(?res);
                    match res {
                        Ok(res) => match res.status {
                            api::Status::Ok => {
                                if let Some(data) = res.data {
                                    let group = IdentityGroupUpdateSend {
                                        group_id: data.group_id,
                                        now_name: data.group_name,
                                    };
                                    IdentityUpdate::group_update(group);
                                }
                            }
                            api::Status::Failed => {
                                error!(
                                    message = ?res.message,
                                    "获取群聊信息失败"
                                );
                            }
                            api::Status::Async => {
                                info!("正在异步处理获取群聊信息");
                            }
                        },
                        Err(e) => {
                            error!(error = ?e, "等待群聊信息回声时出错");
                        }
                    }
                }
                Err(e) => {
                    error!(error = ?e, "获取群聊信息 API 调用失败");
                }
            }
        }
    }
}
