use crate::api::{
    storage::{FileBackend, FileStorage, TempFile},
    xmu_service::jw::{Schedule, ScheduleResponse},
};
use anyhow::Result;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::detection::{DetectionOptions, default_executable};
use chromiumoxide::{Browser, BrowserConfig};
use std::collections::{HashMap, HashSet};
use tracing::debug;
use uuid::Uuid;

const COLORS: [&str; 20] = [
    "#4f46e5", "#7c3aed", "#2563eb", "#0891b2", "#059669", "#d97706", "#db2777", "#4b5563",
    "#b91c1c", "#ea580c", "#16a34a", "#0ea5e9", "#8b5cf6", "#f43f5e", "#e11d48", "#14b8a6",
    "#22c55e", "#2563eb", "#7c3aed", "#4f46e5",
];

pub struct ScheduleRenderer;

impl ScheduleRenderer {
    fn preferred_chrome_executable() -> Option<std::path::PathBuf> {
        if let Some(exe) = std::env::var_os("CHROME_PATH") {
            let p = std::path::PathBuf::from(exe);
            debug!(path = %p.display(), "using CHROME_PATH environment variable");
            return Some(p);
        }

        if let Ok(p) = default_executable(DetectionOptions {
            msedge: false,
            unstable: true,
        }) {
            debug!(path = %p.display(), "Chrome auto-detected (unstable)");
            return Some(p);
        }

        if let Ok(p) = default_executable(DetectionOptions::default()) {
            debug!(path = %p.display(), "Chrome auto-detected (default)");
            return Some(p);
        }

        debug!("Chrome auto-detect failed, using BrowserConfig default detection");
        None
    }

