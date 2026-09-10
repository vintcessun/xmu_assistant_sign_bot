use super::timetable::{all_timetable_users, query_sign_time};
use crate::api::scheduler::{TaskRunner, TimeTask};
use crate::api::xmu_service::jw::{ClockTime, TimeBitMap};
use crate::api::xmu_service::time::get_today;
use ahash::RandomState;
use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::task::block_in_place;
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

pub fn get_today_courses(qq: i64) -> Option<TimeBitMap> {
    let (week_number, day_number) = get_today();
    let timetable_data = query_sign_time(qq)?;
    let mut bitmap = TimeBitMap::new();
    for course in &timetable_data.times {
        if course.day == day_number
            && week_number > 0
            && course.week_mask.get_bit((week_number - 1) as u8)
        {
            bitmap.merge(&course.watch_window());
        }
    }

    Some(bitmap)
}

/// 此刻正处在自己蹲守窗口内的课，返回它们的班级代码（`BJDM` ＝ lnt 的 `course_code`）。
///
/// 蹲守窗口是**每门课各自**的「上课时间前后十分钟」（14:30-16:10 的课 => 14:20-16:20），
/// 而不是把今天所有课并成一张表——定时签到要按课分组，就得知道此刻在上的是哪几门。
///
/// 还没重新跑过 `/signtime` 的用户（v3 存量数据）没有班级代码，这里会返回空，
/// 调用方据此让他自己蹲守自己的课，不会漏签，只是省不掉那条请求。
pub fn class_codes_in_session_now(qq: i64) -> Vec<String> {
    let (week_number, day_number) = get_today();
    let Some(timetable_data) = query_sign_time(qq) else {
        return Vec::new();
    };
    let now = ClockTime::now();
    let mut codes: Vec<String> = timetable_data
        .times
        .iter()
        .filter(|course| course.is_watching(week_number, &day_number, now))
        .filter(|course| !course.class_code.is_empty())
        .map(|course| course.class_code.clone())
        .collect();
    codes.sort_unstable();
    codes.dedup();
    codes
}
