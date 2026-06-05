use std::sync::LazyLock;

use crate::abi::utils::SmartJsonExt;
use chrono::{DateTime, FixedOffset, Utc};
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Serialize, Deserialize, Debug)]
pub struct Exam {
    pub title: String,
    pub id: i64,
    pub is_started: bool,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
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

static BEIJING_OFFSET: LazyLock<FixedOffset> = LazyLock::new(|| {
    FixedOffset::east_opt(8 * 3600).unwrap_or_else(|| {
        error!("无法创建北京时间偏移");
        panic!("无法创建北京时间偏移")
    })
});

impl Exams {
    pub fn to_beijing_date(time: &DateTime<Utc>) -> String {
        time.with_timezone(&*BEIJING_OFFSET)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-4217253-xbc8sI9hkW3Zy7mhq0FpB8NVfFIjmHobl3I7AUfadKSYerFmOpRsPpwAdjSVuI1V--0null_main";
        let session = castgc_get_session(castgc).await?;
        let data = Exams::get(&session, 78180).await?;
        println!("Exams: {:?}", data);
        Ok(())
    }

    #[tokio::test]
    async fn test_error_data_2026_6_5() -> Result<()> {
        let data = r#"{"exams":[{"announce_answer_status":"immediate_announce","announce_answer_time":null,"announce_answer_type":"answer_and_explanation","announce_score_status":"immediate_announce","announce_score_time":null,"assign_group_ids":[],"assign_student_ids":[],"check_submit_ip_consistency":false,"completion_criterion":"\u63d0\u4ea4\u6d4b\u8bd5","completion_criterion_key":"submitted","completion_criterion_value":"0","created_at":"2026-04-16T11:42:21Z","data":{"announce_answer_status":1,"announce_answer_time":null,"announce_answer_type":1,"announce_score_status":1,"announce_score_time":null,"check_submit_ip_consistency":false,"default_options_layout":2,"description":"","disable_copy_paste":false,"disable_devtool":true,"disable_right_click":false,"enable_anti_cheat":false,"enable_invigilation":false,"eztest":{},"is_fullscreen_mode":false,"is_leaving_window_constrained":false,"is_leaving_window_timeout":false,"leaving_window_limit":null,"leaving_window_timeout":null,"limit_answer_on_signle_client":false,"platform":"tronclass","publish_count":1,"publish_time":"2026-04-16T11:43:00Z","subject_index_type":"group"},"default_options_layout":"vertical","description":"","disable_copy_paste":false,"disable_devtool":true,"disable_right_click":false,"enable_anti_cheat":false,"enable_edit":true,"enable_exam_prerequisite":false,"enable_invigilation":false,"end_time":"2026-04-17T03:00:00Z","exam_prerequisite":null,"exam_submissions":[28377],"group_set_id":0,"group_set_name":"","has_assign_group":false,"has_assign_student":false,"id":28377,"imported_from":null,"is_announce_answer_time_passed":true,"is_announce_score_time_passed":true,"is_assigned_to_all":true,"is_closed":true,"is_fullscreen_mode":false,"is_in_progress":false,"is_ip_constrained":false,"is_leaving_window_constrained":false,"is_leaving_window_timeout":false,"is_opened_catalog":false,"is_practice_mode":false,"is_started":true,"knowledge_node_ids":[],"knowledge_node_reference":[],"leaving_window_limit":null,"leaving_window_timeout":null,"limit_answer_on_signle_client":false,"limit_short_answer_upload":false,"limit_time":15,"limited_ip":"","make_up_record":null,"module_id":196992,"module_sort":3,"prerequisites":[],"publish_time":"2026-04-16T11:43:00Z","published":true,"referrer_id":196992,"referrer_type":"module","score_percentage":"0.00","score_rule":"highest","score_type":"percentage","sort":1,"start_time":"2026-04-17T02:40:00Z","subjects_rule":{"select_subjects_randomly":false,"shuffle_options_randomly":false,"shuffle_subjects_randomly":"default","sub_subjects_randomly":false},"submit_by_group":false,"submit_times":1,"syllabus_id":0,"syllabus_sort":"1-01-01T00:00:00Z","teaching_model":"online","title":"\u7b2c\u4e00\u6b21\u5c0f\u6d4b","type":"exam","unique_key":"exam-28377","using_phase":"during_class"},{"announce_answer_status":"immediate_announce","announce_answer_time":null,"announce_answer_type":"answer_and_explanation","announce_score_status":"immediate_announce","announce_score_time":null,"assign_group_ids":[],"assign_student_ids":[],"check_submit_ip_consistency":false,"completion_criterion":"\u63d0\u4ea4\u6d4b\u8bd5","completion_criterion_key":"submitted","completion_criterion_value":"0","created_at":"2026-05-07T03:18:07Z","data":{"announce_answer_status":1,"announce_answer_time":null,"announce_answer_type":1,"announce_score_status":1,"announce_score_time":null,"check_submit_ip_consistency":false,"default_options_layout":2,"description":"","disable_copy_paste":false,"disable_devtool":true,"disable_right_click":false,"enable_anti_cheat":false,"enable_invigilation":false,"eztest":{},"is_fullscreen_mode":false,"is_leaving_window_constrained":false,"is_leaving_window_timeout":false,"leaving_window_limit":null,"leaving_window_timeout":null,"limit_answer_on_signle_client":false,"limit_short_answer_upload":false,"platform":"tronclass","publish_count":1,"publish_time":"2026-05-07T03:18:00Z","subject_index_type":"group"},"default_options_layout":"vertical","description":"","disable_copy_paste":false,"disable_devtool":true,"disable_right_click":false,"enable_anti_cheat":false,"enable_edit":true,"enable_exam_prerequisite":false,"enable_invigilation":false,"end_time":"2026-05-08T03:25:00Z","exam_prerequisite":null,"exam_submissions":[30102],"group_set_id":0,"group_set_name":"","has_assign_group":false,"has_assign_student":false,"id":30102,"imported_from":null,"is_announce_answer_time_passed":true,"is_announce_score_time_passed":true,"is_assigned_to_all":true,"is_closed":true,"is_fullscreen_mode":false,"is_in_progress":false,"is_ip_constrained":false,"is_leaving_window_constrained":false,"is_leaving_window_timeout":false,"is_opened_catalog":false,"is_practice_mode":false,"is_started":true,"knowledge_node_ids":[],"knowledge_node_reference":[],"leaving_window_limit":null,"leaving_window_timeout":null,"limit_answer_on_signle_client":false,"limit_short_answer_upload":false,"limit_time":15,"limited_ip":"","make_up_record":null,"module_id":196992,"module_sort":3,"prerequisites":[],"publish_time":"2026-05-07T03:18:00Z","published":true,"referrer_id":196992,"referrer_type":"module","score_percentage":"0.00","score_rule":"highest","score_type":"percentage","sort":2,"start_time":"2026-05-08T03:05:00Z","subjects_rule":{"select_subjects_randomly":false,"shuffle_options_randomly":false,"shuffle_subjects_randomly":"default","sub_subjects_randomly":false},"submit_by_group":false,"submit_times":1,"syllabus_id":0,"syllabus_sort":"1-01-01T00:00:00Z","teaching_model":"online","title":"\u7b2c\u4e8c\u6b21\u5c0f\u6d4b","type":"exam","unique_key":"exam-30102","using_phase":"during_class"},{"announce_answer_status":"immediate_announce","announce_answer_time":null,"announce_answer_type":"answer_and_explanation","announce_score_status":"immediate_announce","announce_score_time":null,"assign_group_ids":[],"assign_student_ids":[],"check_submit_ip_consistency":false,"completion_criterion":"\u63d0\u4ea4\u6d4b\u8bd5","completion_criterion_key":"submitted","completion_criterion_value":"0","created_at":"2026-05-23T06:35:49Z","data":{"announce_answer_status":1,"announce_answer_time":null,"announce_answer_type":1,"announce_score_status":1,"announce_score_time":null,"auto_ai_grading_operator_id":62704,"check_submit_ip_consistency":false,"default_options_layout":2,"description":"","disable_copy_paste":false,"disable_devtool":true,"disable_right_click":false,"enable_anti_cheat":false,"enable_auto_ai_grading":false,"enable_invigilation":false,"eztest":{},"is_fullscreen_mode":false,"is_leaving_window_constrained":false,"is_leaving_window_timeout":false,"leaving_window_limit":null,"leaving_window_timeout":null,"limit_answer_on_signle_client":false,"limit_short_answer_upload":false,"platform":"tronclass","publish_time":"2026-05-23T06:58:00Z","subject_index_type":"group"},"default_options_layout":"vertical","description":"","disable_copy_paste":false,"disable_devtool":true,"disable_right_click":false,"enable_anti_cheat":false,"enable_edit":true,"enable_exam_prerequisite":false,"enable_invigilation":false,"end_time":null,"exam_prerequisite":null,"exam_submissions":[],"group_set_id":0,"group_set_name":"","has_assign_group":false,"has_assign_student":false,"id":31608,"imported_from":null,"is_announce_answer_time_passed":true,"is_announce_score_time_passed":true,"is_assigned_to_all":true,"is_closed":false,"is_fullscreen_mode":false,"is_in_progress":true,"is_ip_constrained":false,"is_leaving_window_constrained":false,"is_leaving_window_timeout":false,"is_opened_catalog":false,"is_practice_mode":true,"is_started":true,"knowledge_node_ids":[],"knowledge_node_reference":[],"leaving_window_limit":null,"leaving_window_timeout":null,"limit_answer_on_signle_client":false,"limit_short_answer_upload":false,"limit_time":null,"limited_ip":"","make_up_record":null,"module_id":196992,"module_sort":3,"prerequisites":[],"publish_time":"2026-05-23T06:58:00Z","published":true,"referrer_id":196992,"referrer_type":"module","score_percentage":"0.00","score_rule":"latest","score_type":"percentage","sort":3,"start_time":"2026-05-23T07:00:00Z","subjects_rule":{"select_subjects_randomly":false,"shuffle_options_randomly":false,"shuffle_subjects_randomly":"default","sub_subjects_randomly":false},"submit_by_group":false,"submit_times":0,"syllabus_id":0,"syllabus_sort":"1-01-01T00:00:00Z","teaching_model":"online","title":"2026\u6625\u5b63\u5b66\u671f\u671f\u672b\u603b\u7ec3\u4e60\u9898\u5e93","type":"exam","unique_key":"exam-31608","using_phase":"final"}]}
"#;
        let exam_response: ExamResponse = serde_json::from_str(data)?;
        println!("ExamResponse: {:?}", exam_response);
        Ok(())
    }
}
