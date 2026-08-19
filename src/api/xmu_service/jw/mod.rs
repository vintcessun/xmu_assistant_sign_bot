mod jdfpm;
mod schedule;
mod userinfo;
mod zzy;

use std::sync::LazyLock;

pub use jdfpm::*;
pub use schedule::*;
pub use userinfo::*;
pub use zzy::*;

use async_trait::async_trait;
use url::Url;
use url_macro::url;

use anyhow::Result;

use crate::api::network::SessionClient;

pub static IDS_URL: LazyLock<Url> = LazyLock::new(|| url!("https://ids.xmu.edu.cn/authserver"));

#[async_trait]
pub trait JwAPI {
    const URL_DATA: &'static str;
    const APP_ENTRANCE: &'static str;
}

pub fn get_castgc_client(castgc: &str) -> SessionClient {
    let client = SessionClient::new();
    client.set_cookie("CASTGC", castgc, &IDS_URL);
    client
}

/// 教务会话过期时，业务接口会以 200 返回统一身份认证登录页或“系统异常”页（text/html）。
/// 在解析 JSON 之前拦掉，否则用户看到的是难以理解的 serde 报错。
/// 与 `lnt_get_api` 对 LNT 登录页的处理同理。
pub fn ensure_json_response(resp: &reqwest::Response) -> Result<()> {
    let is_html = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|content_type| content_type.contains("text/html"));

    if is_html {
        anyhow::bail!("教务返回了网页而不是数据（登录会话可能已过期，请重新登录后重试）");
    }
    Ok(())
}
