use crate::api::{
    network::SessionClient,
    storage::{File, FileBackend},
};
use anyhow::Result;
use futures::{FutureExt, future::BoxFuture};
use futures_util::StreamExt;
use std::{path::PathBuf, sync::Arc};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

/// 根据 2026-01-09 最新 Bench 结果（详见 session.rs 的 test）：
/// 11 个分块在 87MB 大文件上表现最优 (5.09s)，在小文件上也能维持在 500ms 左右。
const OPTIMAL_CHUNKS: u64 = 11;

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
    // 预分配磁盘空间，减少 metadata 更新频率
    let f_placeholder = tokio::fs::File::create(path).await?;
    f_placeholder.set_len(total_size).await?;
    drop(f_placeholder);

    let chunk_size = total_size / OPTIMAL_CHUNKS;
    let mut tasks = Vec::with_capacity(OPTIMAL_CHUNKS as usize);

    for i in 0..OPTIMAL_CHUNKS {
        let start = i * chunk_size;
        let end = if i == OPTIMAL_CHUNKS - 1 {
            total_size - 1
        } else {
            (i + 1) * chunk_size - 1
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
