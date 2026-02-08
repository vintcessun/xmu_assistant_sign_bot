use crate::abi::echo::Echo;
use crate::abi::logic_import::Message;
use crate::abi::message::MessageSend;
use crate::abi::message::Sender;
use crate::abi::message::Type;
use crate::abi::message::api;
use crate::abi::message::{MessageType, Target};
use crate::abi::network::BotClient;
use crate::abi::websocket::BotHandler;
use anyhow::Result;
use std::fmt;
use std::sync::Arc;
use tracing::{debug, error, info, trace, warn};

#[derive(Debug)]
pub struct Context<
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
    M: MessageType + fmt::Debug + Send + Sync + 'static,
> {
    pub client: Arc<T>,
    pub message: Arc<M>,
    pub sender: Arc<Sender>,
    pub message_list: Vec<MessageSend>,
    pub message_text: Arc<str>,
    pub target: Target,
    pub is_echo: bool,
    send_msg: Option<Arc<Message>>,
}

impl<
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
    M: MessageType + fmt::Debug + Send + Sync + 'static,
> Clone for Context<T, M>
{
    fn clone(&self) -> Self {
        Context {
            client: self.client.clone(),
            message: self.message.clone(),
            sender: self.sender.clone(),
            message_list: self.message_list.clone(),
            message_text: self.message_text.clone(),
            target: self.target,
            is_echo: self.is_echo,
            send_msg: self.send_msg.clone(),
        }
    }
}

impl<
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
    M: MessageType + fmt::Debug + Send + Sync + 'static,
> Context<T, M>
{
    pub fn new(client: Arc<T>, message: Arc<M>) -> Self {
        let target = message.get_target();
        let message_text = message.get_text();
        let sender = message.get_sender();
        let message_list = Vec::new();

        let msg = if message.get_type() == Type::Message {
            Some(unsafe {
                // 将 Arc<M> 强转为 Arc<Message>
                // 这种强转前提是 M 的实例在内存中确实是一个 Message 枚举
                std::mem::transmute::<Arc<M>, Arc<Message>>(message.clone())
            })
        } else {
            None
        };
        let ctx = Context {
            client,
            message,
            sender: Arc::from(sender),
            target,
            message_list,
            message_text: Arc::from(message_text),
            is_echo: false,
            send_msg: msg,
        };
        trace!(
            message_type = ?ctx.message.get_type(),
            target = ?ctx.target,
            sender = ?ctx.sender,
            "Context 创建成功"
        );
        ctx
    }

    pub fn set_echo(&mut self) {
        self.is_echo = true;
    }

    pub async fn send_message(&self, message: MessageSend) -> Result<()> {
        let message = Arc::new(message);
        match self.target {
            Target::Group(group_id) => {
                trace!(group_id = ?group_id, message = ?message, "准备发送群消息");
                let params = api::SendGroupMessageParams::new(group_id, message.clone());
                let call = self.client.call_api(&params, Echo::new()).await?;
                let res = call.wait_echo().await?;
                trace!(response = ?res, "发送群消息 API 返回");
                match res.status {
                    api::Status::Ok => {
                        info!(group_id = ?group_id, "群消息发送成功");
                        Ok(())
                    }
                    api::Status::Failed => {
                        error!(
                            group_id = ?group_id,
                            error_message = ?res.message,
                            "发送群消息失败"
                        );
                        Err(anyhow::anyhow!(
                            "发送群消息失败: {:?}",
                            res.message.unwrap_or("未知错误".to_string())
                        ))
                    }
                    api::Status::Async => {
                        warn!(group_id = ?group_id, "发送群消息异步处理中");
                        Err(anyhow::anyhow!("发送群消息异步处理中"))
                    }
                }
            }
            Target::Private(user_id) => {
                trace!(user_id = ?user_id, message = ?message, "准备发送私聊消息");
                let params = api::SendPrivateMessageParams::new(user_id, message.clone());
                let call = self.client.call_api(&params, Echo::new()).await?;
                let res = call.wait_echo().await?;
                trace!(response = ?res, "发送私聊消息 API 返回");
                match res.status {
                    api::Status::Ok => {
                        info!(user_id = ?user_id, "私聊消息发送成功");
                        Ok(())
                    }
                    api::Status::Failed => {
                        error!(
                            user_id = ?user_id,
                            error_message = ?res.message,
                            "发送私聊消息失败"
                        );
                        Err(anyhow::anyhow!(
                            "发送私聊消息失败: {:?}",
                            res.message.unwrap_or("未知错误".to_string())
                        ))
                    }
                    api::Status::Async => {
                        warn!(user_id = ?user_id, "发送私聊消息异步处理中");
                        Err(anyhow::anyhow!("发送私聊消息异步处理中"))
                    }
                }
            }
        }
    }

    pub fn send_message_async(&mut self, message: MessageSend) {
        self.message_list.push(message);
    }

    pub fn get_message(&self) -> Arc<M> {
        self.message.clone()
    }

    pub fn get_message_text(&self) -> &str {
        &self.message_text
    }

    pub fn get_target(&self) -> Target {
        self.target
    }

    pub async fn set_title(&self, title: String) -> Result<()> {
        let group_id = match self.target {
            Target::Group(group_id) => group_id,
            Target::Private(_) => {
                warn!(target = ?self.target, "尝试在私聊中设置特殊头衔");
                return Err(anyhow::anyhow!(
                    "只能在群聊中设置特殊头衔，当前目标不是群聊"
                ));
            }
        };

        let user_id = self.sender.user_id.unwrap_or(0);
        trace!(group_id = ?group_id, user_id = ?user_id, title = ?title, "准备设置特殊头衔");

        let params = api::SpecialTitle::new(group_id, user_id, title);
        let call = self.client.call_api(&params, Echo::new()).await?;
        let res = call.wait_echo().await?;
        trace!(response = ?res, "设置特殊头衔 API 返回");
        match res.status {
            api::Status::Ok => {
                info!(group_id = ?group_id, user_id = ?user_id, "特殊头衔设置成功");
                Ok(())
            }
            api::Status::Failed => {
                error!(
                    group_id = ?group_id,
                    user_id = ?user_id,
                    error_message = ?res.message,
                    "设置特殊头衔失败"
                );
                Err(anyhow::anyhow!(
                    "设置特殊头衔失败: {:?}",
                    res.message.unwrap_or("未知错误".to_string())
                ))
            }
            api::Status::Async => {
                warn!(group_id = ?group_id, user_id = ?user_id, "设置特殊头衔异步处理中");
                Err(anyhow::anyhow!("设置特殊头衔异步处理中"))
            }
        }
    }
}

