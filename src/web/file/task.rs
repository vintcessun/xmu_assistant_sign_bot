use crate::{
    api::storage::{FileStorage, TempFile},
    web::{URL, file::expose::ON_QUEUE},
};
use anyhow::Result;
use dashmap::DashMap;
use std::{
    path::PathBuf,
    sync::{Arc, LazyLock},
    time::SystemTime,
};

const EXPIRE_DURATION_SECS: u64 = 60 * 60 * 24; // 1 天

// 内存注册表：持有下载的 TempFile 以保活，任务过期后随之 drop、临时文件被清理。
// 用 DashMap 而非持久化 HotTable —— 临时文件重启即失效，无需落盘。
static DATA: LazyLock<DashMap<String, Arc<ExposeFileList>>> = LazyLock::new(DashMap::new);

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 清理过期任务：移除即触发 Arc→ExposeFileList→TempFile 的 drop，从而清理磁盘临时文件。
fn sweep() {
    let now = now_ts();
    DATA.retain(|_, v| v.expire_at > now);
}

/// 供 expose 展示/下载读取的文件元数据（不含 TempFile 句柄）。
pub struct FileMeta {
    pub path: PathBuf,
    pub mime: String,
}

pub struct ExposeFileList {
    pub files: Vec<FileMeta>,
    /// 保活句柄：与本结构同生命周期，drop 时把临时文件交给延迟清理队列。
    _temps: Vec<TempFile>,
    pub expire_at: u64,
}

pub fn query(id: &String) -> Option<Arc<ExposeFileList>> {
    let list = DATA.get(id)?.clone();
    if list.expire_at <= now_ts() {
        DATA.remove(id);
        return None;
    }
    Some(list)
}

pub struct ExposeFileTask {
    pub id: String,
    temps: Vec<TempFile>,
}

impl ExposeFileTask {
    pub fn new(files: Vec<TempFile>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        ON_QUEUE.insert(id.clone());
        Self { id, temps: files }
    }

    pub async fn finish(self) -> Result<()> {
        sweep(); // 顺手清理过期任务，避免注册表堆积

        let files = self
            .temps
            .iter()
            .map(|t| {
                let path = t.get_path().to_owned();
                let mime = mime_guess::from_path(&path)
                    .first_or_octet_stream()
                    .to_string();
                FileMeta { path, mime }
            })
            .collect();

        let expose_list = ExposeFileList {
            files,
            _temps: self.temps,
            expire_at: now_ts() + EXPIRE_DURATION_SECS,
        };

        ON_QUEUE.remove(&self.id);
        DATA.insert(self.id, Arc::new(expose_list));

        Ok(())
    }

    pub fn get_url(&self) -> String {
        format!("{}/file/task/{}", URL, self.id)
    }
}
