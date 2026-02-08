use crate::{
    abi::{
        message::{
            Event, MessageType, Type, event_body::message_sent::MessageSent, event_meta::MetaEvent,
        },
        network::BotClient,
        router::context::Context,
        websocket::{BotHandler, BotWebsocketClient},
    },
    logic::dispatch_all_handlers,
};
use anyhow::Result;
use async_trait::async_trait;
use std::{fmt, sync::Arc};
use tokio::sync::mpsc;
use tracing::{debug, trace};

pub trait Handler<T, M>: Send + Sync
where
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
    M: MessageType + fmt::Debug + Send + Sync + 'static,
{
    const FILTER_TYPE: Option<Type>;
    const FILTER_CMD: Option<&'static str>;

    fn handle(&self, context: &Context<T, M>) -> Result<()>;
}

#[async_trait]
pub trait Router<T>
where
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
{
    fn new(subscribe: mpsc::UnboundedReceiver<Event>, client: BotWebsocketClient<T>) -> Self;
    fn get_client(&self) -> Arc<T>;
    async fn run(&mut self) -> ();
}

pub trait SpawnContext<T, R>
where
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
    R: Router<T>,
{
    fn spawn_context<M: MessageType + fmt::Debug + Send + Sync + 'static>(&self, msg: Arc<M>);
}

impl<T, R> SpawnContext<T, R> for R
where
    T: BotClient + BotHandler + fmt::Debug + Send + Sync + 'static,
    R: Router<T>,
{
    fn spawn_context<M: MessageType + fmt::Debug + Send + Sync + 'static>(&self, msg: Arc<M>) {
        let client_arc = self.get_client();
        let context = Context::new(client_arc, msg);

        dispatch_all_handlers(context);
    }
}

pub struct NapcatRouter<T: BotHandler> {
    subscribe: mpsc::UnboundedReceiver<Event>,
    client: BotWebsocketClient<T>,
}

#[async_trait]
impl<T: BotHandler + BotClient + fmt::Debug> Router<T> for NapcatRouter<T> {
    fn new(subscribe: mpsc::UnboundedReceiver<Event>, client: BotWebsocketClient<T>) -> Self {
        NapcatRouter { subscribe, client }
    }

    fn get_client(&self) -> Arc<T> {
        self.client.handler.clone()
    }

    async fn run(&mut self) {
        while let Some(event) = self.subscribe.recv().await {
            match event {
                Event::Message(msg) => {
                    debug!("处理消息事件: {:?}", msg);
                    let ctx_data = Arc::new(*msg);
                    self.spawn_context(ctx_data);
                }
                Event::Notice(notice) => {
                    debug!("处理通知事件: {:?}", notice);
                    let ctx_data = Arc::new(notice);
                    self.spawn_context(ctx_data);
                }
                Event::Request(req) => {
                    debug!("处理请求事件: {:?}", req);
                    let ctx_data = Arc::new(req);
                    self.spawn_context(ctx_data);
                }
                Event::MetaEvent(meta) => {
                    debug!("处理元事件: {:?}", meta);

                    match meta {
                        MetaEvent::Heartbeat(hb) => {
                            trace!("收到心跳事件: {:?}", hb);
                        }
                        MetaEvent::Lifecycle(lc) => {
                            trace!("收到生命周期事件: {:?}", lc);
                        }
                    }
                }
                Event::MessageSent(message_sent) => {
                    debug!("处理消息发送事件: {:?}", message_sent);

                    match *message_sent {
                        MessageSent::Private(p) => {
                            trace!("私人消息已发送: {:?}", p);
                        }
                        MessageSent::Group(g) => {
                            trace!("群消息已发送: {:?}", g);
                        }
                    }
                }
            }
        }
    }
}
