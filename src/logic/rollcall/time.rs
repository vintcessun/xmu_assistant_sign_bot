use super::data::TIMETABLE_DATA;
use crate::api::scheduler::{TaskRunner, TimeTask};
use crate::api::xmu_service::jw::{TimeBitMap, Weekday};
use ahash::RandomState;
use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDate;
use dashmap::DashMap;
use num_traits::FromPrimitive;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::task::block_in_place;

//NOTICE: 2026年3月2日是新学期的第一天，之后的签到时间表都以此为基准进行计算
//因此，START_DATE 是一个非常重要的常量，确保它的正确性对于签到功能的正常运行至关重要。
static START_DATE: NaiveDate = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();

//NOTICE: 使用北京时间对准
static TIME_ZONE: chrono::FixedOffset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();

pub struct TimeSignUpdateTask;

#[async_trait]
impl TimeTask for TimeSignUpdateTask {
    type Output = DashMap<i64, TimeBitMap, RandomState>;

    fn name(&self) -> &'static str {
        "time_sign_update"
    }

    fn interval(&self) -> Duration {
        Duration::from_hours(24)
    }

    async fn run(&self) -> Result<Self::Output> {
        block_in_place(|| {
            let data = &*TIMETABLE_DATA;
            let map = DashMap::with_hasher(RandomState::default());
            for entry in data {
                let qq = *entry.key();
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

fn get_week_number(start_date: NaiveDate, target_date: NaiveDate) -> (i32, Weekday) {
    let duration = target_date.signed_duration_since(start_date);
    let days = duration.num_days();
    let ret_week = if days >= 0 {
        (days / 7) as i32 + 1
    } else {
        (days / 7) as i32
    };

    (
        ret_week,
        Weekday::from_i32(((days % 7) as i32 + 7) % 7 + 1).unwrap(),
    )
}

pub fn get_today_courses(qq: i64) -> Option<TimeBitMap> {
    let today = chrono::Utc::now().with_timezone(&TIME_ZONE).date_naive();
    let (week_number, day_number) = get_week_number(START_DATE, today);
    let timetable_data = TIMETABLE_DATA.get(&qq)?;
    let mut bitmap = TimeBitMap::new();
    for course in &timetable_data.times {
        if course.day == day_number
            && week_number > 0
            && course.week_mask.get_bit((week_number - 1) as u8)
        {
            bitmap.merge(&course.time_bitmap);
        }
    }

    Some(bitmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_week_number() {
        let start_date = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        let target_date = NaiveDate::from_ymd_opt(2026, 3, 9).unwrap(); // 第一周的下一周
        assert_eq!(
            get_week_number(start_date, target_date),
            (2, Weekday::Monday)
        );
    }

    #[test]
    fn test_get_today() {
        let today = chrono::Utc::now().with_timezone(&TIME_ZONE).date_naive();
        let duration = today.signed_duration_since(START_DATE);
        let days = duration.num_days();
        println!("今天是 {} 天后", days);

        let (week_number, day_number) = get_week_number(START_DATE, today);
        println!("今天是第 {} 周，{}", week_number, day_number);
    }
}
