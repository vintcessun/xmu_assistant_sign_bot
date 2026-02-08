pub mod api;
pub mod event_body;
pub mod file;
pub mod helper;
pub mod message_body;
pub mod sender;

use crate::abi::message::file::FileUrl;
use crate::abi::message::message_body::*;

use ::helper::box_new;
pub use api::Params;
pub use event_body as event;
pub use event_body::Event;
pub use event_body::message as event_message;
pub use event_body::meta as event_meta;
pub use event_body::notice as event_notice;
pub use event_body::request as event_request;
pub use event_body::{MessageType, Target, Type};
pub use helper::*;
pub use message_body::MessageReceive;
pub use message_body::MessageSend;
pub use sender::{Sender, SenderGroup, SenderPrivate};
use tracing::trace;

pub fn from_str<S: Into<String>>(s: S) -> MessageSend {
    MessageSend::new_message().text(s).build()
}

fn receive_seq_to_send_seq(seq: &SegmentReceive) -> SegmentSend {
    match seq {
        SegmentReceive::At(p) => {
            trace!(qq = ?p.qq, "转换消息段: @At");
            SegmentSend::At(message_body::at::DataSend { qq: p.qq.clone() })
        }
        SegmentReceive::Contact(p) => {
            trace!(data = ?p, "转换消息段: 联系人 Contact");
            let val = match p {
                message_body::contact::DataReceive::Qq(e) => {
                    message_body::contact::DataSend::Qq(message_body::contact::QqSend {
                        id: e.id.clone(),
                    })
                }
                message_body::contact::DataReceive::Group(e) => {
                    message_body::contact::DataSend::Group(message_body::contact::GroupSend {
                        id: e.id.clone(),
                    })
                }
            };
            SegmentSend::Contact(val)
        }
        SegmentReceive::Dice(p) => {
            trace!(dice = ?p, "转换消息段: 骰子 Dice");
            SegmentSend::Dice(*p)
        }
        SegmentReceive::Face(p) => {
            trace!(id = ?p.id, "转换消息段: 表情 Face");
            let val = message_body::face::DataSend { id: p.id.clone() };
            SegmentSend::Face(val)
        }
        SegmentReceive::Forward(p) => {
            trace!(id = ?p.id, "转换消息段: 转发 Forward");
            SegmentSend::Node(message_body::node::DataSend::Id(
                message_body::node::DataSend1 { id: p.id.clone() },
            ))
        }
        SegmentReceive::Image(p) => {
            trace!(url = ?p.url, r#type = ?p.r#type, "转换消息段: 图片 Image");
            SegmentSend::Image(box_new!(message_body::image::DataSend, {
                file: file::FileUrl::new(p.url.clone()),
                r#type: p.r#type.clone(),
                cache: message_body::Cache::default(),
                proxy: message_body::Proxy::default(),
                timeout: None,
            }))
        }
        SegmentReceive::Json(p) => {
            trace!(data = ?p.data, "转换消息段: JSON");
            SegmentSend::Json(message_body::json::DataSend {
                data: p.data.clone(),
            })
        }
        SegmentReceive::Location(p) => {
            trace!(lat = ?p.lat, lon = ?p.lon, title = ?p.title, "转换消息段: 位置 Location");
            SegmentSend::Location(box_new!(message_body::location::DataSend, {
                lat: p.lat.clone(),
                lon: p.lon.clone(),
                title: Some(p.title.clone()),
                content: Some(p.content.clone()),
            }))
        }
        SegmentReceive::Poke(p) => {
            trace!(type = ?p.r#type, id = ?p.id, "转换消息段: 戳一戳 Poke");
            SegmentSend::Poke(message_body::poke::DataSend {
                r#type: p.r#type.clone(),
                id: p.id.clone(),
            })
        }
        SegmentReceive::Record(p) => {
            trace!(url = ?p.url, "转换消息段: 语音 Record");
            SegmentSend::Record(message_body::record::DataSend {
                file: file::FileUrl::new(p.url.clone()),
                magic: message_body::Magic::default(),
                cache: message_body::Cache::default(),
                proxy: message_body::Proxy::default(),
                timeout: None,
            })
        }
        SegmentReceive::Reply(p) => {
            trace!(id = ?p.id, "转换消息段: 回复 Reply");
            SegmentSend::Reply(message_body::reply::DataSend { id: p.id.clone() })
        }
        SegmentReceive::Rps(p) => {
            trace!(rps = ?p, "转换消息段: 猜拳 Rps");
            SegmentSend::Rps(*p)
        }
        SegmentReceive::Share(p) => {
            trace!(url = ?p.url, title = ?p.title, "转换消息段: 分享 Share");
            SegmentSend::Share(box_new!(message_body::share::DataSend, {
                url: p.url.clone(),
                title: p.title.clone(),
                content: Some(p.content.clone()),
                image: Some(p.image.clone()),
            }))
        }
        SegmentReceive::Text(p) => {
            trace!(text = ?p.text, "转换消息段: 文本 Text");
            SegmentSend::Text(message_body::text::DataSend {
                text: p.text.clone(),
            })
        }
        SegmentReceive::Video(p) => {
            trace!(url = ?p.url, "转换消息段: 视频 Video");
            SegmentSend::Video(message_body::video::DataSend {
                file: file::FileUrl::new(p.url.clone()),
                cache: message_body::Cache::default(),
                proxy: message_body::Proxy::default(),
                timeout: None,
            })
        }
        SegmentReceive::Xml(p) => {
            trace!(data = ?p.data, "转换消息段: XML");
            SegmentSend::Xml(message_body::xml::DataSend {
                data: p.data.clone(),
            })
        }
        SegmentReceive::File(p) => {
            trace!(url = ?p.url, "转换消息段: 文件 File");
            SegmentSend::File(message_body::file::DataSend {
                file: FileUrl::new(p.url.clone()),
            })
        }
    }
}

pub fn receive2send(msg: &MessageReceive) -> MessageSend {
    let msg_vec = match msg {
        MessageReceive::Array(arr) => arr.iter().map(receive_seq_to_send_seq).collect::<Vec<_>>(),
        MessageReceive::Single(sing) => {
            vec![receive_seq_to_send_seq(sing)]
        }
    };

    MessageSend::Array(msg_vec)
}

pub fn receive2send_add_prefix(msg: &MessageReceive, prefix: String) -> MessageSend {
    let mut msg_vec = match msg {
        MessageReceive::Array(arr) => arr.iter().map(receive_seq_to_send_seq).collect::<Vec<_>>(),
        MessageReceive::Single(sing) => {
            vec![receive_seq_to_send_seq(sing)]
        }
    };

    let mut ret = vec![SegmentSend::Text(message_body::text::DataSend {
        text: prefix,
    })];

    ret.append(&mut msg_vec);

    MessageSend::Array(ret)
}
