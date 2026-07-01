use crate::abi::utils::SmartJsonExt;
use crate::api::{
    network::{SessionClient, download_to_temp},
    storage::TempFile,
};
use anyhow::Result;
use helper::{lnt_get_api, session_client_helper};
use serde::{Deserialize, Serialize};

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
        client: SessionClient,
        id: i64,
        filename: &str,
    ) -> Result<TempFile> {
        // 不再缓存/持久化：每次下载为临时文件，发送后自动清理，避免 data 目录堆积。
        let url = FileUrlWithoutDownload::get_from_client(&client, id).await?;
        let url = url.url;
        let file = download_to_temp(client, &url, filename).await?;
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
