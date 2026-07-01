//! SecureLink SSO 客户端，移植自 xmu_secure_link 的 `api.rs`。
//!
//! 仅保留 /flushvpn 所需：拿到 SSO 登录链接（getAuthConfig+getAuthUrl）、
//! 用网页捕获回来的 callback code 完成登录（validateCode+sso_login），
//! 以及拉取 VPN 配置（initConfig）用于展示“VPN 内容”。
//! 会话（session.json + device_id）与独立工具一致地持久化在 `./data/securelink`。

use anyhow::{Context, Result, anyhow};
use base64::{Engine as _, engine::general_purpose};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tracing::{debug, info};

use super::crypto;

const SHEETA_CERT_ID: &str = "secureLink";
const DEFAULT_API_BASE: &str = "https://svpnlink.xmu.edu.cn";
const DEFAULT_ORG: &str = "xmu";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Session {
    pub two_factor_token: Option<String>,
    pub two_factor_token_expire: Option<i64>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub access_token_expire: Option<i64>,
    pub sl_server_type: Option<String>,
    pub base_url: Option<String>,
    pub org: Option<String>,
}

pub struct SecureLinkApi {
    client: Client,
    pub base_url: String,
    pub org: String,
    pub token: Option<String>,
    pub session: Session,
    session_path: PathBuf,
    device_id_path: PathBuf,
    nonce: u32,
}

