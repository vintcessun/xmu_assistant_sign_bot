use crate::{
    api::network::SessionClient,
    web::{
        URL,
        guard::{Guard, PasswordMode, remove_guard},
    },
};
use anyhow::Result;
use bytes::Bytes;
use dashmap::DashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

/// 一次 /jdpm 查询会话的服务端状态：持有用户已登录的教务会话，
/// 跨“列成绩范围”“申请计算”“下载证明”多次请求存活。
///
/// 页面 id 直接复用 [`Guard::id`]，这样 `/jdfpm/{id}` 与 `/guard/{id}/unlock` 天然配对。
pub struct JdfpmSession {
    pub id: String,
    pub qq: i64,
    pub client: SessionClient,
    pub guard: Arc<Guard>,
    pub expire_at: u64,
    /// 最近一次下载的绩点证明，供页面直接下载 PDF，避免重复打教务接口。
    certificate: Mutex<Option<CachedCertificate>>,
}

/// 缓存的绩点证明：PDF 原文 + 提取出来的正文。
#[derive(Clone)]
pub struct CachedCertificate {
    pub wid: String,
    pub pdf: Bytes,
    pub text: String,
}

static SESSIONS: LazyLock<DashMap<String, Arc<JdfpmSession>>> = LazyLock::new(DashMap::new);

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 清理过期会话，避免被遗弃的链接在内存里堆积（沿用 vpn/task.rs 的做法）。
fn sweep() {
    let now = now_ts();
    SESSIONS.retain(|id, session| {
        let alive = session.expire_at > now;
        if !alive {
            remove_guard(id);
        }
        alive
    });
}

impl JdfpmSession {
    pub fn seconds_left(&self) -> u64 {
        self.expire_at.saturating_sub(now_ts())
    }

    pub fn cached_certificate(&self, wid: &str) -> Option<CachedCertificate> {
        self.lock_certificate()
            .as_ref()
            .filter(|cached| cached.wid == wid)
            .cloned()
    }

    pub fn cache_certificate(&self, cached: CachedCertificate) {
        debug!(session_id = %self.id, wid = %cached.wid, "缓存绩点证明 PDF");
        *self.lock_certificate() = Some(cached);
    }

    /// `Mutex` 只包着一个 Option 的短临界区；中毒时直接取回内部值继续用。
    fn lock_certificate(&self) -> MutexGuard<'_, Option<CachedCertificate>> {
        self.certificate.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 新建一次受口令保护的绩点查询会话，返回会话与需要交给用户的明文口令。
pub fn create_session(
    qq: i64,
    client: SessionClient,
    mode: PasswordMode,
) -> Result<(Arc<JdfpmSession>, String)> {
    sweep();

    let (guard, password) = Guard::create(qq, mode)?;
    let session = Arc::new(JdfpmSession {
        id: guard.id.clone(),
        qq,
        client,
        expire_at: guard.expire_at,
        guard,
        certificate: Mutex::new(None),
    });

    SESSIONS.insert(session.id.clone(), session.clone());
    Ok((session, password))
}

pub fn get_session(id: &str) -> Option<Arc<JdfpmSession>> {
    let session = SESSIONS.get(id)?.clone();
    if session.expire_at <= now_ts() {
        SESSIONS.remove(id);
        remove_guard(id);
        return None;
    }
    Some(session)
}

pub fn page_url(id: &str) -> String {
    format!("{URL}/jdfpm/{id}")
}
