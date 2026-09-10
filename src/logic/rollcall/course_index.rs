//! 选课反查索引：定时拉取每个已登录用户的 my-courses，同时维护
//! `course_id -> [qq]` 与 `qq -> [course_id]` 两张表。
//!
//! - 正查（`course_id -> qq`）供二维码推送签到按课程精准推送，只推给选了该课的人；
//! - 反查（`qq -> course_id`）供定时签到做“同课分组蹲守”（见 [`super::watch`]），
//!   同时让登录/登出的增量维护从「全表扫描」降到「只动这个人选的那几门课」。
//!
//! 后台任务由 `main` 在连接建立后 [`spawn_background_tasks`] 触发（`LazyLock::force`），
//! 而非等到收到消息才懒触发。启动时立即抓一次，之后做低频兜底重建；
//! 登录 / 登出 / 更新课表分别由 [`spawn_upsert`] / [`spawn_remove`] 即时增量维护。

use super::data::LOGIN_DATA;
use super::utils::uniform;
use crate::api::scheduler::{TaskRunner, TimeTask};
use crate::api::xmu_service::lnt::MyCourses;
use ahash::RandomState;
use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tracing::{debug, info, warn};

/// 兜底重建间隔。选课在学期中基本不变，登录 / `/signtime` 都会即时增量更新，
/// 所以这里只是对账（顺带清掉已登出用户、补上没走过增量路径的改动）。
/// 取 6 小时：一次重建的代价是「每个已登录用户一条 my-courses」，量极小；
/// 而索引落后的代价是同课蹲守可能漏掉新加的课，宁可勤一点。
const REBUILD_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// 兜底重建时把每个用户的 my-courses 随机摊开的窗口（秒），避免 N 条请求同时齐射。
const REBUILD_SPREAD_SECS: u64 = 30;

/// 首次构建不摊开：启动后要尽快可用，晚 30 秒会让这期间的推送退化成全量。
static FIRST_BUILD: AtomicBool = AtomicBool::new(true);

/// 选课索引本体。用 Arc 共享，clone 廉价；两张表始终一起改，保持一致。
pub struct CourseIndexInner {
    by_course: DashMap<i64, Vec<i64>, RandomState>,
    by_qq: DashMap<i64, Vec<i64>, RandomState>,
}

impl Default for CourseIndexInner {
    fn default() -> Self {
        Self::new()
    }
}

impl CourseIndexInner {
    pub fn new() -> Self {
        Self {
            by_course: DashMap::with_hasher(RandomState::default()),
            by_qq: DashMap::with_hasher(RandomState::default()),
        }
    }

    /// 索引里登记过的用户数。
    pub fn user_count(&self) -> usize {
        self.by_qq.len()
    }

    /// 索引里登记过的课程数。
    pub fn course_count(&self) -> usize {
        self.by_course.len()
    }

    /// 选了该课且已登录的 qq 列表。
    pub fn qq_of_course(&self, course_id: i64) -> Option<Vec<i64>> {
        self.by_course.get(&course_id).map(|v| v.value().clone())
    }

    /// 该用户选的课；索引里没有这个人时返回 None（调用方需要兜底）。
    pub fn courses_of_qq(&self, qq: i64) -> Option<Vec<i64>> {
        self.by_qq.get(&qq).map(|v| v.value().clone())
    }

    /// 覆盖写入某用户的选课：先按反查表精确摘掉旧课，再写入新课。
    pub fn set_user(&self, qq: i64, mut courses: Vec<i64>) {
        courses.sort_unstable();
        courses.dedup();
        self.detach(qq);
        for c in &courses {
            self.by_course.entry(*c).or_default().push(qq);
        }
        self.by_qq.insert(qq, courses);
    }

    /// 把某用户从索引里彻底摘掉（登出）。
    pub fn remove_user(&self, qq: i64) {
        self.detach(qq);
    }

