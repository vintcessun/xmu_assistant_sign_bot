use std::fmt::Display;

use crate::{
    abi::utils::SmartJsonExt,
    api::{
        network::SessionClient,
        xmu_service::lnt::html::{HtmlParseResult, html_to_message_and_markdown},
    },
};
use anyhow::{Ok, Result};
use futures::{FutureExt, future::BoxFuture};
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SubjectType {
    SingleSelection,   //单选题
    MultipleSelection, //多选题
    TrueOrFalse,       //判断题
    FillInBlank,       //填空题
    ShortAnswer,       //简答题
    ParagraphDesc,     //段落说明
    Analysis,          //综合题
    Media,             //听力题
    Text,              //纯文本
}

impl Display for SubjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SubjectType::SingleSelection => "单选题",
            SubjectType::MultipleSelection => "多选题",
            SubjectType::TrueOrFalse => "判断题",
            SubjectType::FillInBlank => "填空题",
            SubjectType::ShortAnswer => "简答题",
            SubjectType::ParagraphDesc => "段落说明",
            SubjectType::Analysis => "综合题",
            SubjectType::Media => "听力题",
            SubjectType::Text => "纯文本",
        };
        write!(f, "{s}")
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubjectOption {
    pub content: String,
    pub r#type: SubjectType,
    pub id: i64,
    pub sort: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Subject {
    pub description: String,
    pub options: Vec<SubjectOption>,
    pub point: String,
    pub sub_subjects: Vec<Subject>,
    pub r#type: SubjectType,
    pub id: i64,
    pub sort: i64,
    //pub answer_number: IgnoredAny,
    //pub data: IgnoredAny,
    //pub options: Vec<SubjectOption>,
    //pub point: f64,
    //pub sub_subjects: Vec<Box<Subject>>,
    //pub r#type: SubjectType,
    //pub difficulty_level: IgnoredAny,
    //pub last_updated_at: IgnoredAny,
    //pub note: IgnoredAny,
    //pub parent_id: IgnoredAny,
    //pub settings: IgnoredAny,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DistributeResponse {
    pub subjects: Vec<Subject>,
    //pub exam_paper_instance_id: IgnoredAny,
}

async fn get_problem_message_content_plain(
    subject: &Subject,
    client: SessionClient,
) -> Result<HtmlParseResult> {
    let message_type = format!("{}", subject.r#type);
    let sort = subject.sort + 1;
    let mut ret = HtmlParseResult::new();

    let prefix = format!("{sort}. ({message_type}) ");
    ret.text(prefix);

    let description_content =
        html_to_message_and_markdown(&subject.description, client.clone()).await?;
    ret.extend(description_content);

    ret.text("\n\n");

    for option in &subject.options {
        let option_chr = (b'A' + (option.sort as u8 % 26)) as char;
        let option_prefix = format!("{option_chr}. ");
        ret.text(option_prefix);

        let option_content = html_to_message_and_markdown(&option.content, client.clone()).await?;
        ret.extend(option_content);

        ret.text('\n');
    }

    Ok(ret)
}

fn get_problem_message_content(
    subject: &Subject,
    client: SessionClient,
) -> BoxFuture<'_, Result<HtmlParseResult>> {
    async move {
        if subject.sub_subjects.is_empty() {
            get_problem_message_content_plain(subject, client).await
        } else {
            let mut ret = HtmlParseResult::new();

            let message_type = format!("{}", subject.r#type);

            ret.text(format!(
                "------------------\n{message_type} 开始\n------------------"
            ));

            let this_problem = get_problem_message_content_plain(subject, client.clone()).await?;
            ret.extend(this_problem);

            for sub in &subject.sub_subjects {
                let sub_problem: HtmlParseResult =
                    get_problem_message_content(sub, client.clone()).await?;
                ret.extend(sub_problem);
            }

            ret.text(format!(
                "------------------\n{message_type} 结束\n------------------"
            ));

            Ok(ret)
        }
    }
    .boxed()
}

impl DistributeResponse {
    pub async fn parse(&self, client: SessionClient) -> Result<HtmlParseResult> {
        let mut ret = HtmlParseResult::new();

        for subject in &self.subjects {
            let problem_content = get_problem_message_content(subject, client.clone()).await?;
            ret.node_message(problem_content);
        }

        Ok(ret)
    }
}

#[lnt_get_api(
    DistributeResponse,
    "https://lnt.xmu.edu.cn/api/exams/{exam_id:i64}/distribute"
)]
pub struct Distribute;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        //TODO:目前只是REWRITE了尚未测试
        let castgc = "TGT-3827578-M2HQ5YkLD9VjNneiiEWeEXaizQy1X67ewOmxyCS4pfYHiMdQSYUwWP1HsHcrVM4A8WInull_main";
        let session = castgc_get_session(castgc).await?;
        let data = Distribute::get(&session, 71211).await?;
        println!("MyCourses: {:?}", data);
        Ok(())
    }
}
