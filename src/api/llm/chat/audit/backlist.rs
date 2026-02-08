use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock},
    time::SystemTime,
};

use anyhow::Result;
use futures::{SinkExt, StreamExt, channel::mpsc};
use genai::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::{
    llm::chat::{
        llm::{get_chat_embedding, get_single_text_embedding},
        repeat::reply::MessageAbstract,
    },
    storage::{ColdTable, HasEmbedding, VectorSearchEngine},
};
use tracing::{debug, error, info, trace};

static BACKLIST_DB: LazyLock<ColdTable<MessageAbstract, Uuid>> =
    LazyLock::new(|| ColdTable::new("llm_chat_audit_blacklist"));

static BACKLIST_SEARCH: LazyLock<VectorSearchEngine<BlacklistSearch>> =
    LazyLock::new(|| VectorSearchEngine::new("llm_chat_audit_blacklist_vector"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistEntry {
    pub bad_detail: String,
    pub bad_reason: String,
    pub suggestions: Vec<String>,

    pub fail_count: u32,
    pub penalty_end: VecDeque<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistSearch {
    pub entry: Arc<BlacklistEntry>,
    pub embedding: Vec<f32>,
}

impl HasEmbedding for BlacklistSearch {
    fn get_embedding(&self) -> &[f32] {
        &self.embedding
    }
}

static REMOVE: LazyLock<BacklistRemove> = LazyLock::new(BacklistRemove::new);

pub struct BacklistRemove {
    pub tx: mpsc::UnboundedSender<MessageAbstract>,
}

impl BacklistRemove {
    async fn try_remove(key: MessageAbstract) {
        let uuid = BACKLIST_DB.get_async(&key).await.unwrap_or_else(|e| {
            error!(key = ?key, error = ?e, "获取黑名单键对应的 UUID 失败");
            None
        });

        if let Some(uuid) = uuid
            && let Some(ret_search) = BACKLIST_SEARCH.get(uuid).await
            && let Some(top) = ret_search.entry.penalty_end.front()
        {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_else(|e| {
                    error!(error = ?e, "获取系统时间失败");
                    std::time::Duration::from_secs(0)
                })
                .as_secs();

            if *top < now {
                info!(key = ?key, penalty_end = ?top, now = ?now, "黑名单惩罚时间已结束，尝试移除一个惩罚记录");
                let mut ret = ret_search.entry.as_ref().clone();
                ret.penalty_end.pop_front();
                ret.fail_count -= 1;
                let new_search = BlacklistSearch {
                    embedding: ret_search.embedding.clone(),
                    entry: Arc::new(ret),
                };
                if let Err(e) = BACKLIST_SEARCH.insert(Arc::new(new_search)).await {
                    error!(key = ?key, error = ?e, "更新黑名单搜索记录失败");
                } else {
                    debug!(key = ?key, "黑名单惩罚记录更新成功");
                }
            } else {
                trace!(key = ?key, penalty_end = ?top, now = ?now, "黑名单惩罚时间未到期");
            }
        } else {
            trace!(key = ?key, "黑名单记录不存在或获取失败");
        }
    }

    fn new() -> Self {
        let (tx, mut rx) = mpsc::unbounded::<MessageAbstract>();
        info!("启动黑名单过期移除后台任务");
        tokio::spawn(async move {
            while let Some(key) = rx.next().await {
                BacklistRemove::try_remove(key).await;
            }
            info!("黑名单过期移除后台任务退出");
        });
        Self { tx }
    }

    pub async fn send(key: MessageAbstract) {
        trace!(key = ?key, "发送黑名单移除检查请求");
        let mut tx = REMOVE.tx.clone();
        if let Err(e) = tx.send(key).await {
            error!(error = ?e, "黑名单移除检查请求发送失败，通道可能已关闭");
        }
    }
}

pub struct Backlist;

impl Backlist {
    pub async fn search(
        key: &[f32],
        top_k: usize,
    ) -> anyhow::Result<Vec<(Uuid, Arc<BlacklistSearch>)>> {
        info!(key_len = ?key.len(), top_k = ?top_k, "开始搜索黑名单");
        let results = BACKLIST_SEARCH.search(key, top_k).await.map_err(|e| {
            error!(error = ?e, "黑名单向量搜索失败");
            e
        })?;
        info!(result_count = ?results.len(), "黑名单搜索完成");
        Ok(results)
    }

    pub async fn insert(key: MessageAbstract, entry: Arc<BlacklistEntry>) -> Result<()> {
        info!(key = ?key, "开始插入黑名单记录");

        let existing_uuid = BACKLIST_DB.get_async(&key).await.unwrap_or_else(|e| {
            error!(key = ?key, error = ?e, "获取现有黑名单 UUID 失败");
            None
        });

        if let Some(uuid) = existing_uuid
            && let Some(old_search) = BACKLIST_SEARCH.get(uuid).await
        {
            debug!(key = ?key, uuid = ?uuid, "黑名单记录已存在，正在合并惩罚信息");
            let mut new_entry = old_search.entry.as_ref().clone();
            new_entry.fail_count += entry.fail_count;
            for ts in &entry.penalty_end {
                new_entry.penalty_end.push_back(*ts);
            }
            let new_search = BlacklistSearch {
                embedding: old_search.embedding.clone(),
                entry: Arc::new(new_entry),
            };
            BACKLIST_SEARCH
                .insert(Arc::new(new_search))
                .await
                .map_err(|e| {
                    error!(key = ?key, error = ?e, "更新黑名单搜索记录失败");
                    e
                })?;
            info!(key = ?key, "黑名单记录合并更新成功");
            return Ok(());
        }

        debug!(key = ?key, "黑名单记录不存在，正在创建新记录");
        let embedding = get_single_text_embedding(format!(
            "不良内容详情: {}\n不良内容原因: {}\n改进建议: {:?}",
            entry.bad_detail, entry.bad_reason, entry.suggestions
        ))
        .await
        .map_err(|e| {
            error!(key = ?key, error = ?e, "生成黑名单文本嵌入失败");
            e
        })?;

        let uuid = BACKLIST_SEARCH
            .insert(Arc::new(BlacklistSearch { embedding, entry }))
            .await
            .map_err(|e| {
                error!(key = ?key, error = ?e, "插入黑名单向量数据库失败");
                e
            })?;

        BACKLIST_DB.insert(&key, &uuid).await.map_err(|e| {
            error!(key = ?key, error = ?e, "插入黑名单键值数据库失败");
            e
        })?;

        info!(key = ?key, "黑名单记录插入成功");
        Ok(())
    }

    pub async fn get(key: MessageAbstract) -> Option<Arc<BlacklistEntry>> {
        trace!(key = ?key, "尝试获取黑名单记录");
        let uuid = BACKLIST_DB.get_async(&key).await.unwrap_or_else(|e| {
            error!(key = ?key, error = ?e, "获取黑名单键对应的 UUID 失败");
            None
        });

        let ret = if let Some(uuid) = uuid
            && let Some(ret_search) = BACKLIST_SEARCH.get(uuid).await
            && ret_search.entry.fail_count > 0
        {
            debug!(key = ?key, fail_count = ?ret_search.entry.fail_count, "找到活跃的黑名单记录");
            Some(ret_search.entry.clone())
        } else {
            debug!(key = ?key, "未找到活跃的黑名单记录");
            None
        };

        BacklistRemove::send(key).await;

        ret
    }

    pub async fn insert_just_search(
        key: Vec<ChatMessage>,
        entry: Arc<BlacklistEntry>,
    ) -> Result<()> {
        info!("开始仅插入黑名单搜索向量");
        let embedding = get_chat_embedding(
            [
                vec![
                    ChatMessage::system(format!(
                        "不良内容详情: {}\n不良内容原因: {}\n改进建议: {:?}",
                        entry.bad_detail, entry.bad_reason, entry.suggestions
                    )),
                    ChatMessage::system("以下是相关消息内容:"),
                ],
                key,
            ]
            .concat(),
        )
        .await
        .map_err(|e| {
            error!(error = ?e, "生成黑名单聊天记录嵌入失败");
            e
        })?;

        if let Err(e) = BACKLIST_SEARCH
            .insert(Arc::new(BlacklistSearch { embedding, entry }))
            .await
        {
            error!(error = ?e, "插入黑名单搜索向量数据库失败");
            return Err(e);
        }

        info!("黑名单搜索向量插入成功");
        Ok(())
    }
}
