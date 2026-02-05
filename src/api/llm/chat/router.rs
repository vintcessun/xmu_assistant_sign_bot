use tracing::debug;

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
    if send_message_from_hot(ctx).await.is_ok() {
        return;
    }

    //L1: 搜索回复
    if send_message_from_store(ctx).await.is_ok() {
        return;
    }

    debug!(
        "No LLM reply generated for message: {:?}",
        ctx.get_message()
    );
}

pub async fn handle_llm_notice<T>(ctx: &mut Context<T, Notice>)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    notice_archive(ctx).await;
}
