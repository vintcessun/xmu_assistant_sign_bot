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
    logic::rollcall::qr_sign_parse::{QrSignRequest, QrSignResponse},
};
use anyhow::Result;
use tracing::{debug, info, trace, warn};

const ERR_SAMPLE_LIMIT: usize = 8;

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
        .flatten()
        .collect::<Vec<_>>();

    let time = start.elapsed().as_secs_f64();

    let mut used = false;
    let mut msgs = Vec::with_capacity(rets.len());
    for ret in rets {
        let mut msg = MessageSend::new_message().text("二维码帮助如下:\n");
        for r in ret {
            msg = msg.text(format!("QQ: {}, 响应: {}\n", r.qq, r.response));
        }
        msgs.push(msg.build());
        used = true;
    }

    if used {
        for msg in msgs {
            ctx.send_message_async(msg);
        }

        ctx.send_message_async(from_str(format!(
            "二维码解析总耗时: {} s",
            start.elapsed().as_secs_f64()
        )));
        info!("二维码解析耗时: {} s", time);
    }

    Ok(())
}

async fn qr_sign_cmd_process_file(img: &image::DataReceive) -> Result<Vec<Vec<QrSignResponse>>> {
    let file = download_to_file(SessionClient::new(), &img.url, &img.file).await?;
    let data = QrCode::from_file(file.get_path()).await?;
    let mut tasks = Vec::with_capacity(data.len());
    for d in &data {
        tasks.push(async move { qr_sign_request(d).await });
    }
    let ret = futures::future::join_all(tasks).await;
    let ret = ret.into_iter().filter_map(|x| x.ok()).collect();
    Ok(ret)
}

pub async fn qr_sign_request(data: &str) -> Result<Vec<QrSignResponse>> {
    trace!("进行二维码推送签到{data}");
    let parsed = QrSignRequest::parse(data).await?;
    let mut qq_list = Vec::new();
    for entry in &*LOGIN_DATA {
        qq_list.push(*entry.key());
    }
    let mut task = Vec::with_capacity(qq_list.len());
    for qq in qq_list.iter().copied() {
        task.push(QrSignRequest::push(qq, &parsed));
    }

    let ret = futures::future::join_all(task).await;

    let mut none_count = 0usize;
    let mut err_count = 0usize;
    let mut err_samples = Vec::new();
    let mut result = Vec::new();

    for item in ret {
        match item {
            Ok(Some(resp)) => result.push(resp),
            Ok(None) => {
                none_count += 1;
            }
            Err(e) => {
                err_count += 1;
                debug!(error = ?e, "推送签到错误");
                if err_samples.len() < ERR_SAMPLE_LIMIT {
                    err_samples.push(format!("{:#}", e));
                }
            }
        }
    }

    info!(
        total_accounts = qq_list.len(),
        success_count = result.len(),
        none_count,
        err_count,
        "二维码推送批次统计"
    );

    if err_count > 0 {
        warn!(
            total_accounts = qq_list.len(),
            err_count,
            samples = ?err_samples,
            "二维码推送存在失败样本"
        );
    }

    debug!(help_list=?result, "帮助详情");
    Ok(result)
}
