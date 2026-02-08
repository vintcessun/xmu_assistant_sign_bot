use crate::api::xmu_service::{
    jw::{Schedule, ScheduleResponse, ScheduleTime, ScheduleTimeResponse},
    location::{LOCATIONS, Location},
};
use anyhow::{Result, anyhow};
use std::ops::Index;

#[cfg(test)]
#[derive(Debug)]
pub struct Partial<T> {
    pub value: T,                   // 已经解析成功的部分（或默认值）
    pub errors: Vec<anyhow::Error>, // 收集到的所有错误
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Debug)]
pub struct TimeShape {
    pub name: String,
    pub id: i64,
    pub start: ClockTime,
    pub end: ClockTime,
}

impl TimeShape {
    pub fn new(data: ScheduleTimeResponse) -> Result<Self> {
        let start = ClockTime::from_military(data.kssj)
            .ok_or(anyhow!("开始时间(kssj)解析错误; 原始结构体: {:?}", data))?;
        let end = ClockTime::from_military(data.jssj)
            .ok_or(anyhow!("结束时间(jssj)解析错误; 原始结构体: {:?}", data))?;
        Ok(TimeShape {
            name: data.mc,
            id: data.px,
            start,
            end,
        })
    }
}

/// 索引从1开始的课表类
#[derive(Debug)]
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

#[derive(Debug)]
pub struct CourseTime {
    pub name: String,
    pub location: Option<Location>,
    pub start: ClockTime,
    pub end: ClockTime,
    pub week_mask: u32,
}

fn parse_weeks(src: &str) -> u32 {
    let mut mask = 0u32;
    for (i, byte) in src.bytes().enumerate() {
        if byte == b'1' && i < 32 {
            mask |= 1 << i;
        }
    }
    mask
}

impl CourseTime {
    pub fn new(data: ScheduleResponse) -> Result<Self> {
        let start = ClockTime::from_military(data.kssj)
            .ok_or(anyhow!("开始时间(kssj)解析错误; 原始结构体: {:?}", data))?;
        let end = ClockTime::from_military(data.jssj)
            .ok_or(anyhow!("结束时间(jssj)解析错误; 原始结构体: {:?}", data))?;
        let location_str = data.jasmc.as_ref();
        let mut location = None;
        if let Some(location_str) = location_str {
            let location_get = LOCATIONS
                .query(location_str)
                .ok_or(anyhow!("地点(jasmc)解析错误; 原始结构体: {:?}", data))?;
            location = Some(location_get);
        }
        Ok(Self {
            name: data.kcmc,
            location,
            start,
            end,
            week_mask: parse_weeks(&data.zcbh),
        })
    }
}

#[derive(Debug)]
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

#[derive(Debug)]
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
