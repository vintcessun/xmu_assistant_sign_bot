use crate::api::storage::ColdTable;
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use dashmap::DashMap;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// 设置/刷新口令的一次性链接有效期。口令本身是永久的，只有这个链接短命。
const SETUP_EXPIRE_SECS: u64 = 15 * 60;
/// 受保护页面（如绩点查询）的有效期。
const GUARD_EXPIRE_SECS: u64 = 30 * 60;
/// 连续输错多少次后临时锁定。
const MAX_FAILS: u32 = 5;
/// 触发锁定后的冷却时长。
const LOCK_SECS: u64 = 5 * 60;
/// 口令摘要的迭代轮数，用来抬高暴力枚举成本。
const DERIVE_ROUNDS: u32 = 100_000;
/// 摘要的领域分隔串，避免和项目里其它 SHA-256 用途撞车。
const DERIVE_DOMAIN: &[u8] = b"xmu-assistant-bot/web-guard/v1";
/// 口令的最短长度。
pub const MIN_PASSWORD_LEN: usize = 6;

/// 每个 QQ 一份、**永久有效**的访问口令摘要。只有重新走一次设置链接才会被覆盖。
///
/// 用 ColdTable 而不是 HotTable：HotTable 的后台写入协程是一个永不返回的
/// `spawn_blocking`，`#[tokio::test]` 结束时 runtime 析构会一直等它，测试会挂死
/// （所以 md 模块的用例才叫 `..._without_db`）。口令读写量极小，直接落盘足够。
static SECRETS: LazyLock<ColdTable<i64, AccountSecret>> =
    LazyLock::new(|| ColdTable::new("web_guard_secret"));
/// 设置/刷新口令的一次性链接。
static SETUPS: LazyLock<DashMap<String, Arc<SetupTask>>> = LazyLock::new(DashMap::new);
/// 受保护页面。
static GUARDS: LazyLock<DashMap<String, Arc<Guard>>> = LazyLock::new(DashMap::new);

/// 落盘的口令摘要。明文永不保存、也永不进日志。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AccountSecret {
    salt: [u8; 16],
    digest: [u8; 32],
    /// 设置（或最近一次刷新）的时间戳。
    pub updated_at: u64,
}

/// 一条「设置或刷新口令」的一次性链接。
pub struct SetupTask {
    pub id: String,
    pub qq: i64,
    /// true 表示这是刷新（该 QQ 之前已经设过口令）。
    pub refresh: bool,
    pub expire_at: u64,
    used: Mutex<bool>,
}

/// 一个受口令保护的页面。口令不属于页面，属于 QQ 账号。
pub struct Guard {
    pub id: String,
    pub qq: i64,
    pub expire_at: u64,
    state: Mutex<GuardState>,
}

#[derive(Default)]
struct GuardState {
    /// 当前唯一有效的访问令牌。重新输入口令会轮换它，等价于把上一位访问者踢下线。
    token: Option<Arc<str>>,
    fails: u32,
    locked_until: u64,
}

/// 页面当前状态，用于前端渲染锁屏。
#[derive(Debug, Clone, Copy)]
pub struct GuardStatus {
    pub unlocked: bool,
    pub locked: bool,
    pub seconds_left: u64,
    pub lock_seconds_left: u64,
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut rng = rand::rng();
    let mut buf = [0u8; N];
    for byte in buf.iter_mut() {
        *byte = rng.random_range(0..=u8::MAX);
    }
    buf
}

/// 盐 + 多轮 SHA-256。轮数固定，不同口令的耗时一致。
fn derive(salt: &[u8; 16], password: &str) -> [u8; 32] {
    let mut digest: [u8; 32] = Sha256::new()
        .chain_update(DERIVE_DOMAIN)
        .chain_update(salt)
        .chain_update(password.as_bytes())
        .finalize()
        .into();

    for _ in 1..DERIVE_ROUNDS {
        digest = Sha256::new()
            .chain_update(salt)
            .chain_update(digest)
            .finalize()
            .into();
    }

    digest
}

