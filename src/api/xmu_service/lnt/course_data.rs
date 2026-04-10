use crate::{abi::utils::SmartJsonExt, api::network::SessionClient};
use ahash::RandomState;
use anyhow::Result;
use dashmap::DashMap;
use helper::session_client_helper;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};

#[derive(Serialize, Deserialize, Debug)]
pub struct Instructor {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CourseDataResponse {
    pub course_code: String,
    pub name: String,
    pub instructors: Vec<Instructor>,
    //pub allow_admin_update_basic_info: Option<Value>,
    //pub allow_update_basic_info: Option<Value>,
    //pub allowed_to_invite_assistant: Option<Value>,
    //pub allowed_to_invite_student: Option<Value>,
    //pub allowed_to_join_course: Option<Value>,
    //pub archived: Option<Value>,
    //pub auto_archive_course_date: Option<Value>,
    //pub credit_state: Option<Value>,
    //pub has_ai_ability: Option<Value>,
    //pub knowledge_graph_publish_type: Option<Value>,
    //pub problem_graph_publish_type: Option<Value>,
    //pub show_archive_course_tips: Option<Value>,
}

static COURSE_DATA: LazyLock<CourseDataStruct> = LazyLock::new(CourseDataStruct::new);

pub struct CourseDataStruct {
    pub profile_data: DashMap<i64, Arc<CourseDataResponse>, RandomState>,
}

impl Default for CourseDataStruct {
    fn default() -> Self {
        Self::new()
    }
}

impl CourseDataStruct {
    pub fn new() -> Self {
        CourseDataStruct {
            profile_data: DashMap::with_hasher(RandomState::default()),
        }
    }

    pub async fn get_from_client(
        &self,
        client: &SessionClient,
        course_id: i64,
    ) -> Result<Arc<CourseDataResponse>> {
        if let Some(entry) = self.profile_data.get(&course_id) {
            return Ok((*entry.value()).clone());
        }

        let res = client
            .get(format!(
                "https://lnt.xmu.edu.cn/api/courses/{course_id}?fields=name,course_code,instructors"
            ))
            .await?;
        let course_data = res.json_smart::<CourseDataResponse>().await?;
        let course_data = Arc::new(course_data);

        self.profile_data.insert(course_id, course_data.clone());

        Ok(course_data)
    }
}

pub struct CourseData;

impl CourseData {
    #[session_client_helper]
    pub async fn get_from_client(
        client: &SessionClient,
        course_id: i64,
    ) -> Result<Arc<CourseDataResponse>> {
        COURSE_DATA.get_from_client(client, course_id).await
    }
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_error() -> Result<()> {
        let castgc = "TGT-2435869-O8Wwbqik8mV2AiaFWm2RKkKG8nq1zARLvjuN2XWuYtBMaXNrSUaZDng4bJZj-3FfQrsnull_main";
        let session = castgc_get_session(castgc).await?;
        let course_data = CourseData::get(&session, 71211).await?;
        println!("CourseData: {:?}", course_data);
        Ok(())
    }

    #[tokio::test]
    async fn test_success() -> Result<()> {
        let castgc = "TGT-4073508-WHsRVSCV2-j9q5z3D2VXbcR8-ZFkHzsltAKa7aioXRvKY8fRACTJatRxjSdJtdbsRiInull_main";
        let session = castgc_get_session(castgc).await?;
        let course_data = CourseData::get(&session, 71211).await?;
        println!("CourseData: {:?}", course_data);
        Ok(())
    }
}
