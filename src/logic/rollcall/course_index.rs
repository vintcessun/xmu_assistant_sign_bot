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
use crate::api::xmu_service::lnt::my_courses::Course;
use crate::api::xmu_service::time::set_semester_start;
use ahash::RandomState;
use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDate;
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
    /// `course_code(＝教务 BJDM) -> course_id`。课表条目靠它精确落到某个教学班，
    /// 只装当前学期的课，免得旧学期的同名班把它顶掉。
    by_code: DashMap<Arc<str>, i64, RandomState>,
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
            by_code: DashMap::with_hasher(RandomState::default()),
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

    /// 教务班级代码 -> lnt course_id。
    pub fn id_of_class_code(&self, code: &str) -> Option<i64> {
        self.by_code.get(code).map(|v| *v.value())
    }

    /// 覆盖写入某用户的选课：先按反查表精确摘掉旧课，再写入新课。
    pub fn set_user(&self, qq: i64, mut courses: Vec<(i64, Arc<str>)>) {
        courses.sort_unstable_by_key(|(id, _)| *id);
        courses.dedup_by_key(|(id, _)| *id);
        self.detach(qq);
        for (id, code) in &courses {
            self.by_course.entry(*id).or_default().push(qq);
            self.by_code.insert(code.clone(), *id);
        }
        self.by_qq
            .insert(qq, courses.into_iter().map(|(id, _)| id).collect());
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

/// lnt 课程 -> 索引里存的 `(course_id, course_code)`。
fn coded(course: Course) -> (i64, Arc<str>) {
    (course.id, Arc::from(course.course_code.as_str()))
}

/// 从当前学期课程的开课日期推算学期第一天，省掉每学期手改一次常量。
///
/// 取**众数**而不是最小值。这一条是拿七个学期的真实数据回测出来的：
/// 2024-1 学期里有一门课 `2024-08-07`（周三）就开课了，比正常开学早四周，
/// 取最小值会把整个学期的周次算错四周；众数 `2024-09-02` 才是对的。
/// 七个学期里众数全对，且与仓库历史上三次手改的值逐个吻合。
fn refresh_semester_start(users: &[(i64, Vec<Course>)]) {
    let dates: Vec<NaiveDate> = users
        .iter()
        .flat_map(|(_, courses)| courses.iter())
        .filter_map(|c| c.start_date.as_deref())
        .filter_map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .collect();
    let Some(start) = most_common_date(&dates) else {
        debug!("当前学期课程没有开课日期，学期第一天继续用兜底值");
        return;
    };
    set_semester_start(start);
}

/// 出现次数最多的日期；次数相同取较早的那个。
fn most_common_date(dates: &[NaiveDate]) -> Option<NaiveDate> {
    let mut sorted: Vec<NaiveDate> = dates.to_vec();
    sorted.sort_unstable();
    let mut best: Option<(NaiveDate, usize)> = None;
    let mut i = 0usize;
    while i < sorted.len() {
        let mut j = i;
        while j < sorted.len() && sorted[j] == sorted[i] {
            j += 1;
        }
        // 严格大于：并列时保留先遇到的（也就是较早的）那个
        if best.is_none_or(|(_, n)| j - i > n) {
            best = Some((sorted[i], j - i));
        }
        i = j;
    }
    best.map(|(d, _)| d)
}

/// 只保留“当前学期”的课。
///
/// `my-courses` 返回的是历年全部课程（线上实测某账号 60 门，其中当前学期只有 11 门；
/// 全量索引是 79 人 2043 门）。早已结课的课永远不会再有签到，却会把定时签到
/// “要覆盖的课程集合”撑爆——而且往年的课基本没有第二个人选，于是每个人都被迫
/// 自己当哨兵，同课分组就白做了。
///
/// 判据用 lnt 自己给的 `semester.sort`：全校统一、随时间单调递增的序号，
/// 取本轮抓到的最大值即当前学期。比按课程名猜、或按 `end_date` 猜都可靠
/// （实测有历史课程的 `end_date` 也是 null，靠它筛会漏）。
fn keep_current_semester(users: &mut [(i64, Vec<Course>)]) -> Option<String> {
    let latest = users
        .iter()
        .flat_map(|(_, courses)| courses.iter())
        .filter_map(|c| c.semester.as_ref())
        .max_by_key(|s| s.sort)?
        .clone();
    for (_, courses) in users.iter_mut() {
        courses.retain(|c| c.semester.as_ref().is_some_and(|s| s.sort == latest.sort));
    }
    Some(latest.code)
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

        let mut fetched: Vec<(i64, Vec<Course>)> = results.into_iter().flatten().collect();
        let raw_total: usize = fetched.iter().map(|(_, c)| c.len()).sum();
        let semester = keep_current_semester(&mut fetched);
        // 顺手把学期第一天推算出来。这里天然就是"挑会话没过期的那些人"——
        // 拉失败的用户上面已经被跳过了，剩下的都是有效数据。
        refresh_semester_start(&fetched);

        let index = CourseIndexInner::new();
        for (qq, courses) in fetched {
            index.set_user(qq, courses.into_iter().map(coded).collect());
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
            raw_courses = raw_total,
            semester,
            "选课索引刷新完成（只保留当前学期）"
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
    // 单人增量也只留当前学期，口径必须和整表重建一致。这里没有别人的数据可比，
    // 就取这个人自己最新的那个学期——正常在读的学生，最新学期就是当前学期。
    let mut one = vec![(qq, courses)];
    keep_current_semester(&mut one);
    let courses = one.pop().map(|(_, c)| c).unwrap_or_default();
    let len = courses.len();
    index.set_user(qq, courses.into_iter().map(coded).collect());
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
    // 还留在 v3 课表上的人数。归零＝存量已被 `/signtime` 刷干净，
    // 那时就可以把 legacy 模块、v3 表和这行日志一起删掉。
    let legacy = super::timetable::legacy_v3_count();
    info!(
        legacy_v3_users = legacy,
        "已在启动时触发定时签到与选课索引后台任务"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::api::xmu_service::lnt::my_courses::Semester;

    fn course(id: i64, sort: Option<i64>) -> Course {
        Course {
            id,
            name: format!("课程{id}"),
            course_code: format!("2026202711302200001{id:04}"),
            semester: sort.map(|sort| Semester {
                code: format!("2026-{sort}"),
                sort,
            }),
            start_date: Some("2026-09-07".to_string()),
        }
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// 一条回测样本：`(学期, [(开课日期, 出现次数)], 期望推算出的学期第一天)`。
    type BacktestCase = (&'static str, &'static [(&'static str, usize)], &'static str);

    /// 回测：用线上全量数据验证「学期第一天」的推算规则。
    ///
    /// 数据来自 2026-09-11 对服务器上 84 个有效会话的一次性回测，
    /// 覆盖 10 个学期共 4672 条课程记录。每行是 `(学期, [(开课日期, 出现次数)], 期望值)`。
    ///
    /// 其中 2025-2 / 2025-3 / 2026-1 三个学期在仓库历史里有人手改过 START_DATE
    /// （65b0148 / 59991da / 1944680），推算值与手改值逐个吻合。
    #[test]
    fn semester_start_backtest_against_real_data() {
        let cases: &[BacktestCase] = &[
            ("2023-2", &[("2024-02-26", 238)], "2024-02-26"),
            ("2023-3", &[("2024-06-24", 27)], "2024-06-24"),
            // 43 门课在正常开学前四周就"开课"了，取最小值会把整学期算错四周
            (
                "2024-1",
                &[("2024-08-07", 43), ("2024-09-02", 667), ("2024-09-09", 2)],
                "2024-09-02",
            ),
            (
                "2024-2",
                &[("2025-02-17", 757), ("2025-03-03", 1)],
                "2025-02-17",
            ),
            // 小学期真的是周五开学，不是所有学期都从周一起算
            ("2024-3", &[("2025-06-20", 121)], "2025-06-20"),
            (
                "2025-1",
                &[("2025-08-13", 19), ("2025-09-01", 966), ("2025-09-15", 5)],
                "2025-09-01",
            ),
            (
                "2025-2",
                &[("2025-04-08", 2), ("2026-03-02", 944)],
                "2026-03-02",
            ),
            ("2025-3", &[("2026-06-29", 143)], "2026-06-29"),
            // 当前学期里有一门课的开课日期是两年多前，取最小值会把起点推早 122 周
            (
                "2026-1",
                &[("2024-05-07", 1), ("2026-09-07", 735)],
                "2026-09-07",
            ),
        ];

        for (semester, starts, expected) in cases {
            let dates: Vec<NaiveDate> = starts
                .iter()
                .flat_map(|(day, n)| std::iter::repeat_n(d(day), *n))
                .collect();
            assert_eq!(
                most_common_date(&dates),
                Some(d(expected)),
                "{semester} 学期的学期第一天推算错了"
            );
        }
    }

    /// 钉住那几个反例：如果哪天有人想把众数换回最小值，这条会红。
    #[test]
    fn earliest_start_date_would_be_wrong() {
        // 当前学期：一门课挂着两年多前的开课日期
        let mut dates = vec![d("2024-05-07")];
        dates.extend(std::iter::repeat_n(d("2026-09-07"), 735));

        assert_eq!(
            dates.iter().min().copied(),
            Some(d("2024-05-07")),
            "最小值会取到那门离谱的课"
        );
        assert_eq!(most_common_date(&dates), Some(d("2026-09-07")));
        // 差了 121 周——按最小值算周次，整个签到功能直接报废
        assert_eq!((d("2026-09-07") - d("2024-05-07")).num_days() / 7, 121);
    }

    #[test]
    fn no_start_dates_yields_nothing() {
        assert_eq!(most_common_date(&[]), None);
    }

    /// 次数并列时取较早的：学期第一天不会比大多数课的开课日晚。
    #[test]
    fn ties_prefer_the_earlier_date() {
        let dates = vec![d("2026-09-14"), d("2026-09-07")];
        assert_eq!(most_common_date(&dates), Some(d("2026-09-07")));
    }

    /// 线上真实形状：某账号 60 门课横跨 2024-1(sort 9) ~ 2026-1(sort 15)，
    /// 当前学期只有 11 门。只留 sort 最大的那一档。
    #[test]
    fn keeps_only_the_latest_semester() {
        let mut users = vec![
            (
                1,
                vec![
                    course(10, Some(15)),
                    course(11, Some(9)),
                    course(12, Some(13)),
                ],
            ),
            (2, vec![course(20, Some(14)), course(21, Some(15))]),
        ];
        let latest = keep_current_semester(&mut users);

        assert_eq!(latest.as_deref(), Some("2026-15"));
        assert_eq!(
            users[0].1.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![10]
        );
        assert_eq!(
            users[1].1.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![21]
        );
    }

    /// 实测有课程的 semester 是缺的；缺学期信息就不算当前学期，别猜。
    #[test]
    fn courses_without_semester_are_dropped() {
        let mut users = vec![(1, vec![course(10, Some(15)), course(11, None)])];
        keep_current_semester(&mut users);
        assert_eq!(
            users[0].1.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![10]
        );
    }

    /// 谁都没有学期信息时不做过滤（返回 None），退化成原样，不至于把索引清空。
    #[test]
    fn no_semester_anywhere_keeps_everything() {
        let mut users = vec![(1, vec![course(10, None), course(11, None)])];
        assert_eq!(keep_current_semester(&mut users), None);
        assert_eq!(users[0].1.len(), 2);
    }

    /// 当前学期一门课都没有的用户（毕业 / 本学期没在 lnt 上课）会被清空，
    /// 排班里就成了“索引查不到”，自己当哨兵——正确，他本来也没有课要分组。
    #[test]
    fn user_with_no_current_course_ends_up_empty() {
        let mut users = vec![
            (1, vec![course(10, Some(15))]),
            (2, vec![course(20, Some(9)), course(21, Some(10))]),
        ];
        keep_current_semester(&mut users);
        assert_eq!(users[0].1.len(), 1);
        assert!(users[1].1.is_empty());
    }

    fn ids(list: &[i64]) -> Vec<(i64, Arc<str>)> {
        list.iter()
            .map(|id| (*id, Arc::from(format!("code-{id}").as_str())))
            .collect()
    }

    #[test]
    fn set_user_keeps_both_directions_in_sync() {
        let index = CourseIndexInner::new();
        index.set_user(1, ids(&[10, 20]));
        index.set_user(2, ids(&[20, 30]));

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
        index.set_user(1, ids(&[10, 20]));
        // 退掉 10、改选 30
        index.set_user(1, ids(&[20, 30]));

        assert_eq!(index.courses_of_qq(1), Some(vec![20, 30]));
        // 10 上再没有人，条目应当被清掉而不是留一个空 Vec
        assert_eq!(index.qq_of_course(10), None);
        assert_eq!(index.qq_of_course(30), Some(vec![1]));
        assert_eq!(index.course_count(), 2);
    }

    #[test]
    fn set_user_dedups() {
        let index = CourseIndexInner::new();
        index.set_user(1, ids(&[10, 10, 20]));
        assert_eq!(index.courses_of_qq(1), Some(vec![10, 20]));
        assert_eq!(index.qq_of_course(10), Some(vec![1]));
    }

    #[test]
    fn remove_user_clears_everything() {
        let index = CourseIndexInner::new();
        index.set_user(1, ids(&[10, 20]));
        index.set_user(2, ids(&[20]));
        index.remove_user(1);

        assert_eq!(index.courses_of_qq(1), None);
        assert_eq!(index.qq_of_course(10), None);
        assert_eq!(index.qq_of_course(20), Some(vec![2]));
        assert_eq!(index.user_count(), 1);
    }

    #[test]
    fn remove_unknown_user_is_noop() {
        let index = CourseIndexInner::new();
        index.set_user(1, ids(&[10]));
        index.remove_user(999);
        assert_eq!(index.qq_of_course(10), Some(vec![1]));
        assert_eq!(index.user_count(), 1);
    }
}
