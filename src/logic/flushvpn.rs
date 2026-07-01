use crate::abi::logic_import::*;
use crate::abi::message::from_str;
use crate::api::xmu_service::securelink::SecureLinkApi;
use crate::logic::BuildHelp;
use crate::web::vpn::task::{create_flow, page_url};
use anyhow::anyhow;

#[handler(msg_type=Message,command="flushvpn",echo_cmd=true,
help_msg=r#"用法:/flushvpn
功能:创建 SecureLink VPN 登录网页并发送链接。
扫码/点击登录后，在网页粘贴浏览器最终跳转到的 callback 地址即可刷新登录
（服务端捕获 code 完成 SSO），并可在网页查看/刷新 VPN 内容和出口 IP。"#)]
pub async fn flushvpn(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let qq = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;

    ctx.send_message_async(from_str("正在向 SecureLink 申请登录链接…"));

    let mut api = SecureLinkApi::new()?;
    let (login_url, auth_name) = api.begin_sso().await?;
    let flow = create_flow(qq, login_url, auth_name, api);
    let url = page_url(&flow.id);

    ctx.send_message_async(from_str(format!(
        "请打开以下网页扫码登录 SecureLink VPN（20 分钟内有效）：\n{url}\n\
         登录后在网页粘贴 callback 即可刷新登录，并可查看/刷新 VPN 内容和出口 IP。"
    )));

    Ok(())
}
