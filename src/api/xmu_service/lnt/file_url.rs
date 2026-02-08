use crate::abi::utils::SmartJsonExt;
use crate::api::{
    network::{SessionClient, download_to_file},
    storage::{ColdTable, File},
};
use anyhow::Result;
use helper::{lnt_get_api, session_client_helper};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};

static FILE_DATA: LazyLock<ColdTable<i64, Arc<File>>> =
    LazyLock::new(|| ColdTable::new("lnt_file_url"));

#[derive(Debug, Serialize, Deserialize)]
pub struct FileUrlResponse {
    pub url: String,
}

#[lnt_get_api(
    FileUrlResponse,
    "https://lnt.xmu.edu.cn/api/uploads/reference/{id:i64}/url"
)]
pub struct FileUrlWithoutDownload;

pub struct FileUrl;

impl FileUrl {
    #[session_client_helper]
    pub async fn get_from_client(
        client: Arc<SessionClient>,
        id: i64,
        filename: &str,
    ) -> Result<Arc<File>> {
        if let Some(file) = FILE_DATA.get_async(&id).await? {
            return Ok(file);
        }

        let url = FileUrlWithoutDownload::get_from_client(&client, id).await?;
        let url = url.url;

        let file = download_to_file(client, &url, filename).await?;
        let file = Arc::new(file);
        FILE_DATA.insert(&id, &file).await?;

        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-2429305-Eve-ZtWBy2QVcUeWcX0uP15HlBl1Dn4omVAXlbn4U6KTC2-tN00wjFwoAp65XLB4jrMnull_main";
        let session = castgc_get_session(castgc).await?;
        let data = FileUrlWithoutDownload::get(&session, 3036828).await?;
        println!("MyCourses: {:?}", data);
        Ok(())
    }
}
