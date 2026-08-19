//! 联网测试的开关与凭证来源。
//!
//! 联网测试默认自动跳过（不再靠 `#[ignore]`），只有提供了凭证才真正出网，
//! 因此一条命令就能把全部联网测试跑一遍：
//!
//! ```text
//! XMU_TEST_CASTGC=TGT-xxx cargo test --lib
//! ```
//!
//! 凭证是短时效敏感数据，只从环境变量读，绝不写进仓库。
//! 不需要凭证、但仍要出网的测试（如下载基准）用 `XMU_TEST_NETWORK=1` 单独打开。
//!
//! 注意：联网测试之间要串行跑。全局 `SessionClient` 的连接池挂在最先创建它的
//! tokio runtime 上，而每个 `#[tokio::test]` 各有一个 runtime，并发跑会撞上
//! “runtime dropped the dispatch task”。跑联网测试请加 `-- --test-threads=1`。

use anyhow::Result;
use std::sync::LazyLock;

/// 统一身份认证 CASTGC(TGT) 的环境变量名。
pub const CASTGC_ENV: &str = "XMU_TEST_CASTGC";
/// 无需凭证的纯联网测试开关。
pub const NETWORK_ENV: &str = "XMU_TEST_NETWORK";
/// 需要本机 Chrome/Chromium 的测试开关。
pub const CHROME_ENV: &str = "XMU_TEST_CHROME";
/// 需要真人参与（手输账号密码、扫码）的测试开关。这类测试无人值守时**永远**过不了。
pub const INTERACTIVE_ENV: &str = "XMU_TEST_INTERACTIVE";
/// 依赖已经失效的固定值（过期链接、被删掉的考试、第三方服务）的测试开关。
///
/// 这些用例本身没坏，是它们指向的外部数据没了。原样保留、默认跳过，
/// 等哪天换上新的固定值再打开验证，免得它们长期红着掩盖真正的回归。
pub const STALE_ENV: &str = "XMU_TEST_STALE";

fn read(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

static CASTGC: LazyLock<Option<String>> = LazyLock::new(|| read(CASTGC_ENV));
static NETWORK: LazyLock<bool> = LazyLock::new(|| read(NETWORK_ENV).is_some());
static CHROME: LazyLock<bool> = LazyLock::new(|| read(CHROME_ENV).is_some());
static INTERACTIVE: LazyLock<bool> = LazyLock::new(|| read(INTERACTIVE_ENV).is_some());
static STALE: LazyLock<bool> = LazyLock::new(|| read(STALE_ENV).is_some());

/// 取联网测试用的 CASTGC；未设置时返回 `None`，调用方应当跳过该测试。
///
/// 返回 `&'static str` 是为了能原地替换掉原先写死的字符串字面量。
pub fn castgc() -> Option<&'static str> {
    CASTGC.as_deref()
}

/// 无需凭证的纯联网测试是否已打开。
pub fn network_enabled() -> bool {
    *NETWORK
}

/// 打印跳过原因。`cargo test` 默认吞掉 stdout，加 `--nocapture` 可见。
pub fn note_skipped(module: &str, env_key: &str) {
    println!("[skip] {module}: 未设置 {env_key}，跳过联网测试");
}

/// 缺少 CASTGC 时的跳过返回值，供 `-> Result<()>` 的测试直接 `return`。
pub fn skipped(module: &str) -> Result<()> {
    note_skipped(module, CASTGC_ENV);
    Ok(())
}

/// 未打开纯联网开关时的跳过返回值。
pub fn skipped_network(module: &str) -> Result<()> {
    note_skipped(module, NETWORK_ENV);
    Ok(())
}

/// 依赖本机 Chrome/Chromium 的测试是否已打开。
pub fn chrome_enabled() -> bool {
    *CHROME
}

/// 需要真人参与的测试是否已打开。
pub fn interactive_enabled() -> bool {
    *INTERACTIVE
}

/// 未打开交互开关时的跳过返回值。
pub fn skipped_interactive(module: &str) -> Result<()> {
    note_skipped(module, INTERACTIVE_ENV);
    Ok(())
}

/// 依赖已失效固定值的测试是否已打开。
pub fn stale_enabled() -> bool {
    *STALE
}

/// 未打开该开关时的跳过返回值。
pub fn skipped_stale(module: &str) -> Result<()> {
    note_skipped(module, STALE_ENV);
    Ok(())
}
