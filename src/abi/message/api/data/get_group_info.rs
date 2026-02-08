use crate::abi::message::api::{ApiResponse, Data};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct GetGroupInfoData {
    pub group_id: i64,
    pub group_name: String,
    pub member_count: i32,
    pub max_member_count: i32,
}

impl Data for GetGroupInfoData {}

pub type GetGroupInfoResponse = ApiResponse<GetGroupInfoData>;
