mod password;
mod qrcode;
pub use password::*;
pub use qrcode::*;

use crate::api::{
    network::SessionClient,
    xmu_service::{IDS_URL, lnt::LNT_URL},
};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tracing::{debug, error, info};
use url::Url;
use url_macro::url;

#[derive(Serialize, Deserialize, Debug)]
pub struct LoginApiBody {
    #[serde(rename = "lt")]
    token: &'static str, //登录令牌，固定为空
    #[serde(rename = "uuid", skip_serializing_if = "Option::is_none")]
    pub qrcode_id: Option<String>, //二维码UUID
    #[serde(rename = "cllt")]
    client_type: &'static str, //登录类型
    #[serde(rename = "dllt")]
    login_type: &'static str, //登录方式
    #[serde(rename = "execution")]
    execution: String, //执行标识
    #[serde(rename = "_eventId")]
    event_id: &'static str, //事件ID，固定为submit
    #[serde(rename = "rmShown")]
    remember_me: Option<&'static str>, //是否显示记住我，固定为1
    #[serde(rename = "username", skip_serializing_if = "Option::is_none")]
    username: Option<String>, //用户名
    #[serde(rename = "password", skip_serializing_if = "Option::is_none")]
    password: Option<String>, //密码
    #[serde(rename = "captcha", skip_serializing_if = "Option::is_none")]
    captcha: Option<&'static str>, //验证码，默认为Some("")
}