    /// 只摘不写：按反查表定位该用户占用的课程条目并逐个移除，避免全表 retain。
    fn detach(&self, qq: i64) {
        let Some((_, old)) = self.by_qq.remove(&qq) else {
            return;
        };
        for c in old {
            // 先在 get_mut 的作用域里改完再判空，guard 落地后才允许 remove，避免自锁。
            let empty = match self.by_course.get_mut(&c) {
                Some(mut e) => {
                    e.retain(|&x| x != qq);
                    e.is_empty()
                }
                None => false,
            };
            if empty {
                self.by_course.remove(&c);
            }
        }
    }
}

/// `Arc` 包一层，`TaskRunner` 的 Output 需要 Clone。
pub type CourseIndex = Arc<CourseIndexInner>;

pub struct CourseIndexTask;

#[async_trait]
impl TimeTask for CourseIndexTask {
    type Output = CourseIndex;

    fn name(&self) -> &'static str {
        "CourseIndexTask"
    }

    fn interval(&self) -> Duration {
        REBUILD_INTERVAL
    }

    async fn run(&self) -> Result<Self::Output> {
        // 快照已登录用户与其 lnt 会话，直接用会话拉课，避免逐个恢复登录。
        let users: Vec<(i64, String)> = {
            let mut v = Vec::new();
            for entry in &*LOGIN_DATA {
                v.push((*entry.key(), entry.value().lnt.clone()));
            }
            v
        };

        let spread = if FIRST_BUILD.swap(false, Ordering::Relaxed) {
            0
        } else {
            REBUILD_SPREAD_SECS
        };

        let user_total = users.len();
        let mut tasks = Vec::with_capacity(user_total);
        for (qq, lnt) in users {
            tasks.push(async move {
                if spread > 0 {
                    tokio::time::sleep(Duration::from_secs(uniform(0..spread))).await;
                }
                match MyCourses::get(&lnt).await {
                    Ok(resp) => Some((qq, resp.courses)),
                    Err(e) => {
                        debug!(qq, error = ?e, "拉取 my-courses 失败，跳过该用户");
                        None
                    }
                }
            });
        }

        let results = futures::future::join_all(tasks).await;

        let index = CourseIndexInner::new();
        for (qq, courses) in results.into_iter().flatten() {
            index.set_user(qq, courses.into_iter().map(|c| c.id).collect());
        }

        // 刻意不因“一个都没拉到”而报错：TaskRunner 的失败重试是 5 秒一轮且没有上限，
        // 而“全员会话过期”（比如深夜）恰好会让每次重建都失败，那就成了请求风暴。
        // 空索引本身是安全的——排班会退化成改动前的逐人轮询，不会漏签；
        // 而且用户下次登录 / `/signtime` 都会把自己即时并回索引。
        if user_total > 0 && index.user_count() == 0 {
            warn!(user_total, "选课索引重建后为空，本轮退化为逐人轮询");
        }

        info!(
            users = index.user_count(),
            courses = index.course_count(),
            "选课索引刷新完成"
        );
        Ok(Arc::new(index))
    }
}

pub static COURSE_INDEX_TASK: LazyLock<Arc<TaskRunner<CourseIndexTask>>> =
    LazyLock::new(|| TaskRunner::new(CourseIndexTask));

/// 取当前索引；未就绪时返回 None，调用方按“没有索引”兜底。
pub async fn current_index() -> Option<CourseIndex> {
    COURSE_INDEX_TASK.get_latest().await.ok()
}

/// 查“选了该 course_id 且已登录”的 qq 列表；索引未就绪或无该课时返回 None（调用方兜底全量）。
pub async fn qq_for_course(course_id: i64) -> Option<Vec<i64>> {
    current_index().await?.qq_of_course(course_id)
}

