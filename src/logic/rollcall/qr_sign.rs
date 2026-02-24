use super::data::LOGIN_DATA;
use crate::{
    abi::{
        logic_import::*,
        message::{
            MessageReceive, MessageSend, from_str,
            message_body::{SegmentReceive, image},
        },
    },
    api::{
        network::{SessionClient, download_to_file},
        qrcode::QrCode,
        storage::FileStorage,
    },
    logic::rollcall::{auto_sign_data::AutoSignResponse, qr_sign_parse::QrSignRequest},
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

#[handler(msg_type=Message)]
pub async fn qr_sign(ctx: Context) -> Result<()> {
    let start = tokio::time::Instant::now();
    let msg = ctx.get_message();
    let msg_receive = match &*msg {
        Message::Group(msg) => &msg.message,
        Message::Private(msg) => &msg.message,
    };

    let msg_slice = match msg_receive {
        MessageReceive::Array(arr) => arr.iter().collect::<Vec<_>>(),
        MessageReceive::Single(m) => vec![m],
    };

    let mut tasks = vec![];
    for msg in msg_slice {
        if let SegmentReceive::Image(img) = msg {
            tasks.push(qr_sign_cmd_process_file(img));
        }
    }
    let rets = futures::future::join_all(tasks)
        .await
        .into_iter()
        .filter_map(|x| x.ok())
        .flatten();

    let time = start.elapsed().as_secs_f64();
    info!("二维码解析耗时: {} s", time);

    for ret in rets {
        let mut msg = MessageSend::new_message().text("二维码帮助如下:\n");
        for r in ret {
            msg = msg.text(format!("QQ: {}, 响应: {}\n", r.qq, r.response));
        }
        ctx.send_message_async(msg.build());
    }

    ctx.send_message_async(from_str(format!(
        "二维码解析总耗时: {} s",
        start.elapsed().as_secs_f64()
    )));

    Ok(())
}

async fn qr_sign_cmd_process_file(img: &image::DataReceive) -> Result<Vec<Vec<QrSignResponse>>> {
    let file = download_to_file(Arc::new(SessionClient::new()), &img.url, &img.file).await?;
    let data = QrCode::from_file(file.get_path()).await?;
    let mut tasks = Vec::with_capacity(data.len());
    for d in &data {
        tasks.push(async move { qr_sign_request(d).await });
    }
    let ret = futures::future::join_all(tasks).await;
    let ret = ret.into_iter().filter_map(|x| x.ok()).collect();
    Ok(ret)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrSignResponse {
    pub qq: i64,
    pub response: AutoSignResponse,
}

pub async fn qr_sign_request(data: &str) -> Result<Vec<QrSignResponse>> {
    let parsed = QrSignRequest::parse(data).await?;
    let mut task = Vec::new();
    for val in &*LOGIN_DATA {
        let parsed_ref = &parsed;
        let qq = *val.key();
        task.push(async move {
            let req = QrSignRequest::get(qq).await?;
            let res = req.request(parsed_ref).await?;
            Ok::<QrSignResponse, anyhow::Error>(QrSignResponse { qq, response: res })
        });
    }
    let ret = futures::future::join_all(task).await;
    let ret = ret.into_iter().filter_map(|x| x.ok()).collect();
    Ok(ret)
}
