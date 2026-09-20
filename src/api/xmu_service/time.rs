use crate::api::xmu_service::jw::Weekday;
use arc_swap::ArcSwapOption;
use chrono::{Datelike, NaiveDate};
use num_traits::FromPrimitive;
use std::sync::{Arc, LazyLock};
use tracing::{info, warn};

//NOTICE: 学期第一天。它决定"今天是第几周"，错了整天的课都会对不上，是签到功能的命脉。
//
//这个常量现在只是**兜底**：正常情况下由 [`set_semester_start`] 在启动后用
//lnt `my-courses` 里的 `start_date` 自动推算出来覆盖掉（见 course_index），
//所以换学期不再需要手改这里。推算不出来（全员会话过期 / 字段没了）才回落到它。
static FALLBACK_START_DATE: NaiveDate = NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();

/// 运行时推算出来的学期第一天；未推算出来时为空，取值回落到 [`FALLBACK_START_DATE`]。
static SEMESTER_START: LazyLock<ArcSwapOption<NaiveDate>> = LazyLock::new(ArcSwapOption::empty);

/// 当前生效的学期第一天。
pub fn semester_start() -> NaiveDate {
    SEMESTER_START
        .load()
        .as_ref()
        .map(|d| **d)
        .unwrap_or(FALLBACK_START_DATE)
}

/// 用推算出来的学期第一天覆盖兜底值。
///
/// 传进来的日期会被**对齐到所在周的周一**：`get_week_number` 是按"距起点的天数 % 7"
/// 推星期几的，起点不是周一的话整张课表的星期都会错位。
/// 顺带做一次合理性检查，明显离谱的值宁可不用，继续走兜底。
pub fn set_semester_start(date: NaiveDate) -> Option<NaiveDate> {
    let today = chrono::Utc::now().with_timezone(&TIME_ZONE).date_naive();
    let ahead = date.signed_duration_since(today).num_days();
    // 学期第一天不该在未来太远，也不该是半年前的陈年数据。
    if !(-220..=30).contains(&ahead) {
        warn!(
            candidate = %date,
            today = %today,
            "推算出的学期第一天不合理，继续使用兜底值"
        );
        return None;
    }
    let monday = date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64);
    let previous = semester_start();
    SEMESTER_START.store(Some(Arc::new(monday)));
    if previous != monday {
        info!(
            semester_start = %monday,
            previous = %previous,
            "学期第一天已更新（由 lnt 开课日期自动推算）"
        );
    }
    Some(monday)
}

//NOTICE: 使用北京时间对准
pub static TIME_ZONE: chrono::FixedOffset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();

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

/// 今天是哪一天（北京时间）。
///
/// 周次/星期由 [`get_today`] 给，但校历判定（放假、调休）要的是**日期**本身，
/// 见 [`crate::api::xmu_service::calendar`]。
pub fn today_date() -> NaiveDate {
    chrono::Utc::now().with_timezone(&TIME_ZONE).date_naive()
}

pub fn get_today() -> (i32, Weekday) {
    get_week_number(semester_start(), today_date())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_week_number() {
        let start_date = NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let target_date = NaiveDate::from_ymd_opt(2026, 9, 14).unwrap(); // 第一周的下一周
        assert_eq!(
            get_week_number(start_date, target_date),
            (2, Weekday::Monday)
        );
    }

    #[test]
    fn test_get_today() {
        let today = chrono::Utc::now().with_timezone(&TIME_ZONE).date_naive();
        let duration = today.signed_duration_since(semester_start());
        let days = duration.num_days();
        println!("今天是 {} 天后", days);

        let (week_number, day_number) = get_today();
        println!("今天是第 {} 周，{}", week_number, day_number);
    }
}
