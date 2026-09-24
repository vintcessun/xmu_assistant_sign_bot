//! 用户状态卡：把散落在各处的登录/课表/口令状态聚合成一条消息。
//!
//! 两条设计约束：
//! 1. 所有 IO 在 `StatusCtx::build` 里预取一次，于是每个状态项都是纯同步函数，
//!    加一项就是往 `FIELDS` 里加一行，不会有 async 样板；
//! 2. 验活必须用 `ProfileWithoutCache`——`Profile::get` 的缓存按 session 永不失效，
//!    用它判断的话"登录已失效"永远显示不出来。

use super::BuildHelp;
use crate::abi::{logic_import::*, message::from_str};
use crate::api::xmu_service::lnt::ProfileWithoutCache;
use crate::api::xmu_service::lnt::profile::ProfileResponse;
use crate::api::xmu_service::login::LoginData;
use crate::logic::login::{LOGIN_DATA, PWD_DATA};
use crate::logic::rollcall::{is_sign_time_active_now, query_sign_group, query_sign_time};
use crate::web::guard::has_secret;
use anyhow::anyhow;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::debug;

pub struct StatusCtx {
    pub qq: i64,
    /// 群里渲染时置 true：学号这类信息不往群里放。
    pub in_group: bool,
    pub login: Option<Arc<LoginData>>,
    /// 取到说明会话还活着，取不到而 `login` 存在就是登录失效。
    pub profile: Option<ProfileResponse>,
}

impl StatusCtx {
    pub async fn build(qq: i64, in_group: bool) -> Self {
        let login = LOGIN_DATA.get(&qq);
        let profile = match &login {
            // 只读查询，绝不走 get_client_or_err_for_id——那个会顺手触发密码重登。
            Some(login) => ProfileWithoutCache::get(&login.lnt).await.ok(),
            None => None,
        };
        debug!(
            qq = qq,
            in_group = in_group,
            has_login = login.is_some(),
            alive = profile.is_some(),
            "状态卡数据预取完成"
        );
        Self {
            qq,
            in_group,
            login,
            profile,
        }
    }
}

/// 一个状态项：算不出或不该显示就返回 None，那一行整行省略。
type Field = (&'static str, fn(&StatusCtx) -> Option<String>);

/// ★ 唯一要维护的地方：加一项状态 = 加一行 ★
static FIELDS: &[Field] = &[
    ("登录状态", |c| {
        Some(
            match (c.profile.is_some(), c.login.is_some()) {
                (true, _) => "已登录",
                (false, true) => "登录已失效，请重新登录",
                (false, false) => "未登录",
            }
            .to_string(),
        )
    }),
    ("姓名", |c| c.profile.as_ref().map(|p| p.name.clone())),
    ("学院", |c| {
        c.profile.as_ref().map(|p| p.department.name.clone())
    }),
    // 学号只在私聊给，群里整行不出现。
    ("学号", |c| {
        if c.in_group {
            None
        } else {
            c.profile.as_ref().map(|p| p.user_no.clone())
        }
    }),
    ("登录方式", |c| {
        c.login.as_ref().map(|_| {
            if PWD_DATA.get(&c.qq).is_some() {
                "账号密码（失效后可自动恢复）".to_string()
            } else {
                "扫码（失效后需要重新扫码）".to_string()
            }
        })
    }),
    ("课表", |c| {
        Some(match query_sign_time(c.qq) {
            Some(table) => {
                let courses: HashSet<&str> = table.times.iter().map(|t| t.name.as_str()).collect();
                format!(
                    "已保存 {} 门课 / {} 个时段",
                    courses.len(),
                    table.times.len()
                )
            }
            None => "未保存，发 /signtime 录入".to_string(),
        })
    }),
    ("当前时段", |c| {
        if query_sign_time(c.qq).is_some() {
            Some(
                if is_sign_time_active_now(c.qq) {
                    "正处于课程签到时段"
                } else {
                    "当前没有课"
                }
                .to_string(),
            )
        } else {
            None
        }
    }),
    ("定时签到推送群", |c| {
        query_sign_group(c.qq).map(|group| group.to_string())
    }),
    ("访问口令", |c| {
        Some(
            if has_secret(c.qq) {
                "已设置"
            } else {
                "未设置"
            }
            .to_string(),
        )
    }),
];

/// 渲染状态卡。`in_group` 决定敏感项是否出现。
pub async fn render_card(qq: i64, in_group: bool) -> String {
    let ctx = StatusCtx::build(qq, in_group).await;
    let mut out = format!("QQ {} 的状态\n", qq);
    for (label, field) in FIELDS {
        if let Some(value) = field(&ctx) {
            out.push_str(label);
            out.push_str(": ");
            out.push_str(&value);
            out.push('\n');
        }
    }
    out.pop();
    out
}

#[handler(msg_type=Message,command="status",echo_cmd=true,
help_msg=r#"用法:/status
功能:查看自己的登录、课表、口令等状态（群里发会隐藏学号，私聊发显示全部）"#)]
pub async fn status(ctx: Context) -> Result<()> {
    let sender = ctx.message.get_sender();
    let id = sender.user_id.ok_or(anyhow!("获取用户ID失败"))?;
    let in_group = matches!(ctx.target, Target::Group(_));

    let card = render_card(id, in_group).await;
    ctx.send_message_async(from_str(card));

    Ok(())
}
