use crate::abi::message::event_notice::Notify;
use crate::abi::{logic_import::*, message::from_str};
use crate::logic::Notice;

#[handler(msg_type=Notice)]
pub async fn poke(ctx: Context) -> Result<()> {
    if let Notice::Notify(e) = ctx.get_message().as_ref()
        && let Notify::Poke(_) = e
    {
        ctx.send_message(from_str("喵喵喵")).await?;
    }
    Ok(())
}
