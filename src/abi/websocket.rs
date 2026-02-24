use crate::config::ServerConfig;
use anyhow::Result;
use async_trait::async_trait;
use futures_util::StreamExt;
use futures_util::sink::SinkExt;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, Utf8Bytes},
};
use tracing::{debug, error, info, trace, warn};

#[async_trait]
pub trait BotHandler: Send + Sync + 'static {
    async fn init(
        &self,
        event: mpsc::UnboundedSender<String>,
        api: mpsc::UnboundedSender<String>,
    ) -> Result<()>;
    async fn handle_api(&self, message: Utf8Bytes);
    async fn handle_event(&self, event: Utf8Bytes);
    async fn on_connect(&self);
    async fn on_disconnect(&self);
}

pub struct BotWebsocketClient<T: BotHandler> {
    config: ServerConfig,
    pub handler: Arc<T>,

    event_read_task: Option<tokio::task::JoinHandle<()>>,
    event_write_task: Option<tokio::task::JoinHandle<()>>,

    api_read_task: Option<tokio::task::JoinHandle<()>>,
    api_write_task: Option<tokio::task::JoinHandle<()>>,
}

impl<T: BotHandler> BotWebsocketClient<T> {
    pub fn new(config: ServerConfig, handler: T) -> Self {
        BotWebsocketClient {
            config,
            handler: Arc::new(handler),
            event_read_task: None,
            event_write_task: None,
            api_read_task: None,
            api_write_task: None,
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        let (reconnect_tx, mut reconnect_rx) = mpsc::channel::<()>(1);

        loop {
            match self.connect_once(reconnect_tx.clone()).await {
                Ok(_) => {
                    info!("WebSocket 连接已建立");
                    // 等待重连信号
                    if reconnect_rx.recv().await.is_some() {
                        warn!("收到重连信号，准备重新连接...");
                        self.disconnect_tasks();
                    } else {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = ?e, "WebSocket 连接失败，5秒后重试...");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }

        Ok(())
    }

    async fn connect_once(&mut self, reconnect_tx: mpsc::Sender<()>) -> Result<()> {
        info!(host = ?self.config.host, port = ?self.config.port, "正在连接到 WebSocket 服务器");
        debug!(config = ?self.config, "配置详情");

        let url_event = format!("ws://{}:{}/event", self.config.host, self.config.port);
        info!(url = ?url_event, "尝试连接事件 WebSocket");
        let (ws_stream, _) = connect_async(&url_event).await?;
        info!(url = ?url_event, "事件 WebSocket 连接成功");
        let (mut write_event, mut read_event) = ws_stream.split();

        let handler = self.handler.clone();
        let reconnect_tx_event = reconnect_tx.clone();
        self.event_read_task = Some(tokio::spawn(async move {
            while let Some(message) = read_event.next().await {
                match message {
                    Ok(Message::Text(msg)) => {
                        debug!("接收到事件文本消息");
                        let h = handler.clone();
                        tokio::spawn(async move {
                            h.handle_event(msg).await;
                        });
                    }
                    Ok(m) => {
                        trace!(message_type = ?m, "接收到非文本事件消息，已忽略");
                    }
                    Err(e) => {
                        warn!(error = ?e, "读取事件 WebSocket 消息时发生错误，中断读取任务");
                        let _ = reconnect_tx_event.send(()).await;
                        break;
                    }
                }
            }
            let _ = reconnect_tx_event.send(()).await;
        }));

        let (event_sender, mut event_receiver) = mpsc::unbounded_channel::<String>();
        let reconnect_tx_event_w = reconnect_tx.clone();
        self.event_write_task = Some(tokio::spawn(async move {
            while let Some(msg) = event_receiver.recv().await {
                trace!(message = ?msg, "准备发送事件消息");
                if let Err(e) = write_event.send(Message::Text(msg.into())).await {
                    error!(error = ?e, "通过 WsWriter 传输事件失败");
                    let _ = reconnect_tx_event_w.send(()).await;
                    break;
                }
                trace!("事件消息发送成功");
            }
            let _ = reconnect_tx_event_w.send(()).await;
        }));

        let url_api = format!("ws://{}:{}/api", self.config.host, self.config.port);
        info!(url = ?url_api, "尝试连接 API WebSocket");
        let (ws_stream, _) = connect_async(&url_api).await?;
        info!(url = ?url_api, "API WebSocket 连接成功");
        let (mut write_api, mut read_api) = ws_stream.split();

        let handler = self.handler.clone();
        let reconnect_tx_api = reconnect_tx.clone();
        self.api_read_task = Some(tokio::spawn(async move {
            while let Some(message) = read_api.next().await {
                match message {
                    Ok(Message::Text(msg)) => {
                        debug!("接收到 API 文本消息");
                        let h = handler.clone();
                        tokio::spawn(async move {
                            h.handle_api(msg).await;
                        });
                    }
                    Ok(m) => {
                        trace!(message_type = ?m, "接收到非文本 API 消息，已忽略");
                    }
                    Err(e) => {
                        warn!(error = ?e, "读取 API WebSocket 消息时发生错误，中断读取任务");
                        let _ = reconnect_tx_api.send(()).await;
                        break;
                    }
                }
            }
            let _ = reconnect_tx_api.send(()).await;
        }));

        let (api_sender, mut api_receiver) = mpsc::unbounded_channel::<String>();
        let reconnect_tx_api_w = reconnect_tx.clone();
        self.api_write_task = Some(tokio::spawn(async move {
            while let Some(msg) = api_receiver.recv().await {
                trace!(message = ?msg, "准备发送 API 消息");
                if let Err(e) = write_api.send(Message::Text(msg.into())).await {
                    error!(error = ?e, "通过 WsWriter 传输消息失败");
                    let _ = reconnect_tx_api_w.send(()).await;
                    break;
                }
                trace!("API 消息发送成功");
            }
            let _ = reconnect_tx_api_w.send(()).await;
        }));

        self.handler.init(event_sender, api_sender).await?;
        self.handler.on_connect().await;

        Ok(())
    }

    fn disconnect_tasks(&mut self) {
        if let Some(task) = self.event_read_task.take() {
            task.abort();
        }
        if let Some(task) = self.event_write_task.take() {
            task.abort();
        }
        if let Some(task) = self.api_read_task.take() {
            task.abort();
        }
        if let Some(task) = self.api_write_task.take() {
            task.abort();
        }
    }

    pub fn disconnect(&mut self) {
        self.disconnect_tasks();
        // Cannot await in drop; try to call on_disconnect if runtime allows elsewhere.
        let handler = self.handler.clone();
        tokio::spawn(async move {
            handler.on_disconnect().await;
        });
        info!("已断开与 WebSocket 服务器的连接");
    }
}

impl<T: BotHandler> Drop for BotWebsocketClient<T> {
    fn drop(&mut self) {
        self.disconnect();
    }
}
