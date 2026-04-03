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
use tracing::{debug, error, info, trace, warn};

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

    let mut tasks = Vec::with_capacity(files.len());

    let client = Arc::new(client);

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
                        warn!(file_name = file.name, retry_count = i, error = ?e, "下载文件失败，正在重试");
                    }
                }
            }
            error!(file = ?file, "多次尝试后下载文件失败");
            Err(anyhow!("多次尝试后下载文件 {:?} 失败", file))
        }));
    }

    let mut files = Vec::with_capacity(tasks.len());
    for res in futures_util::future::join_all(tasks).await {
        let res_inner = res?;
        match res_inner {
            Ok(f) => {
                let url = f.get_url().await;
                info!(file_url = url, "准备发送文件链接");
                ctx.send_message_async(MessageSend::new_message().file(url).build());
                files.push(f);
            }
            Err(e) => {
                error!(error = ?e, "文件下载任务失败");
                ctx.send_message_async(from_str(format!("下载文件失败: {}", e)))
            }
        }
    }

    debug!("创建 ExposeFileTask");
    let task = ExposeFileTask::new(files);

    let url = task.get_url();
    info!(url = url, "文件暴露任务已创建");

    ctx.send_message_async(from_str(format!("文件准备好了在地址 {url}")));

    task.finish().await?;

    Ok(())
}
