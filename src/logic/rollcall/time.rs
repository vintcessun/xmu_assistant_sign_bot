use super::timetable::{all_timetable_users, query_sign_time};
use crate::api::scheduler::{TaskRunner, TimeTask};
use crate::api::xmu_service::calendar::{self, DayKind};
use crate::api::xmu_service::jw::{ClockTime, CourseTime, TimeBitMap, Weekday};
use crate::api::xmu_service::time::{get_today, today_date};
use ahash::RandomState;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{Datelike, NaiveDate};
use dashmap::DashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::task::block_in_place;
use tracing::warn;
pub struct TimeSignUpdateTask;

#[async_trait]
impl TimeTask for TimeSignUpdateTask {
    type Output = DashMap<i64, TimeBitMap, RandomState>;

    fn name(&self) -> &'static str {
        "TimeSignUpdateTask"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(240)
    }

    async fn run(&self) -> Result<Self::Output> {
        block_in_place(|| {
            warn_if_calendar_stale(today_date());
            let map = DashMap::with_hasher(RandomState::default());
            for qq in all_timetable_users() {
                if let Some(today_courses) = get_today_courses(qq) {
                    map.insert(qq, today_courses);
                }
            }
            Ok(map)
        })
    }
}

pub static TIME_SIGN_TASK: LazyLock<Arc<TaskRunner<TimeSignUpdateTask>>> =
    LazyLock::new(|| TaskRunner::new(TimeSignUpdateTask));

/// 上一次为「校历没覆盖到今天」报警的日子，存成 `num_days_from_ce()`。
///
/// 这个任务每 240 秒跑一轮，不去重的话一天能刷三百多条同样的 WARN。
static COVERAGE_WARNED_ON: AtomicI32 = AtomicI32::new(0);

/// 今天超出当前校历的覆盖范围时喊一声，一天最多一次。
///
/// 范围外 [`calendar::day_kind`] 只会给 [`DayKind::Normal`]，也就是**调休日会被当成普通
/// 周末**——那正是 2026-09-20 漏签的成因。正常情况下每天会从 jwc 拉一次新校历把范围推后
/// （[`calendar::spawn_refresh_task`]），所以这条日志意味着：要么 jwc 还没发布那一年的
/// 节假日，要么抓取一直在失败。两种都得有人看一眼。
fn warn_if_calendar_stale(today: NaiveDate) {
    if calendar::is_covered(today) {
        return;
    }
    let key = today.num_days_from_ce();
    if COVERAGE_WARNED_ON.swap(key, Ordering::Relaxed) == key {
        return;
    }
    warn!(
        today = %today,
        covered_until = ?calendar::coverage_end(),
        "校历没覆盖到今天，调休日会被当成普通周末漏掉（jwc 尚未发布，或校历抓取一直失败）"
    );
}

/// 这节课今天算不算数。
///
/// 三种日子三套规矩，[`get_today_courses`] 和 [`class_codes_in_session_now`] 共用这一份，
/// 免得两条路以后各自漂。
fn course_happens_today(
    course: &CourseTime,
    kind: DayKind,
    week_number: i32,
    day_number: &Weekday,
) -> bool {
    match kind {
        // 法定假期全校停课，一节都不盯。
        DayKind::Holiday => false,
        // 调休日：`school.js` 只说了这一天要上课，**没说上的是哪一天的课**
        // （2026-09-20 上的其实是 10-06 周二第 5 周的课，跨周跨星期）。
        // 既然推不出来，就把这个人整张课表的蹲守窗口全并起来——
        // 这是「保证不漏」前提下最小的一组时段，多出来的窗口顶多是空跑几次查询。
        DayKind::Makeup => true,
        // 普通日子：原样按周次 + 星期查。
        DayKind::Normal => {
            course.day == *day_number
                && week_number > 0
                && course.week_mask.get_bit((week_number - 1) as u8)
        }
    }
}

pub fn get_today_courses(qq: i64) -> Option<TimeBitMap> {
    let (week_number, day_number) = get_today();
    let kind = calendar::day_kind(today_date());
    let timetable_data = query_sign_time(qq)?;
    let mut bitmap = TimeBitMap::new();
    for course in &timetable_data.times {
        if course_happens_today(course, kind, week_number, &day_number) {
            bitmap.merge(&course.watch_window());
        }
    }

    // 放假日会走到这里返回一张空表：空表的 `is_active` 恒为 false，蹲守自然不启动，
    // 而返回 `None` 会被调用方理解成「这个人没有课表」，两回事。
    Some(bitmap)
}