    async fn render_via_chrome_cli(html: &str, output_path: &std::path::Path) -> Result<()> {
        let requested_output_path = output_path.to_path_buf();
        let absolute_output_path = if requested_output_path.is_absolute() {
            requested_output_path.clone()
        } else {
            std::env::current_dir()?.join(&requested_output_path)
        };

        if let Some(parent) = absolute_output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let html_path = absolute_output_path.with_extension("cli.html");
        std::fs::write(&html_path, html.as_bytes())?;
        let file_url = format!("file:///{}", html_path.to_string_lossy().replace('\\', "/"));

        let executable = Self::preferred_chrome_executable()
            .unwrap_or_else(|| std::path::PathBuf::from("chrome"));

        let args = vec![
            "--headless=new".to_string(),
            "--window-size=1400,1800".to_string(),
            format!("--screenshot={}", absolute_output_path.display()),
            file_url,
        ];

        let output = tokio::process::Command::new(&executable)
            .args(&args)
            .output()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to start chrome CLI ({}): {}",
                    executable.display(),
                    e
                )
            })?;

        let _ = std::fs::remove_file(&html_path);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!(
                "chrome CLI screenshot failed, status={:?}, stdout={}, stderr={}",
                output.status.code(),
                stdout,
                stderr
            ));
        }

        if !absolute_output_path.exists() {
            // 某些 Windows 版本下，Chrome 会在 stdout 中打印实际落盘路径。
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(raw_path) = stdout
                .lines()
                .find_map(|line| line.split_once("written to file ").map(|(_, p)| p.trim()))
            {
                let produced_path = std::path::PathBuf::from(raw_path.trim_matches('"'));
                if produced_path.exists() {
                    std::fs::copy(&produced_path, &absolute_output_path)?;
                }
            }
        }

        if !absolute_output_path.exists() {
            return Err(anyhow::anyhow!(
                "chrome CLI returned success but screenshot file not found: requested={}, absolute={}, stdout={}, stderr={}",
                output_path.display(),
                absolute_output_path.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(())
    }

    /// 接收 Schedule 引用并生成图片文件
    pub async fn render_to_file(schedule: &Schedule, week: u64) -> Result<TempFile> {
        let filename = format!("{}.png", Uuid::new_v4());
        let file = TempFile::prepare(&filename);

        Self::render_to_file_inner(schedule, week, file.get_path()).await?;

        Ok(file)
    }
}

impl ScheduleRenderer {
    fn weekday_name(xq: i64) -> &'static str {
        match xq {
            1 => "周一",
            2 => "周二",
            3 => "周三",
            4 => "周四",
            5 => "周五",
            6 => "周六",
            7 => "周日",
            _ => "",
        }
    }

    fn generate_html(schedule: &Schedule, target_week: u64) -> String {
        type CourseRef<'a> = (&'a ScheduleResponse, &'a str);

        #[derive(Clone)]
        struct TimedCourse<'a> {
            start: i64,
            end: i64,
            course: CourseRef<'a>,
        }

        #[derive(Default)]
        struct RenderSegment<'a> {
            start: i64,
            end: i64,
            courses: Vec<CourseRef<'a>>,
            signature: Vec<String>,
        }

        let mut day_courses: HashMap<i64, Vec<TimedCourse>> = HashMap::new();

        for (idx, item) in schedule.pkjgList.iter().enumerate() {
            if item.zcbh.chars().nth((target_week - 1) as usize) != Some('1') {
                continue;
            }

            let color = COLORS[idx % COLORS.len()];
            day_courses.entry(item.xq).or_default().push(TimedCourse {
                start: item.ksjcdm,
                end: item.jsjcdm,
                course: (item, color),
            });
        }

        // 维持标准 12 节网格，不做行压缩。
        let period_count = 12_i64;

        let mut day_indices: Vec<i64> = day_courses.keys().copied().collect();
        day_indices.sort_unstable();
        day_indices.dedup();
        if day_indices.is_empty() {
            day_indices = (1..=7).collect();
        }

        let mut course_html = String::new();
        let mut header_html = String::new();
        header_html.push_str(r#"<div class="cell header" style="grid-area: 1 / 1;"></div>"#);
        for (day_idx, xq) in day_indices.iter().enumerate() {
            header_html.push_str(&format!(
                r#"<div class="cell header" style="grid-area: 1 / {col};">{name}</div>"#,
                col = day_idx + 2,
                name = Self::weekday_name(*xq)
            ));
        }

        for (day_idx, xq) in day_indices.iter().enumerate() {
            let courses = day_courses.remove(xq).unwrap_or_default();
            if courses.is_empty() {
                continue;
            }

            let mut split_points: Vec<i64> = vec![1, period_count + 1];
            for c in &courses {
                let s = c.start.clamp(1, period_count);
                let e = c.end.clamp(1, period_count);
                if s <= e {
                    split_points.push(s);
                    split_points.push(e + 1);
                }
            }
            split_points.sort_unstable();
            split_points.dedup();

            let mut segments: Vec<RenderSegment> = Vec::new();
            for win in split_points.windows(2) {
                let start = win[0];
                let end = win[1] - 1;
                if start > end {
                    continue;
                }

                let mut active: Vec<CourseRef> = Vec::new();
                for c in &courses {
                    let cs = c.start.clamp(1, period_count);
                    let ce = c.end.clamp(1, period_count);
                    if cs <= end && ce >= start {
                        active.push(c.course);
                    }
                }
                if active.is_empty() {
                    continue;
                }

                // 去重后构造签名：仅当课程集合一致时才允许跨区间合并。
                let mut seen: HashSet<String> = HashSet::new();
                let mut keyed: Vec<(String, CourseRef)> = Vec::new();
                for c in active {
                    let loc = c.0.jasmc.as_deref().unwrap_or("");
                    let key = format!("{}|{}|{}|{}", c.0.kcmc, loc, c.0.ksjcdm, c.0.jsjcdm);
                    if seen.insert(key.clone()) {
                        keyed.push((key, c));
                    }
                }
                keyed.sort_by(|a, b| a.0.cmp(&b.0));

                let signature: Vec<String> = keyed.iter().map(|(k, _)| k.clone()).collect();
                let mut dedup_courses: Vec<CourseRef> = keyed.into_iter().map(|(_, c)| c).collect();
                dedup_courses
                    .sort_by_key(|(item, _)| (item.ksjcdm, item.jsjcdm, item.kcmc.clone()));

                if let Some(last) = segments.last_mut() {
                    if last.end + 1 == start && last.signature == signature {
                        last.end = end;
                        continue;
                    }
                }

                segments.push(RenderSegment {
                    start,
                    end,
                    courses: dedup_courses,
                    signature,
                });
            }

            for seg in segments {
                let row_start = seg.start + 1;
                let row_span = seg.end - seg.start + 1;
                let col_start = day_idx + 2;
                let bg_color = seg.courses[0].1;

                let mut inner_content = String::new();
                for (item, _) in seg.courses {
                    let location = item.jasmc.as_deref().unwrap_or("未排地点");
                    inner_content.push_str(&format!(
                        r#"<div class="course-block">
                        <div class="name">{name}</div>
                        <div class="loc">{loc}</div>
                    </div>"#,
                        name = item.kcmc,
                        loc = location
                    ));
                }

                course_html.push_str(&format!(
                    r#"<div class="course-item" style="grid-row: {rs} / span {sp}; grid-column: {cs}; background-color: {color};">
                    {content}
                </div>"#,
                    rs = row_start,
                    sp = row_span,
                    cs = col_start,
                    color = bg_color,
                    content = inner_content
                ));
            }
        }

        format!(
            r#"<!DOCTYPE html>
        <html>
        <head>
            <meta charset="utf-8">
            <style>
                * {{ box-sizing: border-box; margin: 0; padding: 0; }}
                html, body {{
                    width: 1400px;
                    height: 1800px;
                    overflow: hidden;
                    background: white;
                }}
                body {{ 
                    padding: 14px;
                    margin: 0;
                    font-family: "PingFang SC", "Microsoft YaHei", sans-serif;
                }}
                .card {{ 
                    background: white;
                    border-radius: 0;
                    padding: 0;
                    box-shadow: none;
                    border: none;
                    width: 100%;
                    height: 100%;
                    display: flex;
                    flex-direction: column;
                }}
                .title {{ font-size: 24px; font-weight: bold; text-align: center; margin-bottom: 14px; color: #1e293b; }}
                
                .grid {{
                    display: grid;
                    grid-template-columns: 46px repeat({day_count}, minmax(0, 1fr));
                    grid-template-rows: 44px repeat({period_count}, minmax(0, 1fr));
                    gap: 0px;
                    width: 100%;
                    flex: 1;
                }}

                .cell {{
                    border: 0.5px solid #f1f5f9;
                    display: flex; align-items: center; justify-content: center;
                }}

                .header {{ 
                    background: #f8fafc; font-weight: 700; color: #475569; font-size: 24px;
                    grid-row: 1;
                }}

                .time-slot {{ 
                    color: #64748b; font-size: 22px; font-weight: 600;
                    grid-column: 1;
                }}

                .course-item {{ 
                    margin: 3px; color: white; border-radius: 10px; padding: 12px;
                    display: flex; flex-direction: column; gap: 10px;
                    box-shadow: 0 4px 10px rgba(0,0,0,0.1);
                    overflow: hidden; z-index: 10;
                }}
                
                /* 处理多门课程冲突时的样式 */
                .course-block {{
                    display: flex; flex-direction: column;
                    border-bottom: 1px dashed rgba(255,255,255,0.4);
                    padding-bottom: 6px;
                }}
                .course-block:last-child {{ border-bottom: none; padding-bottom: 0; }}

                .name {{ font-weight: 700; font-size: 22px; line-height: 1.25; margin-bottom: 4px; }}
                .loc {{ font-size: 18px; opacity: 0.95; }}
            </style>
        </head>
        <body>
            <div class="card">
                <div class="title">第 {target_week} 周 课程表</div>
                <div class="grid">
                    {header_html}
                    {grid_background}
                    {course_html}
                </div>
            </div>
        </body>
        </html>"#,
            target_week = target_week,
            day_count = day_indices.len(),
            period_count = period_count,
            header_html = header_html,
            grid_background = Self::generate_bg_cells(period_count, day_indices.len()),
            course_html = course_html
        )
    }

    // 辅助函数：生成背景网格，确保节次数字与网格绝对对齐
    fn generate_bg_cells(period_count: i64, day_count: usize) -> String {
        let mut bg = String::new();
        for row in 2..=(period_count + 1) {
            // 时间数字
            bg.push_str(&format!(
                r#"<div class="cell time-slot" style="grid-area: {row} / 1;">{}</div>"#,
                row - 1
            ));
            // 空白网格线填充
            for col in 2..=(day_count + 1) {
                bg.push_str(&format!(
                    r#"<div class="cell" style="grid-area: {row} / {col};"></div>"#
                ));
            }
        }
        bg
    }

    async fn render_to_file_inner(
        schedule: &Schedule,
        week: u64,
        path: &std::path::PathBuf,
    ) -> Result<()> {
        let html = Self::generate_html(schedule, week);

        let cdp_result: Result<()> = async {
            // 每次使用独立的临时目录，避免上次崩溃留下的锁文件导致渲染进程启动失败
            let user_data_dir =
                std::env::temp_dir().join(format!("chromiumoxide-{}", Uuid::new_v4()));

            let mut config_builder = BrowserConfig::builder()
                // 与已验证可启动的测试配置保持一致，避免渲染目标创建超时
                .new_headless_mode()
                // Windows 下某些环境会因渲染器子进程初始化失败导致 Target.createTarget 超时
                .arg("--disable-features=RendererCodeIntegrity")
                .arg("--single-process")
                .arg("--no-zygote")
                .user_data_dir(&user_data_dir)
                .window_size(1400, 1800)
                .launch_timeout(std::time::Duration::from_secs(20));

            if let Some(exe) = Self::preferred_chrome_executable() {
                config_builder = config_builder.chrome_executable(exe);
            }

            let config = config_builder
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to build browser config: {}", e))?;

            let (mut browser, mut handler) = Browser::launch(config)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to launch browser: {}", e))?;

            tokio::spawn(async move {
                use futures_util::StreamExt;
                while let Some(event) = handler.next().await {
                    if let Err(err) = event {
                        debug!(error = %err, "chromium handler event error");
                    }
                }
            });

            // 将 HTML 写入临时文件，用 file:// 协议打开
            let html_path = path.with_extension("html");
            std::fs::write(&html_path, html.as_bytes())?;
            let file_url = format!("file:///{}", html_path.to_string_lossy().replace('\\', "/"));

            let _ =
                tokio::time::timeout(std::time::Duration::from_secs(10), browser.fetch_targets())
                    .await;

            // 优先复用启动后已有页面，绕过部分环境下 Target.createTarget(new_page) 卡死问题。
            let page =
                match tokio::time::timeout(std::time::Duration::from_secs(10), browser.pages())
                    .await
                {
                    Ok(Ok(mut pages)) if !pages.is_empty() => {
                        debug!(page_count = pages.len(), "reusing existing CDP page");
                        pages.swap_remove(0)
                    }
                    _ => {
                        debug!("no existing CDP page, creating new page");
                        tokio::time::timeout(
                            std::time::Duration::from_secs(30),
                            browser.new_page("about:blank"),
                        )
                        .await
                        .map_err(|_| anyhow::anyhow!("new_page(about:blank) timed out after 30s"))?
                        .map_err(|e| anyhow::anyhow!("Failed to create about:blank page: {}", e))?
                    }
                };

            tokio::time::timeout(std::time::Duration::from_secs(30), page.goto(&file_url))
                .await
                .map_err(|_| anyhow::anyhow!("page.goto(file_url) timed out after 30s"))?
                .map_err(|e| anyhow::anyhow!("Failed to navigate to file URL: {}", e))?;

            // 给浏览器渲染时间
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // 截图整个视口（1400x1800），不需要额外裁剪
            // 因为 HTML 已改为占满整个窗口
            let width = 1400.0;
            let height = 1800.0;
            let p1_x = 0.0;
            let p1_y = 0.0;

            page.evaluate("window.scrollTo(0, 0);")
                .await
                .map_err(|e| anyhow::anyhow!("Failed to scroll: {}", e))?;

            // 截图时指定裁剪区域
            let screenshot_params =
                chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotParams {
                    format: Some(CaptureScreenshotFormat::Png),
                    clip: Some(chromiumoxide::cdp::browser_protocol::page::Viewport {
                        x: p1_x,
                        y: p1_y,
                        width,
                        height,
                        scale: 1.0,
                    }),
                    from_surface: Some(true),
                    ..Default::default()
                };

            let png_data = page
                .screenshot(screenshot_params)
                .await
                .map_err(|e| anyhow::anyhow!("Screenshot failed: {}", e))?;

            std::fs::write(path, png_data)?;
            let _ = std::fs::remove_file(&html_path);
            let _ = std::fs::remove_dir_all(&user_data_dir);
            Ok(())
        }
        .await;

        if let Err(err) = cdp_result {
            debug!(error = %err, "CDP rendering failed, falling back to Chrome CLI screenshot");
            Self::render_via_chrome_cli(&html, path.as_path()).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::api::xmu_service::jw::ScheduleList;

    use super::*;

    #[test]
    pub fn test_chrome_launch() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let run = async {
                let mut config_builder = BrowserConfig::builder()
                    .new_headless_mode()
                    .arg("--disable-features=RendererCodeIntegrity")
                    .arg("--single-process")
                    .arg("--no-zygote")
                    .window_size(800, 600)
                    .launch_timeout(Duration::from_secs(15));

                if let Some(executable) = ScheduleRenderer::preferred_chrome_executable() {
                    config_builder = config_builder.chrome_executable(executable);
                }

                let config = config_builder
                    .build()
                    .expect("Failed to build browser config");

                let (mut browser, mut handler) =
                    tokio::time::timeout(Duration::from_secs(20), Browser::launch(config))
                        .await
                        .expect("Browser::launch timed out")
                        .expect("Failed to launch browser");

                let handler_task = tokio::spawn(async move {
                    use futures_util::StreamExt;
                    while let Some(event) = handler.next().await {
                        if let Err(err) = event {
                            debug!(error = %err, "chromium handler event error");
                        }
                    }
                });

                let version = tokio::time::timeout(Duration::from_secs(20), browser.version())
                    .await
                    .expect("browser.version timed out")
                    .expect("Failed to query browser version");

                assert!(
                    !version.product.is_empty(),
                    "browser.version returned an empty product string"
                );

                tokio::time::timeout(Duration::from_secs(8), browser.close())
                    .await
                    .expect("browser.close timed out")
                    .expect("Failed to close browser");
                handler_task.abort();

                println!("Chrome 启动并渲染成功");
            };

            tokio::time::timeout(Duration::from_secs(25), run)
                .await
                .expect("test_chrome_launch timed out");
        });
    }

    #[tokio::test(flavor = "multi_thread")]
    pub async fn test() -> Result<()> {
        let castgc = "TGT-5205798-NK0oXgq45hvHea7P3Uh2Xa0LYmqw64m-AGxvUWcR3-iGLwPHM57b1cVe8jlzLLjmoe8null_main";
        println!("[1/4] 获取课程列表...");
        let schedule_list = ScheduleList::get(castgc).await?;
        println!("[2/4] 获取课程详情...");
        let schedule = Schedule::get(castgc, &schedule_list.datas.kfdxnxqcx.rows[1]).await?;
        println!("[3/4] 渲染图片（启动 Chrome）...");
        let temp_file = ScheduleRenderer::render_to_file(&schedule, 9)
            .await
            .unwrap();
        println!("[4/4] 渲染完成");
        println!("生成文件路径: {:?}", temp_file.get_path());
        Ok(())
    }
}
