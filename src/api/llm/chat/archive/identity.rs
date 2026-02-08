use crate::api::storage::ColdTable;
use ahash::{HashMap, HashMapExt};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tokio::sync::mpsc;
use tracing::{error, info, trace};

static IDENTITY_PERSON_DB: LazyLock<ColdTable<i64, PersonIdentityInfo>> =
    LazyLock::new(|| ColdTable::new("llm_chat_identity_person"));

static IDENTITY_GROUP_DB: LazyLock<ColdTable<i64, GroupIdentityInfo>> =
    LazyLock::new(|| ColdTable::new("llm_chat_identity_group"));

static UPDATE: LazyLock<IdentityUpdate> = LazyLock::new(IdentityUpdate::new);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityPersonUpdateSend {
    pub qq: i64,
    pub group_id: Option<i64>,
    pub now_nickname: String,
    pub now_group_nickname: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityGroupUpdateSend {
    pub group_id: i64,
    pub now_name: String,
}

pub struct IdentityUpdate {
    pub person_channel: mpsc::UnboundedSender<IdentityPersonUpdateSend>,
    pub group_channel: mpsc::UnboundedSender<IdentityGroupUpdateSend>,
}

impl Default for IdentityUpdate {
    fn default() -> Self {
        IdentityUpdate::new()
    }
}

impl IdentityUpdate {
    pub fn person_update(person: IdentityPersonUpdateSend) {
        let send = person;
        trace!(qq = ?send.qq, "发送个人身份信息更新请求");
        if let Err(e) = UPDATE.person_channel.send(send) {
            error!(error = ?e, "个人身份更新通道已关闭");
        }
    }

    pub fn group_update(group: IdentityGroupUpdateSend) {
        let send = group;
        trace!(group_id = ?send.group_id, "发送群身份信息更新请求");
        if let Err(e) = UPDATE.group_channel.send(send) {
            error!(error = ?e, "群身份更新通道已关闭");
        }
    }

    pub fn new() -> Self {
        let (tx_person, mut rx_person) = mpsc::unbounded_channel::<IdentityPersonUpdateSend>();
        info!("启动个人身份信息更新后台任务");
        tokio::spawn(async move {
            while let Some(update) = rx_person.recv().await {
                let identity = IDENTITY_PERSON_DB
                    .get_async(update.qq)
                    .await
                    .unwrap_or_else(|e| {
                        error!(qq = ?update.qq, error = ?e, "获取个人身份信息失败，使用 None");
                        None
                    });

                let new_identity = match identity {
                    None => PersonIdentityInfo {
                        id: update.qq,
                        now_nickname: Name {
                            now: update.now_nickname.clone(),
                            used: vec![update.now_nickname.clone()],
                        },
                        group_nickname: match update.now_group_nickname {
                            Some(nick) => {
                                let mut map = HashMap::new();
                                map.insert(
                                    update.group_id.unwrap_or_default(),
                                    Name {
                                        now: nick.clone(),
                                        used: vec![nick],
                                    },
                                );
                                map
                            }
                            None => HashMap::new(),
                        },
                    },
                    Some(mut e) => {
                        let now_name = &e.now_nickname.now;
                        if now_name != &update.now_nickname {
                            let mut used = e.now_nickname.used.clone();
                            if !used.contains(&update.now_nickname) {
                                used.push(update.now_nickname.clone());
                            }
                            e.now_nickname.now = update.now_nickname.clone();
                            e.now_nickname.used = used;
                        }
                        if let Some(group_nick) = update.now_group_nickname {
                            let group_id = update.group_id.unwrap_or_default();
                            let group_name_entry = e.group_nickname.get_mut(&group_id);
                            match group_name_entry {
                                Some(name) => {
                                    if name.now != group_nick {
                                        let mut used = name.used.clone();
                                        if !used.contains(&group_nick) {
                                            used.push(group_nick.clone());
                                        }
                                        name.now = group_nick;
                                        name.used = used;
                                    }
                                }
                                None => {
                                    e.group_nickname.insert(
                                        group_id,
                                        Name {
                                            now: group_nick.clone(),
                                            used: vec![group_nick],
                                        },
                                    );
                                }
                            }
                        }
                        e
                    }
                };
                trace!(qq = ?update.qq, "正在更新个人身份信息到数据库");
                if let Err(e) = IDENTITY_PERSON_DB.insert(update.qq, new_identity).await {
                    error!(qq = ?update.qq, error = ?e, "个人身份信息插入/更新失败");
                }
            }
            info!("个人身份信息更新后台任务退出");
        });

        let (tx_group, mut rx_group) = mpsc::unbounded_channel::<IdentityGroupUpdateSend>();
        info!("启动群身份信息更新后台任务");
        tokio::spawn(async move {
            while let Some(update) = rx_group.recv().await {
                let identity = IDENTITY_GROUP_DB
                    .get_async(update.group_id)
                    .await
                    .unwrap_or_else(|e| {
                        error!(group_id = ?update.group_id, error = ?e, "获取群身份信息失败，使用 None");
                        None
                    });

                let new_identity = match identity {
                    None => GroupIdentityInfo {
                        id: update.group_id,
                        now_name: Name {
                            now: update.now_name.clone(),
                            used: vec![update.now_name.clone()],
                        },
                    },
                    Some(mut e) => {
                        let now_name = &e.now_name.now;
                        if now_name != &update.now_name {
                            let mut used = e.now_name.used.clone();
                            if !used.contains(&update.now_name) {
                                used.push(update.now_name.clone());
                            }
                            e.now_name.now = update.now_name.clone();
                            e.now_name.used = used;
                        }
                        e
                    }
                };
                trace!(group_id = ?update.group_id, "正在更新群身份信息到数据库");
                if let Err(e) = IDENTITY_GROUP_DB
                    .insert(update.group_id, new_identity)
                    .await
                {
                    error!(group_id = ?update.group_id, error = ?e, "群身份信息插入/更新失败");
                }
            }
            info!("群身份信息更新后台任务退出");
        });
        Self {
            person_channel: tx_person,
            group_channel: tx_group,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Name {
    pub now: String,
    pub used: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonIdentityInfo {
    pub id: i64,
    pub now_nickname: Name,
    pub group_nickname: HashMap<i64, Name>, //group_id -> nickname
}

pub struct IdentityPerson;

impl IdentityPerson {
    pub async fn get(qq: i64) -> Option<PersonIdentityInfo> {
        IDENTITY_PERSON_DB.get_async(qq).await.unwrap_or_else(|e| {
            error!(qq = ?qq, error = ?e, "获取个人身份信息失败，返回 None");
            None
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupIdentityInfo {
    pub id: i64,
    pub now_name: Name,
}

pub struct IdentityGroup;

impl IdentityGroup {
    pub async fn get(group_id: i64) -> Option<GroupIdentityInfo> {
        IDENTITY_GROUP_DB
            .get_async(group_id)
            .await
            .unwrap_or_else(|e| {
                error!(group_id = ?group_id, error = ?e, "获取群身份信息失败，返回 None");
                None
            })
    }
}
