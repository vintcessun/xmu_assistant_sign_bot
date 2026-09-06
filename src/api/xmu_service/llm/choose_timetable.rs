use crate::api::{
    llm::tool::ask_as,
    network::SessionClient,
    xmu_service::{
        jw::{Schedule, ScheduleList, ScheduleListResponse},
        time::{TIME_ZONE, get_today},
    },
};
use anyhow::{Result, anyhow};
use genai::chat::ChatMessage;
use helper::session_client_helper;
use llm_xml_caster::llm_prompt;
use serde::{Deserialize, Serialize};
use tracing::info;

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

/// 取最新的学年学期。
///
/// 教务的 `xnxqdm` 是等长数字串（`20253` 是 2025-2026 学年小学期，`20261` 是下一学年第一学期），
/// 数值最大的那行就是最新学期。个别代码万一不是纯数字，退回字典序，不让一行脏数据把整件事拖垮。
fn latest_semester(rows: &[ScheduleListResponse]) -> Option<&ScheduleListResponse> {
    rows.iter()
        .max_by(
            |a, b| match (a.xnxqdm.parse::<u64>(), b.xnxqdm.parse::<u64>()) {
                (Ok(x), Ok(y)) => x.cmp(&y),
                _ => a.xnxqdm.cmp(&b.xnxqdm),
            },
        )
}

/// 从整条消息里剥掉命令本身，只留用户写的描述。
///
/// 命令分发只看 `starts_with`，所以 `/timetable 第3周` 和 `/timetable第3周` 都会进来；
/// 这里把前缀和紧随其后的命令名（ASCII 字母数字下划线）一起去掉。
fn strip_description(text: &str) -> &str {
    let text = text.trim();
    let Some(rest) = text.strip_prefix(crate::config::get_command_prefix()) else {
        return text;
    };
    let cmd_len = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    rest[cmd_len..].trim()
}

/// 取默认学期的课表：教务「开放的学年学期」列表里最新的那个。
///
/// 注意这张列表会滞后于课表本身——2026-09-07 开学时它最新还停在 20253（夏季学期），
/// 而课表接口传 20261 已经能查到新学期的课。这里按要求只认列表，不自行推算学期代码。
async fn default_schedule(
    client: &SessionClient,
    rows: &[ScheduleListResponse],
) -> Result<(String, Schedule)> {
    let latest = latest_semester(rows).ok_or_else(|| anyhow!("教务没有返回任何可选学期"))?;
    let schedule = Schedule::get_by_code_from_client(client, &latest.xnxqdm).await?;
    Ok((latest.xnxqdm.clone(), schedule))
}

impl ChooseTimetable {
    #[session_client_helper]
    pub async fn get_from_client(client: &SessionClient, prompt: &str) -> Result<(Schedule, u64)> {
        let schedule_list = ScheduleList::get_from_client(client).await?;
        let rows = &schedule_list.datas.kfdxnxqcx.rows;

        // 选中哪个学期以前完全是黑盒，出问题只能靠猜。把教务给的整张单子记下来。
        info!(
            semesters = ?rows
                .iter()
                .map(|row| format!("{}={}", row.xnxqdm, row.xnxqdm_display))
                .collect::<Vec<_>>(),
            "教务返回的可选学期"
        );

        let (week, _) = get_today();
        // 开学之前 get_today() 会给出 0 或负数周，渲染时的 `week - 1` 会下溢成空课表，
        // 这里夹到第 1 周。
        let week = week.max(1) as u64;

        // 用户没写任何描述时不问 LLM：默认就是当前学期，确定性优于模型对日期的推断。
        let description = strip_description(prompt);
        if description.is_empty() {
            let (semester, schedule) = default_schedule(client, rows).await?;
            info!(%semester, week, "没有描述，使用默认学期");
            return Ok((schedule, week));
        }

        let latest = latest_semester(rows).ok_or_else(|| anyhow!("教务没有返回任何可选学期"))?;

        let messages = [vec![
            ChatMessage::system(
                "你是一个专业的理解用户需求的客服，请根据用户的需求字符串和现有信息推测用户最可能选择的课程表时间并且按照要求返回",
            ),
            ChatMessage::user(description.to_string()),ChatMessage::system("获取到学期信息如下: ")
        ],
            rows.iter().map(|semester|{ChatMessage::system(format!(
                "<data>学期名称: {}, 学年学期代码: {}</data>\n",
                semester.xnxqdm_display, semester.xnxqdm))}).collect::<Vec<_>>(),vec![ChatMessage::system(format!(
            "当前时间: {}",
            chrono::Utc::now().with_timezone(&TIME_ZONE)
        )),ChatMessage::system(format!(
            "其中最新的学期是「{}」，学年学期代码为 {}。用户没有明确要求其他学期时，一律返回这个最新学期的代码。",
            latest.xnxqdm_display, latest.xnxqdm
        ))]].concat();

        let response =
            ask_as::<TimetableChoiceResponseLlm>(messages, TIMETABLE_CHOICE_RESPONSE_VALID_EXAMPLE)
                .await?;

        let semester_code = response.semester.replace([' ', '\n', '\r'], "");

        // LLM 偶尔会编一个不在列表里的代码，这时按没有描述处理，别拿去打教务。
        if !rows.iter().any(|row| row.xnxqdm == semester_code) {
            let (semester, schedule) = default_schedule(client, rows).await?;
            info!(bad = %semester_code, %semester, week, "LLM 给的学期不在候选里，改用默认学期");
            return Ok((schedule, week));
        }

        info!(semester = %semester_code, week, description, "按描述选定学期");
        let schedule = Schedule::get_by_code_from_client(client, &semester_code).await?;

        Ok((schedule, week))
    }
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::testenv;
    use crate::api::xmu_service::jw::get_castgc_client;

    use super::*;
    use anyhow::Result;

    fn row(code: &str) -> ScheduleListResponse {
        ScheduleListResponse {
            xnxqdm: code.to_string(),
            xnxqdm_display: format!("display-{code}"),
        }
    }

    #[test]
    fn test_latest_semester() {
        // 跨学年时字典序和数值序一致，但仍按数值挑，代码位数变了也不会挑错。
        let rows = [row("20251"), row("20261"), row("20253")];
        assert_eq!(latest_semester(&rows).unwrap().xnxqdm, "20261");
        assert!(latest_semester(&[]).is_none());
    }

    #[test]
    fn test_strip_description() {
        // 没写描述 -> 空串，调用方据此直接用最新学期，不问 LLM。
        assert_eq!(strip_description("/timetable"), "");
        assert_eq!(strip_description("  /timetable  "), "");
        assert_eq!(strip_description("/timetable 第3周"), "第3周");
        // 命令和描述之间没有空格也要能剥干净。
        assert_eq!(strip_description("/timetable第3周"), "第3周");
        assert_eq!(strip_description("/signtime 上学期"), "上学期");
        // 不带命令前缀时原样返回。
        assert_eq!(strip_description("上学期"), "上学期");
    }

    #[tokio::test]
    async fn test() -> Result<()> {
        let Some(castgc) = testenv::castgc() else {
            return testenv::skipped(module_path!());
        };
        let session = get_castgc_client(castgc);
        let data = ChooseTimetable::get_from_client(&session, "上学期的第9周课表").await?;
        println!("Timetable: {:?}", data);
        Ok(())
    }
}
