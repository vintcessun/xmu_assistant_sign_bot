use super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::MessageSend},
    api::llm::{chat::archive::bridge::llm_msg_from_message_without_archive, tool::generate_image},
};
use tracing::{debug, trace};

#[handler(msg_type=Message,command="image",echo_cmd=true,
help_msg=r#"用法:/image <内容>
<内容>:生成图片的提示词
功能:使用 gemini-3-pro-image 进行图片生成"#)]
pub async fn image(ctx: Context) -> Result<()> {
    let msg = ctx.get_message();

    trace!(message = ?msg, "收到生成图片请求消息");
    let msg = llm_msg_from_message_without_archive(ctx.client.clone(), &msg).await;
    debug!(llm_message = ?msg, "消息已转化为 LLM 格式，内容将作为提示词");

    let img = generate_image(msg).await?;
    debug!(result = ?img, "图片生成结果");

    let fileurl = img.to_fileurl().await?;
    trace!(file_url = ?fileurl, "图片文件 URL");

    let message = MessageSend::new_message().image(fileurl).build();

    ctx.send_message_async(message);

    Ok(())
}
