use crate::abi::{logic_import::*, message::from_str};

#[handler(msg_type=Notice)]
pub async fn github(ctx: Context) -> Result<()> {
    ctx.send_message_async(from_str("喵喵喵"));
    Ok(())
}
