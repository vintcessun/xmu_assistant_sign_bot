use crate::api::{
    storage::{FileBackend, FileStorage, TempFile},
    xmu_service::jw::{Schedule, ScheduleResponse},
};
use anyhow::Result;
use headless_chrome::{Browser, LaunchOptions};
use std::collections::HashMap;
use tokio::task::block_in_place;
use uuid::Uuid;

const COLORS: [&str; 20] = [
    "#4f46e5", "#7c3aed", "#2563eb", "#0891b2", "#059669", "#d97706", "#db2777", "#4b5563",
    "#b91c1c", "#ea580c", "#16a34a", "#0ea5e9", "#8b5cf6", "#f43f5e", "#e11d48", "#14b8a6",
    "#22c55e", "#2563eb", "#7c3aed", "#4f46e5",
];

pub struct ScheduleRenderer;

impl ScheduleRenderer {
    /// 接收 Schedule 引用并生成图片文件
    pub async fn render_to_file(schedule: &Schedule, week: usize) -> Result<TempFile> {
        block_in_place(|| {
            let filename = format!("{}.png", Uuid::new_v4());
            let file = TempFile::prepare(&filename);

            Self::render_to_file_inner(schedule, week, file.get_path())?;

            Ok(file)
        })
    }
}

impl ScheduleRenderer {
    fn generate_html(schedule: &Schedule, target_week: usize) -> String {
        // 使用 HashMap 存储冲突课程: Key = (星期, 开始节次, 结束节次)
        type Key = (i64, i64, i64);
        // Value = Vec<(课程项, 颜色)>
        type Value<'a> = Vec<(&'a ScheduleResponse, &'a str)>;
        let mut grouped_courses: HashMap<Key, Value> = HashMap::new();

        for (idx, item) in schedule.pkjgList.iter().enumerate() {
            if item.zcbh.chars().nth(target_week - 1) != Some('1') {
                continue;
            }

            let key = (item.xq, item.ksjcdm, item.jsjcdm);
            let color = COLORS[idx % COLORS.len()];
            grouped_courses.entry(key).or_default().push((item, color));
        }

        let mut course_html = String::new();

        for ((xq, ksjcdm, jsjcdm), courses) in grouped_courses {
            let row_start = ksjcdm + 1;
            let row_span = jsjcdm - ksjcdm + 1;
            let col_start = xq + 1;

            // 如果有多门课，使用渐变色或第一门课的颜色作为背景
            let bg_color = courses[0].1;

            let mut inner_content = String::new();
            for (item, _) in courses {
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

        format!(
            r#"<!DOCTYPE html>
        <html>
        <head>
            <meta charset="utf-8">
            <style>
                * {{ box-sizing: border-box; margin: 0; padding: 0; }}
                body {{ 
                    background: #f8fafc; padding: 40px; 
                    display: flex; justify-content: center;
                    font-family: "PingFang SC", "Microsoft YaHei", sans-serif;
                }}
                .card {{ 
                    background: white; border-radius: 16px; padding: 25px;
                    box-shadow: 0 15px 40px rgba(0,0,0,0.08);
                    border: 1px solid #e2e8f0;
                }}
                .title {{ font-size: 24px; font-weight: bold; text-align: center; margin-bottom: 25px; color: #1e293b; }}
                
                .grid {{
                    display: grid;
                    grid-template-columns: 45px repeat(7, 130px);
                    grid-template-rows: 45px repeat(12, 95px);
                    gap: 0px; 
                }}

                .cell {{
                    border: 0.5px solid #f1f5f9;
                    display: flex; align-items: center; justify-content: center;
                }}

                .header {{ 
                    background: #f8fafc; font-weight: 600; color: #64748b; font-size: 14px;
                    grid-row: 1;
                }}

                .time-slot {{ 
                    color: #94a3b8; font-size: 13px; font-weight: 500;
                    grid-column: 1;
                }}

                .course-item {{ 
                    margin: 4px; color: white; border-radius: 10px; padding: 8px; 
                    display: flex; flex-direction: column; gap: 8px;
                    box-shadow: 0 4px 10px rgba(0,0,0,0.1);
                    overflow: hidden; z-index: 10;
                }}
                
                /* 处理多门课程冲突时的样式 */
                .course-block {{
                    display: flex; flex-direction: column;
                    border-bottom: 1px dashed rgba(255,255,255,0.4);
                    padding-bottom: 4px;
                }}
                .course-block:last-child {{ border-bottom: none; padding-bottom: 0; }}

                .name {{ font-weight: bold; font-size: 11px; line-height: 1.3; margin-bottom: 2px; }}
                .loc {{ font-size: 10px; opacity: 0.9; }}
            </style>
        </head>
        <body>
            <div class="card">
                <div class="title">第 {target_week} 周 课程表</div>
                <div class="grid">
                    <div class="cell header" style="grid-area: 1 / 1;"></div>
                    <div class="cell header" style="grid-area: 1 / 2;">周一</div>
                    <div class="cell header" style="grid-area: 1 / 3;">周二</div>
                    <div class="cell header" style="grid-area: 1 / 4;">周三</div>
                    <div class="cell header" style="grid-area: 1 / 5;">周四</div>
                    <div class="cell header" style="grid-area: 1 / 6;">周五</div>
                    <div class="cell header" style="grid-area: 1 / 7;">周六</div>
                    <div class="cell header" style="grid-area: 1 / 8;">周日</div>
                    {grid_background}
                    {course_html}
                </div>
            </div>
        </body>
        </html>"#,
            target_week = target_week,
            grid_background = Self::generate_bg_cells(),
            course_html = course_html
        )
    }

    // 辅助函数：生成背景网格，确保 1-12 数字绝对对齐
    fn generate_bg_cells() -> String {
        let mut bg = String::new();
        for row in 2..=13 {
            // 时间数字
            bg.push_str(&format!(
                r#"<div class="cell time-slot" style="grid-area: {row} / 1;">{}</div>"#,
                row - 1
            ));
            // 空白网格线填充
            for col in 2..=8 {
                bg.push_str(&format!(
                    r#"<div class="cell" style="grid-area: {row} / {col};"></div>"#
                ));
            }
        }
        bg
    }

    fn render_to_file_inner(
        schedule: &Schedule,
        week: usize,
        path: &std::path::PathBuf,
    ) -> Result<()> {
        let html = Self::generate_html(schedule, week);

        let browser = Browser::new(LaunchOptions {
            headless: true,
            window_size: Some((1400, 1800)),
            ..Default::default()
        })?;

        let tab = browser.new_tab()?;
        let data_url = format!(
            "data:text/html;charset=utf-8,{}",
            urlencoding::encode(&html)
        );
        tab.navigate_to(&data_url)?;
        tab.wait_until_navigated()?;

        // 给浏览器一点渲染时间，防止 Grid 布局未完成
        std::thread::sleep(std::time::Duration::from_millis(500));

        let element = tab.wait_for_element(".card")?;
        let box_model = element.get_box_model()?;

        // ElementQuad 包含 p1(左上), p2(右上), p3(右下), p4(左下)
        let p1 = box_model.content.top_left;
        let p2 = box_model.content.top_right;
        let p3 = box_model.content.bottom_right;

        // 计算宽度 (右上 x - 左上 x) 和高度 (右下 y - 右上 y)
        let width = (p2.x - p1.x).abs();
        let height = (p3.y - p2.y).abs();

        let clip = headless_chrome::protocol::cdp::Page::Viewport {
            x: p1.x,
            y: p1.y,
            width,
            height,
            scale: 1.0,
        };

        tab.evaluate("window.scrollTo(0, 0);", false)?;

        let png_data = tab.capture_screenshot(
            headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
            None,
            Some(clip),
            true,
        )?;

        std::fs::write(path, png_data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use crate::api::xmu_service::jw::ScheduleList;

    use super::*;

    fn wait_for_keypress() {
        print!("程序已暂停，请按 Enter 键继续...");
        std::io::stdout().flush().unwrap(); // 确保文字立即显示

        // 只读取一个字节（这通常需要按 Enter 确认）
        // 如果需要捕捉“任意键”而不需要回车，通常需要使用 `console` 或 `crossterm` 库
        let _ = std::io::stdin().read(&mut [0u8]);
    }

    #[tokio::test(flavor = "multi_thread")]
    pub async fn test() -> Result<()> {
        let castgc = "TGT-4017429-6KAhATeeVXolstMjtOxHIv1EHDxnJejNaDlXvFiIYazONlAgn0ijGNwjysYzgJCi8iQnull_main";
        let schedule_list = ScheduleList::get(castgc).await?;
        let schedule = Schedule::get(castgc, &schedule_list.datas.kfdxnxqcx.rows[1]).await?;
        let temp_file = ScheduleRenderer::render_to_file(&schedule, 9)
            .await
            .unwrap();
        println!("生成文件路径: {:?}", temp_file.get_path());
        wait_for_keypress();
        Ok(())
    }
}
