use crate::abi::utils::SmartJsonExt;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct RecentlyVisitedCourse {
    pub id: i64,
    pub name: String,
    //pub department: IgnoredAny,
    //pub course_attributes: IgnoredAny,
    //pub course_code: IgnoredAny,
    //pub course_type:IgnoredAny,
    //pub cover:IgnoredAny,
    //pub credit_state:IgnoredAny,
    //pub  current_user_is_member:IgnoredAny,
    //pub grade: IgnoredAny,
    //pub klass: IgnoredAny,
    //pub org_id: IgnoredAny,
    //pub teaching_unit_type: IgnoredAny,
    //pub url: IgnoredAny,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct RecentlyVisitedCoursesResponse {
    pub visited_courses: Vec<RecentlyVisitedCourse>,
}

#[lnt_get_api(
    RecentlyVisitedCoursesResponse,
    "https://lnt.xmu.edu.cn/api/user/recently-visited-courses"
)]
pub struct RecentlyVisitedCourses;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-2419114-JaGfIKFdGy9ybEIpdz5ksKDoT042olbnEnXdJVex1BgrqiCpwSX-2JxqT8k6CzU-3jUnull_main";
        let session = castgc_get_session(castgc).await?;
        let data = RecentlyVisitedCourses::get(&session).await?;
        println!("RecentlyVisitedCourses: {:?}", data);
        Ok(())
    }
}
