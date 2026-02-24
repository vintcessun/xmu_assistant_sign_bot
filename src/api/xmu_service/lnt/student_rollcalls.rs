use crate::abi::utils::SmartJsonExt;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    OnCall,
    Absent,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StudentDetail {
    pub status: Status,
    //pub comment: Option<Value>,
    //pub department: Option<Value>,
    //pub distance: Option<Value>,
    //pub grade: Option<Value>,
    //pub klass: Option<Value>,
    //pub name: Option<Value>,
    //pub nickname: Option<Value>,
    //pub rollcall_status: Option<Value>,
    //pub status_detail: Option<Value>,
    //pub student_id: Option<Value>,
    //pub updated_at: Option<Value>,
    //pub user_no: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StudentRollcallsResponse {
    pub number_code: Option<String>,
    pub student_rollcalls: Vec<StudentDetail>,
    //pub comment: Option<Value>,
    //pub end_time: Option<Value>,
    //pub external_api_key_id: Option<Value>,
    //pub is_number: Option<Value>,
    //pub is_radar: Option<Value>,
    //pub published_at: Option<Value>,
    //pub scored: Option<Value>,
    //pub section: Option<Value>,
    //pub status: Option<Value>,
    //pub title: Option<Value>,
    //pub r#type: Option<Value>,
}

#[lnt_get_api(
    StudentRollcallsResponse,
    "https://lnt.xmu.edu.cn/api/rollcall/{rollcall_id:i64}/student_rollcalls"
)]
pub struct StudentRollcalls;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-2419114-JaGfIKFdGy9ybEIpdz5ksKDoT042olbnEnXdJVex1BgrqiCpwSX-2JxqT8k6CzU-3jUnull_main";
        let session = castgc_get_session(castgc).await?;
        let data = StudentRollcalls::get(&session, 114054).await?;
        println!("StudentRollcalls: {:?}", data);
        Ok(())
    }
}
