use crate::abi::message::api::{ApiResponse, Data};
use serde::{Deserialize, Serialize};

/// 同意好友/加群请求的接口只回状态码，没有 data。
#[derive(Serialize, Deserialize, Debug)]
pub struct SetAddRequestData {}

impl Data for SetAddRequestData {}

pub type SetAddRequestResponse = ApiResponse<SetAddRequestData>;
