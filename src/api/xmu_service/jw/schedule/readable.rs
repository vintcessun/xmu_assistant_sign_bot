use crate::api::xmu_service::{
    jw::{Schedule, ScheduleResponse, ScheduleTime, ScheduleTimeResponse},
    location::{LOCATIONS, Location},
};
use anyhow::{Result, anyhow};
use chrono::Timelike;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use std::{ops::Index, sync::Arc};

#[cfg(test)]
#[derive(Debug, Deserialize, Serialize)]
pub struct Partial<T> {
    pub value: T, // 已经解析成功的部分（或默认值）
    #[serde(skip)]
    pub errors: Vec<anyhow::Error>, // 收集到的所有错误
}

#[cfg(test)]
impl Clone for Partial<ScheduleTable> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            errors: Vec::new(), // 克隆时不复制错误
        }
    }
}

#[cfg(test)]
pub trait CollectResults<T> {
    fn collect_partial(self) -> Partial<Vec<T>>;
}

#[cfg(test)]
impl<T, I> CollectResults<T> for I
where
    I: Iterator<Item = anyhow::Result<T>>,
{
    fn collect_partial(self) -> Partial<Vec<T>> {
        let mut value = Vec::new();
        let mut errors = Vec::new();

        for res in self {
            match res {
                Ok(v) => value.push(v),
                Err(e) => errors.push(e),
            }
        }

        Partial { value, errors }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TimeBitMap {
    // 23 * 64 = 1472 bits, 足够 1440 分钟
    bits: [u64; 23],
}

impl TimeBitMap {
    pub fn new() -> Self {
        Self { bits: [0u64; 23] }
    }

    pub fn from_range(start: ClockTime, end: ClockTime) -> Self {
        let mut bitmap = Self::new();
        bitmap.add_range(start, end);
        bitmap
    }

    /// 核心：添加由 ClockTime 定义的区间 [start, end)
    pub fn add_range(&mut self, start: ClockTime, end: ClockTime) {
        // 由于你确定没有跨天，直接迭代
        // 如果 start < end，正常填充；如果用户误填，这里也不会崩溃
        for m in start.0..end.0 {
            let idx = (m / 64) as usize;
            let bit = (m % 64) as u64;
            self.bits[idx] |= 1 << bit;
        }
    }

    /// 极致查询：传入当前的 ClockTime
    #[inline(always)]
    pub fn is_active(&self, time: ClockTime) -> bool {
        let m = time.0 as usize;
        let idx = m / 64;
        let bit = m % 64;
        (self.bits[idx] & (1 << bit)) != 0
    }

    /// 批量合并：将另一个计划合并进来
    pub fn merge(&mut self, other: &Self) {
        for i in 0..23 {
            self.bits[i] |= other.bits[i];
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClockTime(u16); // 内部存储从 00:00 开始的分钟数

impl ClockTime {
    /// 从时分创建
    pub const fn new(hour: u8, min: u8) -> Self {
        Self((hour as u16) * 60 + (min as u16))
    }

    /// 从军事时间整数创建（如 1345 表示 13:45）
    pub const fn from_military(s: u16) -> Option<Self> {
        let hour = (s / 100) as u8;
        let min = (s % 100) as u8;
        if hour >= 24 || min >= 60 {
            return None;
        }
        Some(Self::new(hour, min))
    }

    /// 核心操作：加减分钟（处理跨天循环）
    pub const fn add_mins(&self, mins: i32) -> Self {
        // 使用 1440 取模确保时间在 00:00-23:59 之间
        let new_val = (self.0 as i32 + mins).rem_euclid(1440);
        Self(new_val as u16)
    }

    /// 转换为人类可读
    pub const fn to_hm(&self) -> (u8, u8) {
        ((self.0 / 60) as u8, (self.0 % 60) as u8)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeShape {
    pub name: String,
    pub id: i64,
    pub start: ClockTime,
    pub end: ClockTime,
    pub time_bitmap: TimeBitMap,
}

impl TimeShape {
    pub fn new(data: ScheduleTimeResponse) -> Result<Self> {
        let start = ClockTime::from_military(data.kssj)
            .ok_or(anyhow!("开始时间(kssj)解析错误; 原始结构体: {:?}", data))?;
        let end = ClockTime::from_military(data.jssj)
            .ok_or(anyhow!("结束时间(jssj)解析错误; 原始结构体: {:?}", data))?;
        let time_bitmap = TimeBitMap::from_range(start, end);
        Ok(TimeShape {
            name: data.mc,
            id: data.px,
            start,
            end,
            time_bitmap,
        })
    }
}

/// 索引从1开始的课表类
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScheduleTimeShape {
    pub times: Vec<TimeShape>,
}

impl ScheduleTimeShape {
    pub fn new(data: ScheduleTime) -> Result<Self> {
        let mut times = Vec::with_capacity(data.data.len());
        for item in data.data {
            times.push(TimeShape::new(item)?);
        }
        times.sort_by_key(|t| t.id);
        Ok(Self { times })
    }

    #[cfg(test)]
    pub fn new_partial(data: ScheduleTime) -> Partial<Self> {
        let mut errors = Vec::with_capacity(data.data.len());
        let mut times = Vec::with_capacity(data.data.len());
        for item in data.data {
            match TimeShape::new(item) {
                Ok(shape) => times.push(shape),
                Err(e) => errors.push(e),
            }
        }
        times.sort_by_key(|t| t.id);

        Partial {
            value: Self { times },
            errors,
        }
    }

    pub fn get(&self, id: i64) -> Option<&TimeShape> {
        if id <= 0 {
            return None;
        }
        self.times.get((id - 1) as usize)
    }
}

impl Index<i64> for ScheduleTimeShape {
    type Output = TimeShape;

    #[inline]
    fn index(&self, id: i64) -> &Self::Output {
        &self.times[(id - 1) as usize]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "Option<String>")]
pub struct LocationStore {
    #[serde(skip_deserializing)]
    pub pos: Option<Location>,
    pub location_str: Option<String>,
}

impl From<Option<String>> for LocationStore {
    fn from(s: Option<String>) -> Self {
        let pos = s.as_ref().and_then(|name| LOCATIONS.query(name));
        Self {
            pos,
            location_str: s,
        }
    }
}

impl From<&Location> for LocationStore {
    fn from(loc: &Location) -> Self {
        Self {
            location_str: Some(loc.name.to_string()),
            pos: Some(loc.clone()),
        }
    }
}

impl From<Location> for LocationStore {
    fn from(loc: Location) -> Self {
        Self {
            location_str: Some(loc.name.to_string()),
            pos: Some(loc),
        }
    }
}

impl From<Arc<Location>> for LocationStore {
    fn from(loc: Arc<Location>) -> Self {
        Self {
            location_str: Some(loc.name.to_string()),
            pos: Some((*loc).clone()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, FromPrimitive, PartialEq, Eq, Hash)]
pub enum Weekday {
    Monday = 1,
    Tuesday = 2,
    Wednesday = 3,
    Thursday = 4,
    Friday = 5,
    Saturday = 6,
    Sunday = 7,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CourseTime {
    pub name: String,
    pub location: Arc<LocationStore>,
    pub start: ClockTime,
    pub end: ClockTime,
    pub time_bitmap: TimeBitMap,
    pub week_mask: BitField32,
    pub day: Weekday,
}

fn parse_weeks(src: &str) -> BitField32 {
    let mut mask = 0u32;
    for (i, byte) in src.bytes().enumerate() {
        if byte == b'1' && i < 32 {
            mask |= 1 << i;
        }
    }
    BitField32::new(mask)
}

impl CourseTime {
    pub fn new(data: ScheduleResponse) -> Result<Self> {
        let start = ClockTime::from_military(data.kssj)
            .ok_or(anyhow!("开始时间(kssj)解析错误; 原始结构体: {:?}", data))?;
        let end = ClockTime::from_military(data.jssj)
            .ok_or(anyhow!("结束时间(jssj)解析错误; 原始结构体: {:?}", data))?;
        let location_str = data.jasmc.as_ref();
        if let Some(location_str) = location_str {
            LOCATIONS
                .query(location_str)
                .ok_or(anyhow!("地点(jasmc)解析错误; 原始结构体: {:?}", data))?;
        };
        let day = Weekday::from_i64(data.xq)
            .ok_or(anyhow!("星期(xq)解析错误; 原始结构体: {:?}", data))?;
        let time_bitmap = TimeBitMap::from_range(start, end);
        Ok(Self {
            name: data.kcmc,
            location: Arc::new(LocationStore::from(location_str.cloned())),
            start,
            end,
            time_bitmap,
            week_mask: parse_weeks(&data.zcbh),
            day,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScheduleCourseTime {
    pub times: Vec<CourseTime>,
}

impl ScheduleCourseTime {
    pub fn new(data: Schedule) -> Result<Self> {
        let mut times = Vec::with_capacity(data.pkjgList.len());
        for item in data.pkjgList {
            times.push(CourseTime::new(item)?);
        }
        Ok(Self { times })
    }

    #[cfg(test)]
    pub fn new_partial(data: Schedule) -> Partial<Self> {
        let mut errors = Vec::with_capacity(data.pkjgList.len());
        let mut times = Vec::with_capacity(data.pkjgList.len());
        for item in data.pkjgList {
            match CourseTime::new(item) {
                Ok(course) => times.push(course),
                Err(e) => errors.push(e),
            }
        }
        Partial {
            value: Self { times },
            errors,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScheduleTable {
    pub shape: ScheduleTimeShape,
    pub course: ScheduleCourseTime,
}

impl ScheduleTable {
    pub fn new(time_data: ScheduleTime, course_data: Schedule) -> Result<Self> {
        Ok(Self {
            shape: ScheduleTimeShape::new(time_data)?,
            course: ScheduleCourseTime::new(course_data)?,
        })
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct BitField32(pub u32);

impl BitField32 {
    /// 获取第 i 位是否为真 (i 从 0 开始)
    ///
    /// # 安全性
    /// 如果 i >= 32，在 Debug 模式下会 panic，在 Release 模式下会触发位移溢出（通常结果为 false）
    #[inline(always)]
    pub fn get_bit(&self, i: u8) -> bool {
        // 将 1 左移 i 位，然后与原值进行“位与”运算
        // 如果结果不为 0，说明该位为 1
        (self.0 & (1 << i)) != 0
    }

    #[inline(always)]
    pub fn new(val: u32) -> Self {
        Self(val)
    }

    /// 设置第 i 位为真
    #[inline(always)]
    pub fn set_bit(&mut self, i: u8) {
        self.0 |= 1 << i;
    }

    /// 清除第 i 位
    #[inline(always)]
    pub fn clear_bit(&mut self, i: u8) {
        self.0 &= !(1 << i);
    }
}

static TIME_ZONE: chrono::FixedOffset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();

impl ClockTime {
    pub fn now() -> Self {
        let now = chrono::Utc::now().with_timezone(&TIME_ZONE);
        Self::new(now.hour() as u8, now.minute() as u8)
    }
}
