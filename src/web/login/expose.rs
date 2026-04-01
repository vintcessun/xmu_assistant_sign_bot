use crate::web::login::task::{LoginTaskState, query, remove};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hkdf::Hkdf;
use p256::{PublicKey, ecdh::diffie_hellman};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::Arc;
use tracing::{debug, trace, warn};
use zeroize::Zeroizing;

#[derive(Clone)]
struct LoginPageState {
    task: Arc<LoginTaskState>,
}

#[derive(Deserialize)]
pub struct LoginPageParams {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginSubmitRequest {
    pub uuid: String,
    pub client_public_key: String,
    pub salt: String,
    pub iv: String,
    pub ciphertext: String,
}

#[derive(Debug, Serialize)]
pub struct LoginSubmitResponse {
    pub ok: bool,
    pub message: String,
    pub qq_id: i64,
    pub uuid: String,
}

#[derive(Debug, Deserialize)]
struct DecryptedLoginPayload {
    uuid: String,
    username: String,
    password: String,
}

async fn login_page_handler(Path(params): Path<LoginPageParams>) -> impl IntoResponse {
    trace!(task_id = params.id, "收到登录页面请求");

    let task = match query(&params.id) {
        Some(task) => task,
        None => {
            warn!(task_id = params.id, "登录任务不存在或已过期");
            return (StatusCode::NOT_FOUND, Html(not_found_html())).into_response();
        }
    };

    debug!(
        task_id = task.id,
        qq_id = task.qq_id,
        "成功加载登录页面任务"
    );
    Html(login_page_html(LoginPageState { task })).into_response()
}

async fn submit_handler(Json(payload): Json<LoginSubmitRequest>) -> impl IntoResponse {
    trace!(task_id = payload.uuid, "收到登录表单提交");

    let task = match remove(&payload.uuid) {
        Some(task) => task,
        None => {
            warn!(task_id = payload.uuid, "登录任务不存在或已过期，拒绝提交");
            return (
                StatusCode::NOT_FOUND,
                Json(LoginSubmitResponse {
                    ok: false,
                    message: "链接已失效，请重新获取登录页面".to_string(),
                    qq_id: 0,
                    uuid: String::new(),
                }),
            )
                .into_response();
        }
    };

    let decrypted = match decrypt_submission(&task, &payload) {
        Ok(decrypted) => decrypted,
        Err(message) => {
            if let Err(error) = task.fail(message).await {
                warn!(task_id = task.id, qq_id = task.qq_id, error = ?error, "登录失败结果未能回传给等待方");
            }
            warn!(
                task_id = task.id,
                qq_id = task.qq_id,
                reason = message,
                "登录密文解密失败"
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(LoginSubmitResponse {
                    ok: false,
                    message: message.to_string(),
                    qq_id: task.qq_id,
                    uuid: task.id.clone(),
                }),
            )
                .into_response();
        }
    };

    if decrypted.uuid != task.id {
        if let Err(error) = task.fail("请求中的 UUID 校验失败").await {
            warn!(task_id = task.id, qq_id = task.qq_id, error = ?error, "UUID 校验失败结果未能回传给等待方");
        }
        return (
            StatusCode::BAD_REQUEST,
            Json(LoginSubmitResponse {
                ok: false,
                message: "请求中的 UUID 校验失败".to_string(),
                qq_id: task.qq_id,
                uuid: task.id.clone(),
            }),
        )
            .into_response();
    }

    if decrypted.username.trim().is_empty() || decrypted.password.is_empty() {
        if let Err(error) = task.fail("用户名和密码不能为空").await {
            warn!(task_id = task.id, qq_id = task.qq_id, error = ?error, "空账号密码结果未能回传给等待方");
        }
        return (
            StatusCode::BAD_REQUEST,
            Json(LoginSubmitResponse {
                ok: false,
                message: "用户名和密码不能为空".to_string(),
                qq_id: task.qq_id,
                uuid: task.id.clone(),
            }),
        )
            .into_response();
    }

    debug!(
        task_id = task.id,
        qq_id = task.qq_id,
        "登录密文已完成解密与校验"
    );

    if let Err(error) = task
        .succeed(decrypted.username.clone(), decrypted.password.clone())
        .await
    {
        warn!(task_id = task.id, qq_id = task.qq_id, error = ?error, "登录结果未能回传给等待方");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginSubmitResponse {
                ok: false,
                message: "服务端未能写入登录结果，请重新获取链接后重试".to_string(),
                qq_id: task.qq_id,
                uuid: task.id.clone(),
            }),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(LoginSubmitResponse {
            ok: true,
            message: "密文已成功解密，后续登录和存储逻辑可在这里继续接入".to_string(),
            qq_id: task.qq_id,
            uuid: task.id.clone(),
        }),
    )
        .into_response()
}

