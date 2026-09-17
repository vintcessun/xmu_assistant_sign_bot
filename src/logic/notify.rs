//! 版本更新广播：进程起来后比对 `Cargo.toml` 的版本号，变了就把更新日志推给所有群。
//!
//! 几个决定：
//! 1. 闸门用 `CARGO_PKG_VERSION`——改版本号才广播，琐碎重编不会刷屏；
//! 2. 只取更新日志里当前版本那一节，否则每次发版都会把全部历史广播一遍；
//! 3. 先写"已广播版本"再发送。崩在中途最多漏几个群一次；反过来（发完才写）
//!    一旦发送持续失败，每次重启都会全群重播；
//! 4. 状态表里从来没记过账时只记账、不广播。空状态意味着全新部署或刚加上这个功能，
//!    这时候发"已更新"是发给从没见过旧版的群。

use crate::abi::client::get_client;
use crate::abi::echo::Echo;
use crate::abi::message::MessageSend;
use crate::abi::message::api::{GetGroupList, SendGroupMessageParams};
use crate::abi::network::BotClient;
use crate::api::storage::HotTable;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tracing::{error, info, warn};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 更新日志全文，编译期嵌进来；运行期只抽当前版本那一节。
const CHANGELOG: &str = include_str!("../../CHANGELOG.md");

/// 上次已广播的版本号，key 固定 0。
static NOTIFY_STATE: LazyLock<HotTable<u8, String>> =
    LazyLock::new(|| HotTable::new("logic_notify_version"));
const STATE_KEY: u8 = 0;

/// 启动后先等一会：给 NapCat 时间把群列表缓存热起来。
const BOOT_DELAY: Duration = Duration::from_secs(8);
const LIST_RETRY: u32 = 3;
const LIST_RETRY_GAP: Duration = Duration::from_secs(5);
/// 逐群发送的间隔，防风控。
const SEND_GAP: Duration = Duration::from_millis(1500);

/// 在 `client_init` 之后调用。WS 的读写协程在 `abi::run` 里就已经起来了，
/// 所以这里发 API 能正常收到 echo，不必等第一条消息。
pub fn spawn_version_broadcast() {
    tokio::spawn(async {
        if let Err(e) = broadcast().await {
            error!(error = ?e, version = VERSION, "更新广播失败");
        }
    });
}

async fn broadcast() -> anyhow::Result<()> {
    tokio::time::sleep(BOOT_DELAY).await;

    match NOTIFY_STATE.get(&STATE_KEY) {
        Some(last) if last.as_str() == VERSION => {
            info!(version = VERSION, "版本与上次广播一致，跳过更新广播");
            return Ok(());
        }
        // 没记过账：全新部署，或者这个功能刚上线。此时广播等于把"已更新"
        // 通知给根本没见过旧版的群，所以只记账，等下次真正换版本再发。
        None => {
            info!(version = VERSION, "首次记录版本，本次不广播");
            mark_broadcasted();
            return Ok(());
        }
        Some(_) => {}
    }

    let notes = release_notes();
    let groups = fetch_group_list().await?;
    info!(
        version = VERSION,
        count = groups.len(),
        groups = ?groups,
        "拉到机器人所在的群列表，开始广播更新"
    );

    // 先标记再发：崩在中途最多漏几个群一次；反过来一旦发送持续失败，每次重启都会全群重播。
    mark_broadcasted();

    for group_id in groups {
        send_notice(group_id, &notes).await;
        tokio::time::sleep(SEND_GAP).await;
    }

    Ok(())
}

fn mark_broadcasted() {
    if let Err(e) = NOTIFY_STATE.insert(STATE_KEY, Arc::new(VERSION.to_string())) {
        // 记不上也不致命：顶多下次重启再播一遍。
        error!(error = ?e, version = VERSION, "记录已广播版本失败");
    }
}

/// 广播正文。纯文本，不带任何 markdown 记号——QQ 不渲染，写了只是噪音。
fn release_notes() -> String {
    match section_of(VERSION) {
        Some(body) => format!("xmu 助手已更新到 v{VERSION}\n\n{body}"),
        None => {
            // 让"忘记写更新日志"变成看得见的问题，而不是静默发一条空公告。
            warn!(
                version = VERSION,
                "CHANGELOG.md 里没有这个版本的小节，只广播版本号"
            );
            format!("xmu 助手已更新到 v{VERSION}")
        }
    }
}

/// 取出 `## v<版本>` 到下一个 `## ` 之间的正文。
fn section_of(version: &str) -> Option<String> {
    let header = format!("## v{version}");
    let mut lines = CHANGELOG.lines().skip_while(|line| {
        let line = line.trim();
        // 前缀判断要连着空格，否则 v1.0.1 会误命中 v1.0.10。
        line != header && !line.starts_with(&format!("{header} "))
    });
    lines.next()?; // 跳过标题行本身

    let body = lines
        .take_while(|line| !line.trim_start().starts_with("## "))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    (!body.is_empty()).then_some(body)
}

async fn fetch_group_list() -> anyhow::Result<Vec<i64>> {
    let client = get_client();

    for attempt in 1..=LIST_RETRY {
        // no_cache=true：刚重启时 NapCat 的缓存可能还是空的。
        let params = GetGroupList::new(true);
        match client.call_api(&params, Echo::new()).await {
            Ok(pending) => match pending.wait_echo().await {
                Ok(res) => match res.data {
                    Some(list) => {
                        return Ok(list.into_iter().map(|group| group.group_id).collect());
                    }
                    None => warn!(attempt = attempt, response = ?res, "群列表响应里没有数据"),
                },
                Err(e) => warn!(attempt = attempt, error = ?e, "等待群列表响应失败"),
            },
            Err(e) => warn!(attempt = attempt, error = ?e, "请求群列表失败"),
        }

        if attempt < LIST_RETRY {
            tokio::time::sleep(LIST_RETRY_GAP).await;
        }
    }

    anyhow::bail!("连续 {LIST_RETRY} 次拉取群列表都失败，本次不广播")
}

async fn send_notice(group_id: i64, notes: &str) {
    let params = SendGroupMessageParams::new(
        group_id,
        Arc::new(MessageSend::new_message().text(notes).build()),
    );
    let client = get_client();

    match client.call_api(&params, Echo::new()).await {
        Ok(pending) => match pending.wait_echo().await {
            Ok(res) => info!(group_id = group_id, response = ?res, "更新广播已发送"),
            Err(e) => error!(group_id = group_id, error = ?e, "更新广播发送失败"),
        },
        Err(e) => error!(group_id = group_id, error = ?e, "更新广播发送失败"),
    }
}
