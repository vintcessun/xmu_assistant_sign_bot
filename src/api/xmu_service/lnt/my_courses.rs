use crate::abi::utils::SmartJsonExt;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Course {
    pub id: i64,
    pub name: String,
    //pub academic_year: IgnoredAny,
    //pub compulsory: IgnoredAny,
    //pub course_attributes: IgnoredAny,
    //pub course_code: IgnoredAny,
    //pub course_type: IgnoredAny,
    //pub credit: IgnoredAny,
    //pub department: IgnoredAny,
    //pub end_date: IgnoredAny,
    //pub grade: IgnoredAny,
    //pub instructors: IgnoredAny,
    //pub is_mute: IgnoredAny,
    //pub klass: IgnoredAny,
    //pub org: IgnoredAny,
    //pub org_id: IgnoredAny,
    //pub semester: IgnoredAny,
    //pub start_date: IgnoredAny,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MyCourseResponse {
    pub courses: Vec<Course>,
}

#[lnt_get_api(MyCourseResponse, "https://lnt.xmu.edu.cn/api/my-courses")]
pub struct MyCourses;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-3852721-5F6eRNQT3hKL70kX3mDbLKQOpeUcKCYbCwJUZNW-btgCA45jHAWRs6iRLEeNzYP3-1cnull_main";
        let session = castgc_get_session(castgc).await?;
        let data = MyCourses::get(&session).await?;
        println!("MyCourses: {:?}", data);
        println!("JSON: {}", serde_json::to_string(&data)?);
        Ok(())
    }
}
