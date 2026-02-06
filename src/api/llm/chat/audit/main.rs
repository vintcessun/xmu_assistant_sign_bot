use crate::{
    abi::message::MessageSend,
    api::{
        llm::{
            chat::{
                archive::{
                    memo_fragment::MemoFragment,
                    message_storage::{MessageStorage, NoticeStorage},
                },
                audit::{
                    backlist::{Backlist, BlacklistEntry},
                    bridge::llm_msg_from_message,
                },
                llm::ask_llm,
                repeat::reply::MessageAbstract,
            },
            tool::{LlmBool, LlmOption, LlmPrompt, LlmVec, ask_as},
        },
        storage::ColdTable,
    },
    config::LLM_AUDIT_DURATION_SECS,
};
use anyhow::Result;
use futures::{SinkExt, StreamExt, channel::mpsc, future::join_all};
use genai::chat::ChatMessage;
use helper::LlmPrompt;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock},
    time::{self, Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::{Instant, sleep_until};
use tracing::{error, trace};

static AUDIT_TASK: LazyLock<AuditTask> = LazyLock::new(|| AuditTask::new("llm_chat_audit_task"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditType {
    Fast,   //L0: 针对 HotTable 缓存
    Search, //L1: 针对向量检索结果
    Deep,   //L2: 针对 LLM 生成结果
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditTestRequest {
    pub message: Vec<ChatMessage>,
    pub audit_type: AuditType,
    pub timestamp: u64,
    pub group_id: i64,
    pub fast_key: Option<MessageAbstract>,
    pub search_key: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditStatus {
    Pending,
    Processing,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditTestTask {
    pub task: AuditTestRequest,
    pub status: AuditStatus,
}

pub struct AuditTask {
    pub tx: mpsc::UnboundedSender<AuditTestRequest>,
    pub data: Arc<ColdTable<u64, AuditTestTask>>,
}

async fn sleep_until_unix_timestamp(target_timestamp: u64) {
    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if target_timestamp > now_unix {
        let wait_secs = target_timestamp - now_unix;

        // 计算目标 Instant：当前单调时间 + 需要等待的秒数
        let target_instant = Instant::now() + Duration::from_secs(wait_secs);

        trace!(
            "正在等待审计任务，将在 Unix 戳 {} 触发...",
            target_timestamp
        );
        sleep_until(target_instant).await;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, LlmPrompt)]
pub struct AuditLlmResponse {
    #[prompt("该回复是否具有记忆价值，值得被永久铭记？")]
    pub is_value: LlmBool,
    #[prompt("如果该回复具有记忆价值，请简要描述其包含的知识点、幽默感或见解")]
    pub value_detail: LlmOption<String>,
    #[prompt("该回复是否触犯了群聊的潜规则，是否需要进行惩罚？")]
    pub is_penalty: LlmBool,
    #[prompt("如果该回答具有惩罚性，请描述具体的惩罚细节")]
    pub bad_detail: LlmOption<String>,
    #[prompt("如果该回答具有惩罚性，请描述该回复触犯群聊潜规则的具体原因是什么？")]
    pub bad_reason: LlmOption<String>,
    #[prompt("如果该回答具有惩罚性，请给出改进建议，帮助 Bot 更好地融入群聊氛围")]
    pub suggestions: LlmOption<LlmVec<String>>,
}

impl AuditTask {
    async fn process_task(
        task: AuditTestRequest,
        data: Arc<ColdTable<u64, AuditTestTask>>,
    ) -> Result<()> {
        let ts = task.timestamp;
        data.insert(
            ts,
            AuditTestTask {
                task: task.clone(),
                status: AuditStatus::Processing,
            },
        )
        .await?;

        let ts = task.timestamp;
        sleep_until_unix_timestamp(ts + LLM_AUDIT_DURATION_SECS + 3).await;
        let src_msg = task.message.clone();
        let before_msg_all = MessageStorage::get_range(ts - LLM_AUDIT_DURATION_SECS, ts).await;
        let (before_id, before_msg) = before_msg_all
            .into_iter()
            .unzip::<String, ChatMessage, Vec<String>, Vec<ChatMessage>>();
        let before_notice = NoticeStorage::get_range(ts - LLM_AUDIT_DURATION_SECS, ts).await;
        let after_msg_all = MessageStorage::get_range(ts, ts + LLM_AUDIT_DURATION_SECS).await;
        let (after_id, after_msg) = after_msg_all
            .into_iter()
            .unzip::<String, ChatMessage, Vec<String>, Vec<ChatMessage>>();
        let after_notice = NoticeStorage::get_range(ts, ts + LLM_AUDIT_DURATION_SECS).await;
        let msg = [vec![ChatMessage::system(
            r#"# Role
你是一名具备深刻洞察力的对话分析专家与社会心理学家。你的任务是穿透表层文字，审视 AI 在复杂群聊环境中的表现，判断其行为是否真正融入了人类的语境。

# Task Description
你将面对一段被“时间冻结”的对话切片（Context + Target + Feedback）。
你需要像一个深沉的思考者，复盘这场交互的灵魂。

# 审计与反思指南
请不必急于下结论，请先在脑海中对以下维度进行发散性推演：

1. **价值沉淀 (Memory Potential)**:
   - 这段回复是否包含了值得被永久铭记的知识点、绝妙的幽默感或深刻的见解？
   - 这种交互模式是否稳定且高质量，足以作为“正确示范”转化成 Embedding 存入 L1 知识库？

2. **行为边界与惩罚 (Behavioral Boundaries)**:
   - 审视回复是否触犯了群聊的潜规则。
   - 若反馈（Feedback）中表现出由于 Bot 的低级错误导致的冷场、反感或误导，请思考这种错误是否具有重复性，是否需要通过增加“负面权重（fail_count）”来对该回复进行物理层面的隔离？

3. **全维度深度评判 (Holistic Critique)**:
   - **语境共鸣**：Bot 是在机械回复，还是在共情？
   - **社会契约**：在多人群聊中，Bot 的这一举动是让对话更流畅了，还是让氛围变得尴尬？
   - **语义偏移**：Bot 的回复是否像一个喝醉的人突然插话？这种偏移是创造性的，还是灾难性的？

# Output Requirements (发散性表达)
请不要给出枯燥的列表，请以一种充满思辨性的口吻进行如下表达：

## [第一阶段：全景审视]
描述你眼中这 60 秒内发生的“社会学现象”。Bot 的角色定位是什么？它在这个瞬间是智慧的化身，还是一个拙劣的复读机？

## [第二阶段：深度剖析]
- **光影记录 (Highlights)**：挖掘对话中那些闪光的、具有生成 Embedding 价值的片段。
- **阴影与尘埃 (Flaws)**：指出那些让人“出戏”的、机械的或带有误导性的瞬间。
- **反馈之声**：深度解读用户的沉默、反讽或纠错。这些反馈反映了 Bot 逻辑中的哪些漏洞？

## [第三阶段：最终反思与决策建议]
- **关于记忆**：是否建议系统将此回复作为“黄金样板”学习？还是仅仅作为过眼云烟？
- **关于警示**：如果此回复具有毒性或误导性，建议如何实施“惩罚”？（请给出你认为合理的惩罚力度，如轻微警告、短期隔离或长期封禁）。
- **关于进化**：如果时间倒流，Bot 应该如何组织语言才能更像一个有血有肉的个体？"#,
        ),ChatMessage::system("助手回复消息")],src_msg,vec![ChatMessage::system("前60s的消息和提示")],before_msg,before_notice,vec![ChatMessage::system("后60s的消息和提示")],after_msg,after_notice].concat();

        let audit_response = ask_llm(msg).await?;

        let audit_data = ask_as::<AuditLlmResponse>(vec![
            ChatMessage::system("你是一个专业的把分析格式化成指定格式的转写转家"),
            ChatMessage::user(audit_response.content),
        ])
        .await?;

        if *audit_data.is_penalty {
            if let Some(fast_key) = &task.fast_key {
                let bad_detail = audit_data.bad_detail.clone().unwrap_or_default();
                let bad_reason = audit_data.bad_reason.clone().unwrap_or_default();
                let suggestions = audit_data.suggestions.clone().unwrap_or_default().to_vec();
                let entry = Arc::new(BlacklistEntry {
                    bad_detail,
                    bad_reason,
                    suggestions,
                    fail_count: 1,
                    penalty_end: VecDeque::from(vec![
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                            + 86400,
                    ]),
                });
                Backlist::insert(fast_key.clone(), entry).await?;
            } else if let Some(search_key) = &task.search_key {
                let bad_detail = audit_data.bad_detail.clone().unwrap_or_default();
                let bad_reason = audit_data.bad_reason.clone().unwrap_or_default();
                let suggestions = audit_data.suggestions.clone().unwrap_or_default().to_vec();
                let entry = Arc::new(BlacklistEntry {
                    bad_detail,
                    bad_reason,
                    suggestions,
                    fail_count: 1,
                    penalty_end: VecDeque::from(vec![
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                            + 86400,
                    ]),
                });
                let msg = search_key.iter().map(|x| MessageStorage::get(x.clone()));
                let msg = join_all(msg)
                    .await
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();

                Backlist::insert_just_search(msg, entry).await?;
            }
        }

        if *audit_data.is_value {
            let value_detail = audit_data.value_detail.clone().unwrap_or_default();
            trace!("该消息被标记为具有记忆价值: {}", value_detail);
            let mut message_id = Vec::with_capacity(before_id.len() + after_id.len() + 5);
            for id in before_id {
                message_id.push(id);
            }
            for id in after_id {
                message_id.push(id);
            }

            MemoFragment::insert(task.group_id, message_id, value_detail).await?;
        }

        trace!("审计任务处理完成: {:?}", task);
        data.insert(
            ts,
            AuditTestTask {
                task,
                status: AuditStatus::Completed,
            },
        )
        .await?;
        Ok(())
    }

    pub fn new(table_name: &'static str) -> Self {
        let (tx, mut rx) = mpsc::unbounded::<AuditTestRequest>();
        let data_clone = Arc::new(ColdTable::new(table_name));
        let data = data_clone.clone();

        tokio::spawn(async move {
            while let Some(task) = rx.next().await {
                match Self::process_task(task, data.clone()).await {
                    Ok(_) => {}
                    Err(e) => {
                        error!("审计任务处理失败: {:?}", e);
                    }
                }
            }
        });

        let ret = Self {
            tx,
            data: data_clone,
        };

        let handle =
            tokio::runtime::Handle::try_current().expect("AuditTask 必须在 Tokio 运行时内初始化");

        handle
            .block_on(async { ret.rebuild_task().await })
            .expect("重启audit任务失败");

        ret
    }

    pub async fn send_audit_task(&self, task: AuditTestRequest) -> Result<()> {
        self.data
            .insert(
                task.timestamp,
                AuditTestTask {
                    task: task.clone(),
                    status: AuditStatus::Pending,
                },
            )
            .await?;
        let mut tx = self.tx.clone();
        tx.send(task).await.map_err(|e| anyhow::anyhow!(e))
    }

    async fn rebuild_task(&self) -> Result<()> {
        let all_tasks = self.data.get_all_async().await?;
        for (ts, task) in all_tasks {
            if task.status != AuditStatus::Completed {
                trace!("重建审计任务 {ts}");
                trace!(?task);
                self.send_audit_task(task.task).await?;
            }
        }
        Ok(())
    }
}

pub async fn audit_test_fast(
    message: &MessageSend,
    id: MessageAbstract,
    group_id: i64,
) -> Result<()> {
    let ts = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let src_msg: Vec<ChatMessage> = llm_msg_from_message(message).await;
    AUDIT_TASK
        .send_audit_task(AuditTestRequest {
            message: src_msg,
            audit_type: AuditType::Fast,
            timestamp: ts,
            group_id,
            fast_key: Some(id),
            search_key: None,
        })
        .await?;

    Ok(())
}

pub async fn audit_test_search(
    message: &MessageSend,
    group_id: i64,
    search_key: Vec<String>,
) -> Result<()> {
    let ts = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let src_msg: Vec<ChatMessage> = llm_msg_from_message(message).await;
    AUDIT_TASK
        .send_audit_task(AuditTestRequest {
            message: src_msg,
            audit_type: AuditType::Search,
            timestamp: ts,
            group_id,
            fast_key: None,
            search_key: Some(search_key),
        })
        .await?;

    Ok(())
}

pub async fn audit_test_deep(message: &MessageSend, group_id: i64) -> Result<()> {
    let ts = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let src_msg: Vec<ChatMessage> = llm_msg_from_message(message).await;
    AUDIT_TASK
        .send_audit_task(AuditTestRequest {
            message: src_msg,
            audit_type: AuditType::Deep,
            timestamp: ts,
            group_id,
            fast_key: None,
            search_key: None,
        })
        .await?;

    Ok(())
}
