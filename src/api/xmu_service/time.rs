use crate::api::xmu_service::jw::Weekday;
use chrono::NaiveDate;
use num_traits::FromPrimitive;

//NOTICE: 2026年3月2日是新学期的第一天，之后的签到时间表都以此为基准进行计算
//因此，START_DATE 是一个非常重要的常量，确保它的正确性对于签到功能的正常运行至关重要。
static START_DATE: NaiveDate = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();

//NOTICE: 使用北京时间对准
static TIME_ZONE: chrono::FixedOffset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();

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

pub fn get_today() -> (i32, Weekday) {
    let today = chrono::Utc::now().with_timezone(&TIME_ZONE).date_naive();
    get_week_number(START_DATE, today)
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

        let (week_number, day_number) = get_today();
        println!("今天是第 {} 周，{}", week_number, day_number);
    }
}