impl<
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
    M: MessageType + fmt::Debug + Send + Sync + 'static,
> Context<T, M>
{
    pub async fn finish(self) {
        if self.message_list.is_empty() {
            return;
        }

        let client = self.client;
        let target = self.target;
        let list = self.message_list;
        let sender = self.sender.clone();
        let is_echo = self.is_echo;
        let msg = self.send_msg;

        if let Some(msg) = msg {
            match target {
                Target::Group(_) => {
                    let params = api::SendGroupForwardMessageParams::new(
                        is_echo,
                        list,
                        sender,
                        msg.clone(),
                        target,
                    );
                    match async move {
                        let call = client.call_api(&params, Echo::new()).await?;
                        let res = call.wait_echo().await?;
                        trace!(response = ?res, "发送群转发消息 API 返回");
                        match res.status {
                            api::Status::Ok => {
                                info!("群转发消息发送成功");
                            }
                            api::Status::Failed => {
                                error!(
                                    message = ?res.message,
                                    "发送群转发消息失败"
                                );
                            }
                            api::Status::Async => {
                                warn!("发送群转发消息正在异步处理");
                            }
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    {
                        Ok(_) => {
                            debug!("群转发消息任务完成");
                        }
                        Err(err) => {
                            error!(error = ?err, "发送群转发消息失败");
                        }
                    }
                }
                Target::Private(_) => {
                    let params = api::SendPrivateForwardMessageParams::new(
                        is_echo,
                        list,
                        sender,
                        msg.clone(),
                        target,
                    );
                    match async move {
                        let call = client.call_api(&params, Echo::new()).await?;
                        let res = call.wait_echo().await?;
                        trace!(response = ?res, "发送私聊转发消息 API 返回");
                        match res.status {
                            api::Status::Ok => {
                                info!("私聊转发消息发送成功");
                            }
                            api::Status::Failed => {
                                error!(
                                    message = ?res.message,
                                    "发送私聊转发消息失败"
                                );
                            }
                            api::Status::Async => {
                                warn!("发送私聊转发消息正在异步处理");
                            }
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    {
                        Ok(_) => {
                            debug!("私聊转发消息任务完成");
                        }
                        Err(err) => {
                            error!(error = ?err, "发送私聊转发消息失败");
                        }
                    }
                }
            }
        }
    }
}