#[derive(Serialize, Debug)]
pub struct LoginRequest {
    pub url: String,
    pub body: LoginApiBody,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoginData {
    pub castgc: String,
    pub lnt: String,
}

static LOGIN_URL: LazyLock<Url> = LazyLock::new(|| {
    url!("https://jw.xmu.edu.cn/login?service=https://jw.xmu.edu.cn/new/index.html")
});

fn extract_execution_fast(html: &str) -> Option<String> {
    let bytes = html.as_bytes();
    let mut offset = 0;

    // 1. 利用 SIMD 快速定位每一个 <input 标签的起始
    while let Some(start_pos) = memchr::memmem::find(&bytes[offset..], b"<input") {
        let tag_start = offset + start_pos;

        // 2. 找到该标签的结束符号 >
        let tag_end = memchr::memchr(b'>', &bytes[tag_start..])? + tag_start;
        let tag_content = &bytes[tag_start..tag_end];

        // 3. 严格匹配逻辑：确保 name="execution" 存在于此标签内
        // memmem::find 在底层会根据 CPU 自动调用 AVX2/SSE 等加速指令
        if memchr::memmem::find(tag_content, b"name=\"execution\"").is_some() {
            // 4. 定位 value=" 的位置并提取内容
            if let Some(v_pos) = memchr::memmem::find(tag_content, b"value=\"") {
                let v_val_start = v_pos + 7; // 跳过 value=" 这 7 个字节
                let remaining = &tag_content[v_val_start..];

                // 5. 找到结尾的引号 "
                if let Some(v_val_end) = memchr::memchr(b'\"', remaining) {
                    let execution_slice = &remaining[..v_val_end];

                    // 将切片转换为拥有所有权的 String
                    return Some(std::str::from_utf8(execution_slice).ok()?.to_string());
                }
            }
        }

        // 如果当前 <input 标签不匹配，跳过它继续寻找下一个
        offset = tag_end + 1;
    }
    None
}

pub fn extract_salt_fast(html: &str) -> Option<String> {
    let bytes = html.as_bytes();
    let mut offset = 0;

    // 1. SIMD 加速：查找每一个 <input 标签
    while let Some(start_pos) = memchr::memmem::find(&bytes[offset..], b"<input") {
        let tag_start = offset + start_pos;

        // 2. 找到标签结束位置 >
        let tag_end = memchr::memchr(b'>', &bytes[tag_start..])? + tag_start;
        let tag_content = &bytes[tag_start..tag_end];

        // 3. 快速检查 id="pwdEncryptSalt" 是否在该标签内
        // memmem 在 x86_64 上会使用 AVX2 或 SSE 指令集进行扫描
        if memchr::memmem::find(tag_content, b"id=\"pwdEncryptSalt\"").is_some() {
            // 4. 定位 value="
            if let Some(v_pos) = memchr::memmem::find(tag_content, b"value=\"") {
                let v_val_start = v_pos + 7; // 跳过 value=" (7 bytes)
                let remaining = &tag_content[v_val_start..];

                // 5. 找到结尾引号 "
                if let Some(v_val_end) = memchr::memchr(b'\"', remaining) {
                    let salt_slice = &remaining[..v_val_end];

                    // 返回拥有所有权的 String
                    return Some(std::str::from_utf8(salt_slice).ok()?.to_string());
                }
            }
        }

        // 继续查找下一个标签
        offset = tag_end + 1;
    }
    None
}

#[cfg(test)]
pub async fn castgc_get_session(castgc: &str) -> anyhow::Result<String> {
    use anyhow::anyhow;

    use crate::api::{
        network::SessionClient,
        xmu_service::{IDS_URL, lnt::LNT_URL},
    };

    let session = SessionClient::new();

    session.set_cookie("CASTGC", castgc, &IDS_URL);

    // 打 /api/profile 走大陆 cas-client broker 用 CASTGC 静默认证（根路径会走马来西亚 broker、只得匿名会话）。
    crate::api::xmu_service::lnt::ProfileWithoutCache::get_from_client(&session)
        .await
        .map_err(|e| anyhow!("LNT 认证失败（/api/profile 未通过）：{e}"))?;

    let lnt = session
        .get_cookie("session", &LNT_URL)
        .ok_or(anyhow!("登录失败，未获取到session"))?;

    Ok(lnt.to_string())
}

#[cfg(test)]
mod session_test {
    use crate::api::xmu_service::testenv;
    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_castgc_get_session() -> Result<()> {
        let Some(castgc) = testenv::castgc() else {
            return testenv::skipped(module_path!());
        };
        let session = castgc_get_session(castgc).await?;
        println!("LNT Session: {}", session);
        Ok(())
    }
}

/// 在 HTML 里找 `marker`，取它所在标签之后的可见文字（去标签、压空白，最多 100 字）。
fn text_after_marker(html: &str, marker: &str) -> Option<String> {
    let start = html.find(marker)?;
    let rest = &html[start..];
    let rest = &rest[rest.find('>')? + 1..];
    let end = ["</div>", "</title>"]
        .iter()
        .filter_map(|m| rest.find(m))
        .min()
        .unwrap_or(rest.len())
        .min(1200);
    let end = (0..=end).rev().find(|&i| rest.is_char_boundary(i))?;
    let mut text = String::new();
    let mut in_tag = false;
    for c in rest[..end].chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    let text: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(100)
        .collect();
    (!text.is_empty()).then_some(text)
}

/// 统一认证没下发 CASTGC 时，把它回的页面概括成一句话：状态码、落地地址（不带参数）、
/// 页面标题、页面上的错误提示，用来判断是二次认证、改密还是请求本身不对。
async fn describe_login_failure(resp: reqwest::Response) -> String {
    let status = resp.status();
    let mut url = resp.url().clone();
    url.set_query(None);
    let html = resp.text().await.unwrap_or_default();

    let mut parts = vec![format!("状态码 {status}"), format!("落地 {url}")];
    if let Some(title) = text_after_marker(&html, "<title") {
        parts.push(format!("标题「{title}」"));
    }
    for marker in [
        "id=\"showErrorTip\"",
        "id=\"errorMsg\"",
        "id=\"msg\"",
        "class=\"auth_error\"",
    ] {
        if let Some(tip) = text_after_marker(&html, marker) {
            parts.push(format!("提示「{tip}」"));
            break;
        }
    }
    if let Some(path) = save_failed_login_page(&html).await {
        info!(path = path, "登录失败页面已保存");
    }
    parts.join("，")
}

const LOGIN_FAIL_DIR: &str = "data/debug";
const LOGIN_FAIL_KEEP: usize = 5;

/// 把统一认证打回来的整页 HTML 存下来，只留最近 [`LOGIN_FAIL_KEEP`] 份。
/// 提示写在哪认不出来的时候，直接打开这份页面看。
async fn save_failed_login_page(html: &str) -> Option<String> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let path = format!("{LOGIN_FAIL_DIR}/login_fail_{ts}.html");
    tokio::fs::create_dir_all(LOGIN_FAIL_DIR).await.ok()?;
    if let Err(e) = tokio::fs::write(&path, html).await {
        error!(error = ?e, path = path, "保存登录失败页面出错");
        return None;
    }

    // 文件名带毫秒时间戳，按名字排序即按时间排序。
    let mut saved = Vec::new();
    if let Ok(mut dir) = tokio::fs::read_dir(LOGIN_FAIL_DIR).await {
        while let Ok(Some(entry)) = dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("login_fail_") && name.ends_with(".html") {
                saved.push(entry.path());
            }
        }
    }
    saved.sort();
    let excess = saved.len().saturating_sub(LOGIN_FAIL_KEEP);
    for old in &saved[..excess] {
        tokio::fs::remove_file(old).await.ok();
    }
    Some(path)
}

