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
use tracing::{debug, error, info};

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
        info!(
            "正在连接到 WebSocket 服务器... {}:{}",
            self.config.host, self.config.port
        );
        debug!(?self.config);

        let url_event = format!("ws://{}:{}/event", self.config.host, self.config.port);
        let (ws_stream, _) = connect_async(&url_event).await?;
        let (mut write_event, mut read_event) = ws_stream.split();

        let handler = self.handler.clone();

        self.event_read_task = Some(tokio::spawn(async move {
            while let Some(message) = read_event.next().await {
                if let Ok(Message::Text(msg)) = message {
                    let h = handler.clone();
                    tokio::spawn(async move {
                        h.handle_event(msg).await;
                    });
                }
            }
        }));

        let (event_sender, mut event_receiver) = mpsc::unbounded_channel::<String>();

        self.event_write_task = Some(tokio::spawn(async move {
            while let Some(msg) = event_receiver.recv().await {
                if let Err(e) = write_event.send(Message::Text(msg.into())).await {
                    error!("传输Event失败通过 WsWriter: {:?}", e);
                    break;
                }
            }
        }));

        let url_api = format!("ws://{}:{}/api", self.config.host, self.config.port);
        let (ws_stream, _) = connect_async(&url_api).await?;
        let (mut write_api, mut read_api) = ws_stream.split();

        let handler = self.handler.clone();

        self.api_read_task = Some(tokio::spawn(async move {
            while let Some(message) = read_api.next().await {
                if let Ok(Message::Text(msg)) = message {
                    let h = handler.clone();
                    tokio::spawn(async move {
                        h.handle_api(msg).await;
                    });
                }
            }
        }));

        let (api_sender, mut api_receiver) = mpsc::unbounded_channel::<String>();

        self.api_write_task = Some(tokio::spawn(async move {
            while let Some(msg) = api_receiver.recv().await {
                if let Err(e) = write_api.send(Message::Text(msg.into())).await {
                    error!("传输Message失败通过 WsWriter: {:?}", e);
                    break;
                }
            }
        }));

        self.handler.init(event_sender, api_sender).await?;
        self.handler.on_connect().await;

        Ok(())
    }

    pub fn disconnect(&mut self) {
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
