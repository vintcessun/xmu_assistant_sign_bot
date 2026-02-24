pub mod client;
pub mod echo;
pub mod message;
pub mod network;
pub mod router;
pub mod utils;
pub mod websocket;

use crate::{
    abi::{network::NapcatAdapter, router::handler::Router},
    config::ServerConfig,
};
use anyhow::Result;
pub use router::context::Context;
pub use router::handler::Handler;
use router::handler::NapcatRouter;
use tracing::debug;

pub async fn run(config: ServerConfig) -> Result<NapcatRouter<NapcatAdapter>> {
    debug!("正在创建 Napcat 网络适配器");
    let (adapter, subscribe) = network::NapcatAdapter::new();
    debug!(config = ?config, "正在创建 Bot WebSocket 客户端");
    let mut client = websocket::BotWebsocketClient::new(config, adapter);
    debug!("尝试连接到 Napcat WebSocket 服务...");
    client.connect().await?;
    debug!("Napcat WebSocket 连接成功");
    let router = NapcatRouter::new(subscribe, client);
    Ok(router)
}

pub mod logic_import {
    pub async fn handle_error<T, M>(
        ctx: &mut Context<T, M>,
        fn_name: &'static str,
        err: anyhow::Error,
    ) where
        T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
        M: message::MessageType + std::fmt::Debug + Send + Sync + 'static,
    {
        use tracing::warn;
        ctx.send_message_async(message::from_str(format!(
            "Logic [{}] 运行出现错误: {}",
            fn_name, err
        )));
        warn!(function = ?fn_name, error = ?err, "Logic 运行出错");
    }

    pub use crate::abi::message;
    pub use crate::abi::{
        Context, Handler,
        message::{
            MessageType, Target, event::Type, event_message::Message, event_notice::Notice,
            event_request::Request,
        },
        network::BotClient,
        websocket::BotHandler,
    };
    pub use crate::config;
    pub use helper::handler;
    pub use helper::register_handler_with_help;
    pub use std::fmt;
}
