use std::sync::Arc;

use crate::{
    abi::utils::SmartJsonExt,
    api::{
        network::SessionClient,
        xmu_service::lnt::{
            html::HtmlParseResult,
            submissions_id::{Subject, get_problem_message_content},
        },
    },
};
use anyhow::{Ok, Result};
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct ClassroomSubjectResponse {
    pub subjects: Vec<Subject>,
}

#[lnt_get_api(
    ClassroomSubjectResponse,
    "https://lnt.xmu.edu.cn/api/classroom/{classroom_id:i64}/subject"
)]
pub struct ClassroomSubject;

impl ClassroomSubjectResponse {
    pub async fn parse(&self, client: Arc<SessionClient>) -> Result<HtmlParseResult> {
        let mut ret = HtmlParseResult::new();

        for subject in &self.subjects {
            let problem_content = get_problem_message_content(subject, client.clone()).await?;
            ret.node_message(problem_content);
        }

        Ok(ret)
    }
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        //TODO:因为课程平台暂时关闭未测试API
        let castgc = "TGT-2605352-4k4AWnBpGvBvVUhDSMZTM9e8F-3rEggafQYwsQkjx6kk2jUwtycBvN2LKrInZ6az6IQnull_main";
        let session = castgc_get_session(castgc).await?;
        let data = ClassroomSubject::get(&session, 2776).await?;
        println!("ClassroomList: {:?}", data);
        let parsed = data.parse(Arc::new(SessionClient::new())).await?;
        println!("Parsed: {:?}", parsed);
        Ok(())
    }
}
