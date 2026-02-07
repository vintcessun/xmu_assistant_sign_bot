use crate::{
    abi::message::{
        MessageSend,
        message_body::{SegmentSend, contact, music, node},
    },
    api::llm::chat::archive::{
        bridge::{get_gif_from_exe, get_mime_type},
        identity::{IdentityGroup, IdentityPerson},
        message_storage::MessageStorage,
    },
};
use futures::{FutureExt, future::BoxFuture};
use genai::chat::{Binary, ChatMessage, ContentPart, MessageContent};

async fn llm_msg_from_segment_receive(segment: &SegmentSend) -> ChatMessage {
    match segment {
        SegmentSend::Text(e) => ChatMessage::user(e.text.clone()),
        SegmentSend::Face(e) => {
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
        SegmentSend::Image(e) => {
            let url = e.file.get_url();
            let content_type = get_mime_type(url);
            ChatMessage::user(ContentPart::Binary(Binary::from_url(
                content_type,
                url,
                None,
            )))
        }
        SegmentSend::Record(e) => {
            let url = e.file.get_url();
            ChatMessage::user(ContentPart::Binary(Binary::from_url(
                get_mime_type(url),
                url,
                None,
            )))
        }
        SegmentSend::Video(e) => {
            let url = e.file.get_url();
            ChatMessage::user(ContentPart::Binary(Binary::from_url(
                get_mime_type(url),
                url,
                None,
            )))
        }
        SegmentSend::At(e) => {
            let user_id = &e.qq;
            let qq_i64 = user_id.parse::<i64>().unwrap_or(0);
            let identity = IdentityPerson::get(qq_i64).await;
            let identity_data = match identity {
                Some(data) => quick_xml::se::to_string(&data).unwrap_or("未知身份".to_string()),
                None => "未知身份".to_string(),
            };
            ChatMessage::user(format!("[At {user_id}]<data>{identity_data}</data>"))
        }
        SegmentSend::Rps(_) => ChatMessage::user("[RPS 猜拳魔法表情]"),
        SegmentSend::Dice(_) => ChatMessage::user("[Dice 掷骰子魔法表情]"),
        SegmentSend::Poke(e) => {
            let type_id = &e.r#type;
            let id = &e.id;

            ChatMessage::user(format!("[戳一戳消息, ID: ({},{})]", type_id, id))
        }
        SegmentSend::Share(e) => {
            let mut content = vec![ContentPart::Text(format!(
                "[分享链接 标题: {} 链接: {} 内容: {:?}]",
                e.title, e.url, e.content,
            ))];
            if let Some(image) = &e.image {
                content.push(ContentPart::Binary(Binary::from_url(
                    get_mime_type(image),
                    image,
                    Some(e.title.clone()),
                )));
            }
            ChatMessage::user(content)
        }
        SegmentSend::Contact(e) => match e {
            contact::DataSend::Group(g) => {
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
            contact::DataSend::Qq(q) => {
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
        SegmentSend::Location(e) => ChatMessage::user(format!(
            "[位置 {:?} 内容: {:?} 经度: {} 纬度: {}]",
            e.title, e.content, e.lon, e.lat,
        )),
        SegmentSend::Reply(e) => {
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
        SegmentSend::Node(e) => match e {
            node::DataSend::Id(d) => {
                let id = d.id.clone();
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
            node::DataSend::Content(d) => {
                let user_id = &d.user_id;
                let nickname = &d.nickname;
                let msgs = llm_msg_from_message_receive_inner(&d.content).await;
                let mut content = vec![ContentPart::Text(format!(
                    "[转发消息 发送者QQ: {} 昵称: {}]",
                    user_id, nickname
                ))];
                for msg in msgs {
                    content.extend(msg.content);
                }
                ChatMessage::user(MessageContent::from(content))
            }
        },
        SegmentSend::Xml(e) => ChatMessage::user(format!("[XML消息 {}]", e.data)),
        SegmentSend::Json(e) => ChatMessage::user(format!("[JSON消息 {}]", e.data)),
        SegmentSend::File(e) => {
            let url = e.file.get_url();
            ChatMessage::user(ContentPart::Binary(Binary::from_url(
                get_mime_type(url),
                url,
                None,
            )))
        }
        SegmentSend::Shake(_) => ChatMessage::user("[窗口抖动]"),
        SegmentSend::Anonymous(_) => ChatMessage::user("[匿名消息]"),
        SegmentSend::Music(e) => match &**e {
            music::DataSend::NetEase163 { id } => {
                ChatMessage::user(format!("[网易云音乐 歌曲ID: {}]", id))
            }
            music::DataSend::Qq { id } => ChatMessage::user(format!("[QQ音乐 歌曲ID: {}]", id)),
            music::DataSend::Xm { id } => ChatMessage::user(format!("[虾米音乐 歌曲ID: {}]", id)),
            music::DataSend::Custom {
                url,
                audio,
                title,
                content,
                image,
            } => {
                let mut parts = vec![
                    ContentPart::Text(format!(
                        "[自定义音乐 链接: {} 标题: {} 内容: {:?}]",
                        url, title, content,
                    )),
                    ContentPart::Binary(Binary::from_url(
                        get_mime_type(url),
                        audio,
                        Some(title.clone()),
                    )),
                ];

                if let Some(image) = image {
                    parts.push(ContentPart::Binary(Binary::from_url(
                        get_mime_type(image),
                        image,
                        Some(title.clone()),
                    )));
                }
                ChatMessage::user(parts)
            }
        },
    }
}

fn llm_msg_from_message_receive_inner<'a>(
    message: &'a MessageSend,
) -> BoxFuture<'a, Vec<ChatMessage>> {
    async move {
        match message {
            MessageSend::Array(e) => {
                let mut result = Vec::with_capacity(e.len());
                for seg in e.iter() {
                    result.push(llm_msg_from_segment_receive(seg).await);
                }
                result
            }
            MessageSend::Single(e) => {
                vec![llm_msg_from_segment_receive(e).await]
            }
        }
    }
    .boxed()
}

pub async fn llm_msg_from_message(message: &MessageSend) -> Vec<ChatMessage> {
    llm_msg_from_message_receive_inner(message).await
}
