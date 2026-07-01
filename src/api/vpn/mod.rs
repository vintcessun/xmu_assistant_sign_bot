//! SecureLink 外部认证 → data 的导入，以及“该 host 是否走 VPN”的判定。
//!
//! VPN 数据面（建隧道/检测/下载 exe/spawn 子进程/看门狗/路由绑定）已整体移除——
//! 那套 spawn + 看门狗的方式太不稳定。将来的方向是**用户态 OpenVPN**
//! （libopenvpn3 + smoltcp，暴露一个本地 SOCKS5），再由 SessionClient 对
//! `*.xmu.edu.cn` 走该代理。此处只保留：外部会话认证的导入，以及 host 判定。

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tracing::info;

/// 该 host 是否应当走 VPN（`*.xmu.edu.cn`）。供将来的 SOCKS5 代理按 host 判定复用。
pub fn should_tunnel(host: &str) -> bool {
    host == "xmu.edu.cn" || host.ends_with(".xmu.edu.cn")
}

/// 复用 SecureLink 会话配置后的摘要。
#[derive(Debug, Default)]
pub struct ImportSummary {
    pub src_dir: String,
    pub dst_dir: String,
    pub copied: Vec<String>,
    pub has_access_token: bool,
    pub sl_server_type: Option<String>,
    pub base_url: Option<String>,
}

/// 定位 xmu_secure_link 存放会话的目录（与其 `state::data_dir()` 一致）：
/// `%LOCALAPPDATA%\MySecureLinkRs\data`，可用 `XMU_SECURELINK_DATA_DIR` 覆盖。
pub fn securelink_src_dir() -> Result<PathBuf> {
    if let Ok(d) = std::env::var("XMU_SECURELINK_DATA_DIR") {
        return Ok(PathBuf::from(d));
    }
    let local = std::env::var("LOCALAPPDATA").context(
        "找不到 LOCALAPPDATA，无法定位 SecureLink 会话目录（可用 XMU_SECURELINK_DATA_DIR 覆盖）",
    )?;
    Ok(PathBuf::from(local).join("MySecureLinkRs").join("data"))
}

/// 机器人侧存放 SecureLink 会话副本的目录：`./data/securelink`。
pub fn securelink_dst_dir() -> PathBuf {
    Path::new(crate::config::DATA_DIR).join("securelink")
}

/// 读取 xmu_secure_link 用的同一份配置（session.json + device_id），
/// 拷贝到机器人 `./data/securelink/`，并解析出一份摘要。将来用户态 OpenVPN
/// 建隧道所需的 access_token / 配置即从这里取。
pub fn import_securelink_config() -> Result<ImportSummary> {
    let src = securelink_src_dir()?;
    let dst = securelink_dst_dir();
    std::fs::create_dir_all(&dst).with_context(|| format!("创建目录失败: {}", dst.display()))?;

    let mut summary = ImportSummary {
        src_dir: src.display().to_string(),
        dst_dir: dst.display().to_string(),
        ..Default::default()
    };

    for name in ["session.json", "device_id"] {
        let s = src.join(name);
        if s.exists() {
            std::fs::copy(&s, dst.join(name)).with_context(|| format!("拷贝 {name} 失败"))?;
            summary.copied.push(name.to_string());
        }
    }
    if summary.copied.is_empty() {
        bail!(
            "在 {} 未找到 session.json / device_id，请先用 xmu_secure_link 登录一次",
            src.display()
        );
    }

    // 解析 session.json 摘要（不强依赖字段存在）。
    if let Ok(text) = std::fs::read_to_string(dst.join("session.json"))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
    {
        summary.has_access_token = v
            .get("access_token")
            .and_then(|x| x.as_str())
            .is_some_and(|s| !s.is_empty());
        summary.sl_server_type = v
            .get("sl_server_type")
            .and_then(|x| x.as_str())
            .map(str::to_string);
        summary.base_url = v.get("base_url").and_then(|x| x.as_str()).map(str::to_string);
    }

    info!(
        src = %summary.src_dir,
        dst = %summary.dst_dir,
        copied = ?summary.copied,
        "已复用 SecureLink 会话配置"
    );
    Ok(summary)
}
