use super::Params;
use crate::abi::message::api::data;
use helper::api;
use serde::{Deserialize, Serialize};

#[api("/get_forward_msg", data::GetForwardMsgResponse)]
pub struct GetForwardMsgParams {
    pub message_id: String,
}

impl GetForwardMsgParams {
    pub const fn new(message_id: String) -> Self {
        Self { message_id }
    }
}