/// 某用户登录 / 更新课表后即时刷新其选课（不必等下次定时重建）。
pub async fn upsert_user(qq: i64, lnt: &str) {
    let courses = match MyCourses::get(lnt).await {
        Ok(resp) => resp.courses,
        Err(e) => {
            debug!(qq, error = ?e, "拉取 my-courses 失败，选课索引暂不更新");
            return;
        }
    };
    let Some(index) = current_index().await else {
        return;
    };
    let len = courses.len();
    index.set_user(qq, courses.into_iter().map(|c| c.id).collect());
    debug!(qq, courses = len, "已即时更新选课索引");
}

/// [`upsert_user`] 的非阻塞封装：登录写入 LOGIN_DATA 后调用，不拖慢登录主流程。
pub fn spawn_upsert(qq: i64, lnt: String) {
    tokio::spawn(async move { upsert_user(qq, &lnt).await });
}

/// 用户登出后即时把其从索引移除（不必等下次兜底重建）。
pub async fn remove_user(qq: i64) {
    let Some(index) = current_index().await else {
        return;
    };
    index.remove_user(qq);
    debug!(qq, "登出后已从选课索引移除");
}

/// [`remove_user`] 的非阻塞封装：登出移除 LOGIN_DATA 后调用。
pub fn spawn_remove(qq: i64) {
    tokio::spawn(async move { remove_user(qq).await });
}

/// 由 `main` 在连接建立后调用：强制初始化并启动后台定时任务（定时签到 + 选课索引），
/// 替代原先“收到消息才懒触发”的方式。`LazyLock::force` 会触发 `TaskRunner::new`，
/// 后者立即 spawn 其后台维护循环。
pub fn spawn_background_tasks() {
    LazyLock::force(&super::time_sign::TIME_SIGN_TASK_RUNNER);
    LazyLock::force(&COURSE_INDEX_TASK);
    info!("已在启动时触发定时签到与选课索引后台任务");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_user_keeps_both_directions_in_sync() {
        let index = CourseIndexInner::new();
        index.set_user(1, vec![10, 20]);
        index.set_user(2, vec![20, 30]);

        assert_eq!(index.courses_of_qq(1), Some(vec![10, 20]));
        assert_eq!(index.qq_of_course(10), Some(vec![1]));
        let mut c20 = index.qq_of_course(20).unwrap();
        c20.sort_unstable();
        assert_eq!(c20, vec![1, 2]);
        assert_eq!(index.user_count(), 2);
        assert_eq!(index.course_count(), 3);
    }

    #[test]
    fn set_user_replaces_old_courses() {
        let index = CourseIndexInner::new();
        index.set_user(1, vec![10, 20]);
        // 退掉 10、改选 30
        index.set_user(1, vec![20, 30]);

        assert_eq!(index.courses_of_qq(1), Some(vec![20, 30]));
        // 10 上再没有人，条目应当被清掉而不是留一个空 Vec
        assert_eq!(index.qq_of_course(10), None);
        assert_eq!(index.qq_of_course(30), Some(vec![1]));
        assert_eq!(index.course_count(), 2);
    }

    #[test]
    fn set_user_dedups() {
        let index = CourseIndexInner::new();
        index.set_user(1, vec![10, 10, 20]);
        assert_eq!(index.courses_of_qq(1), Some(vec![10, 20]));
        assert_eq!(index.qq_of_course(10), Some(vec![1]));
    }

    #[test]
    fn remove_user_clears_everything() {
        let index = CourseIndexInner::new();
        index.set_user(1, vec![10, 20]);
        index.set_user(2, vec![20]);
        index.remove_user(1);

        assert_eq!(index.courses_of_qq(1), None);
        assert_eq!(index.qq_of_course(10), None);
        assert_eq!(index.qq_of_course(20), Some(vec![2]));
        assert_eq!(index.user_count(), 1);
    }

    #[test]
    fn remove_unknown_user_is_noop() {
        let index = CourseIndexInner::new();
        index.set_user(1, vec![10]);
        index.remove_user(999);
        assert_eq!(index.qq_of_course(10), Some(vec![1]));
        assert_eq!(index.user_count(), 1);
    }
}
