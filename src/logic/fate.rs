use super::BuildHelp;
use crate::api::llm::chat::tool::time::time_info_getter;
use genai::chat::ChatMessage;
use tracing::{debug, trace, warn};

// 引入 build.rs 生成的 omikuji 模块
include!(concat!(env!("OUT_DIR"), "/omikuji.rs"));

use crate::{
    abi::{logic_import::*, message::from_str},
    api::llm::{chat::impression::get_impression, tool::ask_str},
};

#[handler(msg_type=Message,command="fate",echo_cmd=true,
help_msg=r#"用法:/fate
功能:用于求签，本功能基于 AI 模型生成的概率预测，仅供娱乐。命运掌握在自己手中，请务必相信科学，拒绝迷信。"#)]
pub async fn fate(ctx: Context) -> Result<()> {
    // 使用随机数，确保每次求签结果不同
    let mut rng = rand::make_rng();

    let fortune_senso_ji = random_senso_ji_fortune(&mut rng);
    let fortune_ruanyf = random_ruanyf_fortune(&mut rng);
    trace!(
        senso_ji = fortune_senso_ji,
        ruanyf = fortune_ruanyf,
        "成功抽取两份签文"
    );

    //From https://github.com/Tamshen/senso-ji-stick-data
    ctx.send_message_async(from_str(fortune_senso_ji));
    //From https://github.com/ruanyf/fortunes
    ctx.send_message_async(from_str(fortune_ruanyf));

    let message = ctx.get_message();
    let sender = message.get_sender();

    if let Some(user_id) = sender.user_id {
        debug!(user_id = user_id, "开始获取用户印象并进行 AI 解签");
        let impression = get_impression(user_id).await;
        debug!(user_id = user_id, impression = ?impression, "成功获取用户印象");

        let time_info = time_info_getter().await.unwrap_or_else(|e| {
            warn!(error = ?e, "获取时间信息失败，使用默认值");
            "无法获取时间信息".to_string()
        });

        let prompt = vec![
            ChatMessage::system(
                "你是一个命理大师，请基于以下印象内容和求签结果，为用户进行一次简短的解签，回答要简洁明了，且富有哲理。",
            ),
            ChatMessage::system(format!("印象内容如下：{:?}", impression)),
            ChatMessage::system(format!("当前时间信息如下：{}", time_info)),
            ChatMessage::user(format!(
                "求签结果: {}\n\n\n\n{}\n\n\n\n",
                fortune_senso_ji, fortune_ruanyf
            )),
        ];

        debug!(prompt = ?prompt, "用于解签的 LLM 提示词");

        let res = ask_str(prompt).await?;
        debug!("成功获得 AI 解签结果");

        trace!(ai_response = res, "发送 AI 解签结果");
        ctx.send_message_async(from_str(format!("AI解答:\n{}", res)));
    } else {
        warn!(
            message = ?ctx.message,
            "未获取到消息发送者用户 ID，跳过 AI 解签流程"
        );
    }

    Ok(())
}
