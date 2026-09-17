//! 自动同意加好友和拉群邀请。
//!
//! 两个注意点：
//! 1. 别在这里发欢迎语——此刻好友关系/入群都还没生效，发送目标根本不可达；
//!    真要欢迎语得挂 `Notice::FriendAdd`（那才是关系已生效的时点）；
//! 2. API 失败就地记日志，不要用 `?` 往上抛。抛出去会让框架把错误消息发回
//!    这个不可达的目标，白白多一条失败日志。

use crate::abi::echo::Echo;
use crate::abi::logic_import::*;
use crate::abi::message::api::{Params, SetFriendAddRequest, SetGroupAddRequest};
use crate::abi::message::event_request::{Request as RequestEvent, SubType};
use tracing::{error, info, warn};

#[handler(msg_type=Request)]
pub async fn friend_add(ctx: Context) -> Result<()> {
    match ctx.get_message().as_ref() {
        RequestEvent::Friend(req) => {
            info!(user_id = req.user_id, comment = %req.comment, "自动同意好友请求");
            approve(
                &ctx,
                SetFriendAddRequest::approve(req.flag.clone()),
                "好友请求",
            )
            .await;
        }
        RequestEvent::Group(req) => match req.sub_type {
            // 别人把机器人拉进群：同意。
            SubType::Invite => {
                info!(
                    group_id = req.group_id,
                    user_id = req.user_id,
                    "自动同意入群邀请"
                );
                let params = SetGroupAddRequest::approve(req.flag.clone(), "invite".to_string());
                approve(&ctx, params, "入群邀请").await;
            }
            // 别人申请加入机器人当管理的群：不是机器人该替群主决定的事，放着不动。
            SubType::Add => {
                warn!(
                    group_id = req.group_id,
                    user_id = req.user_id,
                    "收到入群申请，交给群管理员处理，不自动同意"
                );
            }
        },
    }

    Ok(())
}

async fn approve<T, P>(ctx: &Context<T, RequestEvent>, params: P, what: &str)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
    P: Params + std::fmt::Debug,
    P::Response: std::fmt::Debug,
{
    match ctx.client.call_api(&params, Echo::new()).await {
        Ok(pending) => match pending.wait_echo().await {
            Ok(res) => info!(kind = what, response = ?res, "请求已处理"),
            Err(e) => error!(kind = what, error = ?e, "等待处理结果失败"),
        },
        Err(e) => error!(kind = what, error = ?e, "发起处理请求失败"),
    }
}
