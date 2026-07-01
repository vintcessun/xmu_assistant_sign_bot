use crate::api::xmu_service::securelink::SecureLinkApi;
use crate::web::URL;
use dashmap::DashMap;
use std::sync::{Arc, LazyLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const FLOW_EXPIRE_SECS: u64 = 20 * 60;

/// 一次 /flushvpn 登录流程的服务端状态：持有在途的 SecureLinkApi（含 cookie/会话），
/// 跨“取链接”和“提交 callback”两次请求存活。
pub struct VpnFlow {
    pub id: String,
    pub qq: i64,
    pub login_url: String,
    pub auth_name: String,
    pub api: Mutex<SecureLinkApi>,
    pub expire_at: u64,
}

static FLOWS: LazyLock<DashMap<String, Arc<VpnFlow>>> = LazyLock::new(DashMap::new);

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 清理过期流程，避免被遗弃的链接在内存里堆积（顺手规避 web 任务表的泄漏问题）。
fn sweep() {
    let now = now_ts();
    FLOWS.retain(|_, f| f.expire_at > now);
}

pub fn create_flow(
    qq: i64,
    login_url: String,
    auth_name: String,
    api: SecureLinkApi,
) -> Arc<VpnFlow> {
    sweep();
    let id = uuid::Uuid::new_v4().to_string();
    let flow = Arc::new(VpnFlow {
        id: id.clone(),
        qq,
        login_url,
        auth_name,
        api: Mutex::new(api),
        expire_at: now_ts() + FLOW_EXPIRE_SECS,
    });
    FLOWS.insert(id, flow.clone());
    flow
}

pub fn get_flow(id: &str) -> Option<Arc<VpnFlow>> {
    let flow = FLOWS.get(id)?.clone();
    if flow.expire_at <= now_ts() {
        FLOWS.remove(id);
        return None;
    }
    Some(flow)
}

pub fn page_url(id: &str) -> String {
    format!("{URL}/vpn/{id}")
}
