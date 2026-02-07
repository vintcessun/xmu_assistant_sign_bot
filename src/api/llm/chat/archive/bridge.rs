use std::sync::{Arc, LazyLock};

use crate::{
    abi::{
        echo::Echo,
        logic_import::{Message, Notice},
        message::{
            MessageReceive,
            api::GetForwardMsgParams,
            message_body::{SegmentReceive, contact},
        },
        network::BotClient,
        websocket::BotHandler,
    },
    api::llm::chat::{
        archive::{
            identity::{IdentityGroup, IdentityPerson},
            message_storage::MessageStorage,
        },
        file::LlmFile,
    },
};
use anyhow::anyhow;
use futures::{FutureExt, future::BoxFuture};
use genai::chat::{Binary, ChatMessage, ContentPart, MessageContent};
use tracing::error;

include!(concat!(env!("OUT_DIR"), "/face_data.rs"));

pub fn get_gif_from_exe(
    id: &str,
) -> Option<(&'static str, &'static str, &'static str, &'static str)> {
    FACES.get(id).copied()
}

static FACE_REFERENCE_MESSAGE: LazyLock<ChatMessage> =
    LazyLock::new(get_face_reference_message_inner);

pub fn get_face_reference_message() -> ChatMessage {
    FACE_REFERENCE_MESSAGE.clone()
}

pub fn get_face_reference_message_inner() -> ChatMessage {
    let mut parts = vec![ContentPart::Text(
        "以下是你可以使用的表情参考列表：".to_string(),
    )];

    for (_ct, _content, id, name) in FACES.values() {
        parts.push(ContentPart::Text(format!(
            "\n表情ID:{}\n表情名: {}\n",
            id, name
        )))
        // NOTICE: due to the limitation of the context length, we do not provide the actual image content in the prompt, instead, we provide the content type and name as a reference, and the actual image content can be retrieved from the provided API when needed.
        // if you want to provide the actual image content in the prompt, you can uncomment the following code, but please be aware of the context length limitation and adjust the prompt accordingly.
        /*
        parts.push(ContentPart::Binary(Binary::from_base64(
            *_ct,
            *_content,
            Some(name.to_string()),
        )));
         */
    }

    ChatMessage::system(parts) // 作为系统上下文发送
}

pub fn get_mime_type(file_name: &str) -> String {
    mime_guess::from_path(file_name)
        .first_or_octet_stream() // 如果猜不到，返回默认的 application/octet-stream
        .to_string()
}

