use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use dashmap::DashMap;
use rand::RngExt;
use sha2::{Digest, Sha256};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

/// 受保护页面的有效期，与 /flushvpn、/timetable 的一次性链接保持同一量级。
const GUARD_EXPIRE_SECS: u64 = 30 * 60;
/// 连续输错多少次后临时锁定。
const MAX_FAILS: u32 = 5;
/// 触发锁定后的冷却时长。
const LOCK_SECS: u64 = 5 * 60;
/// 口令摘要的迭代轮数，用来抬高暴力枚举成本。
const DERIVE_ROUNDS: u32 = 100_000;
/// 摘要的领域分隔串，避免和项目里其它 SHA-256 用途撞车。
const DERIVE_DOMAIN: &[u8] = b"xmu-assistant-bot/web-guard/v1";
/// 自动生成口令的长度。
const GENERATED_LEN: usize = 12;
/// 自定义口令的最短长度。
const MIN_CUSTOM_LEN: usize = 4;
/// 生成口令用的字符集，剔除了 0/O、1/l/I 这类肉眼易混的字符。
const PASSWORD_CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";

static GUARDS: LazyLock<DashMap<String, Arc<Guard>>> = LazyLock::new(DashMap::new);

/// 口令的两种来源。
#[derive(Debug, Clone)]
pub enum PasswordMode {
    /// 使用调用方（用户）自己指定的口令。
    Custom(String),
    /// 由本模块随机生成一个口令。
    Generated,
}

/// 一个受口令保护的页面。明文口令不落盘也不进日志，只保留盐和摘要。
pub struct Guard {
    pub id: String,
    pub qq: i64,
    pub expire_at: u64,
    /// 口令是随机生成的还是用户自定义的，仅用于提示文案。
    pub generated: bool,
    salt: [u8; 16],
    digest: [u8; 32],
    state: Mutex<GuardState>,
}

#[derive(Default)]
struct GuardState {
    /// 当前唯一有效的访问令牌。重新输入口令会轮换它，等价于把上一位访问者踢下线。
    token: Option<Arc<str>>,
    fails: u32,
    locked_until: u64,
    unlocked_at: Option<u64>,
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

/// 清理过期条目，避免被遗弃的链接在内存里堆积（沿用 vpn/task.rs 的做法）。
fn sweep() {
    let now = now_ts();
    GUARDS.retain(|_, guard| guard.expire_at > now);
}

fn random_password() -> String {
    let mut rng = rand::rng();
    (0..GENERATED_LEN)
        .map(|_| PASSWORD_CHARS[rng.random_range(0..PASSWORD_CHARS.len())] as char)
        .collect()
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

impl Guard {
    /// 创建一个受口令保护的页面，返回句柄和需要交给用户的明文口令。
    pub fn create(qq: i64, mode: PasswordMode) -> Result<(Arc<Self>, String)> {
        sweep();

        let (password, generated) = match mode {
            PasswordMode::Custom(password) => {
                let password = password.trim().to_owned();
                if password.chars().count() < MIN_CUSTOM_LEN {
                    bail!("自定义口令至少 {MIN_CUSTOM_LEN} 位");
                }
                (password, false)
            }
            PasswordMode::Generated => (random_password(), true),
        };

        let salt = random_bytes::<16>();
        let digest = derive(&salt, &password);

        let guard = Arc::new(Self {
            id: uuid::Uuid::new_v4().to_string(),
            qq,
            expire_at: now_ts() + GUARD_EXPIRE_SECS,
            generated,
            salt,
            digest,
            state: Mutex::new(GuardState::default()),
        });

        GUARDS.insert(guard.id.clone(), guard.clone());
        // 只记录 id 与来源，绝不记录口令本身。
        info!(guard_id = %guard.id, qq = qq, generated = generated, "创建受口令保护的页面");

        Ok((guard, password))
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

    /// 上一次解锁成功的时间戳。
    pub fn unlocked_at(&self) -> Option<u64> {
        self.lock_state().unlocked_at
    }

    /// 校验口令并换取访问令牌。成功时会轮换令牌，之前拿到令牌的人立即失效。
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

        // 迭代摘要是纯 CPU 工作，放到阻塞线程池，避免拖住 axum 的工作线程。
        let salt = self.salt;
        let password = password.to_owned();
        let digest = tokio::task::spawn_blocking(move || derive(&salt, &password))
            .await
            .map_err(|e| anyhow::anyhow!("口令校验任务异常退出: {e}"))?;

        let mut state = self.lock_state();
        if !constant_time_eq(&digest, &self.digest) {
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

        let token: Arc<str> = Arc::from(URL_SAFE_NO_PAD.encode(random_bytes::<32>()));
        let rotated = state.token.is_some();
        state.token = Some(token.clone());
        state.fails = 0;
        state.unlocked_at = Some(now_ts());
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

/// 页面结束（任务已完成或已过期）时主动移除。
pub fn remove_guard(id: &str) {
    GUARDS.remove(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn custom_password_unlocks_and_rotates_token() -> Result<()> {
        let (guard, password) = Guard::create(10001, PasswordMode::Custom("hunter2!".into()))?;
        assert_eq!(password, "hunter2!");
        assert!(!guard.generated);
        assert!(!guard.status().unlocked);

        let first = guard.unlock(&password).await?;
        assert!(guard.check_token(&first));
        assert!(verify(&guard.id, &first).is_some());

        // 再次输入正确口令会轮换令牌，把上一位访问者踢下线。
        let second = guard.unlock(&password).await?;
        assert_ne!(first, second);
        assert!(!guard.check_token(&first));
        assert!(guard.check_token(&second));

        guard.revoke_token();
        assert!(!guard.check_token(&second));
        remove_guard(&guard.id);
        Ok(())
    }

    #[tokio::test]
    async fn wrong_password_is_rejected_and_locks_out() -> Result<()> {
        let (guard, password) = Guard::create(10002, PasswordMode::Generated)?;
        assert_eq!(password.len(), GENERATED_LEN);
        assert!(guard.generated);

        for _ in 0..MAX_FAILS {
            assert!(guard.unlock("wrong-password").await.is_err());
        }
        assert!(guard.status().locked);
        // 锁定期间即便口令正确也拒绝。
        assert!(guard.unlock(&password).await.is_err());

        remove_guard(&guard.id);
        Ok(())
    }

    #[test]
    fn short_custom_password_is_refused() {
        assert!(Guard::create(10003, PasswordMode::Custom("ab".into())).is_err());
    }
}
