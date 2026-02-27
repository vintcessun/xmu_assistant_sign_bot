use super::super::BuildHelp;
use crate::abi::{logic_import::*, message::from_str};
use crate::web::rollcall::get_url;

#[handler(msg_type=Message,command="signapi",echo_cmd=true,
help_msg=r#"用法:/signapi
功能:获取签到API地址"#)]
pub async fn sign_api(ctx: Context) -> Result<()> {
    ctx.send_message_async(from_str(get_url()));
    Ok(())
}
