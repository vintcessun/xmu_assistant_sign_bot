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
use tracing::{error, info, trace};

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
        Context {
            client,
            message,
            sender: Arc::from(sender),
            target,
            message_list,
            message_text: Arc::from(message_text),
            is_echo: false,
            send_msg: msg,
        }
    }

    pub fn set_echo(&mut self) {
        self.is_echo = true;
    }

    pub async fn send_message(&self, message: MessageSend) -> Result<()> {
        let message = Arc::new(message);
        match self.target {
            Target::Group(group_id) => {
                let params = api::SendGroupMessageParams::new(group_id, message.clone());
                let call = self.client.call_api(&params, Echo::new()).await?;
                let res = call.wait_echo().await?;
                trace!(?res);
                match res.status {
                    api::Status::Ok => Ok(()),
                    api::Status::Failed => Err(anyhow::anyhow!(
                        "发送群消息失败: {:?}",
                        res.message.unwrap_or("未知错误".to_string())
                    )),
                    api::Status::Async => Err(anyhow::anyhow!("发送群消息异步处理中")),
                }
            }
            Target::Private(user_id) => {
                let params = api::SendPrivateMessageParams::new(user_id, message.clone());
                let call = self.client.call_api(&params, Echo::new()).await?;
                let res = call.wait_echo().await?;
                trace!(?res);
                match res.status {
                    api::Status::Ok => Ok(()),
                    api::Status::Failed => Err(anyhow::anyhow!(
                        "发送私聊消息失败: {:?}",
                        res.message.unwrap_or("未知错误".to_string())
                    )),
                    api::Status::Async => Err(anyhow::anyhow!("发送私聊消息异步处理中")),
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
        let params = api::SpecialTitle::new(
            match self.target {
                Target::Group(group_id) => group_id,
                Target::Private(_) => {
                    return Err(anyhow::anyhow!(
                        "只能在群聊中设置特殊头衔，当前目标不是群聊"
                    ));
                }
            },
            self.sender.user_id.unwrap_or(0),
            title,
        );
        let call = self.client.call_api(&params, Echo::new()).await?;
        let res = call.wait_echo().await?;
        trace!(?res);
        match res.status {
            api::Status::Ok => Ok(()),
            api::Status::Failed => Err(anyhow::anyhow!(
                "设置特殊头衔失败: {:?}",
                res.message.unwrap_or("未知错误".to_string())
            )),
            api::Status::Async => Err(anyhow::anyhow!("设置特殊头衔异步处理中")),
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
                        trace!(?res);
                        match res.status {
                            api::Status::Ok => {}
                            api::Status::Failed => {
                                error!(
                                    "发送群转发消息失败: {:?}",
                                    res.message.unwrap_or("未知错误".to_string())
                                );
                            }
                            api::Status::Async => {
                                info!("发送群转发消息异步处理中");
                            }
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    {
                        Ok(_) => {}
                        Err(err) => {
                            error!("发送群转发消息失败: {:?}", err);
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
                        trace!(?res);
                        match res.status {
                            api::Status::Ok => {}
                            api::Status::Failed => {
                                error!(
                                    "发送私聊转发消息失败: {:?}",
                                    res.message.unwrap_or("未知错误".to_string())
                                );
                            }
                            api::Status::Async => {
                                info!("发送私聊转发消息异步处理中");
                            }
                        }
                        Ok::<(), anyhow::Error>(())
                    }
                    .await
                    {
                        Ok(_) => {}
                        Err(err) => {
                            error!("发送私聊转发消息失败: {:?}", err);
                        }
                    }
                }
            }
        }
    }
}
