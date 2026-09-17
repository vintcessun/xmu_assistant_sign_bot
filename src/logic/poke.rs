use crate::abi::message::event_notice::Notify;
use crate::abi::{logic_import::*, message::from_str};
use crate::logic::Notice;
use crate::logic::status::render_card;

#[handler(msg_type=Notice)]
pub async fn poke(ctx: Context) -> Result<()> {
    if let Notice::Notify(Notify::Poke(event)) = ctx.get_message().as_ref() {
        if event.target_id == event.self_id {
            // 戳的是机器人：把发起者自己的状态卡回过去。群里不带学号。
            let in_group = event.group_id.is_some();
            let card = render_card(event.user_id, in_group).await;
            ctx.send_message(from_str(card)).await?;
        } else {
            ctx.send_message(from_str("喵喵喵")).await?;
        }
    }
    Ok(())
}
