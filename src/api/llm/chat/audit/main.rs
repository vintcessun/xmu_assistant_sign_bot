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
            tool::{ ask_as},
        },
        storage::ColdTable,
    },
    config::LLM_AUDIT_DURATION_SECS,
};
use anyhow::Result;
use futures::{SinkExt, StreamExt, channel::mpsc, future::join_all};
use genai::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock, atomic::{AtomicU64, Ordering}},
    time::{self, Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::{Instant, sleep, sleep_until};
use tracing::{debug, error, info, trace, warn};
use llm_xml_caster::{llm_prompt};

static AUDIT_TASK: LazyLock<AuditTask> = LazyLock::new(|| AuditTask::new("llm_chat_audit_task_store"));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditType {
    Fast,   //L0: 针对 HotTable 缓存
    Search, //L1: 针对向量检索结果
    Deep,   //L2: 针对 LLM 生成结果
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditTestRequest {  
    pub message: Vec<ChatMessage>,
    pub audit_type: AuditType,
    pub timestamp: u64,
    pub group_id: i64,
    pub fast_key: Option<MessageAbstract>,
    pub search_key: Option<Vec<String>>,
    pub retry_times: AtomicU64,
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
    pub task: Arc<AuditTestRequest>,
    pub status: AuditStatus,
}

pub struct AuditTask {
    pub tx: mpsc::UnboundedSender<Arc<AuditTestRequest>>,
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
            target_timestamp = ?target_timestamp,
            wait_secs = ?wait_secs,
            "正在等待审计任务，将在该 Unix 戳触发..."
        );
        sleep_until(target_instant).await;
    } else {
        trace!(target_timestamp = ?target_timestamp, now_unix = ?now_unix, "审计任务的目标时间戳已过，立即执行");
    }
}

#[llm_prompt]
#[derive(Debug, Clone, Serialize, Deserialize,PartialEq, Eq)]
pub struct AuditLlmResponse {
    #[prompt("该回复是否具有记忆价值，值得被永久铭记？")]
    pub is_value: bool,
    #[prompt("该回复是否触犯了群聊的潜规则，是否需要进行惩罚？")]
    pub is_penalty: Option<bool>,
    #[prompt("负面权重")]
    pub fail_count: Option<u32>,
    #[prompt("如果该回复具有记忆价值，请简要描述其包含的知识点、幽默感或见解")]
    pub value_detail: Option<String>,
    #[prompt("如果该回答具有惩罚性，请描述具体的惩罚细节")]
    pub bad_detail: Option<String>,
    #[prompt("如果该回答具有惩罚性，请描述该回复触犯群聊潜规则的具体原因是什么？")]
    pub bad_reason: Option<String>,
    #[prompt("如果该回答具有惩罚性，请给出改进建议，帮助 Bot 更好地融入群聊氛围，请用换行符分割每个建议")]
    pub suggestions: Option<Vec<String>>,
}

const AUDIT_LLM_RESPONSE_VALID_EXAMPLE: &str = r#"
<AuditLlmResponse>
    <is_value>true</is_value>
    <is_penalty>false</is_penalty>
    <value_detail><![CDATA[该回复包含了深刻的见解，值得被永久铭记]]></value_detail>
    <bad_detail><![CDATA[]]></bad_detail>
    <bad_reason><![CDATA[]]></bad_reason>
    <suggestions>
        <item><![CDATA[建议1]]></item>
        <item><![CDATA[建议2]]></item>
    </suggestions>
</AuditLlmResponse>"#;

#[cfg(test)]
#[test]
fn test_audit_llm_response_parsing() {
    let parsed: AuditLlmResponse = quick_xml::de::from_str(AUDIT_LLM_RESPONSE_VALID_EXAMPLE)
        .expect("Failed to parse AuditLlmResponse");
    assert_eq!(
        parsed,
        AuditLlmResponse {
            is_value: true,
            is_penalty: Some(false),
            value_detail: Some("该回复包含了深刻的见解，值得被永久铭记".to_string()),
            bad_detail: Some("".to_string()),
            bad_reason: Some("".to_string()),
            suggestions: Some(vec!["建议1".to_string(), "建议2".to_string()]),
            fail_count: None,
        }
    );
}