/// 定时比较，避免通过响应耗时逐字节推断摘要。
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 迭代摘要是纯 CPU 工作，放到阻塞线程池，避免拖住 axum 的工作线程。
async fn derive_off_thread(salt: [u8; 16], password: &str) -> Result<[u8; 32]> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || derive(&salt, &password))
        .await
        .map_err(|e| anyhow::anyhow!("口令计算任务异常退出: {e}"))
}

// ---------------------------------------------------------------- 账号口令

/// 该 QQ 是否已经设置过口令。
pub fn has_secret(qq: i64) -> bool {
    load_secret(qq).is_some()
}

/// 该 QQ 口令的最近设置时间。
pub fn secret_updated_at(qq: i64) -> Option<u64> {
    load_secret(qq).map(|secret| secret.updated_at)
}

/// 读取口令摘要。读不出来一律当成"没设过"，让调用方去引导用户重设。
fn load_secret(qq: i64) -> Option<AccountSecret> {
    match SECRETS.get(&qq) {
        Ok(secret) => secret,
        Err(e) => {
            warn!(qq = qq, error = ?e, "读取访问口令摘要失败");
            None
        }
    }
}

/// 写入（或覆盖）该 QQ 的永久口令。
async fn save_secret(qq: i64, password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        bail!("访问口令至少 {MIN_PASSWORD_LEN} 位");
    }

    let salt = random_bytes::<16>();
    let digest = derive_off_thread(salt, password).await?;

    SECRETS
        .insert(
            &qq,
            &AccountSecret {
                salt,
                digest,
                updated_at: now_ts(),
            },
        )
        .await?;
    // 只记录发生了什么，绝不记录口令本身。
    info!(qq = qq, "访问口令已保存（永久有效，直到再次刷新）");
    Ok(())
}

/// 校验该 QQ 的口令。
async fn check_secret(qq: i64, password: &str) -> Result<bool> {
    let Some(secret) = load_secret(qq) else {
        bail!("该账号还没有设置访问口令，请先发送 /setpwd");
    };
    let digest = derive_off_thread(secret.salt, password).await?;
    Ok(constant_time_eq(&digest, &secret.digest))
}

// ------------------------------------------------------------ 设置口令链接

impl SetupTask {
    pub fn seconds_left(&self) -> u64 {
        self.expire_at.saturating_sub(now_ts())
    }

    fn is_used(&self) -> bool {
        *self.used.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 保存口令并让本链接立即失效。
    pub async fn save(&self, password: &str) -> Result<()> {
        if self.expire_at <= now_ts() {
            bail!("这个设置链接已过期，请重新发送 /setpwd");
        }
        {
            let used = self.used.lock().unwrap_or_else(|e| e.into_inner());
            if *used {
                bail!("这个设置链接已经用过了，请重新发送 /setpwd");
            }
        }

        save_secret(self.qq, password).await?;

        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        *used = true;
        drop(used);
        SETUPS.remove(&self.id);
        Ok(())
    }
}

/// 新建一条设置/刷新口令的一次性链接。
pub fn create_setup(qq: i64) -> Arc<SetupTask> {
    let now = now_ts();
    SETUPS.retain(|_, task| task.expire_at > now && !task.is_used());

    let task = Arc::new(SetupTask {
        id: uuid::Uuid::new_v4().to_string(),
        qq,
        refresh: has_secret(qq),
        expire_at: now + SETUP_EXPIRE_SECS,
        used: Mutex::new(false),
    });
    SETUPS.insert(task.id.clone(), task.clone());
    info!(qq = qq, refresh = task.refresh, setup_id = %task.id, "创建口令设置链接");
    task
}

pub fn get_setup(id: &str) -> Option<Arc<SetupTask>> {
    let task = SETUPS.get(id)?.clone();
    if task.expire_at <= now_ts() || task.is_used() {
        SETUPS.remove(id);
        return None;
    }
    Some(task)
}

// -------------------------------------------------------------- 受保护页面

impl Guard {
    /// 新建一个受保护页面。要求该 QQ 已经设置过永久口令。
    pub fn create(qq: i64) -> Result<Arc<Self>> {
        if !has_secret(qq) {
            bail!("请先发送 /setpwd 设置访问口令");
        }

        let now = now_ts();
        GUARDS.retain(|_, guard| guard.expire_at > now);

        let guard = Arc::new(Self {
            id: uuid::Uuid::new_v4().to_string(),
            qq,
            expire_at: now + GUARD_EXPIRE_SECS,
            state: Mutex::new(GuardState::default()),
        });
        GUARDS.insert(guard.id.clone(), guard.clone());
        info!(guard_id = %guard.id, qq = qq, "创建受口令保护的页面");
        Ok(guard)
    }

