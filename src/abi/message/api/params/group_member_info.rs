use super::Params;
use crate::abi::message::api::data;
use helper::api;
use serde::{Deserialize, Serialize};

#[api("/get_group_member_info", data::GroupMemberInfoResponse)]
pub struct GroupMemberInfo {
    group_id: i64,
    user_id: i64,
    no_cache: bool,
}

impl GroupMemberInfo {
    pub const fn new(group_id: i64, user_id: i64, no_cache: bool) -> Self {
        Self {
            group_id,
            user_id,
            no_cache,
        }
    }
}
