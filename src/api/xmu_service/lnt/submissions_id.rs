use std::sync::Arc;

use crate::{
    abi::utils::SmartJsonExt,
    api::{
        network::SessionClient,
        xmu_service::lnt::html::{HtmlParseResult, html_to_message_and_markdown},
    },
};
use anyhow::Result;
use futures::{FutureExt, future::BoxFuture};
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

use crate::api::xmu_service::lnt::distribute::SubjectType;

/*
#[derive(Serialize, Deserialize, Debug)]
pub struct CorrectAnswer {
    pub answer_option_ids: Vec<i64>,
    pub subject_id: i64,
    //pub content: IgnoredAny,
    //pub point: IgnoredAny,
    //pub r#type: IgnoredAny,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CorrectAnswerData {
    pub correct_answers: Vec<CorrectAnswer>,
}
*/

#[derive(Serialize, Deserialize, Debug)]
pub struct SubjectOption {
    pub content: String,
    pub id: i64,
    pub is_answer: bool,
    pub r#type: SubjectType,
    pub sort: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Subject {
    pub answer_explanation: String,
    pub description: String,
    pub id: i64,
    pub options: Vec<SubjectOption>,
    pub r#type: SubjectType,
    pub wrong_explanation: String,
    pub sort: i64,
    pub sub_subjects: Vec<Subject>,
    //pub answer_number: IgnoredAny,
    //pub correct_answers: IgnoredAny,
    //pub data: IgnoredAny,
    //pub difficulty_level: IgnoredAny,
    //pub last_updated_at: IgnoredAny,
    //pub note: IgnoredAny,
    //pub parent_id: IgnoredAny,
    //pub point: IgnoredAny,
    //pub settings: IgnoredAny,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubjectData {
    pub subjects: Vec<Subject>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubmissionResponse {
    pub subjects_data: SubjectData,
    //pub auto_mark: IgnoredAny,
    //pub check_ip_consistency_passed: IgnoredAny,
    //pub correct_answers_data: IgnoredAny,
    //pub correct_data:IgnoredAny,
    //pub exam_type:IgnoredAny,
    //pub is_makeup:IgnoredAny,
    //pub is_simulated:IgnoredAny,
    //pub knowledge_node_data:IgnoredAny,
    //pub score: IgnoredAny,
    //pub submission_comment_data: IgnoredAny,
    //pub submission_data: IgnoredAny,
    //pub submission_score_data: IgnoredAny,
    //pub submit_method: IgnoredAny,
    //pub submit_method_text: IgnoredAny,
}

async fn get_problem_message_content_plain(
    subject: &Subject,
    client: Arc<SessionClient>,
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

    let mut ans = String::new();

    for option in &subject.options {
        let option_chr = (b'A' + (option.sort as u8 % 26)) as char;
        let option_prefix = format!("{option_chr}. ");
        ret.text(option_prefix);

        let option_content = html_to_message_and_markdown(&option.content, client.clone()).await?;
        ret.extend(option_content);

        ret.text('\n');

        if option.is_answer {
            ans.push(option_chr);
        }
    }

    ret.text(format!("本题答案: {}", ans));

    Ok(ret)
}

pub fn get_problem_message_content(
    subject: &Subject,
    client: Arc<SessionClient>,
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

impl SubmissionResponse {
    pub async fn parse(&self, client: Arc<SessionClient>) -> Result<HtmlParseResult> {
        let mut ret = HtmlParseResult::new();

        for subject in &self.subjects_data.subjects {
            let problem_content = get_problem_message_content(subject, client.clone()).await?;
            ret.node_message(problem_content);
        }

        Ok(ret)
    }
}

#[lnt_get_api(
    SubmissionResponse,
    "https://lnt.xmu.edu.cn/api/exams/{exam_id:i64}/submissions/{submission_id:i64}"
)]
pub struct SubmissionsId;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_request() -> Result<()> {
        let castgc = "TGT-3852721-5F6eRNQT3hKL70kX3mDbLKQOpeUcKCYbCwJUZNW-btgCA45jHAWRs6iRLEeNzYP3-1cnull_main";
        let session = castgc_get_session(castgc).await?;
        let data = SubmissionsId::get(&session, 18543, 1007385).await?;
        println!("SubmissionsId: {:?}", data);
        Ok(())
    }

