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

        Self {
            id,
            markdown_content,
        }
    }

    pub fn get_url(&self) -> String {
        format!("{}/md/task/{}", URL, self.id)
    }

    pub async fn finish(self) -> Result<()> {
        // 1. 渲染 Markdown 到 HTML
        let html_content = Arc::<str>::from(renderer::render_to_html(&self.markdown_content));
        let html_content_clone = html_content.clone();

        // 2. 生成 PDF
        // 创建 File 结构体来分配路径并管理文件
        let mut pdf_file = File::prepare(&format!("{}.pdf", self.id));
        let pdf_path = pdf_file.get_path().clone();

        // 必须在阻塞任务中运行 headless_chrome
        let pdf_bytes = task::spawn_blocking(move || {
            let browser = Browser::new(
                LaunchOptions::default_builder()
                    .build()
                    .context("Failed to build LaunchOptions")?,
            )
            .context("Failed to launch Chrome browser")?;

            let tab = browser.new_tab().context("Failed to create new tab")?;

            // 将 HTML 内容编码为 Data URL
            let data_url = format!(
                "data:text/html;charset=utf-8,{}",
                encode(&html_content_clone) // 使用导入的 encode
            );

            // 替代 set_content
            tab.navigate_to(&data_url)
                .context("Failed to navigate to data URL")?;

            // 替代 wait_for_navigation
            tab.wait_until_navigated()
                .context("Failed to wait for navigation")?;

            // 打印 PDF，显式开启背景打印以确保样式完整
            let pdf_options = PrintToPdfOptions {
                print_background: Some(true),
                display_header_footer: Some(false),
                ..Default::default()
            };

            let pdf_bytes = tab
                .print_to_pdf(Some(pdf_options))
                .context("Failed to print to PDF")?;

            Ok::<Vec<u8>, anyhow::Error>(pdf_bytes)
        })
        .await
        .context("Task spawning failed")? // 处理 spawn_blocking 自身的错误
        .context("PDF generation failed in blocking task")?; // 处理 blocking task 内部的 Result 错误

        // 写入 PDF 文件
        fs::write(&pdf_path, pdf_bytes).context("Failed to write PDF file")?;

        // 终结文件，设置只读权限并预读
        pdf_file.finish().await?;

        // 3. 构造并存储结果
        let expose_result = MdTaskResult {
            html_content: html_content.clone(),
            pdf_path,
            expire_at: MdTaskResult::get_expire_at(),
        };

        // 4. 写入 HotTable
        ON_QUEUE.remove(&self.id);
        DATA.insert(self.id, Arc::new(expose_result))?;

        Ok(())
    }
}
