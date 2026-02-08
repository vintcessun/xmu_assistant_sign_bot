use crate::abi::message::{
    MessageReceive,
    api::{ApiResponse, Data},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct SendMsgData {
    pub message_id: i64,
}

impl Data for SendMsgData {}

pub type SendMsgResponse = ApiResponse<SendMsgData>;

#[derive(Serialize, Deserialize, Debug)]
pub struct ForwardMsgData {
    pub message_id: i64,
    pub res_id: Option<String>,
}

impl Data for ForwardMsgData {}

pub type ForwardMsgResponse = ApiResponse<ForwardMsgData>;

#[derive(Serialize, Deserialize, Debug)]
pub struct PrivateForwardMsgData {
    #[serde(alias = "message")]
    pub messages: MessageReceive,
}

impl Data for PrivateForwardMsgData {}

pub type PrivateForwardMsgResponse = ApiResponse<PrivateForwardMsgData>;