impl AuditTask {
    async fn process_task(
        task: Arc<AuditTestRequest>,
        data: Arc<ColdTable<u64, AuditTestTask>>,
    ) -> Result<()> {
        let ts = task.timestamp;
        info!(timestamp = ?ts, audit_type = ?task.audit_type, "开始处理审计任务");
        let retry_times = task.retry_times.load(Ordering::Relaxed);
        if retry_times > 5{
            error!(timestamp = ?ts, audit_type = ?task.audit_type, retry_times = ?retry_times, "审计任务重试次数过多，放弃处理");
            return Ok(());
        }

        // 1. 记录任务状态为处理中
        if let Err(e) = data
            .insert(
                &ts,
                &AuditTestTask {
                    task: task.clone(),
                    status: AuditStatus::Processing,
                },
            )
            .await
        {
            error!(timestamp = ?ts, error = ?e, "更新审计任务状态为 Processing 失败");
            return Err(e);
        }

        // 2. 等待上下文窗口结束
        let target_delay = ts + LLM_AUDIT_DURATION_SECS + 3;
        sleep_until_unix_timestamp(target_delay).await;

        let src_msg = task.message.clone();
        
        // 3. 获取消息上下文
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
        
        // 4. 构建 LLM 提示词
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
   - **语境共鸣**: Bot 是在机械回复，还是在共情？
   - **社会契约**: 在多人群聊中，Bot 的这一举动是让对话更流畅了，还是让氛围变得尴尬？
   - **语义偏移**: Bot 的回复是否像一个喝醉的人突然插话？这种偏移是创造性的，还是灾难性的？

# Output Requirements (发散性表达)
请不要给出枯燥的列表，请以一种充满思辨性的口吻进行如下表达：

## [第一阶段：全景审视]
描述你眼中这 60 秒内发生的“社会学现象”。Bot 的角色定位是什么？它在这个瞬间是智慧的化身，还是一个拙劣的复读机？

## [第二阶段：深度剖析]
- **光影记录 (Highlights)**: 挖掘对话中那些闪光的、具有生成 Embedding 价值的片段。
- **阴影与尘埃 (Flaws)**: 指出那些让人“出戏”的、机械的或带有误导性的瞬间。
- **反馈之声**: 深度解读用户的沉默、反讽或纠错。这些反馈反映了 Bot 逻辑中的哪些漏洞？

## [第三阶段：最终反思与决策建议]
- **关于记忆**: 是否建议系统将此回复作为“黄金样板”学习？还是仅仅作为过眼云烟？
- **关于警示**: 如果此回复具有毒性或误导性，建议如何实施“惩罚”？（请给出你认为合理的惩罚力度，如轻微警告、短期隔离或长期封禁）。
- **关于进化**: 如果时间倒流，Bot 应该如何组织语言才能更像一个有血有肉的个体？"#,
        ),
            ChatMessage::system("助手回复消息")],src_msg,
            if before_msg.is_empty(){
                vec![ChatMessage::system("前没有消息记录")]
            }else{ 
                [vec![ChatMessage::system("前消息和提示")],before_msg].concat()
            },before_notice,
            vec![ChatMessage::system("后消息和提示")],
            if after_msg.is_empty(){
                vec![ChatMessage::system("后没有消息记录")]
            } else {
                [vec![ChatMessage::system("后消息和提示")], after_msg].concat()
            },after_notice
        ].concat();
        trace!("审计提示词构建完成");

        // 5. 调用 LLM 进行审计
        let audit_response = ask_llm(msg).await.map_err(|e| {
            error!(timestamp = ?ts, error = ?e, "LLM 审计调用失败");
            e
        })?;
        trace!(response = ?audit_response.content, "LLM 审计原始回复");

        // 6. 解析结构化数据
        let audit_data = ask_as::<AuditLlmResponse>(vec![
            ChatMessage::system("你是一个专业的把分析格式化成指定格式的转写转家"),
            ChatMessage::user(audit_response.content),
        ], AUDIT_LLM_RESPONSE_VALID_EXAMPLE)
        .await
        .map_err(|e| {
            error!(timestamp = ?ts, error = ?e, "LLM 审计结果结构化解析失败");
            e
        })?;
        
        debug!(audit_data = ?audit_data, "LLM 审计结果结构化解析成功");

        // 7. 处理惩罚机制 (黑名单)
        if let Some(is_penalty) = audit_data.is_penalty && is_penalty {
            warn!(timestamp = ?ts, audit_type = ?task.audit_type, "审计结果判定为需要惩罚");
            let now_unix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // 惩罚时长：24小时
            let penalty_end_ts = now_unix + 86400;

            if let Some(fast_key) = &task.fast_key {
                let bad_detail = audit_data.bad_detail.clone().unwrap_or_default();
                let bad_reason = audit_data.bad_reason.clone().unwrap_or_default();
                let suggestions = audit_data.suggestions.clone().unwrap_or_default().into_iter().map(|s| s.to_string()).collect::<Vec<_>>();
                let entry = Arc::new(BlacklistEntry {
                    bad_detail,
                    bad_reason,
                    suggestions,
                    fail_count: audit_data.fail_count.unwrap_or(1),
                    penalty_end: VecDeque::from(vec![penalty_end_ts]),
                });
                Backlist::insert(fast_key.clone(), entry).await.map_err(|e| {
                    error!(timestamp = ?ts, fast_key = ?fast_key, error = ?e, "插入快速黑名单失败");
                    e
                })?;
                info!(timestamp = ?ts, fast_key = ?fast_key, penalty_end = ?penalty_end_ts, "快速黑名单记录插入成功");
            } else if let Some(search_key) = &task.search_key {
                let bad_detail = audit_data.bad_detail.clone().unwrap_or_default();
                let bad_reason = audit_data.bad_reason.clone().unwrap_or_default();
                let suggestions = audit_data.suggestions.clone().unwrap_or_default().into_iter().map(|s| s.to_string()).collect::<Vec<_>>();
                let entry = Arc::new(BlacklistEntry {
                    bad_detail,
                    bad_reason,
                    suggestions,
                    fail_count: audit_data.fail_count.unwrap_or(1),
                    penalty_end: VecDeque::from(vec![penalty_end_ts]),
                });
                
                // 收集消息内容
                let msg_futures = search_key.iter().map(MessageStorage::get);
                let msg = join_all(msg_futures)
                    .await
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();

                if msg.is_empty() {
                    //warn!(timestamp = ?ts, "基于搜索键的黑名单插入失败：无法获取任何消息内容");
                } else {
                    Backlist::insert_just_search(msg, entry).await.map_err(|e| {
                        error!(timestamp = ?ts, search_key = ?search_key, error = ?e, "插入向量黑名单失败");
                        e
                    })?;
                    info!(timestamp = ?ts, search_key = ?search_key, penalty_end = ?penalty_end_ts, "向量黑名单记录插入成功");
                }
            }
        } else {
            debug!(timestamp = ?ts, "审计结果判定为无需惩罚");
        }

        // 8. 处理记忆价值 (MemoFragment)
        if audit_data.is_value {
            info!(timestamp = ?ts, "审计结果判定为具有记忆价值");
            let value_detail = audit_data.value_detail.clone().unwrap_or_default();
            trace!(value_detail = %value_detail, "记忆价值详情");
            
            let mut message_id = Vec::with_capacity(before_id.len() + after_id.len() + 5);
            message_id.extend(before_id);
            message_id.extend(after_id);

            MemoFragment::insert(task.group_id, message_id, value_detail).await.map_err(|e| {
                error!(timestamp = ?ts, group_id = ?task.group_id, error = ?e, "插入记忆片段失败");
                e
            })?;
            info!(timestamp = ?ts, group_id = ?task.group_id, "记忆片段插入成功");
        } else {
            debug!(timestamp = ?ts, "审计结果判定为不具有记忆价值");
        }


        // 9. 标记任务完成
        data.insert(
            &ts,
            & AuditTestTask {
                task,
                status: AuditStatus::Completed,
            },
        )
        .await
        .map_err(|e| {
            error!(timestamp = ?ts, error = ?e, "更新审计任务状态为 Completed 失败");
            e
        })?;
        info!(timestamp = ?ts, "审计任务处理完成");
        Ok(())
    }

    pub fn new(table_name: &'static str) -> Self {
        info!(table_name = table_name, "初始化审计任务调度器");
        let (tx, mut rx) = mpsc::unbounded::<Arc<AuditTestRequest>>();
        let data_clone = Arc::new(ColdTable::new(table_name));
        let data = data_clone.clone();

        let mut tx_clone = tx.clone();
        tokio::spawn(async move {
            info!("审计任务处理器启动");
            while let Some(task) = rx.next().await {
                match Self::process_task(task.clone(), data.clone()).await {
                    Ok(_) => {}
                    Err(e) => {
                        task.retry_times.fetch_add(1, Ordering::Relaxed);
                        error!(error = ?e, "审计任务处理失败，将在 60 秒后重试");
                        sleep(Duration::from_secs(60)).await;
                        let res = tx_clone.send(task.clone()).await;
                        if res.is_err() {
                            error!(error = ?res.err(), "审计任务重新入队失败，通道可能已关闭");
                        } else {
                            warn!(task = ?task, "审计任务已重新入队等待重试");
                        }
                    }
                }
            }
            info!("审计任务处理器退出");
        });

        let ret = Self {
            tx,
            data: data_clone,
        };

        if let Err(e) = ret.rebuild_task() {
            error!(error = ?e, "重建未完成的审计任务失败");
            panic!("重启audit任务失败: {:?}", e); // 保持原有 panic 行为，但添加日志
        } else {
            info!("审计任务重建完成");
        }

        ret
    }

    pub async fn send_audit_task(&self, task: Arc<AuditTestRequest>) -> Result<()> {
        info!(timestamp = ?task.timestamp, audit_type = ?task.audit_type, "发送新的审计任务");
        if let Err(e) = self.data
            .insert(
                &task.timestamp,
                &AuditTestTask {
                    task: task.clone(),
                    status: AuditStatus::Pending,
                },
            )
            .await
        {
            error!(timestamp = ?task.timestamp, error = ?e, "保存审计任务到 ColdTable 失败");
            return Err(e);
        }

        let mut tx = self.tx.clone();
        tx.send(task)
            .await
            .map_err(|e| {
                error!(error = ?e, "发送审计任务到处理器通道失败");
                anyhow::anyhow!(e)
            })
    }

    fn rebuild_task(&self) -> Result<()> {
        info!("开始重建未完成的审计任务");
        let all_tasks = match self.data.get_all() {
            Ok(e) => e,
            Err(e) => {
                error!(error = ?e, "获取所有审计任务失败");
                return Err(e);
            }
        };
        
        let mut pending_count = 0;
        for (ts, task) in all_tasks {
            if task.status != AuditStatus::Completed {
                pending_count += 1;
                debug!(timestamp = ts, status = ?task.status, "发现未完成任务，正在重建");

                tokio::spawn(async move {
                    if let Err(e) = AUDIT_TASK.send_audit_task(task.task).await {
                        error!(timestamp = ?ts, error = ?e, "重建审计任务时发送失败");
                    }
                });
            }
        }
        info!(pending_count = ?pending_count, "重建任务已全部发送到队列");
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
    info!(group_id = ?group_id, id = ?id, "接收到快速审计请求");
    let src_msg: Vec<ChatMessage> = llm_msg_from_message(message).await;
    AUDIT_TASK
        .send_audit_task(Arc::new(AuditTestRequest {
            message: src_msg,
            audit_type: AuditType::Fast,
            timestamp: ts,
            group_id,
            fast_key: Some(id),
            search_key: None,
            retry_times: AtomicU64::new(0),
        }))
        .await
        .map_err(|e| {
            error!(group_id = ?group_id, error = ?e, "发送快速审计任务失败");
            e
        })?;

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
    info!(group_id = ?group_id, search_key_count = ?search_key.len(), "接收到搜索审计请求");
    let src_msg: Vec<ChatMessage> = llm_msg_from_message(message).await;
    AUDIT_TASK
        .send_audit_task(Arc::new(AuditTestRequest {
            message: src_msg,
            audit_type: AuditType::Search,
            timestamp: ts,
            group_id,
            fast_key: None,
            search_key: Some(search_key),
            retry_times: AtomicU64::new(0),
        }))
        .await
        .map_err(|e| {
            error!(group_id = ?group_id, error = ?e, "发送搜索审计任务失败");
            e
        })?;

    Ok(())
}

pub async fn audit_test_deep(message: &MessageSend, group_id: i64) -> Result<()> {
    let ts = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    info!(group_id = ?group_id, "接收到深度审计请求");
    let src_msg: Vec<ChatMessage> = llm_msg_from_message(message).await;
    AUDIT_TASK
        .send_audit_task(Arc::new(AuditTestRequest {
            message: src_msg,
            audit_type: AuditType::Deep,
            timestamp: ts,
            group_id,
            fast_key: None,
            search_key: None,
            retry_times: AtomicU64::new(0),
        }))
        .await
        .map_err(|e| {
            error!(group_id = ?group_id, error = ?e, "发送深度审计任务失败");
            e
        })?;

    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "manual"]
    pub async fn clean_audit_list()->Result<()>{
        let all_tasks = AUDIT_TASK.data.get_all()?;
        if all_tasks.is_empty() {
            println!("没有需要清理的审计任务");
            return Ok(());
        }
        for (ts, _) in all_tasks {
            println!("正在清理审计任务，时间戳: {}", ts);
            AUDIT_TASK.data.remove(&ts).await?;
        }

        Ok(())
    }
}