use super::BuildHelp;
use genai::chat::ChatMessage;
use rand::SeedableRng;
use rand::rngs::SmallRng;
use tracing::debug;

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
    let mut rng = SmallRng::from_os_rng();

    let fortune_senso_ji = random_senso_ji_fortune(&mut rng);
    let fortune_ruanyf = random_ruanyf_fortune(&mut rng);

    //From https://github.com/Tamshen/senso-ji-stick-data
    ctx.send_message_async(from_str(fortune_senso_ji));
    //From https://github.com/ruanyf/fortunes
    ctx.send_message_async(from_str(fortune_ruanyf));

    let message = ctx.get_message();
    let sender = message.get_sender();

    if let Some(user_id) = sender.user_id {
        let impression = get_impression(user_id as i32).await;

        let prompt = vec![
            ChatMessage::system(
                "你是一个命理大师，请基于以下印象内容和求签结果，为用户进行一次简短的解签，回答要简洁明了，且富有哲理。",
            ),
            ChatMessage::system(format!("印象内容如下：{:?}", impression)),
            ChatMessage::user(format!(
                "求签结果: {}\n\n\n\n{}\n\n\n\n",
                fortune_senso_ji, fortune_ruanyf
            )),
        ];

        debug!(?prompt, "Fate Prompt");

        let res = ask_str(prompt).await?;

        ctx.send_message_async(from_str(format!("AI解答:\n{}", res)));
    }

    Ok(())
}
