use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use dashmap::DashMap;
use rand::RngExt;
use sha2::{Digest, Sha256};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// 受保护页面的总有效期，与 /flushvpn、/timetable 的一次性链接保持同一量级。
const GUARD_EXPIRE_SECS: u64 = 30 * 60;
/// 认领窗口：链接是公开发在群里的，口令没人设置就一直敞着风险太大。
const SETUP_WINDOW_SECS: u64 = 10 * 60;
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

static GUARDS: LazyLock<DashMap<String, Arc<Guard>>> = LazyLock::new(DashMap::new);

/// 一个受口令保护的页面。
///
/// 口令不再经过聊天窗口：`/jdpm` 只发链接，口令由**第一个打开页面的人**在网页上设置
/// （可以手输，也可以让页面随机生成）。明文口令不落盘也不进日志，只留盐和摘要。
pub struct Guard {
    pub id: String,
    /// 发起该页面的 QQ，仅用于页面展示与日志。
    pub qq: i64,
    pub expire_at: u64,
    /// 口令设置（认领）的截止时间。
    pub setup_deadline: u64,
    state: Mutex<GuardState>,
}

/// 已设置的口令：随机盐 + 多轮摘要。
struct Secret {
    salt: [u8; 16],
    digest: [u8; 32],
}

#[derive(Default)]
struct GuardState {
    /// 尚未设置口令时为 None。
    secret: Option<Secret>,
    /// 当前唯一有效的访问令牌。重新输入口令会轮换它，等价于把上一位访问者踢下线。
    token: Option<Arc<str>>,
    fails: u32,
    locked_until: u64,
}

/// 页面当前状态，用于前端决定渲染「设置口令」还是「输入口令」。
#[derive(Debug, Clone, Copy)]
pub struct GuardStatus {
    /// 口令是否已经设置过。
    pub configured: bool,
    pub unlocked: bool,
    pub locked: bool,
    pub seconds_left: u64,
    pub setup_seconds_left: u64,
    pub lock_seconds_left: u64,
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 清理过期条目，避免被遗弃的链接在内存里堆积（沿用 vpn/task.rs 的做法）。
fn sweep() {
    let now = now_ts();
    GUARDS.retain(|_, guard| guard.expire_at > now);
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

fn check_password_shape(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        bail!("访问口令至少 {MIN_PASSWORD_LEN} 位");
    }
    Ok(())
}

impl Guard {
    /// 新建一个「还没有口令」的受保护页面。口令由第一个打开页面的人设置。
    pub fn create(qq: i64) -> Arc<Self> {
        sweep();

        let now = now_ts();
        let guard = Arc::new(Self {
            id: uuid::Uuid::new_v4().to_string(),
            qq,
            expire_at: now + GUARD_EXPIRE_SECS,
            setup_deadline: now + SETUP_WINDOW_SECS,
            state: Mutex::new(GuardState::default()),
        });

        GUARDS.insert(guard.id.clone(), guard.clone());
        info!(guard_id = %guard.id, qq = qq, "创建待设置口令的受保护页面");
        guard
    }

    pub fn seconds_left(&self) -> u64 {
        self.expire_at.saturating_sub(now_ts())
    }

    pub fn status(&self) -> GuardStatus {
        let now = now_ts();
        let state = self.lock_state();
        GuardStatus {
            configured: state.secret.is_some(),
            unlocked: state.token.is_some(),
            locked: state.locked_until > now,
            seconds_left: self.expire_at.saturating_sub(now),
            setup_seconds_left: self.setup_deadline.saturating_sub(now),
            lock_seconds_left: state.locked_until.saturating_sub(now),
        }
    }

    /// 首次设置访问口令。只能成功一次，成功后直接把令牌发给设置者。
    ///
    /// 链接公开在群里，所以这一步是「先到先得」：谁先设置口令，谁就拿到这个页面。
    pub async fn setup(&self, password: &str) -> Result<Arc<str>> {
        check_password_shape(password)?;

        {
            let state = self.lock_state();
            if state.secret.is_some() {
                bail!("本页面的访问口令已经被设置过了，请直接输入口令");
            }
            if self.setup_deadline <= now_ts() {
                bail!("设置口令的时间窗口已过期，请重新发送 /jdpm");
            }
        }

        let salt = random_bytes::<16>();
        let digest = self.derive_off_thread(salt, password).await?;

        let mut state = self.lock_state();
        // 并发保护：派生期间可能已经有人抢先设置好了。
        if state.secret.is_some() {
            bail!("本页面的访问口令刚刚已被设置，请直接输入口令");
        }
        state.secret = Some(Secret { salt, digest });
        let token = Self::issue_token(&mut state);
        drop(state);

        info!(guard_id = %self.id, qq = self.qq, "访问口令设置完成，已签发令牌");
        Ok(token)
    }

