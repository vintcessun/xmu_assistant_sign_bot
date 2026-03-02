use super::data::LOGIN_DATA;
use crate::api::network::SessionClient;
use crate::api::xmu_service::lnt::get_session_client;
use crate::logic::rollcall::auto_sign_data::{AutoSignRequest, AutoSignResponse};
use ahash::RandomState;
use anyhow::{Result, anyhow};
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::task::block_in_place;
use tracing::trace;
use url::Url;

pub struct QrSignRequest {
    pub qq: i64,
    pub client: Arc<SessionClient>,
}

static QR_SIGN_CACHE: LazyLock<DashMap<i64, Arc<QrSignRequest>>> = LazyLock::new(DashMap::new);

impl QrSignRequest {
    pub async fn get(qq: i64) -> Result<Arc<Self>> {
        if let Some(req) = QR_SIGN_CACHE.get(&qq) {
            trace!("从缓存中获取二维码签到请求");
            Ok(req.clone())
        } else {
            trace!("缓存中未找到二维码签到请求，尝试创建新的请求");
            let login_data = LOGIN_DATA
                .get(&qq)
                .ok_or(anyhow!("未找到登录数据，请先登录"))?;
            let client = Arc::new(get_session_client(&login_data.lnt));
            let req = Arc::new(QrSignRequest { qq, client });
            QR_SIGN_CACHE.insert(qq, req.clone());
            Ok(req)
        }
    }
}

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

impl QrSignRequest {
    pub async fn request(&self, data: &QrSignParsed) -> Result<AutoSignResponse> {
        let request = AutoSignRequest::get(data.course_id, self.qq, self.client.clone()).await?;
        request.qr(data.rollcall_id, &data.data).await
    }

    pub async fn parse(data: &str) -> Result<QrSignParsed> {
        block_in_place(|| {
            let tail = extract_url_tail(data);
            parse_data(&tail)
        })
    }
}
