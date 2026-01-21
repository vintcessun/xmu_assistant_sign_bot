use crate::api::{
    llm::tool::{LlmPrompt, LlmUsize, ask_as},
    network::SessionClient,
    xmu_service::jw::{Schedule, ScheduleList},
};
use anyhow::Result;
use genai::chat::{ChatMessage, MessageContent};
use helper::{LlmPrompt, session_client_helper};
use serde::{Deserialize, Serialize};

#[derive(Debug, LlmPrompt, Serialize, Deserialize)]
pub struct TimetableChoiceResponseLlm {
    #[prompt("应该返回选择学期的学年学期代码在提供的数据中")]
    pub semester: String,
    #[prompt("选择周数，从 1 开始计数")]
    pub week: LlmUsize,
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
    ) -> Result<(Schedule, usize)> {
        let schedule_list = ScheduleList::get_from_client(client).await?;

        let mut messages = vec![
            ChatMessage::system(
                "你是一个专业的理解用户需求的客服，请根据用户的需求字符串和现有信息推测用户最可能选择的课程表时间并且按照要求返回",
            ),
            ChatMessage::user(prompt.into()),
        ];

        messages.push(ChatMessage::system("获取到学期信息如下: "));

        for semester in &schedule_list.datas.kfdxnxqcx.rows {
            let semester_info = format!(
                "<data>学期名称: {}, 学年学期代码: {}</data>\n",
                semester.xnxqdm_display, semester.xnxqdm
            );
            messages.push(ChatMessage::system(semester_info));
        }

        messages.push(ChatMessage::system(format!(
            "当前时间: {}",
            chrono::Local::now()
        )));

        let response = ask_as::<TimetableChoiceResponseLlm>(messages).await?;

        //println!("Choose timetable response: {:?}", response);

        let semester_code = response.semester.replace([' ', '\n', '\r'], "");

        let schedule = Schedule::get_by_code_from_client(client, &semester_code).await?;

        Ok((schedule, *response.week))
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
