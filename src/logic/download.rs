use super::BuildHelp;
use crate::{
    abi::{
        logic_import::*,
        message::{MessageSend, from_str},
    },
    api::network::api_status_of,
    api::xmu_service::{
        llm::{ChooseCourse, ChooseFiles},
        lnt::FileUrl,
    },
    logic::helper::get_client_or_err,
    web::file::task::ExposeFileTask,
};
use anyhow::{anyhow, bail};
use std::collections::BTreeMap;
use tracing::{debug, error, info, trace, warn};

/// 把 lnt 的 `2026-07-17T07:42:00Z` 截成 `2026-07-17`。给用户看日期就够了；
/// 解析不出形状就不显示，别把原始串糊到消息里。
fn format_closed_at(raw: &str) -> Option<&str> {
    let day = raw.split('T').next()?;
    (day.len() == 10 && day.split('-').count() == 3).then_some(day)
}

#[handler(msg_type=Message,command="download",echo_cmd=true,
help_msg=r#"用法:/download <描述>
<描述>:描述课程及文件，后端使用LLM进行智能识别查询，如果没有提到使用哪个 文件那么就会下载这门课的全部文件
功能: 下载指定课程文件"#)]
pub async fn download(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&mut ctx).await?;
    let msg_text = ctx.get_message_text();
    let course_id = {
        let course = ChooseCourse::get_from_client(&client, msg_text).await?;
        trace!(course = ?course, "已返回课程选择结果");
        course.course_id
    }
    .ok_or_else(|| {
        warn!("LLM 未能从输入中识别出课程");
        anyhow!("未找到课程，请更加清晰的阐释课程的名称")
    })?;
    debug!(course_id = ?course_id, "成功识别课程 ID");

    let files = {
        trace!(course_id = ?course_id, "已选择课程 ID, 开始识别文件");
        let files = ChooseFiles::get_from_client(&client, msg_text, course_id).await?;
        trace!(files = ?files, "已返回文件选择结果");
        files.files
    };
    if files.is_empty() {
        warn!("未找到符合条件的文件");
        bail!("未找到符合条件的文件，请更加清晰的阐释文件的名称");
    }
    debug!(
        file_count = files.len(),
        "找到 {} 个文件，开始异步下载",
        files.len()
    );

    // 活动一旦过了截止时间就会关闭，lnt 随即不再发放里面文件的下载地址（直接 403），
    // 但文件并没有被删。这类先摘出来单独说明，不为注定失败的请求白跑一趟。
    let (closed, files): (Vec<_>, Vec<_>) = files.into_iter().partition(|f| f.closed_at.is_some());

    let mut tasks = Vec::with_capacity(files.len());

    for file in files {
        let c = client.clone();
        tasks.push(tokio::spawn(async move {
            for i in 1..=3 {
                match FileUrl::get_from_client(c.clone(), file.reference_id, &file.name).await {
                    Ok(f) => {
                        debug!(file_name = file.name, "文件下载成功");
                        return Ok(f);
                    }
                    Err(e) => {
                        // 没权限 / 文件不存在这种再试一百次也是同一个结果。
                        // 课程里混着别人提交的作业报告，这类 403 是常态，不是故障：
                        // 立刻放弃并如实说明原因，别拿"多次尝试后失败"糊弄用户，
                        // 也别为一堆注定失败的文件白打三倍请求。
                        if let Some(status) = api_status_of(&e)
                            && status.is_permanent()
                        {
                            debug!(
                                file_name = file.name,
                                status = %status.status,
                                "文件不可下载，跳过重试"
                            );
                            return Err(anyhow!("{}「{}」", status.user_reason(), file.name));
                        }
                        warn!(file_name = file.name, retry_count = i, error = ?e, "下载文件失败，正在重试");
                    }
                }
            }
            error!(file = ?file, "多次尝试后下载文件失败");
            Err(anyhow!("多次尝试后下载「{}」失败", file.name))
        }));
    }

    let mut files = Vec::with_capacity(tasks.len());
    let mut failures = Vec::new();
    for res in futures_util::future::join_all(tasks).await {
        let res_inner = res?;
        match res_inner {
            Ok(f) => {
                let url = f.get_url().await;
                info!(file_url = url, "准备发送文件");
                ctx.send_message_async(MessageSend::new_message().file(url).build());
                files.push(f);
            }
            Err(e) => {
                error!(error = ?e, "文件下载任务失败");
                failures.push(format!("{}", e));
            }
        }
    }

    // 已关闭活动的文件按活动归并成一条说明。一个「专题报告集锦」底下就有二十多个文件，
    // 逐个报会把群刷爆，而且它们的原因完全相同。
    if !closed.is_empty() {
        let mut by_activity: BTreeMap<(String, String), usize> = BTreeMap::new();
        for f in &closed {
            let when = f.closed_at.clone().unwrap_or_default();
            *by_activity
                .entry((f.activity_title.clone(), when))
                .or_default() += 1;
        }
        let detail = by_activity
            .into_iter()
            .map(|((title, when), n)| match format_closed_at(&when) {
                Some(day) => format!("「{title}」已于 {day} 关闭，{n} 个文件"),
                None => format!("「{title}」已关闭，{n} 个文件"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        info!(count = closed.len(), "跳过已关闭活动的文件");
        ctx.send_message_async(from_str(format!(
            "{} 个文件所在的活动已关闭，无法下载（文件还在，只是过了截止时间）:\n{}",
            closed.len(),
            detail
        )));
    }

    // 其余失败也汇总成一条发，一个一条会把群刷爆。
    if !failures.is_empty() {
        ctx.send_message_async(from_str(format!(
            "{} 个文件没能下载:\n{}",
            failures.len(),
            failures.join("\n")
        )));
    }

    // 文件列表网页：把临时文件所有权交给任务保活，网页有效期（1 天）内可下载，
    // 过期后随任务一起清理，不再持久化。
    let task = ExposeFileTask::new(files);
    let url = task.get_url();
    info!(url = url, "文件暴露任务已创建");
    ctx.send_message_async(from_str(format!("文件准备好了在地址 {url}")));
    task.finish().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::xmu_service::llm::choose_files::File;

    fn file(id: i64, activity: &str, closed_at: Option<&str>) -> File {
        File {
            reference_id: id,
            name: format!("{activity}-文件{id}"),
            activity_title: activity.to_string(),
            closed_at: closed_at.map(str::to_string),
        }
    }

    #[test]
    fn closed_time_is_shown_as_a_date() {
        assert_eq!(format_closed_at("2026-07-17T07:42:00Z"), Some("2026-07-17"));
        assert_eq!(format_closed_at("2026-08-01T15:33:00Z"), Some("2026-08-01"));
    }

    /// 形状不对就不显示，别把原始串糊到用户消息里。
    #[test]
    fn unparseable_close_time_is_hidden() {
        assert_eq!(format_closed_at(""), None);
        assert_eq!(format_closed_at("不知道什么时候"), None);
        assert_eq!(format_closed_at("2026-7-17T07:42:00Z"), None);
    }

    /// 已关闭活动的文件要被摘出来，不去发那些注定 403 的请求。
    /// 数字取自线上那门课：62 个文件里 27 个来自 4 个已关闭的活动。
    #[test]
    fn closed_files_are_separated_from_downloadable_ones() {
        let files = vec![
            file(1, "专题报告集锦", Some("2026-07-17T07:42:00Z")),
            file(2, "专题报告集锦", Some("2026-07-17T07:42:00Z")),
            file(3, "0 课程概述", Some("2026-08-01T15:33:00Z")),
            file(4, "12 昇腾实验助手实现", None),
            file(5, "1 Linux基础", None),
        ];

        let (closed, open): (Vec<_>, Vec<_>) =
            files.into_iter().partition(|f| f.closed_at.is_some());

        assert_eq!(closed.len(), 3, "已关闭的要全部摘出来");
        assert_eq!(open.len(), 2, "进行中的照常下载");
        assert!(open.iter().all(|f| f.closed_at.is_none()));
    }

    /// 同一个活动下的几十个文件归并成一行，不逐个刷屏。
    #[test]
    fn closed_files_are_grouped_by_activity() {
        let closed = vec![
            file(1, "专题报告集锦", Some("2026-07-17T07:42:00Z")),
            file(2, "专题报告集锦", Some("2026-07-17T07:42:00Z")),
            file(3, "专题报告集锦", Some("2026-07-17T07:42:00Z")),
            file(4, "0 课程概述", Some("2026-08-01T15:33:00Z")),
        ];

        let mut by_activity: BTreeMap<(String, String), usize> = BTreeMap::new();
        for f in &closed {
            let when = f.closed_at.clone().unwrap_or_default();
            *by_activity
                .entry((f.activity_title.clone(), when))
                .or_default() += 1;
        }
        let detail = by_activity
            .into_iter()
            .map(|((title, when), n)| match format_closed_at(&when) {
                Some(day) => format!("「{title}」已于 {day} 关闭，{n} 个文件"),
                None => format!("「{title}」已关闭，{n} 个文件"),
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            );

        assert_eq!(
            detail,
            "「0 课程概述」已于 2026-08-01 关闭，1 个文件
「专题报告集锦」已于 2026-07-17 关闭，3 个文件"
        );
    }
}
