use std::sync::LazyLock;

use crate::{
    abi::{
        logic_import::{Message, Notice},
        message::{
            MessageReceive,
            message_body::{SegmentReceive, contact},
        },
    },
    api::llm::chat::{
        archive::{
            file_embedding::embedding_llm_file,
            identity::{IdentityGroup, IdentityPerson},
            message_storage::MessageStorage,
        },
        file::LlmFile,
    },
};
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

async fn llm_msg_from_segment_receive(segment: &SegmentReceive) -> ChatMessage {
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
            let content_type = match &e.r#type {
                Some(t) => t,
                None => "image/jpeg",
            };
            let url = &e.url;
            let name = e.file.clone();
            ChatMessage::user(ContentPart::Binary(Binary::from_url(
                content_type,
                url,
                Some(name),
            )))
        }
        SegmentReceive::Record(e) => ChatMessage::user(ContentPart::Binary(Binary::from_url(
            "audio/amr",
            &e.url,
            Some(e.file.clone()),
        ))),
        SegmentReceive::Video(e) => ChatMessage::user(ContentPart::Binary(Binary::from_url(
            "video/mp4",
            &e.url,
            Some(e.file.clone()),
        ))),
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
                ContentPart::Binary(Binary::from_url(
                    "image/jpeg",
                    &e.image,
                    Some(e.title.clone()),
                )),
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
            let name = "file".to_string();
            ChatMessage::user(ContentPart::Binary(Binary::from_url(
                "application/octet-stream",
                url,
                Some(name),
            )))
        }
    }
}

pub async fn llm_msg_from_message_receive(message: &MessageReceive) -> Vec<ChatMessage> {
    match message {
        MessageReceive::Array(e) => {
            let mut result = Vec::with_capacity(e.len());
            for seg in e.iter() {
                result.push(llm_msg_from_segment_receive(seg).await);
            }
            result
        }
        MessageReceive::Single(e) => {
            vec![llm_msg_from_segment_receive(e).await]
        }
    }
}

pub async fn llm_msg_from_message_without_archive(message: &Message) -> Vec<ChatMessage> {
    match message {
        Message::Private(p) => {
            let data = quick_xml::se::to_string(&p).unwrap_or("未知消息".to_string());
            let mut ret = vec![ChatMessage::user(format!("<data>{}</data>", data))];
            ret.extend(llm_msg_from_message_receive(&p.message).await);
            ret
        }
        Message::Group(g) => {
            let data = quick_xml::se::to_string(&g).unwrap_or("未知消息".to_string());
            let mut ret = vec![ChatMessage::user(format!("<data>{}</data>", data))];
            ret.extend(llm_msg_from_message_receive(&g.message).await);
            ret
        }
    }
}

pub async fn llm_msg_from_message(message: &Message) -> Vec<ChatMessage> {
    let ret = llm_msg_from_message_without_archive(message).await;
    archive_message_files(message).await;
    ret
}

pub async fn llm_msg_from_notice(notice: &Notice) -> ChatMessage {
    ChatMessage::user(quick_xml::se::to_string(notice).unwrap_or("未知提示".to_string()))
}

async fn archive_message_file_inner(url: &str, name: String) {
    match async move {
        let file = LlmFile::from_url(url, name).await?;
        let file = embedding_llm_file(file).await?;
        LlmFile::insert(file).await?;
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
