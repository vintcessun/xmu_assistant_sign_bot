use crate::api::network::SessionClient;
use dashmap::DashMap;
use std::sync::LazyLock;
use tracing::info;

pub static CLIENT_CACHE: LazyLock<DashMap<i64, SessionClient>> = LazyLock::new(DashMap::new);

pub fn get_client_from_cache(id: i64) -> Option<SessionClient> {
    CLIENT_CACHE.get(&id).map(|e| e.value().clone())
}

/// 登录或恢复成功后写入客户端缓存，并记录结构化日志。
/// `event` 用于标识调用来源（如 "login_qr" / "login_pwd" / "recover_lnt" / "recover_pwd"）。
pub fn write_client_cache(id: i64, client: SessionClient, event: &'static str) {
    CLIENT_CACHE.insert(id, client);
    info!(user_id = id, event = event, "login_success_cache_written");
}
