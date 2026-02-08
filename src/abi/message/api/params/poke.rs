use super::Params;
use crate::abi::message::api::data;
use helper::api;
use serde::{Deserialize, Serialize};

#[api("/group_poke", data::PokeResponse)]
pub struct GroupPoke {
    group_id: i64,
    user_id: i64,
}

impl GroupPoke {
    pub const fn new(group_id: i64, user_id: i64) -> Self {
        Self { group_id, user_id }
    }
}

#[api("/friend_poke", data::PokeResponse)]
pub struct FriendPoke {
    user_id: i64,
    target_id: Option<i64>,
}

impl FriendPoke {
    pub const fn new(user_id: i64) -> Self {
        Self {
            user_id,
            target_id: None,
        }
    }
}