fn decrypt_submission(
    task: &LoginTaskState,
    payload: &LoginSubmitRequest,
) -> Result<DecryptedLoginPayload, &'static str> {
    let client_public_key = decode_base64(&payload.client_public_key, "客户端公钥")?;
    let client_public_key =
        PublicKey::from_sec1_bytes(&client_public_key).map_err(|_| "客户端公钥格式无效")?;
    let salt = decode_base64(&payload.salt, "HKDF salt")?;
    let iv = decode_base64(&payload.iv, "AES-GCM IV")?;
    let ciphertext = decode_base64(&payload.ciphertext, "密文")?;

    if iv.len() != 12 {
        return Err("AES-GCM IV 长度必须为 12 字节");
    }

    let shared_secret = diffie_hellman(
        task.private_key().to_nonzero_scalar(),
        client_public_key.as_affine(),
    );
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), shared_secret.raw_secret_bytes().as_slice());
    let mut aes_key = Zeroizing::new([0u8; 32]);
    hkdf.expand(
        format!("login:{}", task.id).as_bytes(),
        aes_key.as_mut_slice(),
    )
    .map_err(|_| "派生会话密钥失败")?;

    let cipher = Aes256Gcm::new_from_slice(aes_key.as_slice()).map_err(|_| "AES 密钥无效")?;
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: &ciphertext,
                aad: task.id.as_bytes(),
            },
        )
        .map_err(|_| "密文解密失败，请确认页面公钥和会话链接仍然有效")?;
    let plaintext = Zeroizing::new(plaintext);

    serde_json::from_slice::<DecryptedLoginPayload>(&plaintext).map_err(|_| "解密后的数据格式无效")
}

fn decode_base64(value: &str, field: &'static str) -> Result<Vec<u8>, &'static str> {
    STANDARD.decode(value).map_err(|_| match field {
        "客户端公钥" => "客户端公钥不是有效的 Base64 编码",
        "HKDF salt" => "HKDF salt 不是有效的 Base64 编码",
        "AES-GCM IV" => "AES-GCM IV 不是有效的 Base64 编码",
        _ => "密文不是有效的 Base64 编码",
    })
}

fn login_page_html(state: LoginPageState) -> String {
    let uuid = &state.task.id;
    let qq_id = state.task.qq_id;
    let server_public_key = state.task.public_key();

    super::super::LOGIN_HTML
        .replace("__QQ_ID__", &qq_id.to_string())
        .replace("__UUID__", uuid)
        .replace("__SERVER_PUBLIC_KEY__", server_public_key)
        .replace("__UUID_JS__", &format!("{uuid:?}"))
        .replace(
            "__SERVER_PUBLIC_KEY_JS__",
            &format!("{server_public_key:?}"),
        )
}

fn not_found_html() -> &'static str {
    super::super::NOT_FOUND_HTML
}

pub fn task_router(router: Router) -> Router {
    router
        .route("/{id}", get(login_page_handler))
        .route("/submit", post(submit_handler))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::web::login::task::{LoginCredentials, LoginTask};
    use anyhow::Result;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tokio::task::JoinHandle;

    pub struct LoginTestServer {
        pub page_url: String,
        task: LoginTask,
        join_handle: JoinHandle<()>,
    }

    impl LoginTestServer {
        pub fn shutdown(self) {
            self.join_handle.abort();
        }

        pub async fn wait_result(self) -> Result<LoginCredentials> {
            let result = self.task.wait_result().await;
            self.join_handle.abort();
            result
        }
    }

    pub async fn spawn_login_test_server(port: u16, qq_id: i64) -> Result<LoginTestServer> {
        let task = LoginTask::new(qq_id);
        let page_path = format!("/login/{}", task.id());

        let app = Router::new().nest("/login", task_router(Router::new()));
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
                .await?;
        let local_addr = listener.local_addr()?;

        let join_handle = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                warn!(error = ?error, "测试登录页面服务运行失败");
            }
        });

        Ok(LoginTestServer {
            page_url: format!("http://{}{}", local_addr, page_path),
            task,
            join_handle,
        })
    }

    #[tokio::test]
    async fn login_page_can_be_fetched() {
        let server = spawn_login_test_server(0, 10001)
            .await
            .expect("应成功启动登录页面测试服务");

        let response = reqwest::get(&server.page_url)
            .await
            .expect("应成功访问登录页面");
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.text().await.expect("应读取页面 HTML");
        assert!(body.contains("登录信息录入"));
        assert!(body.contains("服务端 ECC 公钥"));

        server.shutdown();
    }

    #[tokio::test]
    #[ignore = "manual"]
    async fn manual_visit_login_page_with_custom_port() {
        let port = std::env::var("LOGIN_TEST_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(48374);

        let server = spawn_login_test_server(port, 10086)
            .await
            .expect("应成功启动登录页面测试服务");

        println!("Manual login page URL: {}", server.page_url);
        let result =
            tokio::time::timeout(std::time::Duration::from_secs(3600), server.wait_result())
                .await
                .expect("等待页面提交超时")
                .expect("页面提交结果应成功返回");
        println!(
            "Received credentials from page: username={}, password={}",
            result.0, result.1
        );
    }
}