    pub fn seconds_left(&self) -> u64 {
        self.expire_at.saturating_sub(now_ts())
    }

    pub fn status(&self) -> GuardStatus {
        let now = now_ts();
        let state = self.lock_state();
        GuardStatus {
            unlocked: state.token.is_some(),
            locked: state.locked_until > now,
            seconds_left: self.expire_at.saturating_sub(now),
            lock_seconds_left: state.locked_until.saturating_sub(now),
        }
    }

    /// 用账号的永久口令解锁。成功会轮换令牌，之前拿到令牌的人立即失效。
    pub async fn unlock(&self, password: &str) -> Result<Arc<str>> {
        {
            let state = self.lock_state();
            let now = now_ts();
            if state.locked_until > now {
                bail!(
                    "口令连续输错次数过多，请 {} 秒后再试",
                    state.locked_until - now
                );
            }
        }

        if !check_secret(self.qq, password).await? {
            let mut state = self.lock_state();
            state.fails += 1;
            if state.fails >= MAX_FAILS {
                state.locked_until = now_ts() + LOCK_SECS;
                state.fails = 0;
                warn!(guard_id = %self.id, qq = self.qq, "口令连续输错，页面已临时锁定");
                bail!(
                    "口令连续输错 {MAX_FAILS} 次，页面已锁定 {} 分钟",
                    LOCK_SECS / 60
                );
            }
            let left = MAX_FAILS - state.fails;
            warn!(guard_id = %self.id, qq = self.qq, fails = state.fails, "口令校验失败");
            bail!("口令不正确（还可再试 {left} 次）");
        }

        let mut state = self.lock_state();
        let rotated = state.token.is_some();
        let token: Arc<str> = Arc::from(URL_SAFE_NO_PAD.encode(random_bytes::<32>()));
        state.token = Some(token.clone());
        state.fails = 0;
        drop(state);

        info!(guard_id = %self.id, qq = self.qq, rotated = rotated, "口令校验通过，已签发访问令牌");
        Ok(token)
    }

    /// 令牌是否是当前唯一有效的那一个。
    pub fn check_token(&self, token: &str) -> bool {
        if self.expire_at <= now_ts() {
            return false;
        }
        self.lock_state()
            .token
            .as_deref()
            .is_some_and(|current| current == token)
    }

    /// 主动登出：清掉当前令牌，页面回到锁屏状态。
    pub fn revoke_token(&self) {
        self.lock_state().token = None;
        debug!(guard_id = %self.id, "访问令牌已注销");
    }

