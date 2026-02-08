use super::Params;
use crate::abi::message::api::data;
use helper::api;
use serde::{Deserialize, Serialize};

#[api("/get_group_info", data::GetGroupInfoResponse)]
pub struct GetGroupInfo {
    group_id: i64,
    no_cache: bool,
}

impl GetGroupInfo {
    pub const fn new(group_id: i64, no_cache: bool) -> Self {
        Self { group_id, no_cache }
    }
}
