//! 选课反查索引：定时拉取每个已登录用户的 my-courses，构建 `course_id -> [qq]`，
//! 供二维码推送签到按课程精准推送（只推给选了该课的人），减少无差别全量推送的开销。
//!
//! 后台任务由 `main` 在连接建立后 [`spawn_background_tasks`] 触发（`LazyLock::force`），
//! 而非等到收到消息才懒触发。启动时立即抓一次，之后只做极低频的兜底重建（默认一周一次）；
//! 登录 / 登出分别由 [`spawn_upsert`] / [`spawn_remove`] 即时增量维护，不必等定时重建。

use super::data::LOGIN_DATA;
use crate::api::scheduler::{TaskRunner, TimeTask};
use crate::api::xmu_service::lnt::MyCourses;
use ahash::RandomState;
use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tracing::{debug, info};

/// `course_id -> 已登录且选了该课的 qq 列表`。用 Arc 共享，clone 廉价。
pub type CourseIndex = Arc<DashMap<i64, Vec<i64>, RandomState>>;

pub struct CourseIndexTask;

#[async_trait]
impl TimeTask for CourseIndexTask {
    type Output = CourseIndex;

    fn name(&self) -> &'static str {
        "CourseIndexTask"
    }

    fn interval(&self) -> Duration {
        // 启动时会立即抓一次（TaskRunner 的 maintain 首个 tick 立即执行）；之后选课学期内
        // 几乎不变、且新登录已由 upsert_user 即时并入，所以后续只做极低频的兜底对账
        // （顺带清掉已登出用户），一周一次足够。
        Duration::from_secs(7 * 24 * 60 * 60)
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

        let mut tasks = Vec::with_capacity(users.len());
        for (qq, lnt) in users {
            tasks.push(async move {
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

        let index: DashMap<i64, Vec<i64>, RandomState> = DashMap::with_hasher(RandomState::default());
        let mut user_count = 0usize;
        for (qq, courses) in results.into_iter().flatten() {
            user_count += 1;
            for c in courses {
                index.entry(c.id).or_default().push(qq);
            }
        }

        info!(
            users = user_count,
            courses = index.len(),
            "选课索引刷新完成"
        );
        Ok(Arc::new(index))
    }
}

pub static COURSE_INDEX_TASK: LazyLock<Arc<TaskRunner<CourseIndexTask>>> =
    LazyLock::new(|| TaskRunner::new(CourseIndexTask));

/// 查“选了该 course_id 且已登录”的 qq 列表；索引未就绪或无该课时返回 None（调用方兜底全量）。
pub async fn qq_for_course(course_id: i64) -> Option<Vec<i64>> {
    let index = COURSE_INDEX_TASK.get_latest().await.ok()?;
    index.get(&course_id).map(|v| v.value().clone())
}

/// 某用户登录后即时把其选课并入索引（随登录动态更新，不必等下次定时刷新）。
/// 先从所有课程列表移除该 qq（处理重复登录 / 退课），再按最新选课重新加入。
pub async fn upsert_user(qq: i64, lnt: &str) {
    let courses = match MyCourses::get(lnt).await {
        Ok(resp) => resp.courses,
        Err(e) => {
            debug!(qq, error = ?e, "登录后拉取 my-courses 失败，选课索引暂不更新");
            return;
        }
    };
    let Ok(index) = COURSE_INDEX_TASK.get_latest().await else {
        return;
    };
    // 移除该 qq 在旧数据里的所有出现，并顺手清掉变空的课程条目。
    index.retain(|_, v| {
        v.retain(|&x| x != qq);
        !v.is_empty()
    });
    for c in &courses {
        index.entry(c.id).or_default().push(qq);
    }
    debug!(qq, courses = courses.len(), "登录后已即时更新选课索引");
}

/// [`upsert_user`] 的非阻塞封装：登录写入 LOGIN_DATA 后调用，不拖慢登录主流程。
pub fn spawn_upsert(qq: i64, lnt: String) {
    tokio::spawn(async move { upsert_user(qq, &lnt).await });
}

/// 用户登出后即时把其从索引移除（不必等下次兜底重建）。
pub async fn remove_user(qq: i64) {
    let Ok(index) = COURSE_INDEX_TASK.get_latest().await else {
        return;
    };
    index.retain(|_, v| {
        v.retain(|&x| x != qq);
        !v.is_empty()
    });
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
