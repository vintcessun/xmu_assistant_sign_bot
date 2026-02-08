use crate::web::file::task::query;
use axum::{
    Router,
    extract::Path,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use dashmap::DashSet;
use serde::Deserialize;
use std::sync::LazyLock;
use tokio_util::io::ReaderStream;

// 对应 /task/:id
#[derive(Deserialize)]
pub struct ListParams {
    pub id: String,
}

// 对应 /task/:id/:index
#[derive(Deserialize)]
pub struct DownloadParams {
    pub id: String,
    pub index: usize,
}

pub static ON_QUEUE: LazyLock<DashSet<String>> = LazyLock::new(DashSet::new);

/// 任务状态处理器：返回 HTML 文件列表
async fn task_status_handler(Path(params): Path<ListParams>) -> impl IntoResponse {
    // 1. 调用你提供的查询函数
    if ON_QUEUE.contains(&params.id) {
        return (StatusCode::OK, "任务正在处理中，请稍后刷新页面").into_response();
    }
    let list = match query(&params.id) {
        Some(l) => l,
        None => return (StatusCode::NOT_FOUND, "该任务不存在或已过期").into_response(),
    };

    // 2. 构造 HTML 视图内容
    let mut rows = String::new();
    for (idx, file) in list.files.iter().enumerate() {
        let name = file.path.file_name().unwrap_or_default().to_string_lossy();
        // 链接指向自动生成的路由：/file/task/{id}/{index}
        rows.push_str(&format!(
            "<tr>
                <td>{name}</td>
                <td>{}</td>
                <td><a href='/file/task/{}/{}' style='color: #007bff;'>点击下载</a></td>
            </tr>",
            file.mime, params.id, idx
        ));
    }

    // 3. 返回完整的 HTML 页面
    Html(format!(
        "<!DOCTYPE html><html>
        <head><meta charset='utf-8'><title>Expose 文件预览</title>
        <style>
            body {{ font-family: sans-serif; padding: 40px; line-height: 1.5; }}
            table {{ width: 100%; border-collapse: collapse; }}
            th, td {{ padding: 12px; border-bottom: 1px solid #eee; text-align: left; }}
            tr:hover {{ background: #f9f9f9; }}
        </style></head>
        <body>
            <h2>待下载文件列表</h2>
            <table>
                <thead><tr><th>文件名</th><th>类型</th><th>操作</th></tr></thead>
                <tbody>{rows}</tbody>
            </table>
        </body></html>"
    ))
    .into_response()
}

/// 文件下载处理器：根据索引返回文件流d
async fn file_download_handler(Path(params): Path<DownloadParams>) -> impl IntoResponse {
    // 1. 检索列表
    let list = match query(&params.id) {
        Some(l) => l,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    // 2. 使用自动反序列化的 index 获取文件元数据
    let file_info = match list.files.get(params.index) {
        Some(f) => f,
        None => return (StatusCode::NOT_FOUND, "索引超出范围").into_response(),
    };

    // 3. 打开文件并流化
    let file = match tokio::fs::File::open(&file_info.path).await {
        Ok(f) => f,
        Err(_) => return StatusCode::GONE.into_response(),
    };

    let filename = file_info
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let stream = ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);

    // 4. 构建响应头，强制浏览器下载
    Response::builder()
        .header(header::CONTENT_TYPE, &file_info.mime)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(body)
        .unwrap()
}

pub fn task_router(router: Router) -> Router {
    router // 捕获任务 ID，显示 HTML 列表
        .route("/task/{id}", get(task_status_handler))
        // 捕获 ID 和 Index，执行下载
        .route("/task/{id}/{index}", get(file_download_handler))
}
