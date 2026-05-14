use crate::{
    api::storage::{File, FileBackend, FileStorage, HotTable}, // 引入 File 类和所需的 traits
    web::{URL, md::expose::ON_QUEUE},
};
use anyhow::{Context, Result};
use chromiumoxide::detection::{DetectionOptions, default_executable};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
    time::SystemTime,
};
use tracing::{debug, info, trace};
use uuid::Uuid;

static DATA: LazyLock<HotTable<String, MdTaskResult>> = LazyLock::new(|| HotTable::new("md"));

fn preferred_chrome_executable() -> PathBuf {
    if let Some(exe) = std::env::var_os("CHROME_PATH") {
        let path = PathBuf::from(exe);
        debug!(path = %path.display(), "using CHROME_PATH environment variable");
        return path;
    }

    if let Ok(path) = default_executable(DetectionOptions {
        msedge: false,
        unstable: true,
    }) {
        debug!(path = %path.display(), "Chrome auto-detected (unstable)");
        return path;
    }

    if let Ok(path) = default_executable(DetectionOptions::default()) {
        debug!(path = %path.display(), "Chrome auto-detected (default)");
        return path;
    }

    PathBuf::from("chrome")
}

async fn render_pdf_via_chrome_cli(html: &str, output_path: &Path) -> Result<()> {
    let absolute_output_path = if output_path.is_absolute() {
        output_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(output_path)
    };

    if let Some(parent) = absolute_output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let html_path = absolute_output_path.with_extension("cli.html");
    fs::write(&html_path, html.as_bytes()).context("write temporary HTML for PDF failed")?;
    let file_url = format!("file:///{}", html_path.to_string_lossy().replace('\\', "/"));
    let executable = preferred_chrome_executable();
    let pdf_arg = format!("--print-to-pdf={}", absolute_output_path.display());

    let output = tokio::process::Command::new(&executable)
        .args([
            "--headless",
            "--disable-gpu",
            "--disable-features=RendererCodeIntegrity,VizDisplayCompositor",
            "--no-sandbox",
            "--no-pdf-header-footer",
            "--print-to-pdf-no-header",
            &pdf_arg,
            &file_url,
        ])
        .output()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "failed to start chrome CLI for PDF ({}): {}",
                executable.display(),
                e
            )
        })?;

    let _ = fs::remove_file(&html_path);

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "chrome CLI PDF failed, status={:?}, stdout={}, stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    if !absolute_output_path.exists() {
        return Err(anyhow::anyhow!(
            "chrome CLI returned success but PDF file was not created: requested={}, absolute={}, stdout={}, stderr={}",
            output_path.display(),
            absolute_output_path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

#[cfg(test)]
macro_rules! md_test_step {
    ($task_id:expr, $($arg:tt)*) => {{
        println!("[md-task:{}] {}", $task_id, format_args!($($arg)*));
    }};
}

#[cfg(not(test))]
macro_rules! md_test_step {
    ($task_id:expr, $($arg:tt)*) => {{}};
}
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

async fn render_markdown_task_result(id: &str, markdown_content: &str) -> Result<MdTaskResult> {
    md_test_step!(id, "render markdown to html start");
    let html_content = Arc::<str>::from(renderer::render_to_html(markdown_content));
    md_test_step!(
        id,
        "render markdown to html ok, html_len={}",
        html_content.len()
    );
    trace!(task_id = %id, html_len = html_content.len(), "HTML æ¸²æŸ“å®Œæˆ");

    md_test_step!(id, "prepare pdf file start");
    let pdf_file = File::prepare(&format!("{id}.pdf"));
    let pdf_path_ref = pdf_file.get_path();
    md_test_step!(id, "prepare pdf file ok, path={}", pdf_path_ref.display());
    trace!(task_id = %id, path = %pdf_path_ref.display(), "å‡†å¤‡ PDF æ–‡ä»¶è·¯å¾„");

    md_test_step!(id, "render pdf via chrome cli start");
    render_pdf_via_chrome_cli(&html_content, pdf_path_ref).await?;
    md_test_step!(id, "render pdf via chrome cli ok");

    md_test_step!(id, "finish storage file start");
    pdf_file.finish().await?;
    md_test_step!(id, "finish storage file ok");
    debug!(path = %pdf_path_ref.display(), "æ–‡ä»¶ç»ˆç»“å®Œæˆ");

    Ok(MdTaskResult {
        html_content,
        pdf_path: pdf_path_ref.to_path_buf(),
        expire_at: MdTaskResult::get_expire_at(),
    })
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
        md_test_step!(&self.id, "finish start");
        info!(task_id = %self.id, "å¼€å§‹å¤„ç† Markdown è½¬æ¢ä»»åŠ¡");

        let expose_result = render_markdown_task_result(&self.id, &self.markdown_content).await?;

        info!(task_id = %self.id, "Markdown è½¬æ¢ä»»åŠ¡å®Œæˆ");
        ON_QUEUE.remove(&self.id);
        md_test_step!(&self.id, "insert result start");
        DATA.insert(self.id, Arc::new(expose_result))?;
        md_test_step!("completed", "finish ok");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{render_markdown_task_result, renderer};

    #[test]
    fn minimal_markdown_renders_expected_html() {
        let markdown = "# Hello\n\nThis is **bold** and [a link](https://example.com).";

        println!("[md-render-test] input markdown:\n{markdown}");
        let html = renderer::render_to_html(markdown);
        println!("[md-render-test] rendered html len={}", html.len());
        println!("[md-render-test] rendered html:\n{html}");

        assert!(html.contains("<h1>Hello</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<a href=\"https://example.com\">a link</a>"));
    }

    #[tokio::test]
    async fn minimal_md_render_pipeline_without_db() {
        let id = format!("test-{}", uuid::Uuid::new_v4());

        println!("[md-task-test:{id}] render without db start");
        let result = render_markdown_task_result(&id, "# Hello\n\nThis is **bold**.")
            .await
            .unwrap();
        println!(
            "[md-task-test:{id}] render returned ok, pdf_path={}",
            result.pdf_path.display()
        );

        assert!(result.html_content.contains("<h1>Hello</h1>"));
        assert!(result.html_content.contains("<strong>bold</strong>"));
        assert!(result.pdf_path.exists());
        assert!(
            std::fs::metadata(&result.pdf_path)
                .expect("pdf metadata should be readable")
                .len()
                > 0
        );
    }
}