pub async fn login_request(session: &SessionClient, data: LoginRequest) -> Result<LoginData> {
    info!(url = data.url, "发送登录请求");
    let resp = session.post(&data.url, &data.body).await?;
    resp.error_for_status_ref().map_err(|e| {
        error!(url = data.url, error = ?e, "登录请求返回非成功状态码");
        e
    })?;

    let Some(castgc) = session.get_cookie("CASTGC", &IDS_URL) else {
        let reason = describe_login_failure(resp).await;
        error!(reason = reason, "登录失败，未获取到 CASTGC Cookie");
        return Err(anyhow!("登录失败，未获取到CASTGC Cookie（{reason}）"));
    };
    debug!("成功获取 CASTGC Cookie");

    // 通过 /api/profile 触发大陆 cas-client broker 完成 SSO：CASTGC 挂在 ids.xmu.edu.cn 上，
    // 只有大陆 broker 能用它静默认证。若改打根路径 "/"，会被 Keycloak 路由到马来西亚 broker
    // (cas-client-malaysia)、只拿到匿名 session，导致随后所有 /api/* 被重定向到登录页解析失败。
    crate::api::xmu_service::lnt::ProfileWithoutCache::get_from_client(session)
        .await
        .map_err(|e| {
            error!(error = ?e, "LNT 认证失败（/api/profile 未通过）");
            anyhow!("LNT 认证失败（/api/profile 未通过）：{e}")
        })?;

    let lnt = session.get_cookie("session", &LNT_URL).ok_or_else(|| {
        error!("登录失败，未获取到 LNT session Cookie");
        anyhow!("登录失败，未获取到session")
    })?;
    debug!("成功获取 LNT session Cookie");

    info!("二维码登录流程完成，成功获取登录数据");
    Ok(LoginData {
        castgc: castgc.to_string(),
        lnt: lnt.to_string(),
    })
}

pub async fn login_request_castgc(session: &SessionClient, data: LoginRequest) -> Result<String> {
    info!(url = data.url, "发送登录请求以获取 CASTGC");
    let resp = session.post(&data.url, &data.body).await?;
    resp.error_for_status_ref().map_err(|e| {
        error!(url = data.url, error = ?e, "二维码登录请求返回非成功状态码");
        e
    })?;

    let Some(castgc) = session.get_cookie("CASTGC", &IDS_URL) else {
        let reason = describe_login_failure(resp).await;
        error!(reason = reason, "登录失败，未获取到 CASTGC Cookie");
        return Err(anyhow!("登录失败，未获取到CASTGC Cookie（{reason}）"));
    };

    info!("成功通过登录流程获取 CASTGC");
    Ok(castgc.to_string())
}

#[cfg(test)]
mod describe_tests {
    use super::text_after_marker;

    #[test]
    fn picks_title_and_error_tip() {
        let html = r#"<html><head><title> 统一身份认证 </title></head><body>
            <div><span id="showErrorTip"><span>您的账号存在
            安全风险</span>，请进行二次认证</span></div></body></html>"#;
        assert_eq!(
            text_after_marker(html, "<title").as_deref(),
            Some("统一身份认证")
        );
        assert_eq!(
            text_after_marker(html, "id=\"showErrorTip\"").as_deref(),
            Some("您的账号存在 安全风险 ，请进行二次认证")
        );
        assert_eq!(text_after_marker(html, "id=\"errorMsg\""), None);
    }
}

#[cfg(test)]
mod regex_tests_execution {
    use regex::Regex;
    use std::sync::Arc;
    use std::time::Instant;

    static REGEX_EXECUTION: LazyLock<Arc<Regex>> = LazyLock::new(|| {
        Arc::new(
            Regex::new("<input[^>]*?name=\"execution\"[^>]*?value=\"([^\"]*)\"[^>]*?>").unwrap(),
        )
    });

