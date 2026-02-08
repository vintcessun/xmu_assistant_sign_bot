mod password;
mod qrcode;

use std::sync::LazyLock;

pub use password::*;
pub use qrcode::*;

use serde::{Deserialize, Serialize};
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

    let _ = session.get(LNT_URL.clone()).await?.error_for_status()?;

    let lnt = session
        .get_cookie("session", &LNT_URL)
        .ok_or(anyhow!("登录失败，未获取到session"))?;

    Ok(lnt.to_string())
}

#[cfg(test)]
mod session_test {
    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_castgc_get_session() -> Result<()> {
        let castgc = "TGT-2287042-KTGUC02s8q1yH06BAFT1cT6bV01mv3-M9MOczLVnOzMesYVhCZcU8-VMD6d2ZFBgRBcnull_main";
        let session = castgc_get_session(castgc).await?;
        println!("LNT Session: {}", session);
        Ok(())
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
