use super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::from_str},
    logic::login::process::process_login_castgc,
    web::{
        guard::PasswordMode,
        jdfpm::task::{create_session, page_url},
    },
};
use anyhow::anyhow;
use tracing::warn;

#[handler(msg_type=Message,command="jdpm",echo_cmd=true,
help_msg=r#"用法:/jdpm [自定义口令]
[自定义口令]:可选，至少 4 位，只在私聊里有效；不填则由机器人随机生成一个
功能:创建一个受口令保护的绩点/排名查询网页。
在网页上可以查看成绩范围、申请绩点计算、生成绩点证明 PDF，
并把证明正文（含教务接口以 * 屏蔽的排名）直接显示在网页上
注:群聊里使用时链接和口令改走私聊；群聊里打出的自定义口令视为已泄露，
会被忽略并改用随机口令。网页与口令 30 分钟内有效"#)]
pub async fn jdpm(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let qq = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;

    let in_group = matches!(ctx.get_target(), Target::Group(_));
    let (mode, leaked) = choose_mode(in_group, parse_custom_password(ctx.get_message_text()));
    if leaked {
        warn!(user_id = qq, "群聊中提供了自定义口令，已作废并改用随机口令");
    }

    let client = process_login_castgc(&mut ctx, qq).await?;
    let (session, password) = create_session(qq, client, mode)?;
    let url = page_url(&session.id);

    let mut detail = format!(
        "绩点/排名查询网页（30 分钟内有效）：\n{url}\n访问口令：{password}\n\
         口令只在网页内存中保留，重新输入口令会把上一位访问者顶下线。"
    );
    if leaked {
        detail = format!(
            "你在群里打出的自定义口令同群成员都能看到（指令回显还会再广播一次），\
             已作废并改用随机口令。需要自定义口令请私聊发送 /jdpm <口令>。\n\n{detail}"
        );
    }

    match ctx.get_target() {
        // 群聊里直接贴口令等于没保护，先尝试私聊。
        Target::Group(_) => match ctx.send_private_message(qq, from_str(detail.clone())).await {
            Ok(()) => {
                ctx.send_message_async(from_str("绩点查询链接与访问口令已私聊发送，请查收。"));
            }
            Err(e) => {
                warn!(user_id = qq, error = ?e, "私聊发送绩点查询口令失败，退回群内发送");
                ctx.send_message_async(from_str(format!(
                    "私聊发送失败（可能未加好友），只能在群内发送，请尽快撤回本条消息：\n{detail}"
                )));
            }
        },
        Target::Private(_) => ctx.send_message_async(from_str(detail)),
    }

    Ok(())
}

/// 决定用哪种口令，并返回“群聊里的自定义口令是否被作废”。
///
/// 群消息里打出来的口令同群成员都能看到，`echo_cmd` 的指令回显还会把它再广播一次，
/// 所以这种口令一律作废、退回随机生成。
fn choose_mode(in_group: bool, custom: Option<&str>) -> (PasswordMode, bool) {
    match custom {
        Some(password) if !in_group => (PasswordMode::Custom(password.to_owned()), false),
        Some(_) => (PasswordMode::Generated, true),
        None => (PasswordMode::Generated, false),
    }
}

/// 从 `/jdpm <口令>` 里取出自定义口令；只有 `/jdpm` 时返回 `None`。
fn parse_custom_password(text: &str) -> Option<&str> {
    let arg = text
        .strip_prefix(config::get_command_prefix())
        .and_then(|rest| rest.strip_prefix("jdpm"))
        .unwrap_or("")
        .trim();

    (!arg.is_empty()).then_some(arg)
}

#[cfg(test)]
mod tests {
    use super::{choose_mode, parse_custom_password};
    use crate::web::guard::PasswordMode;

    #[test]
    fn parses_optional_password() {
        let prefix = crate::config::get_command_prefix();
        assert_eq!(parse_custom_password(&format!("{prefix}jdpm")), None);
        assert_eq!(parse_custom_password(&format!("{prefix}jdpm   ")), None);
        assert_eq!(
            parse_custom_password(&format!("{prefix}jdpm  my-secret ")),
            Some("my-secret")
        );
    }

    #[test]
    fn custom_password_only_survives_in_private_chat() {
        assert!(matches!(
            choose_mode(false, Some("my-secret")),
            (PasswordMode::Custom(_), false)
        ));
        // 群里打出来的口令已经泄露，必须作废。
        assert!(matches!(
            choose_mode(true, Some("my-secret")),
            (PasswordMode::Generated, true)
        ));
        assert!(matches!(
            choose_mode(true, None),
            (PasswordMode::Generated, false)
        ));
    }
}
