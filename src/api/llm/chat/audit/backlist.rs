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
        if let Some(uuid) = BACKLIST_DB.get_async(key).await.unwrap_or_default()
            && let Some(ret_search) = BACKLIST_SEARCH.get(uuid).await
            && let Some(top) = ret_search.entry.penalty_end.front()
        {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if *top < now {
                let mut ret = ret_search.entry.as_ref().clone();
                ret.penalty_end.pop_front();
                ret.fail_count -= 1;
                let new_search = BlacklistSearch {
                    embedding: ret_search.embedding.clone(),
                    entry: Arc::new(ret),
                };
                let _ = BACKLIST_SEARCH.insert(Arc::new(new_search)).await;
            }
        }
    }

    fn new() -> Self {
        let (tx, mut rx) = mpsc::unbounded::<MessageAbstract>();
        tokio::spawn(async move {
            while let Some(key) = rx.next().await {
                BacklistRemove::try_remove(key).await;
            }
        });
        Self { tx }
    }

    pub async fn send(key: MessageAbstract) {
        let mut tx = REMOVE.tx.clone();
        let _ = tx.send(key).await;
    }
}

pub struct Backlist;

impl Backlist {
    pub async fn search(
        key: Vec<f32>,
        top_k: usize,
    ) -> anyhow::Result<Vec<(Uuid, Arc<BlacklistSearch>)>> {
        BACKLIST_SEARCH.search(key, top_k).await
    }

    pub async fn insert(key: MessageAbstract, entry: Arc<BlacklistEntry>) -> Result<()> {
        if let Some(uuid) = BACKLIST_DB.get_async(key.clone()).await.unwrap_or_default()
            && let Some(old_search) = BACKLIST_SEARCH.get(uuid).await
        {
            let mut new_entry = old_search.entry.as_ref().clone();
            new_entry.fail_count += entry.fail_count;
            for ts in &entry.penalty_end {
                new_entry.penalty_end.push_back(*ts);
            }
            let new_search = BlacklistSearch {
                embedding: old_search.embedding.clone(),
                entry: Arc::new(new_entry),
            };
            BACKLIST_SEARCH.insert(Arc::new(new_search)).await?;
            return Ok(());
        }

        let uuid = BACKLIST_SEARCH
            .insert(Arc::new(BlacklistSearch {
                embedding: get_single_text_embedding(format!(
                    "不良内容详情: {}\n不良内容原因: {}\n改进建议: {:?}",
                    entry.bad_detail, entry.bad_reason, entry.suggestions
                ))
                .await?,
                entry,
            }))
            .await?;

        BACKLIST_DB.insert(key, uuid).await?;

        Ok(())
    }

    pub async fn get(key: MessageAbstract) -> Option<Arc<BlacklistEntry>> {
        let ret = if let Some(uuid) = BACKLIST_DB.get_async(key.clone()).await.unwrap_or_default()
            && let Some(ret_search) = BACKLIST_SEARCH.get(uuid).await
            && ret_search.entry.fail_count > 0
        {
            Some(ret_search.entry.clone())
        } else {
            None
        };

        BacklistRemove::send(key).await;

        ret
    }

    pub async fn insert_just_search(
        key: Vec<ChatMessage>,
        entry: Arc<BlacklistEntry>,
    ) -> Result<()> {
        let _ = BACKLIST_SEARCH
            .insert(Arc::new(BlacklistSearch {
                embedding: get_chat_embedding(
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
                .await?,
                entry,
            }))
            .await?;

        Ok(())
    }
}
