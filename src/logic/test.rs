use std::sync::Arc;

use super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::from_str},
    api::xmu_service::{
        llm::ChooseCourse,
        lnt::{Distribute, Exams, Submissions, SubmissionsId},
    },
    logic::helper::get_client_or_err,
    web::md::task::MdTask,
};
use anyhow::anyhow;
use tracing::{debug, error, trace, warn};

#[handler(msg_type=Message,command="test",echo_cmd=true,
help_msg=r#"用法:/test <描述>
<描述>:描述课程，后端使用LLM进行智能识别查询指定的课程的测试信息
功能: 查询指定课程的测试信息"#)]
pub async fn test(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&ctx).await?;
    let msg_text = ctx.get_message_text();
    debug!(input = msg_text, "使用 LLM 识别课程");

    let course_id = {
        let course = ChooseCourse::get_from_client(&client, msg_text).await?;
        trace!(course = ?course, "LLM 返回课程选择结果");
        course.course_id
    }
    .ok_or_else(|| {
        warn!("未找到课程，请更加清晰的阐释课程的名称");
        anyhow!("未找到课程，请更加清晰的阐释课程的名称")
    })?;

    debug!(course_id = ?course_id, "成功识别课程 ID，开始查询小测数据");
    let exam_data = Exams::get_from_client(&client, course_id).await?;
    debug!(
        count = exam_data.exams.len(),
        "成功获取 {} 个小测信息",
        exam_data.exams.len()
    );

    for exam in exam_data.exams {
        trace!(exam = ?exam, "处理当前小测信息");
        ctx.send_message_async(from_str(format!(
            r#"小测名称: {}
小测开始时间: {}
小测结束时间: {}
小测是否开始: {}
小测ID: {}"#,
            exam.title,
            Exams::to_beijing_date(&exam.start_time),
            Exams::to_beijing_date(&exam.end_time),
            exam.is_started,
            exam.id
        )));
    }

    Ok(())
}

#[handler(msg_type=Message,command="gettest",echo_cmd=true,
help_msg=r#"用法:/gettest <ID>
<ID>: 查询小测的ID，通过 /test 命令获取
功能: 查询指定小测的内容"#)]
pub async fn get_test(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&ctx).await?;
    let id_text = ctx
        .get_message_text()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>();
    let id = id_text.parse::<i64>().map_err(|e| {
        error!(input = id_text, error = ?e, "无效的小测 ID");
        anyhow!("不是有效的ID: {}\n可以通过/test 获取ID", e)
    })?;
    debug!(quiz_id = id, "成功解析小测 ID");

    debug!(quiz_id = id, "开始获取小测内容 Distribute 数据");
    let distribute = Distribute::get_from_client(&client, id).await?;

    let client = Arc::new(client);

    debug!(quiz_id = id, "开始解析小测内容");
    let result = distribute.parse(client).await?;

    for msg in result.message.build_chunk(30) {
        ctx.send_message_async(msg);
    }

    let task = MdTask::new(result.markdown);
    debug!(quiz_id = id, "创建 Markdown 任务");

    ctx.send_message_async(from_str(format!(
        "小测内容已生成，访问链接下载或预览：{}",
        task.get_url()
    )));

    task.finish().await?;

    Ok(())
}

#[handler(msg_type=Message,command="testans",echo_cmd=true,
help_msg=r#"用法:/testans <ID>
<ID>: 查询小测的ID，通过 /test 命令获取
功能: 查询小测的答案，如果老师有公布的话"#)]
pub async fn test_ans(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&ctx).await?;
    let id_text = ctx
        .get_message_text()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>();
    let id = id_text.parse::<i64>().map_err(|e| {
        warn!(input = id_text, error = ?e, "无效的小测 ID");
        anyhow!("不是有效的ID: {}\n可以通过/test 获取ID", e)
    })?;
    debug!(quiz_id = id, "成功解析小测 ID");

    debug!(quiz_id = id, "开始获取小测答案 Submissions 数据");
    let submissions = Submissions::get_from_client(&client, id).await?;

    let submission_id = submissions
        .submissions
        .first()
        .ok_or_else(|| {
            warn!("未找到小测答案，Submissions 列表为空");
            anyhow!("未找到小测答案，请确认老师是否公布答案")
        })?
        .id;
    debug!(
        quiz_id = id,
        submission_id = submission_id,
        "成功获取答案 Submission ID"
    );

    debug!(submission_id = submission_id, "开始获取具体答案内容");
    let submission = SubmissionsId::get_from_client(&client, id, submission_id).await?;

    let client = Arc::new(client);

    debug!(submission_id = submission_id, "开始解析答案内容");
    let result = submission.parse(client).await?;

    for msg in result.message.build_chunk(30) {
        ctx.send_message_async(msg);
    }

    let task = MdTask::new(result.markdown);
    debug!(submission_id = submission_id, "创建 Markdown 任务");

    ctx.send_message_async(from_str(format!(
        "小测答案已生成，访问链接下载或预览：{}",
        task.get_url()
    )));

    task.finish().await?;

    Ok(())
}
