use crate::api::xmu_service::IDS_URL;
use crate::api::xmu_service::lnt::LNT_URL;
use crate::api::xmu_service::login::{
    LOGIN_URL, LoginData, extract_execution_fast, extract_salt_fast,
};
use crate::api::{network::SessionClient, xmu_service::login::LoginRequest};
use anyhow::{Result, anyhow, bail};
use base64::Engine;
use rand::RngExt;

impl LoginRequest {
    pub fn password(
        url: String,
        execution: String,
        salt: &str,
        username: String,
        password: &str,
    ) -> Result<Self> {
        let mut random_password = Vec::with_capacity(64 + password.len());
        fill_random_bytes_vec(&mut random_password, 64);
        random_password.extend_from_slice(password.as_bytes());

        let mut iv = [0u8; 16];
        fill_random_bytes(&mut iv);

        let encrypted_password_u8 =
            soft_aes::aes::aes_enc_cbc(&random_password, salt.as_bytes(), &iv, Some("PKCS7"))
                .map_err(|e| anyhow!("加密错误，可能是传入的salt不正确: {}", e))?;

        let encrypted_password =
            base64::engine::general_purpose::STANDARD.encode(encrypted_password_u8);

        Ok(Self {
            url,
            body: super::LoginApiBody {
                token: "",
                qrcode_id: None,
                client_type: "userNameLogin",
                login_type: "generalLogin",
                execution,
                event_id: "submit",
                remember_me: Some("1"),
                username: Some(username),
                password: Some(encrypted_password),
                captcha: Some(""),
            },
        })
    }
}

const AES_CHARS: &[u8] = b"ABCDEFGHJKMNPQRSTWXYZabcdefhijkmnprstwxyz2345678";

fn fill_random_bytes_vec(buf: &mut Vec<u8>, len: usize) {
    let mut rng = rand::rng();
    for _ in 0..len {
        let idx = rng.random_range(0..AES_CHARS.len());
        buf.push(AES_CHARS[idx]);
    }
}

fn fill_random_bytes(dest: &mut [u8]) {
    let mut rng = rand::rng();
    for byte in dest.iter_mut() {
        let idx = rng.random_range(0..AES_CHARS.len());
        *byte = AES_CHARS[idx];
    }
}

pub async fn login_password(
    session: &SessionClient,
    username: String,
    password: &str,
) -> Result<LoginData> {
    let login_page = session.get(LOGIN_URL.clone()).await?;
    let base_url = login_page.url().to_string();
    let login_page_text = login_page.text().await?;
    if login_page_text.contains("IP冻结提示") {
        return Err(anyhow!("登录服务被冻结，请联系管理员解决。".to_string(),));
    }
    let pos = match login_page_text.find("pwdFromId") {
        Some(e) => e,
        None => {
            bail!("登录错误，可能是登录页面结构发生了变化.");
        }
    };

    let login_form_data = &login_page_text[pos..];

    //找到第一个符合要求的
    let execution =
        extract_execution_fast(login_form_data).ok_or(anyhow!("获取 execution 失败"))?;

    let salt = extract_salt_fast(login_form_data).ok_or(anyhow!("获取 salt 失败"))?;

    let login_request = LoginRequest::password(base_url, execution, &salt, username, password)?;

    session
        .post(&login_request.url, &login_request.body)
        .await?
        .error_for_status_ref()?;

    let castgc = session
        .get_cookie("CASTGC", &IDS_URL)
        .ok_or(anyhow!("登录失败，未获取到CASTGC Cookie"))?;

    let _ = session.get(LNT_URL.clone()).await?.error_for_status()?;

    let lnt = session
        .get_cookie("session", &LNT_URL)
        .ok_or(anyhow!("登录失败，未获取到session"))?;

    Ok(LoginData {
        castgc: castgc.to_string(),
        lnt: lnt.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use crate::api::network::SessionClient;
    use crate::api::xmu_service::jw::Zzy;
    use crate::api::xmu_service::lnt::Profile;
    use std::io::{self, Write};
    use std::sync::Arc;

    use super::*;
    use anyhow::Result;
    pub fn input(prompt: &str) -> String {
        // 1. 打印提示信息
        print!("{}", prompt);

        // 2. 必须手动刷新 stdout，否则提示文字可能不会立即出现在终端
        io::stdout().flush().expect("Failed to flush stdout");

        // 3. 读取一行输入
        let mut buffer = String::new();
        io::stdin()
            .read_line(&mut buffer)
            .expect("Failed to read line");

        // 4. 去掉末尾的换行符（Python 的 input 也不包含换行符）
        buffer.trim_end().to_string()
    }

    #[tokio::test]
    async fn test_qrcode() -> Result<()> {
        let session = SessionClient::new();

        let username = input("请输入用户名：");
        let password = input("请输入密码：");

        let data = login_password(&session, username, &password).await?;
        let login_data = Arc::new(data);

        println!("登录成功！");

        let profile = Profile::get(&login_data.lnt).await?;

        println!("用户信息：{:?}", profile);

        let data = Zzy::get_from_client(&session, &profile.user_no).await?;

        let zzy_profile = data.get_profile()?;

        println!(
            "信息:{} 转入学院:{:?}",
            zzy_profile.entry_year, zzy_profile.trans_dept
        );

        Ok(())
    }
}
