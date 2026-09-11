use crate::abi::utils::SmartJsonExt;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Upload {
    pub name: String,
    pub reference_id: i64,
    //pub allow_aliyun_office_view:IgnoredAny,
    //pub allow_download:IgnoredAny,
    //pub allow_private_wps_office_view:IgnoredAny,
    //pub audio:IgnoredAny,
    //pub cc_license_code:IgnoredAny,
    //pub cc_license_description:IgnoredAny,
    //pub cc_license_link:IgnoredAny,
    //pub cc_license_name: IgnoredAny,
    //pub created_at: IgnoredAny,
    //pub created_by_id: IgnoredAny,
    //pub deleted: IgnoredAny,
    //pub enable_set_h5_courseware_completion: IgnoredAny,
    //pub id: IgnoredAny,
    //pub is_cc_video: IgnoredAny,
    //pub key: IgnoredAny,
    //pub link: IgnoredAny,
    //pub origin_allow_download: IgnoredAny,
    //pub owner_id: IgnoredAny,
    //pub question_count: IgnoredAny,
    //pub referenced_at: IgnoredAny,
    //pub scorm: IgnoredAny,
    //pub size: IgnoredAny,
    //pub source: IgnoredAny,
    //pub status: IgnoredAny,
    //pub third_part_referrer_id: IgnoredAny,
    //pub thumbnail: IgnoredAny,
    //pub r#type: IgnoredAny,
    //pub updated_at: IgnoredAny,
    //pub video_src_type: IgnoredAny,
    //pub videos: IgnoredAny,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Activity {
    pub title: String,
    pub uploads: Vec<Upload>,
    /// 活动是否已关闭。老师给活动设了截止时间，过期后就关闭，lnt 随即不再发放
    /// 里面文件的下载地址（`/api/uploads/reference/{id}/url` 直接 403），
    /// 但文件本身没被删（每个 upload 的 `deleted` 仍是 false）。
    ///
    /// 2026-09-11 实测：某课 62 个文件里，27 个来自已关闭活动、全部 403；
    /// 35 个来自进行中活动、全部 200——`is_closed` 完全预测能不能下。
    ///
    /// 不序列化：这个结构体会被整个塞进 `choose_files` 给 LLM 的提示词，
    /// 选文件的口径不因这个字段改变（用户要求「LLM 选 all 正常」）。
    #[serde(default, skip_serializing)]
    pub is_closed: bool,
    /// 活动的截止时间，用来告诉用户是什么时候关的。
    #[serde(default, skip_serializing)]
    pub end_time: Option<String>,
    //pub announce_answer_and_explanation: IgnoredAny,
    //pub assign_group_ids: IgnoredAny,
    //pub assign_student_ids: IgnoredAny,
    //pub can_show_score: IgnoredAny,
    //pub completion_criterion: IgnoredAny,
    //pub completion_criterion_key: IgnoredAny,
    //pub completion_criterion_value: IgnoredAny,
    //pub course_id: IgnoredAny,
    //pub created_at: IgnoredAny,
    //pub data: IgnoredAny,
    //pub enable_edit: IgnoredAny,
    //pub forum_count: IgnoredAny,
    //pub group_set_id: IgnoredAny,
    //pub group_set_name: IgnoredAny,
    //pub has_assign_group: IgnoredAny,
    //pub has_assign_student: IgnoredAny,
    //pub id: IgnoredAny,
    //pub imported_from: IgnoredAny,
    //pub imported_track_id: IgnoredAny,
    //pub inter_review_named: IgnoredAny,
    //pub inter_score_map: IgnoredAny,
    //pub intra_rubric_id: IgnoredAny,
    //pub intra_rubric_instance: IgnoredAny,
    //pub intra_rubric_instance_id: IgnoredAny,
    //pub intra_score_map: IgnoredAny,
    //pub is_assigned_to_all: IgnoredAny,
    //pub is_in_progress: IgnoredAny,
    //pub is_inter_review_by_submitter: IgnoredAny,
    //pub is_opened_catalog: IgnoredAny,
    //pub is_review_homework: IgnoredAny,
    //pub is_started: IgnoredAny,
    //pub knowledge_node_ids: IgnoredAny,
    //pub knowledge_node_reference: IgnoredAny,
    //pub late_submission_count: IgnoredAny,
    //pub module_id: IgnoredAny,
    //pub non_submit_times: IgnoredAny,
    //pub prerequisites: IgnoredAny,
    //pub published: IgnoredAny,
    //pub question_count: IgnoredAny,
    //pub rubric_id: IgnoredAny,
    //pub rubric_instance: IgnoredAny,
    //pub rubric_instance_id: IgnoredAny,
    //pub score_item_group_id: IgnoredAny,
    //pub score_item_group_name: IgnoredAny,
    //pub score_item_scored: IgnoredAny,
    //pub score_percentage: IgnoredAny,
    //pub score_published: IgnoredAny,
    //pub score_type: IgnoredAny,
    //pub sort: IgnoredAny,
    //pub start_time: IgnoredAny,
    //pub submit_by_group: IgnoredAny,
    //pub submit_times: IgnoredAny,
    //pub syllabus_id: IgnoredAny,
    //pub teaching_model: IgnoredAny,
    //pub teaching_unit_id: IgnoredAny,
    //pub r#type: IgnoredAny,
    //pub unique_key: IgnoredAny,
    //pub updated_at: IgnoredAny,
    //pub description: String,
    //pub using_phase: IgnoredAny,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActivitiesResponse {
    pub activities: Vec<Activity>,
}

#[lnt_get_api(
    ActivitiesResponse,
    "https://lnt.xmu.edu.cn/api/courses/{course_id:i64}/activities"
)]
pub struct Activities;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;
    use crate::api::xmu_service::testenv;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let Some(castgc) = testenv::castgc() else {
            return testenv::skipped(module_path!());
        };
        let session = castgc_get_session(castgc).await?;
        let data = Activities::get(&session, 71211).await?;
        println!("MyCourses: {:?}", data);
        Ok(())
    }
}
