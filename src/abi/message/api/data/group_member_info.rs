use crate::abi::message::{
    api::{ApiResponse, Data},
    sender::{Role, Sex},
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct GroupMemberInfoData {
    pub group_id: i64,
    pub user_id: i64,
    pub nickname: String,
    pub card: String,
    pub sex: Sex,
    pub age: i32,
    pub area: String,
    pub join_time: i32,
    pub last_sent_time: i32,
    pub level: String,
    pub role: Role,
    pub unfriendly: bool,
    pub title: String,
    pub title_expire_time: i32,
    pub card_changeable: bool,
}

impl Data for GroupMemberInfoData {}

pub type GroupMemberInfoResponse = ApiResponse<GroupMemberInfoData>;
