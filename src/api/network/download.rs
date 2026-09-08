use crate::api::{
    network::SessionClient,
    storage::{FileBackend, TempFile},
};
use anyhow::{Result, bail};
use futures::{FutureExt, future::BoxFuture};
use futures_util::StreamExt;
use std::path::PathBuf;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tracing::{debug, error, trace, warn};

const OPTIMAL_CHUNKS: u64 = 1; //为了稳定性，先固定为1，后续可以根据实际情况调整

fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim();
    if stem.is_empty() {
        return false;
    }
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn escape_filename_for_path(filename: &str) -> String {
    let leaf = std::path::Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(filename)
        .trim();

    let source = if leaf.is_empty() { "file" } else { leaf };
    let mut escaped = String::with_capacity(source.len());

    for ch in source.chars() {
        let invalid =
            ch.is_control() || matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*');
        if invalid {
            let mut buf = [0_u8; 4];
            for b in ch.encode_utf8(&mut buf).as_bytes() {
                escaped.push('%');
                escaped.push_str(&format!("{:02X}", b));
            }
        } else {
            escaped.push(ch);
        }
    }

    let escaped = escaped.trim_end_matches([' ', '.']);
    let escaped = if escaped.is_empty() { "file" } else { escaped };

    if is_windows_reserved_name(escaped) {
        format!("_{}", escaped)
    } else {
        escaped.to_string()
    }
}

/// 下载为临时文件（用完 drop 后自动清理），不再持久化到 data/file。
pub async fn download_to_temp(client: SessionClient, url: &str, filename: &str) -> Result<TempFile> {
    download_to_backend::<TempFile>(client, url, filename).await
}

pub struct FutureFile {
    pub path: PathBuf,
    pub future: BoxFuture<'static, Result<()>>,
}

pub fn download_to_temp_sync(client: SessionClient, url: &str, filename: &str) -> FutureFile {
    download_to_backend_sync::<TempFile>(client, url, filename)
}

pub fn download_to_backend_sync<T: FileBackend + 'static>(
    client: SessionClient,
    url: &str,
    filename: &str,
) -> FutureFile {
    let safe_filename = escape_filename_for_path(filename);
    if safe_filename != filename {
        warn!(
            original_filename = filename,
            safe_filename = safe_filename,
            "下载文件名包含非法路径字符，已自动转义"
        );
    }
    debug!(
        url = url,
        filename = safe_filename,
        "开始创建异步下载任务 (Sync 接口)"
    );
    // 1. 准备后端（分配路径并创建占位）
    let backend = T::prepare(&safe_filename);
    let path = backend.get_path().clone();
    let path_clone = path.clone();
    let url_clone = url.to_string();

    let future = async move {
        let url = url_clone;
        let path = path_clone;
        debug!(url = %url, "开始获取下载元数据");
        // 2. 获取元数据（复用 SessionClient 自动处理 Cookie）
        match probe_total_size(&client, &url).await? {
            SizeProbe::Known(total_size) => {
                debug!(
                    url = %url,
                    file_size = total_size,
                    "成功获取文件大小，启动并行下载"
                );
                // 3. 执行分块并行下载
                download_parallel_benchmarked(client, &url, &path, total_size).await?;
            }
            SizeProbe::Stream(resp) => {
                // 3'. 拿不到长度：手上这个响应就是完整文件，直接流式落盘
                download_single_stream(resp, &path).await?;
            }
        }

        debug!(path = ?path, "下载任务完成");
        // 保活后端到下载完成：TempFile 在此 drop 后延迟清理，File 无副作用。
        drop(backend);
        Ok::<(), anyhow::Error>(())
    }
    .boxed();

    debug!(path = ?path, "返回 FutureFile 结构体");
    FutureFile { path, future }
}

pub async fn download_to_backend<T: FileBackend>(
    client: SessionClient,
    url: &str,
    filename: &str,
) -> Result<T> {
    let safe_filename = escape_filename_for_path(filename);
    if safe_filename != filename {
        warn!(
            original_filename = filename,
            safe_filename = safe_filename,
            "下载文件名包含非法路径字符，已自动转义"
        );
    }
    debug!(
        url = url,
        filename = safe_filename,
        "开始调用下载任务 (Async 接口)"
    );
    // 1. 获取元数据（复用 SessionClient 自动处理 Cookie）
    let probe = probe_total_size(&client, url).await?;

    // 2. 准备后端（分配路径并创建占位）
    let backend = T::prepare(&safe_filename);
    let path = backend.get_path();

    match probe {
        SizeProbe::Known(total_size) => {
            debug!(
                url = url,
                file_size = total_size,
                "成功获取文件大小，启动并行下载"
            );
            // 3. 执行分块并行下载
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
        }
        SizeProbe::Stream(resp) => {
            // 3'. 拿不到长度：手上这个响应就是完整文件，直接流式落盘
            download_single_stream(resp, path).await.map_err(|e| {
                error!(
                    url = url,
                    path = %path.display(),
                    error = ?e,
                    "单线程流式下载失败"
                );
                e
            })?;
        }
    }

    debug!(path = %path.display(), "下载任务成功完成");
    Ok(backend)
}

