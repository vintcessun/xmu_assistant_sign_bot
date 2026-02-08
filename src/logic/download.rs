use super::BuildHelp;
use crate::{
    abi::{
        logic_import::*,
        message::{MessageSend, from_str},
    },
    api::xmu_service::{
        llm::{ChooseCourse, ChooseFiles},
        lnt::FileUrl,
    },
    logic::helper::get_client_or_err,
    web::file::task::ExposeFileTask,
};
use anyhow::{anyhow, bail};
use std::sync::Arc;
use tracing::trace;

#[handler(msg_type=Message,command="download",echo_cmd=true,
help_msg=r#"用法:/download <描述>
<描述>:描述课程及文件，后端使用LLM进行智能识别查询，如果没有提到使用哪个 文件那么就会下载这门课的全部文件
功能: 下载指定课程文件"#)]
pub async fn download(ctx: Context) -> Result<()> {
    let client = get_client_or_err(&ctx).await?;
    let msg_text = ctx.get_message_text();
    let course_id = {
        let course = ChooseCourse::get_from_client(&client, msg_text).await?;
        trace!("返回课程选择结果：");
        trace!(?course);
        course.course_id
    }
    .ok_or(anyhow!("未找到课程，请更加清晰的阐释课程的名称"))?;
    let files = {
        trace!("选择课程 ID: {}", course_id);
        let files = ChooseFiles::get_from_client(&client, msg_text, *course_id).await?;
        trace!("返回文件选择结果：");
        trace!(?files);
        files.files
    };
    if files.is_empty() {
        bail!("未找到符合条件的文件，请更加清晰的阐释文件的名称");
    }

    let mut tasks = Vec::with_capacity(files.len());

    let client = Arc::new(client);

    for file in files {
        let c = client.clone();
        tasks.push(tokio::spawn(async move {
            for _ in 0..3 {
                match FileUrl::get_from_client(c.clone(), file.reference_id, &file.name).await {
                    Ok(f) => return Ok(f),
                    Err(e) => {
                        trace!("下载文件 {:?} 失败，重试中... 错误信息: {}", file, e);
                    }
                }
            }
            Err(anyhow!("多次尝试后下载文件 {:?} 失败", file))
        }));
    }

    let mut files = Vec::with_capacity(tasks.len());
    for res in futures_util::future::join_all(tasks).await {
        let file = res?;
        match file {
            Ok(f) => {
                let url = f.get_url().await;
                ctx.send_message_async(MessageSend::new_message().file(url).build());
                files.push(f);
            }
            Err(e) => ctx.send_message_async(from_str(format!("下载文件失败: {}", e))),
        }
    }

    let task = ExposeFileTask::new(files);

    let url = task.get_url();

    ctx.send_message_async(from_str(format!("文件准备好了在地址 {url}")));

    task.finish().await?;

    Ok(())
}
