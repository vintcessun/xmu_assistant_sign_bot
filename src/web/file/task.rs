use crate::{
    api::storage::{self, FileStorage, HotTable},
    web::{URL, file::expose::ON_QUEUE},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, LazyLock},
    time::SystemTime,
};
static DATA: LazyLock<HotTable<String, ExposeFileList>> = LazyLock::new(|| HotTable::new("file"));

pub fn query(id: &String) -> Option<Arc<ExposeFileList>> {
    DATA.get(id)
}

const EXPIRE_DURATION_SECS: u64 = 60 * 60 * 24; // 1 天

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct File {
    pub path: PathBuf,
    pub mime: String,
    pub is_temp: bool,
    pub expire_at: u64,
}

impl File {
    pub fn new<T: FileStorage>(file: &T) -> Self {
        let mime = mime_guess::from_path(file.get_path())
            .first_or_octet_stream()
            .to_string();
        Self {
            path: file.get_path().to_owned(),
            mime,
            is_temp: file.is_temp(),
            expire_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + EXPIRE_DURATION_SECS,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExposeFileList {
    pub files: Vec<File>,
    pub expire_at: u64,
}

impl ExposeFileList {
    pub fn new(files: Vec<File>) -> Self {
        Self {
            files,
            expire_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + EXPIRE_DURATION_SECS,
        }
    }
}

pub struct ExposeFileTask {
    pub id: String,
    pub list: Vec<File>,
}

impl ExposeFileTask {
    pub fn new(files: Vec<Arc<storage::File>>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        ON_QUEUE.insert(id.clone());
        let mut list = Vec::with_capacity(files.len());
        for file in files {
            list.push(File::new(&*file));
        }

        Self { id, list }
    }

    pub async fn finish(self) -> Result<()> {
        // 1. 构造 ExposeFileList
        let expose_list = ExposeFileList::new(self.list);

        // 2. 生成唯一 ID 并写入 HotTable
        ON_QUEUE.remove(&self.id);
        DATA.insert(self.id, Arc::new(expose_list))?;

        Ok(())
    }

    pub fn get_url(&self) -> String {
        format!("{}/file/task/{}", URL, self.id)
    }
}
