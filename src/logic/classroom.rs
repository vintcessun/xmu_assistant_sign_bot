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
use tracing::{debug, trace, warn};

#[handler(msg_type=Message,command="class",echo_cmd=true,
help_msg=r#"用法:/class <描述>
<描述>:描述课程，后端使用LLM进行智能识别查询指定的课程的课堂互动信息
功能: 查询指定课程的课堂互动信息"#)]
pub async fn class(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&ctx).await?;
    let msg_text = ctx.get_message_text();
    let course_id = {
        let course = ChooseCourse::get_from_client(&client, msg_text).await?;
        trace!(course = ?course, "LLM 返回课程选择结果");
        course.course_id
    }
    .ok_or_else(|| {
        warn!("LLM 未能从输入中识别出课程");
        anyhow!("未找到课程，请更加清晰的阐释课程的名称")
    })?;
    debug!(course_id = ?course_id, "成功识别课程 ID，开始查询课堂互动数据");

    let classroom_data = ClassroomList::get_from_client(&client, course_id).await?;
    debug!(
        count = classroom_data.classrooms.len(),
        "成功获取 {} 个课堂互动信息",
        classroom_data.classrooms.len()
    );

    for classroom in classroom_data.classrooms {
        trace!(classroom = ?classroom, "处理当前课堂互动信息");
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
    let id = ctx.get_message_number::<i64>()?;
    debug!(class_id = id, "成功解析课堂互动 ID");

    debug!(class_id = id, "开始获取课堂互动内容 ClassroomSubject 数据");
    let distribute = ClassroomSubject::get_from_client(&client, id).await?;

    let client = Arc::new(client);

    debug!(class_id = id, "开始解析课堂互动内容");
    let result = distribute.parse(client).await?;

    for msg in result.message.build_chunk(30) {
        ctx.send_message_async(msg);
    }

    let task = MdTask::new(result.markdown);
    debug!(class_id = id, "创建 Markdown 任务");

    ctx.send_message_async(from_str(format!(
        "小测内容已生成，访问链接下载或预览：{}",
        task.get_url()
    )));

    task.finish().await?;

    Ok(())
}