/// 下载前探测到的文件长度。
enum SizeProbe {
    /// 已知总字节数，可以走分块并行下载。
    Known(u64),
    /// 长度未知，但手上这个响应体就是完整文件，直接流式落盘。
    Stream(reqwest::Response),
}

/// 从 `Content-Range: bytes 0-0/4245024` 里取出总长度；`bytes 0-0/*` 之类拿不到就返回 None。
fn parse_total_from_content_range(value: &str) -> Option<u64> {
    let (_, total) = value.rsplit_once('/')?;
    total.trim().parse().ok()
}

/// 探测文件总长度。
///
/// c-media.xmu.edu.cn 这类站点整体 GET 是流式吐数据的（HTTP/1.1 chunked、HTTP/2 干脆没有
/// content-length），只认 Content-Length 会 100% 失败。但它们支持 Range，所以先用
/// `Range: bytes=0-0` 探一下，从 `Content-Range` 里取总长；服务器忽略 Range 就退回
/// Content-Length；两者都没有时把完整响应交给单线程流式下载，而不是直接报错。
async fn probe_total_size(client: &SessionClient, url: &str) -> Result<SizeProbe> {
    let resp = client.get_range(url, 0, 0).await?;
    let status = resp.status();

    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        if let Some(total) = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_total_from_content_range)
        {
            debug!(
                url = url,
                file_size = total,
                "从 Content-Range 获取到文件大小"
            );
            return Ok(SizeProbe::Known(total));
        }
        warn!(
            url = url,
            "206 响应的 Content-Range 无法解析，回退到完整 GET"
        );
    } else if status.is_success() {
        // 服务器忽略了 Range，手上这个响应即完整文件
        if let Some(total) = resp.content_length() {
            debug!(
                url = url,
                file_size = total,
                "服务器忽略 Range，从 Content-Length 获取到文件大小"
            );
            return Ok(SizeProbe::Known(total));
        }
        warn!(
            url = url,
            "服务器忽略 Range 且无 Content-Length，改用单线程流式下载"
        );
        return Ok(SizeProbe::Stream(resp));
    } else {
        warn!(url = url, status = %status, "Range 探测返回非成功状态，回退到完整 GET");
    }

    // 回退：整体 GET
    let resp = client.get(url).await?;
    let status = resp.status();
    if !status.is_success() {
        error!(url = url, status = %status, "下载请求返回非成功状态");
        bail!("下载请求失败，状态码 {status}");
    }
    match resp.content_length() {
        Some(total) => {
            debug!(
                url = url,
                file_size = total,
                "从 Content-Length 获取到文件大小"
            );
            Ok(SizeProbe::Known(total))
        }
        None => {
            warn!(url = url, "响应头没有 Content-Length，改用单线程流式下载");
            Ok(SizeProbe::Stream(resp))
        }
    }
}

/// 单线程流式下载：长度未知时按响应体到达顺序顺序写盘。
async fn download_single_stream(resp: reqwest::Response, path: &std::path::Path) -> Result<()> {
    debug!(path = %path.display(), "长度未知，开始单线程流式下载");
    let mut f = tokio::fs::File::create(path).await.map_err(|e| {
        error!(path = %path.display(), error = ?e, "创建下载文件失败");
        e
    })?;

    let mut stream = resp.bytes_stream();
    let mut bytes_written: u64 = 0;
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| {
            error!(path = %path.display(), error = ?e, "接收数据流失败");
            e
        })?;
        f.write_all(&chunk).await.map_err(|e| {
            error!(path = %path.display(), error = ?e, "写入数据到文件失败");
            e
        })?;
        bytes_written += chunk.len() as u64;
    }
    f.flush().await.map_err(|e| {
        error!(path = %path.display(), error = ?e, "文件 Flush 失败");
        e
    })?;

    debug!(
        path = %path.display(),
        bytes_written = bytes_written,
        "单线程流式下载完成"
    );
    Ok(())
}

async fn download_parallel_benchmarked(
    client: SessionClient,
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
    use crate::api::xmu_service::testenv;
    use super::*;

    #[test]
    fn test_parse_total_from_content_range() {
        assert_eq!(
            parse_total_from_content_range("bytes 0-0/4245024"),
            Some(4245024)
        );
        // 长度未知时服务器会给 `*`，解析不出来就得走流式回退
        assert_eq!(parse_total_from_content_range("bytes 0-0/*"), None);
        assert_eq!(
            parse_total_from_content_range("bytes 0-99/1234"),
            Some(1234)
        );
    }

    #[test]
    fn test_escape_filename_for_path_illegal_chars() {
        let got = escape_filename_for_path("课程资料/第一章:绪论?.pdf");
        assert_eq!(got, "第一章%3A绪论%3F.pdf");
    }

    #[test]
    fn test_escape_filename_for_path_reserved_name() {
        let got = escape_filename_for_path("CON.txt");
        assert_eq!(got, "_CON.txt");
    }

    #[tokio::test]
    async fn test_download() -> Result<()> {
        if !testenv::network_enabled() {
            return testenv::skipped_network(module_path!());
        }
        let client = SessionClient::new();
        let url = "https://download.samplelib.com/png/sample-boat-400x300.png";
        let filename = "sample-boat-400x300.png";
        let file = download_to_temp(client, url, filename).await?;
        println!("Downloaded file at path: {:?}", file.path);
        Ok(())
    }
}