    /// `Mutex` 只包着几个字段的短临界区；中毒时直接取回内部值继续用。
    fn lock_state(&self) -> MutexGuard<'_, GuardState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 按 id 取一个仍在有效期内的受保护页面。
pub fn get_guard(id: &str) -> Option<Arc<Guard>> {
    let guard = GUARDS.get(id)?.clone();
    if guard.expire_at <= now_ts() {
        GUARDS.remove(id);
        return None;
    }
    Some(guard)
}

/// 同时校验 id 与令牌，供被保护的模块在每个接口入口调用。
pub fn verify(id: &str, token: &str) -> Option<Arc<Guard>> {
    let guard = get_guard(id)?;
    guard.check_token(token).then_some(guard)
}

/// 删除某个 QQ 的永久口令（目前仅测试清理用）。
#[cfg(test)]
pub(crate) async fn forget_secret(qq: i64) -> Result<()> {
    SECRETS.remove(&qq).await
}

/// 页面结束（任务已完成或已过期）时主动移除。
pub fn remove_guard(id: &str) {
    GUARDS.remove(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "hunter2!";

    /// 口令是**落盘**的，固定 QQ 会让上一次 `cargo test` 的残留污染这一次。
    /// 用随机负数 QQ（真实 QQ 都是正数），跑多少次都互不干扰。
    fn test_qq() -> i64 {
        -rand::rng().random_range(1..1_000_000_000i64)
    }

    /// 每个测试用不同 QQ，避免共用同一张持久化表互相干扰。
    // 用到落盘的 HotTable，其懒初始化会阻塞，单线程 runtime 上会把自己堵死。
    #[tokio::test(flavor = "multi_thread")]
    async fn setup_link_saves_permanent_password() -> Result<()> {
        let qq = test_qq();
        assert!(!has_secret(qq));
        // 没设口令就不能建受保护页面
        assert!(Guard::create(qq).is_err());

        let setup = create_setup(qq);
        assert!(!setup.refresh);
        setup.save(PASSWORD).await?;
        assert!(has_secret(qq));
        // 一次性：同一条链接不能再用
        assert!(get_setup(&setup.id).is_none());

        // 口令是永久的：新建页面直接就能用它解锁
        let guard = Guard::create(qq)?;
        let token = guard.unlock(PASSWORD).await?;
        assert!(guard.check_token(&token));

        // 再建一个页面，仍然是同一个口令
        let another = Guard::create(qq)?;
        assert!(another.unlock(PASSWORD).await.is_ok());

        remove_guard(&guard.id);
        remove_guard(&another.id);
        forget_secret(qq).await?;
        Ok(())
    }

    // 用到落盘的 HotTable，其懒初始化会阻塞，单线程 runtime 上会把自己堵死。
    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_replaces_old_password() -> Result<()> {
        let qq = test_qq();
        create_setup(qq).save(PASSWORD).await?;

        let refresh = create_setup(qq);
        assert!(refresh.refresh, "第二次应被标记为刷新");
        refresh.save("brand-new-password").await?;

        let guard = Guard::create(qq)?;
        assert!(guard.unlock(PASSWORD).await.is_err(), "旧口令应失效");
        assert!(guard.unlock("brand-new-password").await.is_ok());

        remove_guard(&guard.id);
        forget_secret(qq).await?;
        Ok(())
    }

    // 用到落盘的 HotTable，其懒初始化会阻塞，单线程 runtime 上会把自己堵死。
    #[tokio::test(flavor = "multi_thread")]
    async fn wrong_password_locks_out_and_token_rotates() -> Result<()> {
        let qq = test_qq();
        create_setup(qq).save(PASSWORD).await?;
        let guard = Guard::create(qq)?;

        let first = guard.unlock(PASSWORD).await?;
        let second = guard.unlock(PASSWORD).await?;
        assert_ne!(first, second);
        assert!(!guard.check_token(&first), "旧令牌应被顶下线");

        for _ in 0..MAX_FAILS {
            assert!(guard.unlock("wrong-password").await.is_err());
        }
        assert!(guard.status().locked);
        assert!(guard.unlock(PASSWORD).await.is_err(), "锁定期内正确口令也拒绝");

        remove_guard(&guard.id);
        forget_secret(qq).await?;
        Ok(())
    }

    // 用到落盘的 HotTable，其懒初始化会阻塞，单线程 runtime 上会把自己堵死。
    #[tokio::test(flavor = "multi_thread")]
    async fn short_password_is_refused() -> Result<()> {
        let qq = test_qq();
        let setup = create_setup(qq);
        assert!(setup.save("ab").await.is_err());
        assert!(!has_secret(qq), "失败不应写入");
        // 失败不消耗链接
        assert!(get_setup(&setup.id).is_some());
        Ok(())
    }
}
