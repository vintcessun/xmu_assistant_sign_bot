use std::sync::Arc;

use super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::from_str},
    api::xmu_service::{
        llm::ChooseCourse,
        lnt::{ClassroomList, ClassroomSubject},
    },
    logic::helper::get_client_or_err,
    web::md::task::MdTask,
};
use anyhow::anyhow;
use tracing::trace;

#[handler(msg_type=Message,command="class",echo_cmd=true,
help_msg=r#"用法:/class <描述>
<描述>:描述课程，后端使用LLM进行智能识别查询指定的课程的课堂互动信息
功能: 查询指定课程的课堂互动信息"#)]
pub async fn class(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&ctx).await?;
    let msg_text = ctx.get_message_text();
    let course_id = {
        let course = ChooseCourse::get_from_client(&client, msg_text).await?;
        trace!("返回课程选择结果：");
        trace!(?course);
        course.course_id
    }
    .ok_or(anyhow!("未找到课程，请更加清晰的阐释课程的名称"))?;

    let classroom_data = ClassroomList::get_from_client(&client, *course_id).await?;

    for classroom in classroom_data.classrooms {
        trace!("测试信息：{:?}", classroom);
        ctx.send_message_async(from_str(format!(
            r#"小测名称: {}
小测开始时间: {}
小测结束时间: {}
小测状态: {}
小测ID: {}"#,
            classroom.title,
            classroom.start_at,
            classroom.finish_at,
            classroom.status,
            classroom.id
        )));
    }

    Ok(())
}

#[handler(msg_type=Message,command="getclass",echo_cmd=true,
help_msg=r#"用法:/getclass <ID>
<ID>: 查询小测的ID，通过 /class 命令获取
功能: 查询指定课堂互动小测的内容"#)]
pub async fn get_class(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&ctx).await?;
    let id = ctx
        .get_message_text()
        .split_whitespace()
        .collect::<String>()
        .parse::<i64>()
        .map_err(|e| anyhow!("不是有效的ID: {}\n可以通过/class 获取ID", e))?;

    let distribute = ClassroomSubject::get_from_client(&client, id).await?;

    let client = Arc::new(client);

    let result = distribute.parse(client).await?;

    for msg in result.message.build_chunk(30) {
        ctx.send_message_async(msg);
    }

    let task = MdTask::new(result.markdown);

    ctx.send_message_async(from_str(format!(
        "小测内容已生成，访问链接下载或预览：{}",
        task.get_url()
    )));

    task.finish().await?;

    Ok(())
}