    /// 校验口令并换取访问令牌。成功时会轮换令牌，之前拿到令牌的人立即失效。
    pub async fn unlock(&self, password: &str) -> Result<Arc<str>> {
        let salt = {
            let state = self.lock_state();
            let now = now_ts();
            if state.locked_until > now {
                bail!(
                    "口令连续输错次数过多，请 {} 秒后再试",
                    state.locked_until - now
                );
            }
            match state.secret.as_ref() {
                Some(secret) => secret.salt,
                None => bail!("本页面还没有设置访问口令，请先在页面上设置"),
            }
        };

        let digest = self.derive_off_thread(salt, password).await?;

        let mut state = self.lock_state();
        let expected = match state.secret.as_ref() {
            Some(secret) => secret.digest,
            None => bail!("本页面还没有设置访问口令，请先在页面上设置"),
        };

        if !constant_time_eq(&digest, &expected) {
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

        let rotated = state.token.is_some();
        let token = Self::issue_token(&mut state);
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

    /// 迭代摘要是纯 CPU 工作，放到阻塞线程池，避免拖住 axum 的工作线程。
    async fn derive_off_thread(&self, salt: [u8; 16], password: &str) -> Result<[u8; 32]> {
        let password = password.to_owned();
        tokio::task::spawn_blocking(move || derive(&salt, &password))
            .await
            .map_err(|e| anyhow::anyhow!("口令校验任务异常退出: {e}"))
    }

    /// 签发新令牌并顶掉旧的。
    fn issue_token(state: &mut GuardState) -> Arc<str> {
        let token: Arc<str> = Arc::from(URL_SAFE_NO_PAD.encode(random_bytes::<32>()));
        state.token = Some(token.clone());
        state.fails = 0;
        token
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

/// 页面结束（任务已完成或已过期）时主动移除。
pub fn remove_guard(id: &str) {
    GUARDS.remove(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "hunter2!";

    #[tokio::test]
    async fn setup_then_unlock_and_rotate() -> Result<()> {
        let guard = Guard::create(10001);
        assert!(!guard.status().configured);
        assert!(!guard.status().unlocked);

        // 设置口令的人当场拿到令牌，不必再输一次。
        let first = guard.setup(PASSWORD).await?;
        assert!(guard.status().configured);
        assert!(guard.check_token(&first));
        assert!(verify(&guard.id, &first).is_some());

        // 口令只能设置一次。
        assert!(guard.setup("another-password").await.is_err());

        // 再次输入正确口令会轮换令牌，把上一位访问者踢下线。
        let second = guard.unlock(PASSWORD).await?;
        assert_ne!(first, second);
        assert!(!guard.check_token(&first));
        assert!(guard.check_token(&second));

        guard.revoke_token();
        assert!(!guard.check_token(&second));
        remove_guard(&guard.id);
        Ok(())
    }

    #[tokio::test]
    async fn unlock_before_setup_is_refused() -> Result<()> {
        let guard = Guard::create(10002);
        assert!(guard.unlock(PASSWORD).await.is_err());
        remove_guard(&guard.id);
        Ok(())
    }

    #[tokio::test]
    async fn wrong_password_is_rejected_and_locks_out() -> Result<()> {
        let guard = Guard::create(10003);
        guard.setup(PASSWORD).await?;

        for _ in 0..MAX_FAILS {
            assert!(guard.unlock("wrong-password").await.is_err());
        }
        assert!(guard.status().locked);
        // 锁定期间即便口令正确也拒绝。
        assert!(guard.unlock(PASSWORD).await.is_err());

        remove_guard(&guard.id);
        Ok(())
    }

    #[tokio::test]
    async fn short_password_is_refused() -> Result<()> {
        let guard = Guard::create(10004);
        assert!(guard.setup("ab").await.is_err());
        assert!(!guard.status().configured);
        remove_guard(&guard.id);
        Ok(())
    }
}
