use crate::web::URL;
use anyhow::{Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use dashmap::DashMap;
use p256::{SecretKey, elliptic_curve::rand_core::OsRng, elliptic_curve::sec1::ToEncodedPoint};
use std::sync::{Arc, LazyLock};
use tokio::sync::mpsc;

static DATA: LazyLock<DashMap<String, Arc<LoginTaskState>>> = LazyLock::new(DashMap::new);

pub type LoginCredentials = (String, String);
pub type LoginResult = Result<LoginCredentials>;

pub fn query(id: &str) -> Option<Arc<LoginTaskState>> {
    DATA.get(id).map(|entry| Arc::clone(entry.value()))
}

pub fn remove(id: &str) -> Option<Arc<LoginTaskState>> {
    DATA.remove(id).map(|(_, task)| task)
}

pub struct LoginTask {
    shared: Arc<LoginTaskState>,
    result_receiver: mpsc::Receiver<LoginResult>,
}

pub struct LoginTaskState {
    pub id: String,
    pub qq_id: i64,
    private_key: SecretKey,
    public_key: String,
    result_sender: mpsc::Sender<LoginResult>,
}

impl LoginTask {
    pub fn new(qq_id: i64) -> Self {
        let private_key = SecretKey::random(&mut OsRng);
        let public_key =
            STANDARD.encode(private_key.public_key().to_encoded_point(false).as_bytes());
        let (result_sender, result_receiver) = mpsc::channel(1);
        let shared = Arc::new(LoginTaskState {
            id: uuid::Uuid::new_v4().to_string(),
            qq_id,
            private_key,
            public_key,
            result_sender,
        });
        DATA.insert(shared.id.clone(), shared.clone());
        Self {
            shared,
            result_receiver,
        }
    }

    #[cfg(test)]
    pub fn id(&self) -> &str {
        &self.shared.id
    }

    pub fn get_url(&self) -> String {
        format!("{}/login/{}", URL, self.shared.id)
    }

    pub async fn wait_result(mut self) -> Result<LoginCredentials> {
        self.result_receiver
            .recv()
            .await
            .unwrap_or_else(|| Err(anyhow!("登录页面已关闭，未收到提交结果")))
    }
}

impl LoginTaskState {
    pub fn public_key(&self) -> &str {
        &self.public_key
    }

    pub fn private_key(&self) -> &SecretKey {
        &self.private_key
    }

    pub async fn succeed(&self, username: String, password: String) -> Result<()> {
        self.finish(Ok((username, password))).await
    }

    pub async fn fail(&self, message: impl Into<String>) -> Result<()> {
        self.finish(Err(anyhow!(message.into()))).await
    }

    async fn finish(&self, result: Result<LoginCredentials>) -> Result<()> {
        self.result_sender
            .send(result)
            .await
            .map_err(|_| anyhow!("登录结果接收端已经关闭"))
    }
}
