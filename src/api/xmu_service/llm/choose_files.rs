use std::collections::HashMap;

use crate::api::{llm::tool::ask_as, network::SessionClient, xmu_service::lnt::Activities};
use anyhow::Result;
use genai::chat::{ChatMessage, MessageContent};
use helper::session_client_helper;
use llm_xml_caster::llm_prompt;
use serde::{Deserialize, Serialize};

#[llm_prompt]
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilesChoiceResponseLlm {
    #[prompt("如果目的是选择所有的内容或者没有特定指定范围则设置为 true，否则为 false")]
    pub all: bool,
    #[prompt("请注意这里对应的是提供的内容的reference_id字段")]
    pub files: Option<Vec<i64>>,
}

const FILES_CHOICE_RESPONSE_VALID_EXAMPLE: &str = r#"
<FilesChoiceResponseLlm>
    <all>false</all>
    <files>
        <item>123456</item>
        <item>234567</item>
    </files>
</FilesChoiceResponseLlm>"#;

#[cfg(test)]
#[test]
fn test_files_choice_response_valid_example() {
    let parsed: FilesChoiceResponseLlm =
        quick_xml::de::from_str(FILES_CHOICE_RESPONSE_VALID_EXAMPLE).unwrap();
    assert_eq!(
        parsed,
        FilesChoiceResponseLlm {
            all: false,
            files: Some(vec![123456, 234567])
        }
    );
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct File {
    pub reference_id: i64,
    pub name: String,
    /// 所属活动的标题。活动已关闭时用来按活动归并提示，不必一个文件报一行。
    pub activity_title: String,
    /// 活动已关闭时的截止时间；`None` 表示活动还开着，可以正常下载。
    pub closed_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FilesChoiceResponse {
    pub files: Vec<File>,
}

pub struct ChooseFiles;

impl ChooseFiles {
    #[session_client_helper]
    pub async fn get_from_client<P: Into<MessageContent> + Sync + Send>(
        client: &SessionClient,
        prompt: P,
        course_id: i64,
    ) -> Result<FilesChoiceResponse> {
        let activities = Activities::get_from_client(client, course_id).await?;

        let mut activities_map = HashMap::new();
        for activity in &activities.activities {
            let title = &activity.title;
            // 活动关了就把截止时间记下来：下载阶段据此直接跳过并给出准确原因，
            // 不用再为注定 403 的文件各打一次请求。
            let closed_at = activity
                .is_closed
                .then(|| activity.end_time.clone().unwrap_or_default());
            for upload in &activity.uploads {
                activities_map.insert(
                    upload.reference_id,
                    File {
                        reference_id: upload.reference_id,
                        name: format!("{}-{}", title, upload.name),
                        activity_title: title.clone(),
                        closed_at: closed_at.clone(),
                    },
                );
            }
        }

        let messages = vec![
            ChatMessage::system(
                "你是一个专业的理解用户需求的客服，请根据用户的需求字符串和现有信息推测用户最可能选择的文件并且按照要求返回，注意如果用户没有提到文件的范围，一个默认值是用户想要所有的文件，也就是说设置all为true",
            ),
            ChatMessage::user(prompt.into()),
            ChatMessage::system("获取到这门课的相关的活动如下："),
            ChatMessage::system(quick_xml::se::to_string(&activities)?),
        ];

        let response =
            ask_as::<FilesChoiceResponseLlm>(messages, FILES_CHOICE_RESPONSE_VALID_EXAMPLE).await?;

        println!("LLM 返回的文件选择结果：{:?}", response);

        if response.all {
            if activities_map.is_empty() {
                anyhow::bail!("课程无文件可以下载");
            }

            let files = activities_map.into_values().collect();

            Ok(FilesChoiceResponse { files })
        } else {
            match response.files {
                Some(files) => Ok(FilesChoiceResponse {
                    files: files
                        .into_iter()
                        .map(|reference_id| {
                            activities_map.get(&reference_id).cloned().unwrap_or(File {
                                reference_id,
                                name: format!("file_{}", reference_id),
                                activity_title: String::new(),
                                closed_at: None,
                            })
                        })
                        .collect(),
                }),
                None => Ok(FilesChoiceResponse { files: Vec::new() }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;
    use crate::api::xmu_service::testenv;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_all() -> Result<()> {
        let Some(castgc) = testenv::castgc() else {
            return testenv::skipped(module_path!());
        };
        let course_name = "离散数学";
        let session = castgc_get_session(castgc).await?;
        let data = ChooseFiles::get(&session, course_name, 71211).await?;
        println!("MyCourses: {:?}", data);
        Ok(())
    }

    #[tokio::test]
    async fn test_part() -> Result<()> {
        let Some(castgc) = testenv::castgc() else {
            return testenv::skipped(module_path!());
        };
        let course_name = "离散数学命题逻辑课件";
        let session = castgc_get_session(castgc).await?;
        let data = ChooseFiles::get(&session, course_name, 71211).await?;
        println!("MyCourses: {:?}", data);
        Ok(())
    }
}
