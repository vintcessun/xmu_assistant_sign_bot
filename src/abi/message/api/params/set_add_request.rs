use super::Params;
use crate::abi::message::api::data;
use helper::api;
use serde::{Deserialize, Serialize};

#[api("/set_friend_add_request", data::SetAddRequestResponse)]
pub struct SetFriendAddRequest {
    flag: String,
    approve: bool,
}

impl SetFriendAddRequest {
    pub const fn approve(flag: String) -> Self {
        Self {
            flag,
            approve: true,
        }
    }
}

#[api("/set_group_add_request", data::SetAddRequestResponse)]
pub struct SetGroupAddRequest {
    flag: String,
    /// 必须回传事件里的 sub_type（`add` / `invite`），NapCat 靠它区分请求种类。
    sub_type: String,
    approve: bool,
}

impl SetGroupAddRequest {
    pub const fn approve(flag: String, sub_type: String) -> Self {
        Self {
            flag,
            sub_type,
            approve: true,
        }
    }
}
