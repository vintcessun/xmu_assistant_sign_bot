use super::data::TIMETABLE_DATA;
use crate::api::scheduler::{TaskRunner, TimeTask};
use crate::api::xmu_service::jw::TimeBitMap;
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
        "time_sign_update"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(240)
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

pub fn get_today_courses(qq: i64) -> Option<TimeBitMap> {
    let (week_number, day_number) = get_today();
    let timetable_data = TIMETABLE_DATA.get(&qq)?;
    let mut bitmap = TimeBitMap::new();
    for course in &timetable_data.times {
        if course.day == day_number
            && week_number > 0
            && course.week_mask.get_bit((week_number - 1) as u8)
        {
            let mut course_bitmap = course.time_bitmap;
            course_bitmap.extend(-10);
            course_bitmap.extend(10);
            bitmap.merge(&course_bitmap);
        }
    }

    Some(bitmap)
}
