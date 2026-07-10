use std::fmt;

use crate::abi::utils::SmartJsonExt;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Rollcall {
    pub course_id: i64,
    pub course_title: String,
    pub is_number: bool,
    pub is_radar: bool,
    pub rollcall_id: i64,
    pub status: RollcallStatus,
    //pub avatar_big_url: Option<Value>,
    //pub class_name: Option<Value>,
    //pub created_by: Option<Value>,
    //pub created_by_name: Option<Value>,
    //pub department_name: Option<Value>,
    //pub grade_name: Option<Value>,
    //pub group_set_id: Option<Value>,
    //pub is_expired: Option<Value>,
    //pub published_at: Option<Value>,
    //pub rollcall_status: Option<Value>,
    //pub rollcall_time: Option<Value>,
    //pub scored: Option<Value>,
    //pub source: Option<Value>,
    //pub student_rollcall_id: Option<Value>,
    //pub title: Option<Value>,
    //pub r#type: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum RollcallStatus {
    OnCallFine,
    Absent,
}

impl fmt::Display for RollcallStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RollcallStatus::OnCallFine => write!(f, "已签到"),
            RollcallStatus::Absent => write!(f, "缺勤"),
        }
    }
}

pub trait DisplayExt {
    fn display(&self) -> &'static str;
}

impl DisplayExt for Option<RollcallStatus> {
    fn display(&self) -> &'static str {
        match self {
            Some(RollcallStatus::OnCallFine) => "已签到",
            Some(RollcallStatus::Absent) => "缺勤",
            None => "获取失败",
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct RollcallsResponse {
    pub rollcalls: Vec<Rollcall>,
}

#[lnt_get_api(
    RollcallsResponse,
    "https://lnt.xmu.edu.cn/api/radar/rollcalls?api_version=1.1.0"
)]
pub struct Rollcalls;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    #[ignore = "需要有效 CASTGC(TGT)，网络+凭证依赖，手动运行: cargo test -- --ignored"]
    async fn test() -> Result<()> {
        let castgc = "TGT-2419114-JaGfIKFdGy9ybEIpdz5ksKDoT042olbnEnXdJVex1BgrqiCpwSX-2JxqT8k6CzU-3jUnull_main";
        let session = castgc_get_session(castgc).await?;
        let data = Rollcalls::get(&session).await?;
        println!("Rollcalls: {:?}", data);
        Ok(())
    }

    #[tokio::test]
    async fn test_result_2026_7_10() -> Result<()> {
        let data = r#"{"rollcalls":[{"avatar_big_url":"","class_name":"","course_id":102245,"course_title":"\u667a\u80fd\u4fe1\u606f\u68c0\u7d22","created_by":62032,"created_by_name":"\u6797\u8fbe\u771f","department_name":"\u4fe1\u606f\u5b66\u9662","grade_name":"","group_set_id":0,"is_expired":false,"is_number":false,"is_radar":true,"published_at":null,"rollcall_id":407067,"rollcall_status":"in_progress","rollcall_time":"2026-07-10T06:25:12Z","scored":true,"source":"radar","status":"on_call_fine","student_rollcall_id":0,"title":"2026.07.10 14:25","type":"another"}]}"#;
        let data: RollcallsResponse = serde_json::from_str(data)?;
        println!("Rollcalls: {:?}", data);
        Ok(())
    }
}