impl SecureLinkApi {
    pub fn new() -> Result<Self> {
        let data_dir = Path::new(crate::config::DATA_DIR).join("securelink");
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("创建目录失败: {}", data_dir.display()))?;
        Ok(Self {
            client: Client::builder()
                .cookie_store(true)
                .user_agent("SecureLink/3.8.1 (Windows NT 10.0; Win64; x64)")
                .build()?,
            base_url: DEFAULT_API_BASE.to_string(),
            org: DEFAULT_ORG.to_string(),
            token: None,
            session: Session::default(),
            session_path: data_dir.join("session.json"),
            device_id_path: data_dir.join("device_id"),
            nonce: rand_compat::random::<u32>() | 0x4000_0000,
        })
    }

    pub fn load_session(&mut self) -> Result<bool> {
        if !self.session_path.exists() {
            return Ok(false);
        }
        let data = fs::read_to_string(&self.session_path)?;
        self.session = serde_json::from_str(&data)?;
        if let Some(base_url) = &self.session.base_url {
            self.base_url = base_url.clone();
        }
        if let Some(org) = &self.session.org {
            self.org = org.clone();
        }
        self.token = self.session.access_token.clone();
        Ok(self.token.is_some())
    }

    pub fn save_session(&self) -> Result<()> {
        fs::write(
            &self.session_path,
            serde_json::to_string_pretty(&self.session)?,
        )?;
        Ok(())
    }

    /// 取得 SSO 登录链接（连同 provider 名）。网页把它渲染成二维码/链接给用户去登录。
    pub async fn begin_sso(&mut self) -> Result<(String, String)> {
        let _ = self.load_session();
        let auth_config = self.get_auth_config().await?;
        if return_code(&auth_config) != 1 {
            return Err(anyhow!(
                "getAuthConfig 失败: {}",
                auth_config.get("returnMsg").unwrap_or(&auth_config)
            ));
        }
        let providers = auth_config
            .pointer("/content/list")
            .and_then(Value::as_array)
            .context("auth provider list missing")?;
        let provider = providers
            .iter()
            .find(|p| p.get("type").and_then(Value::as_i64) == Some(1))
            .or_else(|| providers.first())
            .context("no SSO auth provider found")?;
        let auth_name = provider
            .get("name")
            .and_then(Value::as_str)
            .context("auth provider name missing")?
            .to_string();

        let auth_url = self.get_auth_url(&auth_name).await?;
        if return_code(&auth_url) != 1 {
            return Err(anyhow!(
                "getAuthUrl 失败: {}",
                auth_url.get("returnMsg").unwrap_or(&auth_url)
            ));
        }
        let login_url = auth_url
            .pointer("/content/loginUrl")
            .and_then(Value::as_str)
            .context("loginUrl missing")?
            .to_string();
        Ok((login_url, auth_name))
    }

    /// 用网页捕获的 callback code 完成登录并持久化会话。
    pub async fn complete_sso(&mut self, auth_name: &str, code: &str) -> Result<()> {
        let result = self.validate_code(auth_name, code).await?;
        if return_code(&result) != 1 {
            return Err(anyhow!(
                "validateCode 失败: {}",
                result.get("returnMsg").unwrap_or(&result)
            ));
        }
        let result = self.sso_login().await?;
        if return_code(&result) != 1 {
            return Err(anyhow!(
                "sso_login 失败: {}",
                result.get("returnMsg").unwrap_or(&result)
            ));
        }
        self.save_session()?;
        info!("SecureLink SSO 登录成功，会话已保存");
        Ok(())
    }

    pub async fn get_auth_config(&mut self) -> Result<Value> {
        let body = sorted_json(&json!({
            "authConfigId": Value::Null,
            "org": self.org,
            "version": 0,
        }))?;
        let url = format!(
            "{}/authApi/sso/authConfig/getAuthConfig?locale=zh_CN&locale=zh_CN",
            self.base_url
        );
        self.sheeta_post(&url, &body, "5.14.0.0").await
    }

    pub async fn get_auth_url(&mut self, auth_name: &str) -> Result<Value> {
        let body = sorted_json(&json!({
            "authConfigId": Value::Null,
            "name": auth_name,
            "org": self.org,
        }))?;
        let url = format!(
            "{}/authApi/sso/authConfig/getAuthUrl?locale=zh_CN&locale=zh_CN",
            self.base_url
        );
        self.sheeta_post(&url, &body, "5.14.0.0").await
    }

    pub async fn validate_code(&mut self, auth_name: &str, code: &str) -> Result<Value> {
        let body = sorted_json(&json!({
            "authConfigId": Value::Null,
            "code": code,
            "name": auth_name,
            "org": self.org,
            "system": "0",
        }))?;
        let url = format!(
            "{}/authApi/sso/authConfig/validateCode?locale=zh_CN&locale=zh_CN",
            self.base_url
        );
        let result = self.sheeta_post(&url, &body, "5.14.0.0").await?;
        if return_code(&result) == 1
            && let Some(token) = result
                .pointer("/content/token")
                .and_then(Value::as_str)
                .map(str::to_string)
        {
            self.session.two_factor_token_expire = crypto::decode_jwt_payload(&token)
                .ok()
                .and_then(|v| v.get("exp").and_then(Value::as_i64));
            self.session.two_factor_token = Some(token);
        }
        Ok(result)
    }

    pub async fn sso_login(&mut self) -> Result<Value> {
        let body = sorted_json(&self.login_body()?)?;
        let url = format!("{}/authApi/is/sso/login?locale=zh_CN", self.base_url);
        let saved = self.token.clone();
        self.token = self.session.two_factor_token.clone();
        let result = self.sheeta_post(&url, &body, "5.14.0.0").await;
        if self.session.access_token.is_none() {
            self.token = saved;
        }
        let result = result?;
        if return_code(&result) == 1 {
            self.capture_login_tokens(&result)?;
        }
        Ok(result)
    }

    pub async fn get_vpn_config(&mut self) -> Result<Value> {
        let _ = self.sl_server_type()?;
        let body = sorted_json(&json!({
            "dns": "",
            "intranetIp": "127.0.0.1",
            "system": 0,
            "wifiSsid": Value::Null,
        }))?;
        let url = format!(
            "{}/networkApi/is/network/initConfig?locale=zh_CN",
            self.base_url
        );
        self.sheeta_post(&url, &body, "5.0.0.0").await
    }

    pub fn access_token(&self) -> Option<&str> {
        self.session.access_token.as_deref().or(self.token.as_deref())
    }

    pub fn sl_server_type(&mut self) -> Result<String> {
        if let Some(value) = &self.session.sl_server_type {
            return Ok(value.clone());
        }
        let generated = self.generate_sl_server_type()?;
        self.session.sl_server_type = Some(generated.clone());
        Ok(generated)
    }

    fn capture_login_tokens(&mut self, result: &Value) -> Result<()> {
        let content = result.get("content").cloned().unwrap_or(Value::Null);
        let login_msg = content.get("loginMessage").cloned().unwrap_or(Value::Null);
        let access = pick_string(&[&login_msg, &content], &["accessToken", "token"]);
        if let Some(access) = access {
            self.token = Some(access.clone());
            self.session.access_token = Some(access.clone());
            if self.session.access_token_expire.is_none() {
                self.session.access_token_expire = crypto::decode_jwt_payload(&access)
                    .ok()
                    .and_then(|v| v.get("exp").and_then(Value::as_i64));
            }
        }
        self.session.refresh_token = pick_string(&[&login_msg, &content], &["refreshToken"])
            .or_else(|| self.session.refresh_token.clone());
        if let Some(exp_ms) = pick_i64(&[&login_msg, &content], &["accessTokenExpire"]) {
            self.session.access_token_expire = Some(exp_ms / 1000);
        }
        self.session.sl_server_type =
            pick_string(&[&login_msg, &content], &["slServerType", "serverType"])
                .or_else(|| self.session.sl_server_type.clone());
        self.session.base_url = Some(self.base_url.clone());
        self.session.org = Some(self.org.clone());
        if self.session.access_token.is_none() {
            return Err(anyhow!("login response did not include access token"));
        }
        Ok(())
    }

    fn generate_sl_server_type(&self) -> Result<String> {
        let body = sorted_json(&json!({
            "serverType": "SDP",
            "timestamp": millis_now().to_string(),
        }))?;
        crypto::rsa_encrypt_base64(body.as_bytes())
    }

    async fn sheeta_post(&mut self, url: &str, body: &str, api_version: &str) -> Result<Value> {
        let req_timestamp = millis_now().to_string();
        self.nonce = self.nonce.wrapping_add(1);
        let nonce = self.nonce.to_string();

        let (body_to_send, session_key, secret) = {
            let (enc_body, key, secret) = crypto::encrypt_body(body)?;
            (enc_body, key, secret)
        };
        let sign =
            crypto::compute_sheeta_sign(SHEETA_CERT_ID, &req_timestamp, &nonce, &body_to_send);

        let mut req = self
            .client
            .post(url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("apiVersion", api_version)
            .header("certId", SHEETA_CERT_ID)
            .header("reqTimestamp", req_timestamp)
            .header("nonce", nonce)
            .header("sheetaSign", sign)
            .header("secret", secret);
        if let Some(sl_server_type) = &self.session.sl_server_type {
            req = req.header("SlServerType", sl_server_type);
        }
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }

        let text = req.body(body_to_send).send().await?.text().await?;
        if let Ok(ciphertext) = general_purpose::STANDARD.decode(text.trim())
            && let Ok(plain) =
                crypto::aes_128_cbc_decrypt(&session_key, crypto::BODY_AES_IV, &ciphertext)
            && let Ok(value) = serde_json::from_slice::<Value>(&plain)
        {
            return Ok(value);
        }
        Ok(serde_json::from_str(&text)?)
    }

    fn device_id(&self) -> Result<String> {
        if self.device_id_path.exists() {
            return Ok(fs::read_to_string(&self.device_id_path)?.trim().to_string());
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        fs::write(&self.device_id_path, &id)?;
        Ok(id)
    }

    fn login_body(&self) -> Result<Value> {
        Ok(json!({
            "archType": 0,
            "deviceId": self.device_id()?,
            "deviceMac": "00:00:00:00:00:00",
            "deviceModel": "x86_64",
            "deviceName": "PC",
            "org": self.org,
            "password": "ZXJyb3I=",
            "system": "0",
            "systemBits": "64",
            "systemModel": "Windows 10",
            "systemName": "Windows 10",
            "version": "3.8.1",
        }))
    }
}

