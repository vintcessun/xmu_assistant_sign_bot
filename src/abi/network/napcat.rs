use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use tokio::sync::{OnceCell, mpsc};
use tokio_tungstenite::tungstenite::Utf8Bytes;
use tracing::{debug, error, info, trace};

use crate::abi::{
    echo::{Echo, echo_send_result},
    message::{Event, Params, api},
    network::BotClient,
    websocket::BotHandler,
};

#[derive(Debug)]
pub struct NapcatAdapter {
    event_sender: OnceCell<mpsc::UnboundedSender<String>>,
    api_sender: OnceCell<mpsc::UnboundedSender<String>>,
    handler: mpsc::UnboundedSender<Event>,
}

impl NapcatAdapter {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Event>) {
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        (
            NapcatAdapter {
                event_sender: OnceCell::new(),
                api_sender: OnceCell::new(),
                handler: tx,
            },
            rx,
        )
    }
}

#[async_trait]
impl BotClient for NapcatAdapter {
    async fn call_api<'a, T: Params + Serialize + fmt::Debug>(
        &'a self,
        params: &'a T,
        echo: Echo,
    ) -> Result<api::ApiResponsePending<T::Response>> {
        let action = T::ACTION;

        let api_send = api::ApiSend {
            action,
            params,
            echo,
        };
        let msg = serde_json::to_string(&api_send)?;
        debug!(action = %action, "正在调用 API");
        trace!(api_send = ?api_send, "发送的 API 请求详情");

        if let Some(sender) = self.api_sender.get() {
            if let Err(e) = sender.send(msg) {
                error!(error = ?e, "发送 API 消息失败");
            } else {
                trace!(action = %action, "API 消息已成功发送到 WebSocket 线程");
            }
        } else {
            error!("API 消息发送通道未初始化");
        }

        Ok(api::ApiResponsePending::new(echo))
    }
}

#[derive(Deserialize, Debug)]
struct EchoOnly {
    echo: String,
}

#[async_trait]
impl BotHandler for NapcatAdapter {
    async fn init(
        &self,
        event: mpsc::UnboundedSender<String>,
        api: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        self.event_sender.set(event)?;
        self.api_sender.set(api)?;

        info!("Napcat 适配器初始化成功");

        Ok(())
    }

    async fn handle_api(&self, message: Utf8Bytes) {
        debug!(message = %message, "收到 API 返回");
        trace!(message = ?message, "原始 API 返回消息");

        let echo_only = serde_json::from_slice::<EchoOnly>(message.as_bytes()).ok();

        let echo = match echo_only {
            None => {
                error!("解析 API 返回的 Echo 字段失败，无法匹配响应");
                return;
            }
            Some(e) => e.echo,
        };

        echo_send_result(&echo, message);
    }

    async fn handle_event(&self, event: Utf8Bytes) {
        debug!(event = %event, "收到事件");
        trace!(event = ?event, "原始事件消息");

        let data = serde_json::from_slice::<Event>(event.as_bytes());

        match data {
            Ok(evt) => {
                debug!(event = ?evt, "事件解析成功");
                trace!(event = ?evt, "已解析的事件对象");

                if let Err(e) = self.handler.send(evt) {
                    error!(error = ?e, "分发事件失败");
                } else {
                    trace!("事件已成功分发到内部处理通道");
                }
            }
            Err(e) => {
                error!(error = ?e, "事件解析失败");
            }
        }
    }

    async fn on_connect(&self) {
        info!("成功连接到服务器");
    }

    async fn on_disconnect(&self) {
        info!("已断开与服务器的连接");
    }
}
