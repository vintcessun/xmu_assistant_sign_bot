use tracing::{debug, info};

use crate::{
    abi::{
        Context,
        logic_import::{Message, Notice},
        network::BotClient,
        websocket::BotHandler,
    },
    api::llm::chat::{
        archive::{
            identity_group_archive, identity_person_archive, message_archive, notice_archive,
        },
        deep::send_message_from_llm,
        repeat::send_message_from_hot,
        search::send::send_message_from_store,
    },
};

pub async fn handle_llm_message<T>(ctx: &mut Context<T, Message>)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    message_archive(ctx).await;
    identity_person_archive(ctx).await;
    identity_group_archive(ctx).await;

    //L0: 命中回复
    match send_message_from_hot(ctx).await {
        Ok(_) => {
            info!("L0: 命中回复成功，结束路由");
            return;
        }
        Err(e) => debug!(error = ?e, "L0: 命中回复处理失败，继续路由"),
    }

    //L1: 搜索回复
    match send_message_from_store(ctx).await {
        Ok(_) => {
            info!("L1: 搜索回复成功，结束路由");
            return;
        }
        Err(e) => debug!(error = ?e, "L1: 搜索回复处理失败，继续路由"),
    }

    //L2: 深度回复
    match send_message_from_llm(ctx).await {
        Ok(_) => {
            info!("L2: 深度回复成功，结束路由");
            return;
        }
        Err(e) => debug!(error = ?e, "L2: 深度回复处理失败"),
    }

    info!(
        message = ?ctx.get_message(),
        "未生成 LLM 回复，消息路由结束"
    );
}

pub async fn handle_llm_notice<T>(ctx: &mut Context<T, Notice>)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    notice_archive(ctx).await;
}
