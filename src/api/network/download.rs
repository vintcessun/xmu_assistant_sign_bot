use crate::api::{
    network::SessionClient,
    storage::{File, FileBackend},
};
use anyhow::{Result, bail};
use futures::{FutureExt, future::BoxFuture};
use futures_util::StreamExt;
use std::{path::PathBuf, sync::Arc};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tracing::{debug, error, trace, warn};

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
    debug!(
        url = url,
        filename = filename,
        "开始创建异步下载任务 (Sync 接口)"
    );
    // 1. 准备后端（分配路径并创建占位）
    let backend = T::prepare(filename);
    let path = backend.get_path().clone();
    let path_clone = path.clone();
    let url_clone = url.to_string();

    let future = async move {
        let url = url_clone;
        let path = path_clone;
        debug!(url = %url, "开始获取下载元数据");
        // 2. 获取元数据（复用 SessionClient 自动处理 Cookie）
        let head_resp = client.get(&url).await?;
        let total_size = head_resp.content_length().ok_or_else(|| {
            warn!(url = %url, "无法从响应头获取 Content-Length, 尝试单线程下载");
            anyhow::anyhow!("无法获取 Content-Length")
        })?;
        debug!(
            url = %url,
            file_size = total_size,
            "成功获取文件大小，启动并行下载"
        );

        // 3. 执行 11 协程并行下载
        download_parallel_benchmarked(client, &url, &path, total_size).await?;

        debug!(path = ?path, "下载任务完成");
        Ok::<(), anyhow::Error>(())
    }
    .boxed();

    debug!(path = ?path, "返回 FutureFile 结构体");
    FutureFile { path, future }
}

pub async fn download_to_backend<T: FileBackend>(
    client: Arc<SessionClient>,
    url: &str,
    filename: &str,
) -> Result<T> {
    debug!(
        url = url,
        filename = filename,
        "开始调用下载任务 (Async 接口)"
    );
    // 1. 获取元数据（复用 SessionClient 自动处理 Cookie）
    let head_resp = client.get(url).await?;
    let total_size = head_resp.content_length().ok_or_else(|| {
        warn!(url = url, "无法从响应头获取 Content-Length, 尝试单线程下载");
        anyhow::anyhow!("无法获取 Content-Length")
    })?;

    debug!(
        url = url,
        file_size = total_size,
        "成功获取文件大小，准备后端并启动并行下载"
    );

    // 2. 准备后端（分配路径并创建占位）
    let backend = T::prepare(filename);
    let path = backend.get_path();

    // 3. 执行 11 协程并行下载
    download_parallel_benchmarked(client, url, path, total_size)
        .await
        .map_err(|e| {
            error!(
                url = url,
                path = %path.display(),
                error = ?e,
                "并行下载失败"
            );
            e
        })?;

    debug!(path = %path.display(), "下载任务成功完成");
    Ok(backend)
}

async fn download_parallel_benchmarked(
    client: Arc<SessionClient>,
    url: &str,
    path: &std::path::Path,
    total_size: u64,
) -> Result<()> {
    debug!(
        url = url,
        path = %path.display(),
        total_size = total_size,
        "开始执行并行下载"
    );
    if total_size == 0 {
        // 确保文件存在且长度为 0
        let _ = tokio::fs::File::create(path).await.map_err(|e| {
            error!(path = %path.display(), error = ?e, "创建零大小文件失败");
            e
        })?;
        debug!(path = %path.display(), "文件大小为 0，跳过下载，创建空文件");
        return Ok(());
    }

    // 预分配磁盘空间，减少 metadata 更新频率
    let f_placeholder = tokio::fs::File::create(path).await.map_err(|e| {
        error!(path = %path.display(), error = ?e, "创建占位文件失败");
        e
    })?;
    f_placeholder.set_len(total_size).await.map_err(|e| {
        error!(path = %path.display(), error = ?e, "预分配文件空间失败");
        e
    })?;
    drop(f_placeholder);
    debug!(path = %path.display(), file_size = total_size, "磁盘空间预分配完成");

    let base_chunk_size = total_size / OPTIMAL_CHUNKS;

    let (num_chunks, chunk_size_for_loop) = if base_chunk_size == 0 {
        // 如果 total_size 小于 OPTIMAL_CHUNKS，只使用一个 chunk 以避免 0 - 1 溢出。
        (1, total_size)
    } else {
        (OPTIMAL_CHUNKS, base_chunk_size)
    };

    debug!(
        url = url,
        num_chunks = num_chunks,
        chunk_size_for_loop = chunk_size_for_loop,
        "计算分块完成，启动 {} 个下载任务",
        num_chunks
    );

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
            trace!(chunk_index = i, start = start, end = end, "开始下载分块");
            let resp = c.get_range(&u, start, end).await.map_err(|e| {
                error!(url = %u, chunk_index = i, error = ?e, "分块网络请求失败");
                e
            })?;
            let mut stream = resp.bytes_stream();

            let mut f = tokio::fs::OpenOptions::new()
                .write(true)
                .open(p.clone())
                .await
                .map_err(|e| {
                    error!(path = ?p, error = ?e, "打开文件进行写入失败");
                    e
                })?;

            f.seek(std::io::SeekFrom::Start(start)).await.map_err(|e| {
                error!(path = ?p, start = start, error = ?e, "文件 Seek 失败");
                e
            })?;

            let mut bytes_written = 0;
            while let Some(item) = stream.next().await {
                let chunk = item.map_err(|e| {
                    error!(url = %u, chunk_index = i, error = ?e, "接收分块数据流失败");
                    e
                })?;
                f.write_all(&chunk).await.map_err(|e| {
                    error!(path = ?p, error = ?e, "写入分块数据到文件失败");
                    e
                })?;
                bytes_written += chunk.len();
            }
            f.flush().await.map_err(|e| {
                error!(path = ?p, error = ?e, "文件 Flush 失败");
                e
            })?;
            trace!(
                chunk_index = i,
                bytes_written = bytes_written,
                "分块下载完成"
            );
            Ok::<(), anyhow::Error>(())
        }));
    }

    for (i, res) in futures_util::future::join_all(tasks)
        .await
        .into_iter()
        .enumerate()
    {
        if let Err(e) = res {
            error!(chunk_index = i, error = ?e, "下载任务在 tokio 运行时内失败");
            bail!("下载任务失败");
        }
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
