mod get_forward_msg;
mod get_group_info;
mod group_member_info;
mod poke;
mod send_msg;
mod title;

pub use get_forward_msg::*;
pub use get_group_info::*;
pub use group_member_info::*;
pub use poke::*;
pub use send_msg::*;
pub use title::*;

use crate::abi::message::api::data;
use serde::{Deserialize, Serialize};

pub trait Params: Send + Sync + 'static + Serialize {
    type Response: data::ApiResponseTrait + for<'de> Deserialize<'de>;

    const ACTION: &'static str;
}
