use super::Params;
use crate::abi::message::api::data;
use helper::api;
use serde::{Deserialize, Serialize};

#[api("/set_group_special_title", data::SpecialTitleResponse)]
pub struct SpecialTitle {
    group_id: i64,
    user_id: i64,
    special_title: String,
}

impl SpecialTitle {
    pub fn new(group_id: i64, user_id: i64, special_title: String) -> Self {
        Self {
            group_id,
            user_id,
            special_title,
        }
    }
}
