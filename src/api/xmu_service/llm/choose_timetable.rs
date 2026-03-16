use crate::api::{
    llm::tool::ask_as,
    network::SessionClient,
    xmu_service::{
        jw::{Schedule, ScheduleList},
        time::get_today,
    },
};
use anyhow::Result;
use genai::chat::{ChatMessage, MessageContent};
use helper::session_client_helper;
use llm_xml_caster::llm_prompt;
use serde::{Deserialize, Serialize};

#[llm_prompt]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimetableChoiceResponseLlm {
    #[prompt("应该返回选择学期的学年学期代码在提供的数据中")]
    pub semester: String,
}

const TIMETABLE_CHOICE_RESPONSE_VALID_EXAMPLE: &str = r#"
<TimetableChoiceResponseLlm>
    <semester>2023-2024-1</semester>
</TimetableChoiceResponseLlm>"#;

#[cfg(test)]
#[test]
fn test_timetable_choice_response_valid_example() {
    let parsed: TimetableChoiceResponseLlm =
        quick_xml::de::from_str(TIMETABLE_CHOICE_RESPONSE_VALID_EXAMPLE).unwrap();
    assert_eq!(
        parsed,
        TimetableChoiceResponseLlm {
            semester: "2023-2024-1".to_string(),
        }
    );
}

#[derive(Debug, Serialize, Deserialize)]
pub struct File {
    pub reference_id: i64,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FilesChoiceResponse {
    pub files: Vec<File>,
}

pub struct ChooseTimetable;

impl ChooseTimetable {
    #[session_client_helper]
    pub async fn get_from_client<P: Into<MessageContent> + Sync + Send>(
        client: &SessionClient,
        prompt: P,
    ) -> Result<(Schedule, u64)> {
        let schedule_list = ScheduleList::get_from_client(client).await?;
        let (week, _) = get_today();

        let messages = [vec![
            ChatMessage::system(
                "你是一个专业的理解用户需求的客服，请根据用户的需求字符串和现有信息推测用户最可能选择的课程表时间并且按照要求返回",
            ),
            ChatMessage::user(prompt.into()),ChatMessage::system("获取到学期信息如下: ")
        ],
            schedule_list.datas.kfdxnxqcx.rows.iter().map(|semester|{ChatMessage::system(format!(
                "<data>学期名称: {}, 学年学期代码: {}</data>\n",
                semester.xnxqdm_display, semester.xnxqdm))}).collect::<Vec<_>>(),vec![ChatMessage::system(format!(
            "当前时间: {}",
            chrono::Local::now()
        ))]].concat();

        let response =
            ask_as::<TimetableChoiceResponseLlm>(messages, TIMETABLE_CHOICE_RESPONSE_VALID_EXAMPLE)
                .await?;

        //println!("Choose timetable response: {:?}", response);

        let semester_code = response.semester.replace([' ', '\n', '\r'], "");

        let schedule = Schedule::get_by_code_from_client(client, &semester_code).await?;

        Ok((schedule, week as u64))
    }
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::jw::get_castgc_client;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-2617600-NgLMdw1qkKnP6DPnW4fVkK54-p9izXoeSbv-06qGEvVM2NaZ03FCLqgfaRvpoJ1Umzknull_main";
        let session = get_castgc_client(castgc);
        let data = ChooseTimetable::get_from_client(&session, "上学期的第9周课表").await?;
        println!("Timetable: {:?}", data);
        Ok(())
    }
}
