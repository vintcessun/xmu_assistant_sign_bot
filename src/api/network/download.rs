use crate::api::{
    network::SessionClient,
    storage::{File, FileBackend},
};
use anyhow::Result;
use futures::{FutureExt, future::BoxFuture};
use futures_util::StreamExt;
use std::{path::PathBuf, sync::Arc};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

const OPTIMAL_CHUNKS: u64 = 1; //为了稳定性，先固定为1，后续可以根据实际情况调整

pub async fn download_to_file(
    client: Arc<SessionClient>,
    url: &str,
    filename: &str,
) -> Result<File> {
    download_to_backend::<File>(client, url, filename).await
}

pub struct FutureFile {
    pub path: PathBuf,
    pub future: BoxFuture<'static, Result<()>>,
}

pub fn download_to_file_sync(client: Arc<SessionClient>, url: &str, filename: &str) -> FutureFile {
    download_to_backend_sync::<File>(client, url, filename)
}

pub fn download_to_backend_sync<T: FileBackend>(
    client: Arc<SessionClient>,
    url: &str,
    filename: &str,
) -> FutureFile {
    // 1. 准备后端（分配路径并创建占位）
    let backend = T::prepare(filename);
    let path = backend.get_path().clone();
    let path_clone = path.clone();
    let url_clone = url.to_string();

    let future = async move {
        let url = url_clone;
        let path = path_clone;
        // 2. 获取元数据（复用 SessionClient 自动处理 Cookie）
        let head_resp = client.get(&url).await?;
        let total_size = head_resp
            .content_length()
            .ok_or_else(|| anyhow::anyhow!("无法获取 Content-Length"))?;

        // 3. 执行 11 协程并行下载
        download_parallel_benchmarked(client, &url, &path, total_size).await?;

        Ok::<(), anyhow::Error>(())
    }
    .boxed();

    FutureFile { path, future }
}

pub async fn download_to_backend<T: FileBackend>(
    client: Arc<SessionClient>,
    url: &str,
    filename: &str,
) -> Result<T> {
    // 1. 获取元数据（复用 SessionClient 自动处理 Cookie）
    let head_resp = client.get(url).await?;
    let total_size = head_resp
        .content_length()
        .ok_or_else(|| anyhow::anyhow!("无法获取 Content-Length"))?;

    // 2. 准备后端（分配路径并创建占位）
    let backend = T::prepare(filename);
    let path = backend.get_path();

    // 3. 执行 11 协程并行下载
    download_parallel_benchmarked(client, url, path, total_size).await?;

    Ok(backend)
}

async fn download_parallel_benchmarked(
    client: Arc<SessionClient>,
    url: &str,
    path: &std::path::Path,
    total_size: u64,
) -> Result<()> {
    if total_size == 0 {
        // 确保文件存在且长度为 0
        let _ = tokio::fs::File::create(path).await?;
        return Ok(());
    }

    // 预分配磁盘空间，减少 metadata 更新频率
    let f_placeholder = tokio::fs::File::create(path).await?;
    f_placeholder.set_len(total_size).await?;
    drop(f_placeholder);

    let base_chunk_size = total_size / OPTIMAL_CHUNKS;

    let (num_chunks, chunk_size_for_loop) = if base_chunk_size == 0 {
        // 如果 total_size 小于 OPTIMAL_CHUNKS，只使用一个 chunk 以避免 0 - 1 溢出。
        (1, total_size)
    } else {
        (OPTIMAL_CHUNKS, base_chunk_size)
    };

    let mut tasks = Vec::with_capacity(num_chunks as usize);

    for i in 0..num_chunks {
        let start = i * chunk_size_for_loop;
        let end = if i == num_chunks - 1 {
            total_size - 1
        } else {
            (i + 1) * chunk_size_for_loop - 1
        };

        let c = client.clone();
        let u = url.to_string();
        let p = path.to_path_buf();

        tasks.push(tokio::spawn(async move {
            let resp = c.get_range(&u, start, end).await?;
            let mut stream = resp.bytes_stream();

            let mut f = tokio::fs::OpenOptions::new().write(true).open(p).await?;

            f.seek(std::io::SeekFrom::Start(start)).await?;

            while let Some(item) = stream.next().await {
                f.write_all(&item?).await?;
            }
            f.flush().await?;
            Ok::<(), anyhow::Error>(())
        }));
    }

    for res in futures_util::future::join_all(tasks).await {
        res??;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_download() -> Result<()> {
        let client = Arc::new(SessionClient::new());
        let url = "https://download.samplelib.com/png/sample-boat-400x300.png";
        let filename = "sample-boat-400x300.png";
        let file = download_to_file(client, url, filename).await?;
        println!("Downloaded file at path: {:?}", file.path);
        Ok(())
    }
}
