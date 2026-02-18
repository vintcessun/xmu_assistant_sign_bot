use crate::abi::utils::SmartJsonExt;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
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
    async fn test() -> Result<()> {
        let castgc = "TGT-2419114-JaGfIKFdGy9ybEIpdz5ksKDoT042olbnEnXdJVex1BgrqiCpwSX-2JxqT8k6CzU-3jUnull_main";
        let session = castgc_get_session(castgc).await?;
        let data = Rollcalls::get(&session).await?;
        println!("Rollcalls: {:?}", data);
        Ok(())
    }
}