    const DATA: &str = r#"{"auto_mark":true,"check_ip_consistency_passed":true,"correct_answers_data":{"correct_answers":[{"answer_option_ids":[3836430],"content":"\u003Cp\u003E十月革命\u003C/p\u003E","point":"5.2","subject_id":1130976,"type":"single_selection"},{"answer_option_ids":[3836433],"content":"\u003Cp\u003E毛泽东\u003C/p\u003E","point":"5.2","subject_id":1130979,"type":"single_selection"},{"answer_option_ids":[3836448],"content":"\u003Cp\u003E马克思主义中国化时代化\u003C/p\u003E","point":"5.2","subject_id":1130982,"type":"single_selection"},{"answer_option_ids":[3836466],"content":"\u003Cp\u003E党的二十大\u003C/p\u003E","point":"5.2","subject_id":1130985,"type":"single_selection"},{"answer_option_ids":[3836469],"content":"\u003Cp\u003E毛泽东思想\u003C/p\u003E","point":"5.2","subject_id":1130988,"type":"single_selection"},{"answer_option_ids":[3836484],"content":"\u003Cp\u003E与时俱进\u003C/p\u003E","point":"5.2","subject_id":1130991,"type":"single_selection"},{"answer_option_ids":[3836499],"content":"\u003Cp\u003E习近平新时代中国特色社会主义思想\u003C/p\u003E","point":"5.2","subject_id":1130994,"type":"single_selection"},{"answer_option_ids":[3836505],"content":"\u003Cp\u003E中共十一届三中全会 \u003C/p\u003E","point":"5.2","subject_id":1130997,"type":"single_selection"},{"answer_option_ids":[3836526],"content":"\u003Cp\u003E毛泽东思想\u003C/p\u003E","point":"5.2","subject_id":1131000,"type":"single_selection"},{"answer_option_ids":[3836538],"content":"\u003Cp\u003E实现中华民族伟大复兴\u003C/p\u003E","point":"5.2","subject_id":1131003,"type":"single_selection"},{"answer_option_ids":[3836541,3836544],"point":"8.0","subject_id":1131006,"type":"multiple_selection"},{"answer_option_ids":[3836556,3836559],"point":"8.0","subject_id":1131009,"type":"multiple_selection"},{"answer_option_ids":[3836565,3836568],"point":"8.0","subject_id":1131012,"type":"multiple_selection"},{"answer_option_ids":[3836577,3836580,3836583,3836586],"point":"8.0","subject_id":1131015,"type":"multiple_selection"},{"answer_option_ids":[3836595,3836598],"point":"8.0","subject_id":1131018,"type":"multiple_selection"},{"answer_option_ids":[3836601,3836604,3836607,3836610],"point":"8.0","subject_id":1131021,"type":"multiple_selection"}]},"correct_data":{"1130976":true,"1130979":true,"1130982":true,"1130985":true,"1130988":true,"1130991":true,"1130994":true,"1130997":true,"1131000":true,"1131003":true,"1131006":true,"1131009":true,"1131012":true,"1131015":true,"1131018":true,"1131021":true},"exam_type":"exam","is_makeup":false,"is_simulated":false,"knowledge_node_data":{},"score":100,"subjects_data":{"subjects":[{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E（  ）给中国送来了马克思列宁主义，给苦苦探寻救亡图存出路的中国人民指明了前进方向、提供了全新选择。\u003C/p\u003E","difficulty_level":"easy","id":1130976,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E鸦片战争\u003C/p\u003E","id":3836421,"is_answer":false,"sort":0,"type":"text"},{"content":"\u003Cp\u003E新文化运动\u003C/p\u003E","id":3836424,"is_answer":false,"sort":1,"type":"text"},{"content":"\u003Cp\u003E五四运动\u003C/p\u003E","id":3836427,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E十月革命\u003C/p\u003E","id":3836430,"is_answer":true,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"case_sensitive":true,"has_audio":false,"option_type":"text","options_layout":"vertical","play_limit":true,"play_limit_times":1,"required":false,"unordered":false,"uploads":[]},"sort":0,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E1938年，（  ）在党的六届六中全会上作了《论新阶段》的报告，强调：“没有抽象的马克思主义，只有具体的马克思主义……”\u003C/p\u003E","difficulty_level":"easy","id":1130979,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E毛泽东\u003C/p\u003E","id":3836433,"is_answer":true,"sort":0,"type":"text"},{"content":"\u003Cp\u003E任弼时\u003C/p\u003E","id":3836436,"is_answer":false,"sort":1,"type":"text"},{"content":"\u003Cp\u003E刘少奇\u003C/p\u003E","id":3836439,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E周恩来\u003C/p\u003E","id":3836442,"is_answer":false,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":1,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E党的十八大以来，以习近平同志为核心的党中央明确提出要不断推进（  ）。\u003C/p\u003E","difficulty_level":"easy","id":1130982,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E社会主义现代化\u003C/p\u003E","id":3836445,"is_answer":false,"sort":0,"type":"text"},{"content":"\u003Cp\u003E马克思主义中国化时代化\u003C/p\u003E","id":3836448,"is_answer":true,"sort":1,"type":"text"},{"content":"\u003Cp\u003E“两个结合”\u003C/p\u003E","id":3836451,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E社会主义现代化强国\u003C/p\u003E","id":3836454,"is_answer":false,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":2,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E（   ）明确把“不断谱写马克思主义中国化时代化新篇章”作为当代中国共产党人的庄严历史责任，并提出了继续推进马克思主义中国化时代化的新要求。\u003C/p\u003E","difficulty_level":"easy","id":1130985,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E党的十七大\u003C/p\u003E","id":3836457,"is_answer":false,"sort":0,"type":"text"},{"content":"\u003Cp\u003E党的十八大\u003C/p\u003E","id":3836460,"is_answer":false,"sort":1,"type":"text"},{"content":"\u003Cp\u003E党的十九大\u003C/p\u003E","id":3836463,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E党的二十大\u003C/p\u003E","id":3836466,"is_answer":true,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":3,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E（  ）是马克思主义中国化时代化的第一次历史性飞跃。\u003C/p\u003E","difficulty_level":"easy","id":1130988,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E毛泽东思想\u003C/p\u003E","id":3836469,"is_answer":true,"sort":0,"type":"text"},{"content":"\u003Cp\u003E邓小平理论\u003C/p\u003E","id":3836472,"is_answer":false,"sort":1,"type":"text"},{"content":"\u003Cp\u003E“三个代表”重要思想\u003C/p\u003E","id":3836475,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E科学发展观\u003C/p\u003E","id":3836478,"is_answer":false,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":4,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E马克思主义中国化时代化的理论成果是一脉相承又（    ）的关系。\u003C/p\u003E","difficulty_level":"easy","id":1130991,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E实事求是\u003C/p\u003E","id":3836481,"is_answer":false,"sort":0,"type":"text"},{"content":"\u003Cp\u003E与时俱进\u003C/p\u003E","id":3836484,"is_answer":true,"sort":1,"type":"text"},{"content":"\u003Cp\u003E独立自主\u003C/p\u003E","id":3836487,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E精益求精\u003C/p\u003E","id":3836490,"is_answer":false,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":5,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E马克思主义中国化时代化的最新理论成果是（    ）。\u003C/p\u003E","difficulty_level":"easy","id":1130994,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E毛泽东思想\u003C/p\u003E","id":3836493,"is_answer":false,"sort":0,"type":"text"},{"content":"\u003Cp\u003E科学发展观\u003C/p\u003E","id":3836496,"is_answer":false,"sort":1,"type":"text"},{"content":"\u003Cp\u003E习近平新时代中国特色社会主义思想\u003C/p\u003E","id":3836499,"is_answer":true,"sort":2,"type":"text"},{"content":"\u003Cp\u003E邓小平理论\u003C/p\u003E","id":3836502,"is_answer":false,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":6,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E（    ）的召开，实现了新中国成立以来党的历史上具有深远意义的伟大转折，开启了改革开放和社会主义现代化建设历史新时期。\u003C/p\u003E","difficulty_level":"easy","id":1130997,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E中共十一届三中全会 \u003C/p\u003E","id":3836505,"is_answer":true,"sort":0,"type":"text"},{"content":"\u003Cp\u003E中共十二大\u003C/p\u003E","id":3836508,"is_answer":false,"sort":1,"type":"text"},{"content":"\u003Cp\u003E中共十三大\u003C/p\u003E","id":3836511,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E中共十一届六中全会\u003C/p\u003E","id":3836514,"is_answer":false,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":7,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E中国特色社会主义理论体系不包括（  ）。\u003C/p\u003E","difficulty_level":"easy","id":1131000,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E邓小平理论\u003C/p\u003E","id":3836517,"is_answer":false,"sort":0,"type":"text"},{"content":"\u003Cp\u003E科学发展观\u003C/p\u003E","id":3836520,"is_answer":false,"sort":1,"type":"text"},{"content":"\u003Cp\u003E习近平新时代中国特色社会主义思想\u003C/p\u003E","id":3836523,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E毛泽东思想\u003C/p\u003E","id":3836526,"is_answer":true,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":8,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E中国共产党一经诞生，就把为中国人民谋幸福、为中华民族谋复兴确立为自己的初心使命。一百年来，中国共产党团结带领中国人民进行的一切奋斗、一切牺牲、一切创造，归结起来就是一个主题（ ）。\u003C/p\u003E","difficulty_level":"easy","id":1131003,"last_updated_at":"2025-11-24T06:46:13Z","note":null,"options":[{"content":"\u003Cp\u003E实现共同富裕\u003C/p\u003E","id":3836529,"is_answer":false,"sort":0,"type":"text"},{"content":"\u003Cp\u003E实现全面建成小康社会\u003C/p\u003E","id":3836532,"is_answer":false,"sort":1,"type":"text"},{"content":"\u003Cp\u003E实现社会主义现代化\u003C/p\u003E","id":3836535,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E实现中华民族伟大复兴\u003C/p\u003E","id":3836538,"is_answer":true,"sort":3,"type":"text"}],"parent_id":null,"point":"5.2","settings":{"options_layout":"vertical"},"sort":9,"sub_subjects":[],"type":"single_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E准确把握马克思主义中国化时代化的科学内涵，要做到坚持（  ）与（  ）相统一。\u003C/p\u003E","difficulty_level":"easy","id":1131006,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E马克思主义\u003C/p\u003E","id":3836541,"is_answer":true,"sort":0,"type":"text"},{"content":"\u003Cp\u003E发展马克思主义 \u003C/p\u003E","id":3836544,"is_answer":true,"sort":1,"type":"text"},{"content":"\u003Cp\u003E社会主义\u003C/p\u003E","id":3836547,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E中国特色社会主义\u003C/p\u003E","id":3836550,"is_answer":false,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":10,"sub_subjects":[],"type":"multiple_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E推进马克思主义中国化时代化，是（   ）。\u003C/p\u003E","difficulty_level":"easy","id":1131009,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E马克思主义唯物史观的要求\u003C/p\u003E","id":3836553,"is_answer":false,"sort":0,"type":"text"},{"content":"\u003Cp\u003E马克思主义理论本身发展的内在要求\u003C/p\u003E","id":3836556,"is_answer":true,"sort":1,"type":"text"},{"content":"\u003Cp\u003E解决中国实际问题的客观需要\u003C/p\u003E","id":3836559,"is_answer":true,"sort":2,"type":"text"},{"content":"\u003Cp\u003E社会主义经济社会发展的需要\u003C/p\u003E","id":3836562,"is_answer":false,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":11,"sub_subjects":[],"type":"multiple_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E坚持和发展马克思主义，必须（  ）。\u003C/p\u003E","difficulty_level":"easy","id":1131012,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E同中国具体实际相结合\u003C/p\u003E","id":3836565,"is_answer":true,"sort":0,"type":"text"},{"content":"\u003Cp\u003E同中华优秀传统文化相结合\u003C/p\u003E","id":3836568,"is_answer":true,"sort":1,"type":"text"},{"content":"\u003Cp\u003E同社会主义现代化发展相结合\u003C/p\u003E","id":3836571,"is_answer":false,"sort":2,"type":"text"},{"content":"\u003Cp\u003E同中华民族伟大复兴相结合\u003C/p\u003E","id":3836574,"is_answer":false,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":12,"sub_subjects":[],"type":"multiple_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E要坚持解放思想、实事求是、与时俱进、求真务实，一切从实际出发，着眼解决革命、建设、改革中的实际问题，不断回答（  ），作出符合中国实际和时代要求的正确回答，得出符合客观规律的科学认识，形成与时俱进的理论成果，更好指导中国实践。\u003C/p\u003E","difficulty_level":"easy","id":1131015,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E中国之问\u003C/p\u003E","id":3836577,"is_answer":true,"sort":0,"type":"text"},{"content":"\u003Cp\u003E世界之问\u003C/p\u003E","id":3836580,"is_answer":true,"sort":1,"type":"text"},{"content":"\u003Cp\u003E人民之问\u003C/p\u003E","id":3836583,"is_answer":true,"sort":2,"type":"text"},{"content":"\u003Cp\u003E时代之问\u003C/p\u003E","id":3836586,"is_answer":true,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":13,"sub_subjects":[],"type":"multiple_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E实践证明，中国共产党为什么能，中国特色社会主义为什么好，归根到底是（  ）。\u003C/p\u003E","difficulty_level":"easy","id":1131018,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E马克思主义经典作家行\u003C/p\u003E","id":3836589,"is_answer":false,"sort":0,"type":"text"},{"content":"\u003Cp\u003E科学社会主义行\u003C/p\u003E","id":3836592,"is_answer":false,"sort":1,"type":"text"},{"content":"\u003Cp\u003E马克思主义行\u003C/p\u003E","id":3836595,"is_answer":true,"sort":2,"type":"text"},{"content":"\u003Cp\u003E中国化时代化的马克思主义行\u003C/p\u003E","id":3836598,"is_answer":true,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":14,"sub_subjects":[],"type":"multiple_selection","wrong_explanation":""},{"answer_explanation":"\u003Cp\u003E\u003C/p\u003E","answer_number":0,"correct_answers":[],"data":{},"description":"\u003Cp\u003E马克思主义中国化时代化的内涵是（）\u003C/p\u003E","difficulty_level":"easy","id":1131021,"last_updated_at":"2025-11-24T06:46:05Z","note":null,"options":[{"content":"\u003Cp\u003E就是立足中国国情和时代特点\u003C/p\u003E","id":3836601,"is_answer":true,"sort":0,"type":"text"},{"content":"\u003Cp\u003E坚持把马克思主义基本原理同中国具体实际相结合、同中华优秀传统文化相结合，\u003C/p\u003E","id":3836604,"is_answer":true,"sort":1,"type":"text"},{"content":"\u003Cp\u003E深入研究和解决中国革命、建设、改革不同历史时期的实际问题\u003C/p\u003E","id":3836607,"is_answer":true,"sort":2,"type":"text"},{"content":"\u003Cp\u003E真正搞懂面临的时代课题，不断吸收新的时代内容，科学回答时代提出的重大理论和实践课题，创造新的理论成果。\u003C/p\u003E","id":3836610,"is_answer":true,"sort":3,"type":"text"}],"parent_id":null,"point":"8.0","settings":{"options_layout":"vertical"},"sort":15,"sub_subjects":[],"type":"multiple_selection","wrong_explanation":""}]},"submission_comment_data":{},"submission_data":{"progress":{},"subjects":[{"answer":"","answer_option_ids":[3836430],"subject_id":1130976},{"answer":"","answer_option_ids":[3836433],"subject_id":1130979},{"answer":"","answer_option_ids":[3836448],"subject_id":1130982},{"answer":"","answer_option_ids":[3836466],"subject_id":1130985},{"answer":"","answer_option_ids":[3836469],"subject_id":1130988},{"answer":"","answer_option_ids":[3836484],"subject_id":1130991},{"answer":"","answer_option_ids":[3836499],"subject_id":1130994},{"answer":"","answer_option_ids":[3836505],"subject_id":1130997},{"answer":"","answer_option_ids":[3836526],"subject_id":1131000},{"answer":"","answer_option_ids":[3836538],"subject_id":1131003},{"answer":"","answer_option_ids":[3836541,3836544],"subject_id":1131006},{"answer":"","answer_option_ids":[3836556,3836559],"subject_id":1131009},{"answer":"","answer_option_ids":[3836565,3836568],"subject_id":1131012},{"answer":"","answer_option_ids":[3836577,3836580,3836583,3836586],"subject_id":1131015},{"answer":"","answer_option_ids":[3836595,3836598],"subject_id":1131018},{"answer":"","answer_option_ids":[3836601,3836604,3836607,3836610],"subject_id":1131021}]},"submission_score_data":{"1130976":"5.2","1130979":"5.2","1130982":"5.2","1130985":"5.2","1130988":"5.2","1130991":"5.2","1130994":"5.2","1130997":"5.2","1131000":"5.2","1131003":"5.2","1131006":"8.0","1131009":"8.0","1131012":"8.0","1131015":"8.0","1131018":"8.0","1131021":"8.0"},"submit_method":"submitted_by_examinee","submit_method_text":"手动交卷"}"#;

    #[tokio::test]
    pub async fn test_parse() -> Result<()> {
        let parsed: SubmissionResponse = serde_json::from_str(DATA)?;
        let client = Arc::new(SessionClient::new());
        let parsed = parsed.parse(client).await?;
        println!("Parsed: {:?}", parsed);
        Ok(())
    }
}
