use crate::web::md::task::query;
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
use tokio::fs::File;
use tokio_util::io::ReaderStream;

// 用于追踪正在处理的任务
pub static ON_QUEUE: LazyLock<DashSet<String>> = LazyLock::new(DashSet::new);

#[derive(Deserialize)]
pub struct TaskParams {
    pub id: String,
}

#[derive(Deserialize)]
pub struct ContentParams {
    pub id: String,
    pub content_type: String, // "html" 或 "pdf"
}

/// 任务状态处理器：返回带有预览和下载按钮的页面
async fn task_status_handler(Path(params): Path<TaskParams>) -> impl IntoResponse {
    if ON_QUEUE.contains(&params.id) {
        return (StatusCode::OK, "任务正在处理中，请稍后刷新页面").into_response();
    }

    // 构造 HTML 视图内容
    let html_preview_url = format!("/md/task/{}/html", params.id);
    let pdf_download_url = format!("/md/task/{}/pdf", params.id);

    Html(format!(
        "<!DOCTYPE html><html>
        <head><meta charset='utf-8'><title>Markdown 任务结果</title>
        <style>
            body {{ font-family: sans-serif; padding: 40px; line-height: 1.5; }}
            .container {{ max-width: 600px; margin: 0 auto; text-align: center; padding-top: 50px; }}
            .btn {{ display: inline-block; padding: 10px 20px; margin: 10px; text-decoration: none; border-radius: 5px; cursor: pointer; }}
            .btn-primary {{ background-color: #007bff; color: white; border: none; }}
            .btn-secondary {{ background-color: #6c757d; color: white; border: none; }}
        </style></head>
        <body>
            <div class='container'>
                <h2>Markdown 转换任务完成</h2>
                <p>任务ID: {}</p>
                <a class='btn btn-primary' href='{}' target='_blank'>预览 HTML</a>
                <a class='btn btn-secondary' href='{}'>下载 PDF</a>
            </div>
        </body></html>",
        params.id, html_preview_url, pdf_download_url
    ))
    .into_response()
}

/// 内容处理器：返回 HTML 或 PDF
async fn content_handler(Path(params): Path<ContentParams>) -> impl IntoResponse {
    let task_result = match query(&params.id) {
        Some(r) => r,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    match params.content_type.as_str() {
        "html" => {
            // 返回 HTML 内容供预览
            Response::builder()
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(axum::body::Body::from(task_result.html_content.to_string()))
                .unwrap()
        }
        "pdf" => {
            // 返回 PDF 文件流供下载
            let pdf_path = task_result.pdf_path.clone();

            // 使用 tokio::fs::File::open 打开文件
            let file = match File::open(&pdf_path).await {
                Ok(f) => f,
                // 如果文件不存在或无法打开，可能是已过期或已被清理
                Err(_) => return StatusCode::GONE.into_response(),
            };

            let filename = pdf_path.file_name().unwrap_or_default().to_string_lossy();
            let stream = ReaderStream::new(file);
            let body = axum::body::Body::from_stream(stream);

            Response::builder()
                .header(header::CONTENT_TYPE, "application/pdf")
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", filename),
                )
                .body(body)
                .unwrap()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

pub fn task_router(router: Router) -> Router {
    router
        // /md/task/{id} - 状态/按钮页面
        .route("/task/{id}", get(task_status_handler))
        // /md/task/{id}/html 或 /md/task/{id}/pdf - 内容预览/下载
        .route("/task/{id}/{content_type}", get(content_handler))
}
