use super::BuildHelp;
use crate::{
    abi::{logic_import::*, message::MessageSend},
    api::llm::{chat::archive::bridge::llm_msg_from_message_without_archive, tool::generate_image},
};
use tracing::trace;

#[handler(msg_type=Message,command="image",echo_cmd=true,
help_msg=r#"用法:/image <内容>
<内容>:生成图片的提示词
功能:使用 gemini-3-pro-image 进行图片生成"#)]
pub async fn image(ctx: Context) -> Result<()> {
    let msg = ctx.get_message();

    trace!("收到生成图片消息: {:?}", msg);
    let msg = llm_msg_from_message_without_archive(ctx.client.clone(), &msg).await;
    trace!("转化为 LLM 消息: {:?}", msg);

    let img = generate_image(msg).await?;
    trace!("生成图片结果: {:?}", img);

    let message = MessageSend::new_message()
        .image(img.to_fileurl().await?)
        .build();

    ctx.send_message_async(message);

    Ok(())
}
