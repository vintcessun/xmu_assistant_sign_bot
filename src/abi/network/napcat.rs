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
        debug!("调用 API: {}", action);
        trace!(?api_send);

        if let Some(sender) = self.api_sender.get() {
            if let Err(e) = sender.send(msg) {
                error!("发送 API 消息失败: {:?}", e);
            }
        } else {
            error!("API 发送通道未初始化");
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

        Ok(())
    }

    async fn handle_api(&self, message: Utf8Bytes) {
        debug!("收到API返回: {}", message);
        trace!(?message);

        let echo_only = serde_json::from_slice::<EchoOnly>(message.as_bytes()).ok();

        let echo = match echo_only {
            None => {
                error!("解析 API 返回的 Echo 失败");
                return;
            }
            Some(e) => e.echo,
        };

        echo_send_result(&echo, message);
    }

    async fn handle_event(&self, event: Utf8Bytes) {
        debug!("收到事件: {}", event);
        trace!(?event);

        let data = serde_json::from_slice::<Event>(event.as_bytes());

        match data {
            Ok(evt) => {
                debug!("解析事件成功: {:?}", evt);
                trace!(?evt);

                if let Err(e) = self.handler.send(evt) {
                    error!("分发事件失败: {:?}", e);
                }
            }
            Err(e) => {
                error!("解析事件失败: {:?}", e);
            }
        }
    }

    async fn on_connect(&self) {
        info!("连接到服务器。");
    }

    async fn on_disconnect(&self) {
        info!("已断开与服务器的连接。");
    }
}
