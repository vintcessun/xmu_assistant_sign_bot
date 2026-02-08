use crate::abi::utils::SmartJsonExt;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Exam {
    pub title: String,
    pub id: i64,
    pub is_started: bool,
    pub start_time: String,
    pub end_time: String,
    //pub announce_answer_status:IgnoredAny,
    //pub announce_answer_time: IgnoredAny,
    //pub announce_answer_type: IgnoredAny,
    //pub announce_score_status: IgnoredAny,
    //pub announce_score_time: IgnoredAny,
    //pub assign_group_ids: IgnoredAny,
    //pub assign_student_ids: IgnoredAny,
    //pub check_submit_ip_consistency: IgnoredAny,
    //pub completion_criterion: IgnoredAny,
    //pub completion_criterion_key: IgnoredAny,
    //pub completion_criterion_value: IgnoredAny,
    //pub default_options_layout: IgnoredAny,
    //pub description: IgnoredAny,
    //pub disable_copy_paste: IgnoredAny,
    //pub disable_devtool: IgnoredAny,
    //pub disable_right_click: IgnoredAny,
    //pub enable_anti_cheat: IgnoredAny,
    //pub enable_edit: IgnoredAny,
    //pub enable_invigilation: IgnoredAny,
    //pub exam_submissions: IgnoredAny,
    //pub group_set_id: IgnoredAny,
    //pub group_set_name: IgnoredAny,
    //pub has_assign_group: IgnoredAny,
    //pub has_assign_student: IgnoredAny,
    //pub imported_from: IgnoredAny,
    //pub is_announce_answer_time_passed: IgnoredAny,
    //pub is_announce_score_time_passed: IgnoredAny,
    //pub is_assigned_to_all: IgnoredAny,
    //pub is_closed: IgnoredAny,
    //pub is_fullscreen_mode: IgnoredAny,
    //pub is_in_progress: IgnoredAny,
    //pub is_ip_constrained: IgnoredAny,
    //pub is_leaving_window_constrained: IgnoredAny,
    //pub is_leaving_window_timeout: IgnoredAny,
    //pub is_opened_catalog: IgnoredAny,
    //pub is_practice_mode: IgnoredAny,
    //pub knowledge_node_ids: IgnoredAny,
    //pub knowledge_node_reference: IgnoredAny,
    //pub leaving_window_limit: IgnoredAny,
    //pub leaving_window_timeout: IgnoredAny,
    //pub limit_answer_on_signle_client: IgnoredAny,
    //pub limit_time: IgnoredAny,
    //pub limited_ip: IgnoredAny,
    //pub make_up_record: IgnoredAny,
    //pub module_id: IgnoredAny,
    //pub module_sort: IgnoredAny,
    //pub prerequisites: IgnoredAny,
    //pub publish_time: IgnoredAny,
    //pub published: IgnoredAny,
    //pub referrer_id: IgnoredAny,
    //pub referrer_type: IgnoredAny,
    //pub score_percentage: IgnoredAny,
    //pub score_rule: IgnoredAny,
    //pub score_type: IgnoredAny,
    //pub sort: IgnoredAny,
    //pub subjects_rule: IgnoredAny,
    //pub submit_by_group: IgnoredAny,
    //pub submit_times: IgnoredAny,
    //pub syllabus_id: IgnoredAny,
    //pub syllabus_sort: IgnoredAny,
    //pub teaching_model: IgnoredAny,
    //pub r#type: IgnoredAny,
    //pub unique_key: IgnoredAny,
    //pub using_phase: IgnoredAny,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExamResponse {
    pub exams: Vec<Exam>,
}

#[lnt_get_api(
    ExamResponse,
    "https://lnt.xmu.edu.cn/api/courses/{course_id:i64}/exams"
)]
pub struct Exams;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-2573461-NzJdqBAiUk7XIiX4bM-zAKMKJ-BVKIworT50c1XW-Ot904sgTtAJAF3trrQr56QGraInull_main";
        let session = castgc_get_session(castgc).await?;
        let data = Exams::get(&session, 78180).await?;
        println!("Exams: {:?}", data);
        Ok(())
    }
}