    fn extract_execution(html: &str) -> Option<String> {
        let execution = REGEX_EXECUTION
            .captures(html)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str())?
            .to_string();
        Some(execution)
    }

    use crate::api::network::SessionClient;

    use super::*;

    #[tokio::test]
    async fn consistence() {
        use crate::api::xmu_service::testenv;
        if !testenv::network_enabled() {
            testenv::note_skipped(module_path!(), testenv::NETWORK_ENV);
            return;
        }
        let client = SessionClient::new();
        let resp = client.get("https://lnt.xmu.edu.cn/").await.unwrap();
        let html = resp.text().await.unwrap();

        let login_form_data = &html[html.find("qrLoginForm").unwrap()..];

        let execution = extract_execution(login_form_data).unwrap();

        let fast_execution = extract_execution_fast(login_form_data).unwrap();

        assert_eq!(execution, fast_execution);
    }

    #[tokio::test]
    async fn speed() {
        use crate::api::xmu_service::testenv;
        if !testenv::network_enabled() {
            testenv::note_skipped(module_path!(), testenv::NETWORK_ENV);
            return;
        }
        let client = SessionClient::new();
        // 建议增加重试或超时处理，确保测试稳定性
        let resp = client
            .get("https://lnt.xmu.edu.cn/")
            .await
            .expect("网络请求失败");
        let html = resp.text().await.expect("读取文本失败");

        let start_pos = html.find("qrLoginForm").expect("未找到 qrLoginForm 标识");
        let login_form_data = &html[start_pos..];

        // --- 性能测试：原正则方法 ---
        let now = Instant::now();
        let execution = extract_execution(login_form_data).expect("正则匹配失败");
        let duration_regex = now.elapsed();

        // --- 性能测试：快搜索方法 ---
        let now = Instant::now();
        let fast_execution = extract_execution_fast(login_form_data).expect("快速匹配失败");
        let duration_fast = now.elapsed();

        println!("\n[性能报告]");
        println!("正则匹配耗时: {:?}", duration_regex);
        println!("快速匹配耗时: {:?}", duration_fast);
        println!(
            "速度提升倍数: {:.2}x",
            duration_regex.as_nanos() as f64 / duration_fast.as_nanos() as f64
        );

        assert!(execution == fast_execution);
    }
}

#[cfg(test)]
mod regex_tests_salt {
    use regex::Regex;
    use std::sync::{Arc, LazyLock};
    use std::time::Instant;

    static REGEX_EXECUTION: LazyLock<Arc<Regex>> = LazyLock::new(|| {
        Arc::new(
            Regex::new(r#"<input[^>]*?id="pwdEncryptSalt"[^>]*?value="([^"]*)"[^>]*?>"#).unwrap(),
        )
    });

    fn extract_salt(html: &str) -> Option<String> {
        let salt = REGEX_EXECUTION
            .captures(html)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str())?
            .to_string();
        Some(salt)
    }

    use crate::api::network::SessionClient;

    use super::*;

    #[tokio::test]
    async fn consistence() {
        use crate::api::xmu_service::testenv;
        if !testenv::network_enabled() {
            testenv::note_skipped(module_path!(), testenv::NETWORK_ENV);
            return;
        }
        let client = SessionClient::new();
        let resp = client.get("https://lnt.xmu.edu.cn/").await.unwrap();
        let html = resp.text().await.unwrap();

        let login_form_data = &html[html.find("pwdFromId").unwrap()..];

        let salt = extract_salt(login_form_data).unwrap();

        let fast_salt = extract_salt_fast(login_form_data).unwrap();

        assert_eq!(salt, fast_salt);
    }

    #[tokio::test]
    async fn speed() {
        use crate::api::xmu_service::testenv;
        if !testenv::network_enabled() {
            testenv::note_skipped(module_path!(), testenv::NETWORK_ENV);
            return;
        }
        let client = SessionClient::new();
        // 建议增加重试或超时处理，确保测试稳定性
        let resp = client
            .get("https://lnt.xmu.edu.cn/")
            .await
            .expect("网络请求失败");
        let html = resp.text().await.expect("读取文本失败");

        let start_pos = html.find("pwdFromId").expect("未找到 qrLoginForm 标识");
        let login_form_data = &html[start_pos..];

        // --- 性能测试：原正则方法 ---
        let now = Instant::now();
        let salt = extract_salt(login_form_data).expect("正则匹配失败");
        let duration_regex = now.elapsed();

        // --- 性能测试：快搜索方法 ---
        let now = Instant::now();
        let fast_salt = extract_salt_fast(login_form_data).expect("快速匹配失败");
        let duration_fast = now.elapsed();

        println!("\n[性能报告]");
        println!("正则匹配耗时: {:?}", duration_regex);
        println!("快速匹配耗时: {:?}", duration_fast);
        println!(
            "速度提升倍数: {:.2}x",
            duration_regex.as_nanos() as f64 / duration_fast.as_nanos() as f64
        );

        assert!(salt == fast_salt);
    }
}