/// 从 initConfig 返回里提取 VPN 远端列表（host:port），用于在网页展示“VPN 内容”。
pub fn vpn_remotes(vpn_config: &Value) -> Vec<(String, u16)> {
    let Some(network_config) = vpn_config
        .pointer("/content/networkConfig")
        .or_else(|| vpn_config.pointer("/content/NetworkConfig"))
    else {
        return Vec::new();
    };
    let mut targets: Vec<(String, u16)> = Vec::new();
    let mut push = |host: &str, port: u16| {
        let entry = (host.to_string(), port);
        if !targets.contains(&entry) {
            targets.push(entry);
        }
    };
    for key in ["priorServers", "alternateServers"] {
        if let Some(items) = network_config.get(key).and_then(Value::as_array) {
            for item in items {
                let Some(host) = item
                    .get("ip")
                    .or_else(|| item.get("host"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                if let Some(ports) = item.get("ports").and_then(Value::as_array) {
                    for p in ports {
                        if let Some(p) = p.as_u64().and_then(|v| u16::try_from(v).ok()) {
                            push(host, p);
                        }
                    }
                } else {
                    push(host, 10000);
                }
            }
        }
    }
    if let Some(appa) = network_config.get("appaAccConf")
        && let Some(host) = appa.get("host").and_then(Value::as_str)
    {
        push(host, 10000);
    }
    debug!(count = targets.len(), "解析出 VPN 远端");
    targets
}

pub fn return_code(value: &Value) -> i64 {
    value
        .get("returnCode")
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

fn millis_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn sorted_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn pick_string(containers: &[&Value], keys: &[&str]) -> Option<String> {
    for container in containers {
        for key in keys {
            if let Some(value) = container.get(*key).and_then(Value::as_str) {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn pick_i64(containers: &[&Value], keys: &[&str]) -> Option<i64> {
    for container in containers {
        for key in keys {
            if let Some(value) = container.get(*key).and_then(Value::as_i64) {
                return Some(value);
            }
        }
    }
    None
}

/// 从 callback URL 里取出 `code` 参数。
pub fn extract_code(callback_url: &str) -> Option<String> {
    let query = callback_url.split_once('?')?.1;
    for part in query.split('&') {
        if let Some((k, v)) = part.split_once('=')
            && k == "code"
        {
            return urlencoding::decode(v).ok().map(|c| c.into_owned());
        }
    }
    None
}