async fn llm_msg_from_segment_receive<T>(client: Arc<T>, segment: &SegmentReceive) -> ChatMessage
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    match segment {
        SegmentReceive::Text(e) => ChatMessage::user(e.text.clone()),
        SegmentReceive::Face(e) => {
            let id = &e.id;
            let gif_data = get_gif_from_exe(id);
            match gif_data {
                Some(data) => {
                    let (content_type, content, _id, name) = data;
                    let parts = vec![
                        ContentPart::Text(format!("[face: {}]", id)),
                        ContentPart::Binary(Binary::from_base64(
                            content_type,
                            content,
                            Some(name.to_string()),
                        )),
                    ];
                    ChatMessage::user(parts)
                }
                None => ChatMessage::user(format!("[Unknown Face: {}]", id)),
            }
        }
        SegmentReceive::Image(e) => {
            let url = &e.url;
            match async {
                let file = LlmFile::from_url(url, e.file.clone()).await?;
                let file = file.embedded().await?;
                let binary_file = Binary::from_file(&file.file.path)?;
                Ok::<ChatMessage, anyhow::Error>(ChatMessage::user(ContentPart::Binary(
                    binary_file,
                )))
            }
            .await
            {
                Ok(e) => e,
                Err(err) => {
                    error!("获取图片文件URL失败，URL: {}, 错误信息: {}", url, err);
                    #[cfg(test)]
                    {
                        println!("获取文件URL失败，URL: {}, 错误信息: {}", url, err);
                        panic!();
                    }
                    #[cfg_attr(test, allow(unreachable_code))]
                    ChatMessage::user(ContentPart::Binary(Binary::from_url(
                        get_mime_type(&e.file),
                        url,
                        Some(e.file.clone()),
                    )))
                }
            }
        }
        SegmentReceive::Record(e) => {
            let url = &e.url;
            match async {
                let file = LlmFile::from_url(url, e.file.clone()).await?;
                let file = file.embedded().await?;
                let binary_file = Binary::from_file(&file.file.path)?;
                Ok::<ChatMessage, anyhow::Error>(ChatMessage::user(ContentPart::Binary(
                    binary_file,
                )))
            }
            .await
            {
                Ok(e) => e,
                Err(err) => {
                    error!("获取文件URL失败，URL: {}, 错误信息: {}", url, err);
                    ChatMessage::user(ContentPart::Binary(Binary::from_url(
                        get_mime_type(&e.file),
                        url,
                        Some(e.file.clone()),
                    )))
                }
            }
        }
        SegmentReceive::Video(e) => {
            let url = &e.url;
            match async {
                let file = LlmFile::from_url(url, e.file.clone()).await?;
                let file = file.embedded().await?;
                let binary_file = Binary::from_file(&file.file.path)?;
                Ok::<ChatMessage, anyhow::Error>(ChatMessage::user(ContentPart::Binary(
                    binary_file,
                )))
            }
            .await
            {
                Ok(e) => e,
                Err(err) => {
                    error!("获取文件URL失败，URL: {}, 错误信息: {}", url, err);
                    ChatMessage::user(ContentPart::Binary(Binary::from_url(
                        get_mime_type(&e.file),
                        url,
                        Some(e.file.clone()),
                    )))
                }
            }
        }
        SegmentReceive::At(e) => {
            let user_id = &e.qq;
            let qq_i64 = user_id.parse::<i64>().unwrap_or(0);
            let identity = IdentityPerson::get(qq_i64).await;
            let identity_data = match identity {
                Some(data) => quick_xml::se::to_string(&data).unwrap_or("未知身份".to_string()),
                None => "未知身份".to_string(),
            };
            ChatMessage::user(format!("[At {user_id}]<data>{identity_data}</data>"))
        }
        SegmentReceive::Rps(_) => ChatMessage::user("[RPS 猜拳魔法表情]"),
        SegmentReceive::Dice(_) => ChatMessage::user("[Dice 掷骰子魔法表情]"),
        SegmentReceive::Poke(e) => {
            let type_id = &e.r#type;
            let id = &e.id;
            let name = &e.name;

            ChatMessage::user(format!(
                "[戳一戳消息, ID: ({},{}), 名称: {}]",
                type_id, id, name
            ))
        }
        SegmentReceive::Share(e) => {
            let content = vec![
                ContentPart::Text(format!(
                    "[分享链接 标题: {} 链接: {} 内容: {}]",
                    e.title, e.url, e.content,
                )),
                {
                    let url = &e.image;
                    match async {
                        let file = LlmFile::from_url(url, e.image.clone()).await?;
                        let file = file.embedded().await?;
                        let binary_file = Binary::from_file(&file.file.path)?;
                        Ok::<ContentPart, anyhow::Error>(ContentPart::Binary(binary_file))
                    }
                    .await
                    {
                        Ok(e) => e,
                        Err(err) => {
                            error!("获取文件URL失败，URL: {}, 错误信息: {}", url, err);
                            ContentPart::Binary(Binary::from_url(
                                get_mime_type(&e.image),
                                url,
                                Some(e.image.clone()),
                            ))
                        }
                    }
                },
            ];
            ChatMessage::user(content)
        }
        SegmentReceive::Contact(e) => match e {
            contact::DataReceive::Group(g) => {
                let group_id = &g.id;
                let group_i64 = group_id.parse::<i64>().unwrap_or(0);
                let identity = IdentityGroup::get(group_i64);
                let identity_data = match identity.await {
                    Some(data) => {
                        quick_xml::se::to_string(&data).unwrap_or("未知群身份".to_string())
                    }
                    None => "未知群身份".to_string(),
                };
                ChatMessage::user(format!(
                    "[推荐群聊 {}]<data>{}</data>",
                    group_id, identity_data
                ))
            }
            contact::DataReceive::Qq(q) => {
                let qq = &q.id;
                let qq_i64 = qq.parse::<i64>().unwrap_or(0);
                let identity = IdentityPerson::get(qq_i64);
                let identity_data = match identity.await {
                    Some(data) => {
                        quick_xml::se::to_string(&data).unwrap_or("未知群身份".to_string())
                    }
                    None => "未知群身份".to_string(),
                };
                ChatMessage::user(format!("[推荐群聊 {}]<data>{}</data>", qq, identity_data))
            }
        },
        SegmentReceive::Location(e) => ChatMessage::user(format!(
            "[位置 {} 内容: {} 经度: {} 纬度: {}]",
            e.title, e.content, e.lon, e.lat,
        )),
        SegmentReceive::Reply(e) => {
            let msg_id = e.id.clone();
            let content = vec![ContentPart::Text(format!("[回复消息 ID: {}]", msg_id))];
            let msg_content = match MessageStorage::get(msg_id).await {
                Some(c) => {
                    let mut content = MessageContent::from(content);
                    content.extend(c.content);
                    content
                }
                None => MessageContent::from(content),
            };
            ChatMessage::user(msg_content)
        }
        SegmentReceive::Forward(e) => {
            let id = e.id.clone();
            let content = vec![ContentPart::Text(format!("[转发消息 id: {id}]"))];

            let msg = MessageStorage::get(id.clone()).await;

            if msg.is_none() {
                match async {
                    let call = client
                        .call_api(
                            &GetForwardMsgParams {
                                message_id: id.clone(),
                            },
                            Echo::new(),
                        )
                        .await?;

                    let data = call.wait_echo().await?;
                    let msg_data = data.data.ok_or(anyhow!("获取转发消息数据失败"))?.messages;

                    let mut msg = Vec::new();
                    for m in &msg_data {
                        let segment_msgs = llm_msg_from_message(client.clone(), m).await;
                        msg.extend(segment_msgs);
                    }

                    MessageStorage::save(id.clone(), msg).await;

                    Ok::<(), anyhow::Error>(())
                }
                .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        error!("获取转发消息失败，错误信息: {}", e);
                    }
                }
            }

            let msg = MessageStorage::get(id).await;

            let msg_content = match msg {
                Some(e) => {
                    let mut content = MessageContent::from(content);
                    content.extend(e.content);
                    content
                }
                None => MessageContent::from(content),
            };
            ChatMessage::user(msg_content)
        }
        SegmentReceive::Xml(e) => ChatMessage::user(format!("[XML消息 {}]", e.data)),
        SegmentReceive::Json(e) => ChatMessage::user(format!("[JSON消息 {}]", e.data)),
        SegmentReceive::File(e) => {
            let url = &e.url;
            match async {
                let file = LlmFile::from_url(url, e.file.clone()).await?;
                let file = file.embedded().await?;
                let binary_file = Binary::from_file(&file.file.path)?;
                Ok::<ChatMessage, anyhow::Error>(ChatMessage::user(ContentPart::Binary(
                    binary_file,
                )))
            }
            .await
            {
                Ok(e) => e,
                Err(err) => {
                    error!("获取文件URL失败，URL: {}, 错误信息: {}", url, err);
                    ChatMessage::user(ContentPart::Binary(Binary::from_url(
                        get_mime_type(&e.file),
                        url,
                        Some(e.file.clone()),
                    )))
                }
            }
        }
    }
}

