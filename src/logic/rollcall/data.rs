use crate::api::{
    storage::HotTable,
    xmu_service::jw::{LocationStore, ScheduleCourseTime},
};
use smol_str::SmolStr;
use std::sync::LazyLock;

pub static TIMETABLE_DATA: LazyLock<HotTable<i64, ScheduleCourseTime>> =
    LazyLock::new(|| HotTable::new("logic_command_sign_time"));

pub static TIMETABLE_GROUP: LazyLock<HotTable<i64, i64>> =
    LazyLock::new(|| HotTable::new("logic_command_sign_time_group"));

pub use super::super::login::DATA as LOGIN_DATA;

pub static SIGN_NUMBER_DATA: LazyLock<HotTable<i64, SmolStr>> =
    LazyLock::new(|| HotTable::new("logic_command_sign_number"));

pub static SIGN_LOCATION_DATA: LazyLock<HotTable<i64, LocationStore>> =
    LazyLock::new(|| HotTable::new("logic_command_sign_location"));
