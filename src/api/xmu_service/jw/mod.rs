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
use tracing::warn;
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

/// 教务 jwapp 只有在请求“看起来像 AJAX”时才把**业务错误**渲染成 JSON；缺了这个头，
/// 同一个接口会回一张 text/html 的“系统异常”页，而成功响应则不受影响。
///
/// 实测（2026-09-23，本人会话，重复申请绩点计算）：
/// - 纯表单无额外头 → `text/html`，4429 字节，`<title>系统异常</title>`；
/// - 加上本头 → `{"msg":"该成绩范围已有有效的绩点计算结果，无需重复申请！","code":"1"}`。
///
/// 课表三个接口（kfdxnxqcx / queryXspkjg / queryXsskjc）带与不带该头的成功响应逐字节一致，
/// 所以统一加上是安全的。
pub fn ajax_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::with_capacity(1);
    headers.insert(
        reqwest::header::HeaderName::from_static("x-requested-with"),
        reqwest::header::HeaderValue::from_static("XMLHttpRequest"),
    );
    headers
}

/// 教务有两种情况会以 200 返回 text/html：会话过期时是统一身份认证登录页，
/// 业务出错时是它自带的“系统异常”页。在解析 JSON 之前拦掉，否则用户看到的是
/// 难以理解的 serde 报错。与 `lnt_get_api` 对 LNT 登录页的处理同理。
///
/// 一定要把页面标题带进错误里：以前这里无条件写死“登录会话可能已过期”，
/// 结果 2026-09-09 起 /jdpm 的重复申请全部撞上“系统异常”页，却一路把人往
/// “是不是登录掉了”的方向带，白查了两轮。
pub async fn ensure_json_response(resp: reqwest::Response) -> Result<reqwest::Response> {
    let is_html = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|content_type| content_type.contains("text/html"));

    if !is_html {
        return Ok(resp);
    }

    let url = resp.url().to_string();
    let body = resp.text().await.unwrap_or_default();
    let title = html_title(&body).unwrap_or("无标题");
    let hint = if title.contains("身份认证") || title.contains("登录") {
        "，登录状态已失效，请重新登录后重试"
    } else {
        "，这是教务侧的报错页，不是登录问题"
    };
    warn!(url = %url, title = title, bytes = body.len(), "教务返回了 HTML 而不是 JSON");
    anyhow::bail!("教务返回了网页而不是数据：{title}{hint}");
}

/// 取 HTML 的 `<title>`。只认 ASCII 大小写，所以 `to_ascii_lowercase` 不会改变字节长度，
/// 下标可以直接落回原串。
fn html_title(body: &str) -> Option<&str> {
    const OPEN: &str = "<title>";
    let lower = body.to_ascii_lowercase();
    let start = lower.find(OPEN)? + OPEN.len();
    let end = start + lower[start..].find("</title>")?;
    let title = body[start..end].trim();
    if title.is_empty() { None } else { Some(title) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_of_jw_error_page() {
        // 教务“系统异常”页的真实开头（2026-09-23 抓取）。
        let page = r#"<!DOCTYPE html PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN">
            <html> <head> <meta http-equiv="Content-Type" content="text/html; charset=utf-8"/>
            <TITLE>系统异常</TITLE> </head> <body>...</body></html>"#;
        assert_eq!(html_title(page), Some("系统异常"));
    }

    #[test]
    fn title_of_cas_login_page() {
        assert_eq!(
            html_title("<html><head><title>统一身份认证</title></head></html>"),
            Some("统一身份认证")
        );
    }

    #[test]
    fn title_missing_or_empty() {
        assert_eq!(html_title("<html><body>没有标题</body></html>"), None);
        assert_eq!(html_title("<title>   </title>"), None);
        assert_eq!(html_title("<title>没有闭合标签"), None);
    }
}
