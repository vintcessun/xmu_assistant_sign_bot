use crate::abi::utils::SmartJsonExt;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Submission {
    pub id: i64,
    //pub created_at:IgnoredAny,
    //pub exam_id:IgnoredAny,
    //pub exam_type_text:IgnoredAny,
    //pub score:IgnoredAny,
    //pub submit_method:IgnoredAny,
    //pub submit_method_text:IgnoredAny,
    //pub submitted_at:IgnoredAny,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubmissionResponse {
    pub submissions: Vec<Submission>,
    //pub exam_final_score:IgnoredAny,
    //pub exam_score:IgnoredAny,
    //pub exam_score_rule:IgnoredAny,
}

#[lnt_get_api(
    SubmissionResponse,
    "https://lnt.xmu.edu.cn/api/exams/{exam_id:i64}/submissions"
)]
pub struct Submissions;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-3852561-uVVRgspS8GunYAC5ZSN-ile4Lpdkekl5ECPmCF1UjAvQTlVPYQ-XvFcaiuo-erBPtFonull_main";
        let session = castgc_get_session(castgc).await?;
        let data = Submissions::get(&session, 18543).await?;
        println!("Submissions: {:?}", data);
        Ok(())
    }
}
