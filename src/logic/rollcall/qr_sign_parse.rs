use crate::api::network::SessionClient;
use crate::api::xmu_service::lnt::ProfileWithoutCache;
use crate::logic::helper::{get_client_from_cache, get_client_or_err_for_id};
use crate::logic::rollcall::sign::sign_request_inner;
use crate::logic::rollcall::{
    auto_sign_data::AutoSignResponse, auto_sign_request::AutoSignRequest,
};
use ahash::RandomState;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task::block_in_place;
use tracing::{debug, trace};
use url::Url;

pub struct QrSignRequest;

const TAG_LIST: [&str; 11] = [
    "courseId",
    "activityId",
    "activityType",
    "data",
    "rollcallId",
    "groupSetId",
    "accessCode",
    "action",
    "enableGroupRollcall",
    "createUser",
    "joinCourse",
];

pub struct QrSignParsed {
    pub course_id: i64,
    pub rollcall_id: i64,
    pub data: String,
}

pub fn parse_data(source_data: &str) -> Result<QrSignParsed> {
    trace!("处理二维码数据：{}", source_data);

    let mut data_map = HashMap::with_hasher(RandomState::new());

    if source_data.len() < 5 {
        return Err(anyhow!("Invalid data length"));
    }
    let exact_data = &source_data[5..];

    // 解析 tag~value 结构
    for e in exact_data.split('!') {
        if let Some((tag_idx_str, value_raw)) = e.split_once('~') {
            let tag_idx: usize = tag_idx_str.parse()?;
            let tag_name = TAG_LIST.get(tag_idx).unwrap_or(&"unknown");

            let processed_value = if let Some(stripped) = value_raw.strip_prefix('\x10') {
                // 处理 \x10 开头的 36 进制
                u64::from_str_radix(stripped, 36)?.to_string()
            } else if let Some(stripped) = value_raw.strip_prefix("%10") {
                // 处理 %10 开头的 36 进制
                u64::from_str_radix(stripped, 36)?.to_string()
            } else {
                value_raw.to_string()
            };

            data_map.insert(tag_name.to_string(), processed_value);
        }
    }

    let course_id: i64 = data_map
        .get("courseId")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let rollcall_id: i64 = data_map
        .get("rollcallId")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let sign_data = data_map.get("data").cloned().unwrap_or_default();

    trace!(
        "解析结果 - courseId: {}, rollcallId: {}, data: {}",
        course_id, rollcall_id, sign_data
    );

    Ok(QrSignParsed {
        course_id,
        rollcall_id,
        data: sign_data,
    })
}

fn extract_url_tail(input: &str) -> String {
    // 如果是完整的 URL
    if let Ok(u) = Url::parse(input) {
        let path = u.path(); // 获取 "/j"
        let query = u.query().map(|q| format!("?{}", q)).unwrap_or_default(); // 获取 "?p=..."
        return format!("{}{}", path, query);
    }

    // 如果本身就是路径（如 "/j?p="），直接返回
    input.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrSignResponse {
    pub qq: i64,
    pub response: AutoSignResponse,
}

impl QrSignRequest {
    pub async fn parse(data: &str) -> Result<QrSignParsed> {
        block_in_place(|| {
            let tail = extract_url_tail(data);
            parse_data(&tail)
        })
    }

    async fn make_request(
        qq: i64,
        data: &QrSignParsed,
        client: SessionClient,
    ) -> Result<QrSignResponse> {
        let request: Arc<AutoSignRequest> =
            AutoSignRequest::get(data.course_id, qq, client).await?;
        let res = request.qr(data.rollcall_id, &data.data).await?;
        Ok(QrSignResponse { qq, response: res })
    }

    pub async fn push(qq: i64, data: &QrSignParsed) -> Result<Option<QrSignResponse>> {
        let mut err = Err(anyhow::anyhow!("未知错误"));
        let mut client = get_client_from_cache(qq).ok_or(anyhow!("未找到登录数据，请先登录"))?;
        'retry: for _ in 0..3 {
            match Self::make_request(qq, data, client.clone()).await {
                Ok(r) => return Ok(Some(r)),
                Err(_e) => {
                    //trace!("二维码签到请求失败: {:?}", _e);
                    match ProfileWithoutCache::get_from_client(&client).await {
                        Ok(_) => {
                            let sign_data = sign_request_inner(qq, &client).await?;
                            for s in sign_data {
                                if s.builder.activity_id == data.rollcall_id {
                                    trace!("重试二维码签到");
                                    continue 'retry;
                                }
                            }
                            trace!(
                                qq,
                                rollcall_id = data.rollcall_id,
                                "签到数据刷新后未发现对应的签到活动，可能是不属于的签到，停止重试"
                            );
                            return Ok(None);
                        }
                        Err(e) => {
                            client = get_client_or_err_for_id(qq).await?;
                            debug!(qq, error = ?e, "二维码签到请求失败");
                            err = Err(e);
                        }
                    }
                }
            }
        }
        err
    }
}
