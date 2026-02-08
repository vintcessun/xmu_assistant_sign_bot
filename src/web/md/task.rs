use crate::{
    api::storage::{File, FileBackend, FileStorage, HotTable}, // 引入 File 类和所需的 traits
    web::{URL, md::expose::ON_QUEUE},
};
use anyhow::{Context, Result};
use headless_chrome::{Browser, LaunchOptions, types::PrintToPdfOptions}; // 导入 PrintToPdfOptions
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, LazyLock},
    time::SystemTime,
};
use tokio::task;
use urlencoding::encode; // 导入 urlencoding::encode
use uuid::Uuid;
// 导入 headless_chrome 相关的类型。
use tracing::{debug, error, info, trace};

static DATA: LazyLock<HotTable<String, MdTaskResult>> = LazyLock::new(|| HotTable::new("md"));
const EXPIRE_DURATION_SECS: u64 = 60 * 60 * 24; // 1 天

// 在处理渲染的模块
mod renderer {
    include!(concat!(env!("OUT_DIR"), "/theme_template.rs"));

    pub fn render_to_html(markdown_content: &str) -> String {
        let options = pulldown_cmark::Options::all();
        let parser = pulldown_cmark::Parser::new_ext(markdown_content, options);
        let mut body = String::new();
        pulldown_cmark::html::push_html(&mut body, parser);

        HTML_TEMPLATE.replace("{{content}}", &body)
    }
}

pub fn query(id: &String) -> Option<Arc<MdTaskResult>> {
    DATA.get(id)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MdTaskResult {
    pub html_content: Arc<str>,
    pub pdf_path: PathBuf,
    pub expire_at: u64,
}

impl MdTaskResult {
    fn get_expire_at() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + EXPIRE_DURATION_SECS
    }
}

pub struct MdTask {
    pub id: String,
    markdown_content: String,
}

impl MdTask {
    pub fn new(markdown_content: String) -> Self {
        let id = Uuid::new_v4().to_string();
        ON_QUEUE.insert(id.clone());
        debug!(task_id = id, "创建一个新的 Markdown 渲染任务");

        Self {
            id,
            markdown_content,
        }
    }

    pub fn get_url(&self) -> String {
        format!("{}/md/task/{}", URL, self.id)
    }

    pub async fn finish(self) -> Result<()> {
        info!(task_id = %self.id, "开始处理 Markdown 转换任务");

        // 1. 渲染 Markdown 到 HTML
        trace!(task_id = %self.id, "开始渲染 Markdown 到 HTML");
        let html_content = Arc::<str>::from(renderer::render_to_html(&self.markdown_content));
        trace!(task_id = %self.id, html_len = html_content.len(), "HTML 渲染完成");
        let html_content_clone = html_content.clone();

        // 2. 生成 PDF
        // 创建 File 结构体来分配路径并管理文件
        let mut pdf_file = File::prepare(&format!("{}.pdf", self.id));
        let pdf_path = pdf_file.get_path().clone();
        trace!(task_id = %self.id, path = %pdf_path.display(), "准备 PDF 文件路径");

        // 必须在阻塞任务中运行 headless_chrome
        let pdf_bytes = task::spawn_blocking(move || {
            debug!("尝试启动 Chrome 浏览器");
            let browser = Browser::new(
                LaunchOptions::default_builder()
                    .build()
                    .context("构建浏览器启动选项失败")
                    .map_err(|e| {
                        error!(error = ?e, "构建浏览器启动选项失败");
                        e
                    })?,
            )
            .context("启动 Chrome 浏览器失败")
            .map_err(|e| {
                error!(error = ?e, "启动 Chrome 浏览器失败");
                e
            })?;

            debug!("创建浏览器标签页");
            let tab = browser
                .new_tab()
                .context("创建新的浏览器标签页失败")
                .map_err(|e| {
                    error!(error = ?e, "创建新的浏览器标签页失败");
                    e
                })?;

            // 将 HTML 内容编码为 Data URL
            let data_url = format!(
                "data:text/html;charset=utf-8,{}",
                encode(&html_content_clone) // 使用导入的 encode
            );
            trace!(data_url_len = data_url.len(), "Data URL 生成完成");

            debug!("导航到 Data URL");
            // 替代 set_content
            tab.navigate_to(&data_url)
                .context("导航到 Data URL 失败")
                .map_err(|e| {
                    error!(error = ?e, "导航到 Data URL 失败");
                    e
                })?;

            // 替代 wait_for_navigation
            tab.wait_until_navigated()
                .context("等待页面导航完成失败")
                .map_err(|e| {
                    error!(error = ?e, "等待页面导航完成失败");
                    e
                })?;
            debug!("页面导航完成，准备生成 PDF");

            // 打印 PDF，显式开启背景打印以确保样式完整
            let pdf_options = PrintToPdfOptions {
                print_background: Some(true),
                display_header_footer: Some(false),
                ..Default::default()
            };

            let pdf_bytes = tab
                .print_to_pdf(Some(pdf_options))
                .context("打印 PDF 失败")
                .map_err(|e| {
                    error!(error = ?e, "打印 PDF 失败");
                    e
                })?;
            debug!("PDF 字节流已生成");

            Ok::<Vec<u8>, anyhow::Error>(pdf_bytes)
        })
        .await
        .context("Markdown PDF 渲染任务调度失败")?;

        // Handling the inner Result
        let pdf_bytes = pdf_bytes.context("Markdown PDF 渲染任务内部执行失败")?;

        // 写入 PDF 文件
        fs::write(&pdf_path, pdf_bytes).context("写入 PDF 文件失败")?;
        debug!(path = %pdf_path.display(), "PDF 文件写入磁盘成功");

        // 终结文件，设置只读权限并预读
        pdf_file.finish().await?;
        debug!(path = %pdf_path.display(), "文件终结完成");

        // 3. 构造并存储结果
        let expose_result = MdTaskResult {
            html_content: html_content.clone(),
            pdf_path,
            expire_at: MdTaskResult::get_expire_at(),
        };

        // 4. 写入 HotTable

        info!(task_id = %self.id, "Markdown 转换任务完成");

        ON_QUEUE.remove(&self.id);
        DATA.insert(self.id, Arc::new(expose_result))?;
        Ok(())
    }
}
