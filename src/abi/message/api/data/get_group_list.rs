use crate::abi::message::api::{ApiResponse, Data};
use serde::{Deserialize, Serialize};

/// `/get_group_list` 的单项。除群号以外一律可选：NapCat 各版本给的字段不完全一样，
/// 少一个字段就让整条响应解析失败不值得——广播只需要群号。
#[derive(Serialize, Deserialize, Debug)]
pub struct GroupListItem {
    pub group_id: i64,
    pub group_name: Option<String>,
    pub member_count: Option<i32>,
    pub max_member_count: Option<i32>,
}

impl Data for Vec<GroupListItem> {}

pub type GetGroupListResponse = ApiResponse<Vec<GroupListItem>>;
