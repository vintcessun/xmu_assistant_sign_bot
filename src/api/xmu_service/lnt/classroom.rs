use crate::abi::utils::SmartJsonExt;
use anyhow::Ok;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Classroom {
    pub finish_at: String,
    pub id: i64,
    pub start_at: String,
    pub status: String,
    pub title: String,
    //pub announce_answer_status:String,
    //pub created_at:String,
    //pub data:HashMap<String,serde_json::Value>,
    //pub enable_edit:bool,
    //pub exam_paper_template_id:i64,
    //pub finished_subjects_count: i64,
    //pub imported_from: Option<String>,
    //pub is_answer_announced: bool,
    //pub is_in_progress: bool,
    //pub is_opened_catalog: bool,
    //pub is_quiz_control_by_subject: bool,
    //pub is_quiz_public: bool,
    //pub is_score_public: bool,
    //pub module_id: i64,
    //pub module_sort: i64,
    //pub published: bool,
    //pub score_item_group_id: i64,
    //pub score_item_group_name: Option<String>,
    //pub score_item_scored: bool,
    //pub score_percentage: String,
    //pub score_type: String,
    //pub sort: i64,
    //pub started_subjects_count: i64,
    //pub subjects_count: i64,
    //pub syllabus_id: i64,
    //pub syllabus_sort: Option<i64>,
    //pub teaching_model: String,
    //pub r#type: String,
    //pub unique_key: String,
    //pub updated_at: String,
    //pub updated_status_at: String,
    //pub using_phase: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ClassroomListResponse {
    pub classrooms: Vec<Classroom>,
}

#[lnt_get_api(
    ClassroomListResponse,
    "https://lnt.xmu.edu.cn/api/courses/{id:i64}/classroom-list"
)]
pub struct ClassroomList;

#[lnt_get_api(
    Classroom,
    "https://lnt.xmu.edu.cn/api/classroom-exams/{classroom_id:i64}"
)]
pub struct ClassroomExams;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test(flavor = "multi_thread")]
    async fn test() -> Result<()> {
        let castgc = "TGT-4017429-6KAhATeeVXolstMjtOxHIv1EHDxnJejNaDlXvFiIYazONlAgn0ijGNwjysYzgJCi8iQnull_main";
        let session = castgc_get_session(castgc).await?;
        let data = ClassroomList::get(&session, 53785).await?;
        println!("ClassroomList: {:?}", data);
        let data = ClassroomExams::get(&session, 2776).await?;
        println!("ClassroomExams: {:?}", data);
        Ok(())
    }
}
