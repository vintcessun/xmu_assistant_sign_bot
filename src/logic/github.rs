use super::BuildHelp;
use crate::abi::logic_import::*;
use crate::abi::message::{MessageSend, file::FileUrl};
use crate::config::DATA_DIR;
use anyhow::{Context as _, Result};
use std::path::Path;
use tracing::warn;

// 赞赏码由 build.rs 嵌进二进制（和 wechat_qrcode 模型一个路子）。
include!(concat!(env!("OUT_DIR"), "/reward_data.rs"));

const REPO: &str = "https://github.com/vintcessun/xmu_assistant_sign_bot";

/// 把内嵌的赞赏码释放到数据目录，返回 `file://` 地址交给 NapCat。
/// 课表图片走的也是 `file://`，说明 NapCat 与本进程共享文件系统。
async fn reward_qrcode() -> Result<FileUrl> {
    let path = Path::new(DATA_DIR).join("reward_qrcode.png");

    // 换过图之后长度会变，按长度判一下，免得还要手动删旧文件。
    let need_write = match tokio::fs::metadata(&path).await {
        Ok(meta) => meta.len() != REWARD_QRCODE_PNG.len() as u64,
        Err(_) => true,
    };
    if need_write {
        tokio::fs::write(&path, REWARD_QRCODE_PNG)
            .await
            .context("释放赞赏码图片失败")?;
    }

    FileUrl::from_path(&path)
}

#[handler(msg_type=Message,command="github",echo_cmd=true,
help_msg=r#"用法:/github
功能:获取项目github地址和赞赏码 https://github.com/vintcessun/xmu_assistant_sign_bot"#)]
pub async fn github(ctx: Context) -> Result<()> {
    let mut message =
        MessageSend::new_message().text(format!("{REPO}\n开源不易，觉得好用可以请作者喝杯咖啡："));

    match reward_qrcode().await {
        Ok(url) => message = message.image(url),
        // 图片释放失败不该连仓库地址都发不出去。
        Err(e) => warn!(error = ?e, "赞赏码图片不可用，本次只发仓库地址"),
    }

    ctx.send_message_async(message.build());

    Ok(())
}
