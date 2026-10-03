use crate::abi::message::api::{ApiResponse, Data};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct EmojiLikeData {}

impl Data for EmojiLikeData {}

pub type EmojiLikeResponse = ApiResponse<EmojiLikeData>;