/// 此刻正处在自己蹲守窗口内的课，返回它们的班级代码（`BJDM` ＝ lnt 的 `course_code`）。
///
/// 蹲守窗口是**每门课各自**的「上课时间前后十分钟」（14:30-16:10 的课 => 14:20-16:20），
/// 而不是把今天所有课并成一张表——定时签到要按课分组，就得知道此刻在上的是哪几门。
///
/// 还没重新跑过 `/signtime` 的用户（v3 存量数据）没有班级代码，这里会返回空，
/// 调用方据此让他自己蹲守自己的课，不会漏签，只是省不掉那条请求。
///
/// 判「这节课今天算不算」的规矩和 [`get_today_courses`] 是同一份
/// （[`course_happens_today`]）：两边要是漂了，会出现「bitmap 说这人活跃、这里却给不出
/// 班级代码」的错位，结果全组人各查各的，白白丢掉哨兵去重。
pub fn class_codes_in_session_now(qq: i64) -> Vec<String> {
    let (week_number, day_number) = get_today();
    let kind = calendar::day_kind(today_date());
    let Some(timetable_data) = query_sign_time(qq) else {
        return Vec::new();
    };
    let now = ClockTime::now();
    let mut codes: Vec<String> = timetable_data
        .times
        .iter()
        .filter(|course| course_happens_today(course, kind, week_number, &day_number))
        .filter(|course| course.watch_window().is_active(now))
        .filter(|course| !course.class_code.is_empty())
        .map(|course| course.class_code.clone())
        .collect();
    codes.sort_unstable();
    codes.dedup();
    codes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::xmu_service::jw::{BitField32, LocationStore};

    /// 一节周二、只在第 5 周上的课。2026-09-20（周日、第 2 周）按星期查是查不到它的，
    /// 但那天上的恰恰就是它——教务部通知里的「9月20日与10月6日（星期二）课程对调」。
    fn 周二第五周的课() -> CourseTime {
        let start = ClockTime::new(14, 30);
        let end = ClockTime::new(16, 10);
        CourseTime {
            name: "离散数学".into(),
            class_code: "202620271U1030480002202".into(),
            location: Arc::new(LocationStore::from(None)),
            start,
            end,
            time_bitmap: TimeBitMap::from_range(start, end),
            week_mask: BitField32::new(0b1_0000), // 只有第 5 周
            day: Weekday::Tuesday,
        }
    }

    /// 普通日子的判定必须和改动前一模一样：星期对上、周次位对上才算。
    #[test]
    fn 普通日子仍然按周次和星期查() {
        let c = 周二第五周的课();
        assert!(
            course_happens_today(&c, DayKind::Normal, 5, &Weekday::Tuesday),
            "第 5 周周二就是这节课自己的时间"
        );
        assert!(
            !course_happens_today(&c, DayKind::Normal, 2, &Weekday::Tuesday),
            "第 2 周这节课不上"
        );
        assert!(
            !course_happens_today(&c, DayKind::Normal, 5, &Weekday::Sunday),
            "周日不是这节课的日子"
        );
        assert!(
            !course_happens_today(&c, DayKind::Normal, 2, &Weekday::Sunday),
            "2026-09-20 按普通日子查就是这个结果——什么都查不到，这正是那天漏签的原因"
        );
    }

    /// 调休日：跨周跨星期的课也要盯。这是本次改动的全部意义。
    #[test]
    fn 调休日把整张课表都当成今天可能有() {
        let c = 周二第五周的课();
        assert!(
            course_happens_today(&c, DayKind::Makeup, 2, &Weekday::Sunday),
            "调休日不知道顶替的是哪天，只能全盯上"
        );
    }

    /// 放假日一节都不盯，哪怕课表上这一天本来有课。
    #[test]
    fn 放假日一节都不盯() {
        let c = 周二第五周的课();
        assert!(
            !course_happens_today(&c, DayKind::Holiday, 5, &Weekday::Tuesday),
            "第 5 周周二正常有课，但 10-06 在国庆假里，全校停课"
        );
    }

    /// 2026-09-20 当天走完整条链路：校历说是调休，判定就必须是「盯」。
    #[test]
    fn 九月二十号是调休日() {
        let 那天 = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
        let kind = calendar::day_kind(那天);
        assert_eq!(kind, DayKind::Makeup);
        assert_eq!(calendar::makeup_on(那天).unwrap().name, "中秋节");
        assert!(course_happens_today(
            &周二第五周的课(),
            kind,
            2,
            &Weekday::Sunday
        ));
    }

    /// 覆盖范围外只报警一次，不是每轮都报。
    #[test]
    fn 校历过期只报警一次() {
        let 范围外 = NaiveDate::from_ymd_opt(2027, 1, 4).unwrap();
        assert!(!calendar::is_covered(范围外));
        // 不去重置这个全局原子：测试是并行跑的，重置会和别的用例抢。
        // 要验的本来也只是「同一天第二次调用不再改动它」。
        warn_if_calendar_stale(范围外);
        let 记下的 = COVERAGE_WARNED_ON.load(Ordering::Relaxed);
        assert_eq!(记下的, 范围外.num_days_from_ce());
        warn_if_calendar_stale(范围外);
        assert_eq!(COVERAGE_WARNED_ON.load(Ordering::Relaxed), 记下的);
    }
}