pub fn llm_msg_from_message_receive<T>(
    client: Arc<T>,
    message: &MessageReceive,
) -> BoxFuture<'_, Vec<ChatMessage>>
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    async move {
        match message {
            MessageReceive::Array(e) => {
                let mut result = Vec::with_capacity(e.len());
                for seg in e.iter() {
                    result.push(llm_msg_from_segment_receive(client.clone(), seg).await);
                }
                result
            }
            MessageReceive::Single(e) => {
                vec![llm_msg_from_segment_receive(client.clone(), e).await]
            }
        }
    }
    .boxed()
}

pub async fn llm_msg_from_message_without_archive<T>(
    client: Arc<T>,
    message: &Message,
) -> Vec<ChatMessage>
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    match message {
        Message::Private(p) => {
            let data = quick_xml::se::to_string(&p).unwrap_or("未知消息".to_string());
            let mut ret = vec![ChatMessage::user(format!("<data>{}</data>", data))];
            ret.extend(llm_msg_from_message_receive(client.clone(), &p.message).await);
            ret
        }
        Message::Group(g) => {
            let data = quick_xml::se::to_string(&g).unwrap_or("未知消息".to_string());
            let mut ret = vec![ChatMessage::user(format!("<data>{}</data>", data))];
            ret.extend(llm_msg_from_message_receive(client.clone(), &g.message).await);
            ret
        }
    }
}

pub async fn llm_msg_from_message<T>(client: Arc<T>, message: &Message) -> Vec<ChatMessage>
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    archive_message_files(message).await;
    llm_msg_from_message_without_archive(client.clone(), message).await
}

pub async fn llm_msg_from_notice(notice: &Notice) -> ChatMessage {
    ChatMessage::user(quick_xml::se::to_string(notice).unwrap_or("未知提示".to_string()))
}

async fn archive_message_file_inner(url: &str, name: String) {
    match async move {
        let file = LlmFile::from_url(url, name).await?;
        file.embedded().await?;
        Ok::<(), anyhow::Error>(())
    }
    .await
    {
        Ok(_) => {}
        Err(e) => {
            error!("归档embedding文件失败，错误信息: {}", e);
        }
    }
}

pub async fn archive_message_files(message: &Message) {
    let segments = match message {
        Message::Private(p) => &p.message,
        Message::Group(g) => &g.message,
    };

    let segments = match segments {
        MessageReceive::Array(e) => e.iter().collect::<Vec<&SegmentReceive>>(),
        MessageReceive::Single(e) => vec![e],
    };

    for segment in segments {
        match segment {
            SegmentReceive::Image(e) => {
                let url = &e.url;
                let name = e.file.clone();
                archive_message_file_inner(url, name).await;
            }
            SegmentReceive::Record(e) => {
                let url = &e.url;
                let name = e.file.clone();
                archive_message_file_inner(url, name).await;
            }
            SegmentReceive::Video(e) => {
                let url = &e.url;
                let name = e.file.clone();
                archive_message_file_inner(url, name).await;
            }
            SegmentReceive::File(e) => {
                let url = &e.url;
                let name = e.file.clone();
                archive_message_file_inner(url, name).await;
            }
            _ => {}
        }
    }
}
