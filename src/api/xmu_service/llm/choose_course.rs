use crate::api::{
    llm::tool::ask_as,
    network::SessionClient,
    xmu_service::lnt::{MyCourses, RecentlyVisitedCourses},
};
use anyhow::Result;
use genai::chat::{ChatMessage, MessageContent};
use helper::session_client_helper;
use llm_xml_caster::llm_prompt;
use serde::{Deserialize, Serialize};

#[llm_prompt]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CourseChoiceResponse {
    #[prompt("如果找到符合要求的课程就返回课程ID; 如果没找到指定的课程就是 null")]
    pub course_id: Option<i64>,
}

const COURSE_CHOICE_RESPONSE_VALID_EXAMPLE: &str = r#"
<CourseChoiceResponse>
    <course_id>12345678</course_id>
</CourseChoiceResponse>"#;

#[cfg(test)]
#[test]
fn test_course_choice_response_valid_example() {
    let parsed: CourseChoiceResponse =
        quick_xml::de::from_str(COURSE_CHOICE_RESPONSE_VALID_EXAMPLE).unwrap();
    assert_eq!(
        parsed,
        CourseChoiceResponse {
            course_id: Some(12345678)
        }
    );
}

pub struct ChooseCourse;

impl ChooseCourse {
    #[session_client_helper]
    pub async fn get_from_client<P: Into<MessageContent> + Sync + Send>(
        client: &SessionClient,
        prompt: P,
    ) -> Result<CourseChoiceResponse> {
        let recent_course = RecentlyVisitedCourses::get_from_client(client).await?;

        let course_data = MyCourses::get_from_client(client).await?;

        let messages = [vec![
            ChatMessage::system(
                "你是一个专业的理解用户需求的客服，请根据用户的需求字符串和现有信息推测用户最可能选择的信息并且按照要求返回课程ID或者None",
            ),
            ChatMessage::user(prompt.into()),ChatMessage::system("获取到用户最近的课程访问信息如下："),ChatMessage::system(quick_xml::se::to_string(
            &recent_course,
        )?),ChatMessage::system("获取到用户的所有课程信息如下："),ChatMessage::system(quick_xml::se::to_string(&course_data)?)
        ]].concat();

        let response =
            ask_as::<CourseChoiceResponse>(messages, COURSE_CHOICE_RESPONSE_VALID_EXAMPLE).await?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_exist() -> Result<()> {
        let castgc = "TGT-2531390-mxqQ9-BtOM8LxgojrfyoyhQUHAocCgolFFBSdT6nuxq62GVndQ7ULC1G-pK7tECBfoAnull_main";
        let course_name = "离散数学";
        let session = castgc_get_session(castgc).await?;
        let data = ChooseCourse::get(&session, course_name).await?;
        println!("MyCourses: {:?}", data);
        Ok(())
    }

    #[tokio::test]
    async fn test_no() -> Result<()> {
        let castgc = "TGT-2531390-mxqQ9-BtOM8LxgojrfyoyhQUHAocCgolFFBSdT6nuxq62GVndQ7ULC1G-pK7tECBfoAnull_main";
        let course_name = "生理医学";
        let session = castgc_get_session(castgc).await?;
        let data = ChooseCourse::get(&session, course_name).await?;
        println!("MyCourses: {:?}", data);
        Ok(())
    }
}
