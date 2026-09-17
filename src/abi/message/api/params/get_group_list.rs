use super::Params;
use crate::abi::message::api::data;
use helper::api;
use serde::{Deserialize, Serialize};

#[api("/get_group_list", data::GetGroupListResponse)]
pub struct GetGroupList {
    no_cache: bool,
}

impl GetGroupList {
    pub const fn new(no_cache: bool) -> Self {
        Self { no_cache }
    }
}
