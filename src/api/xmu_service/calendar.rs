//! 教务处校历 <https://jwc.xmu.edu.cn/js/school.js> 的规范化副本。
//!
//! 进程启动时抓一次、之后每天刷一次（[`spawn_refresh_task`]），抓不到或解析失败就继续用
//! 手抄的兜底表 [`fallback_calendar`]。
//!
//! # 源文件长什么样
//!
//! 手写的 JS 对象字面量，`window["school-calendars"].push({...})` 一年一块，键名全是中文
//! （`名称` / `第一学期` / `节假日` / `调休`），带 `//` 注释、尾逗号、单双引号混用。
//! 首页用 `js/calendar.min.js` 把它渲染成校历挂件，而渲染器对 `调休` 的处理只有一句
//! `M[i].holiday = { class: "work", sign: "班" }`：
//! **只标记「这一天要上班」，不记录它顶替的是哪一天**。这一条决定了本模块能回答什么，
//! 见下面「调休」一节。
//!
//! # 规范掉的三处不一致
//!
//! 1. **块级 `开始于` / `结束于` 语义漂移**：2021/2022/2023 块里它等于第一学期开学日，
//!    2025/2026 块里注释写的是「今年暑假的第一天」，2025 块的值（2025-7-19）两头都对不上。
//!    解析时**直接丢掉**，需要时用 [`AcademicYear::span`] 从第一学期开学到暑假结束现算。
//! 2. **`节假日` 的归属年不固定**：2021/2022/2023 块按**学年**挂（2022 块里放着 2023 年的
//!    春节、清明、劳动、端午），2024 之后改成按**自然年**挂（2024 块里的元旦是 2024-1-1，
//!    而那天属于 2023 学年）。所以这里把所有年份块的节假日**摊平成一张按日期排序的表**，
//!    查询一律按日期查，不按学年查。
//! 3. **年份块可以是半成品**：jwc 的 2027 块只有 `名称`、块级起止和一个空的 `节假日: {}`，
//!    一个学期都没有。所以 [`AcademicYear`] 的各段全是 `Option`。
//!
//! jwc 自己还有两处笔误，照抄不改，只在测试里点名：2023-4-3 标着「必须是周日」其实是周一；
//! 2021 学年第一学期期末考写成 2022-1-2 ..= 2022-2-15，结束日跑到寒假里去了。
//!
//! # 调休
//!
//! [`Holiday::makeups`] 只有「这一天要上课」的日期，**没有它对应哪一天的课**——源文件里
//! 就没有。真正的对调关系写在教务部/研究生院的放假通知正文里（2026 年中秋国庆是
//! 「9月20日与10月6日（星期二）课程对调，10月7日（星期三）与10月10日课程对调」），
//! 那是散文，不是接口。所以本模块能支撑的是：
//!
//! * **放假日掩码**：[`DayKind::Holiday`] 的日子全校停课，不用蹲守；
//! * **调休日兜底**：[`DayKind::Makeup`] 的日子必须当成上课日，但**不能**只把星期几一换
//!   就去查课表——对调的两天往往不在同一教学周（9-20 在第 2 周，10-6 在第 5 周），
//!   只换星期几会让 `week_mask` 取错位。调用方的做法见
//!   [`crate::logic::rollcall::time`]：把整张课表的蹲守窗口并起来。

use crate::api::network::SessionClient;
use crate::api::scheduler::{TaskRunner, TimeTask};
use anyhow::{Result, anyhow, bail};
use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use chrono::NaiveDate;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tracing::{info, warn};

/// 校历源文件。公开、免登录，但仍然走 [`SessionClient`]——`*.xmu.edu.cn` 在服务器上
/// 是要经 SOCKS5 出去的。
pub const SCHOOL_JS_URL: &str = "https://jwc.xmu.edu.cn/js/school.js";

/// 闭区间的一段日子，`start` 和 `end` 当天都算在内。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl Span {
    pub fn contains(&self, date: NaiveDate) -> bool {
        self.start <= date && date <= self.end
    }

    /// 这段日子一共几天（含头含尾）。
    pub fn days(&self) -> i64 {
        (self.end - self.start).num_days() + 1
    }
}

/// 一个学期，连同它的期中考、期末考周。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Term {
    pub span: Span,
    /// 只有 2021、2022 两个学年的第二学期填了期中考。
    pub midterm: Option<Span>,
    pub finals: Option<Span>,
}

/// 一个学年：三个学期加寒暑假。各段都可能缺——jwc 会先放一个只有 `名称` 的占位块。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcademicYear {
    /// jwc 的 `名称`，即学年起始的自然年（2026 表示 2026-2027 学年）。
    pub name: i32,
    pub first: Option<Term>,
    pub winter: Option<Span>,
    pub second: Option<Term>,
    /// 第三学期（小学期）不排考试，所以是裸的 [`Span`]。
    pub third: Option<Span>,
    pub summer: Option<Span>,
}

impl AcademicYear {
    /// 只有 `名称`、没有任何学期的占位块。
    pub fn placeholder(name: i32) -> Self {
        Self {
            name,
            first: None,
            winter: None,
            second: None,
            third: None,
            summer: None,
        }
    }

    /// 学年的整体跨度：第一学期开学到暑假结束。缺任何一头就是 `None`。
    ///
    /// jwc 块级的 `开始于` / `结束于` 语义不一致，一律用这个现算的值代替。
    pub fn span(&self) -> Option<Span> {
        Some(Span {
            start: self.first.as_ref()?.span.start,
            end: self.summer.as_ref()?.end,
        })
    }

    /// 第一学期的**教学周一**。
    ///
    /// jwc 的一周从**周日**起算，[`crate::api::xmu_service::time`] 里的周次是从**周一**
    /// 起算的，两边差一天。直接把 `first.span.start` 喂给 `set_semester_start` 会被
    /// `num_days_from_monday()` 往前对齐整整一周（2026-9-6 会变成 2026-8-31），必须用这个。
    pub fn teaching_monday(&self) -> Option<NaiveDate> {
        Some(self.first.as_ref()?.span.start + chrono::Duration::days(1))
    }
}

/// 一个法定节假日：放假区间，外加需要补回来的调休上课日。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holiday {
    pub name: String,
    /// 放假的闭区间。
    pub span: Span,
    /// 调休上课日。**只有日期，没有它顶替的是哪一天**——源文件里就没这个信息。
    /// 这些日子可能排在假期之前（2026 中秋的 9-20），也可能在假期之后（国庆的 10-10）。
    pub makeups: Vec<NaiveDate>,
}

/// 某一天在校历里是什么性质。
///
/// 不带载荷：这是蹲守热路径上每轮都要判的东西，要名字的场合用 [`holiday_on`] 单独取。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayKind {
    /// 法定放假，全校停课。
    Holiday,
    /// 调休上课日（周六/周日照常上课）。
    Makeup,
    /// 普通日子，或者超出校历覆盖范围（见 [`Calendar::is_covered`]）。
    Normal,
}

/// 一整份校历。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Calendar {
    /// 按 `name` 降序（新学年在前），和 jwc 文件里的顺序一致。
    pub years: Vec<AcademicYear>,
    /// 所有年份块的节假日摊平后按 `span.start` 升序。
    pub holidays: Vec<Holiday>,
}

impl Calendar {
    pub fn holiday_on(&self, date: NaiveDate) -> Option<&Holiday> {
        self.holidays.iter().find(|h| h.span.contains(date))
    }

    pub fn makeup_on(&self, date: NaiveDate) -> Option<&Holiday> {
        self.holidays.iter().find(|h| h.makeups.contains(&date))
    }

    /// 放假优先于调休（两者不会重叠，这里只是定个次序）。
    pub fn day_kind(&self, date: NaiveDate) -> DayKind {
        if self.holiday_on(date).is_some() {
            DayKind::Holiday
        } else if self.makeup_on(date).is_some() {
            DayKind::Makeup
        } else {
            DayKind::Normal
        }
    }

    pub fn academic_year_of(&self, date: NaiveDate) -> Option<&AcademicYear> {
        self.years
            .iter()
            .find(|y| y.span().is_some_and(|s| s.contains(date)))
    }

    /// 节假日表说得上话的范围：最早的放假/调休日到最晚的那个。
    ///
    /// 调休日可能落在假期之外的两侧（2026 中秋的 9-20 在前，国庆的 10-10 在后），
    /// 所以两头都得把 `makeups` 一起算进来。
    pub fn coverage(&self) -> Option<Span> {
        let mut start: Option<NaiveDate> = None;
        let mut end: Option<NaiveDate> = None;
        for h in &self.holidays {
            let dates = [h.span.start, h.span.end]
                .into_iter()
                .chain(h.makeups.iter().copied());
            for d in dates {
                start = Some(start.map_or(d, |s: NaiveDate| s.min(d)));
                end = Some(end.map_or(d, |e: NaiveDate| e.max(d)));
            }
        }
        Some(Span {
            start: start?,
            end: end?,
        })
    }

    /// 这一天是否落在节假日表的覆盖范围里。
    ///
    /// 范围外 [`Calendar::day_kind`] 会返回 [`DayKind::Normal`]，也就是**调休日会被当成
    /// 普通周末**——那正是 2026-09-20 漏签的成因。调用方要据此报警，见
    /// [`crate::logic::rollcall::time`]。
    pub fn is_covered(&self, date: NaiveDate) -> bool {
        self.coverage().is_some_and(|s| s.contains(date))
    }
}

// ---------------------------------------------------------------------------
// 兜底表：手抄的一份，抓不到线上文件时顶上
// ---------------------------------------------------------------------------

macro_rules! 日期 {
    ($y:literal-$m:literal-$d:literal) => {
        match NaiveDate::from_ymd_opt($y, $m, $d) {
            Some(d) => d,
            None => panic!("兜底校历里写了不存在的日期"),
        }
    };
}

macro_rules! 区间 {
    ($y1:literal-$m1:literal-$d1:literal ..= $y2:literal-$m2:literal-$d2:literal) => {
        Span {
            start: 日期!($y1 - $m1 - $d1),
            end: 日期!($y2 - $m2 - $d2),
        }
    };
}

macro_rules! 日期表 {
    ($($y:literal-$m:literal-$d:literal),* $(,)?) => {
        vec![$(日期!($y-$m-$d)),*]
    };
}

macro_rules! 可选区间 {
    () => {
        None
    };
    ($e:expr) => {
        Some($e)
    };
}

macro_rules! 可选日期表 {
    () => {
        Vec::new()
    };
    ($e:expr) => {
        $e
    };
}

/// 括号里的日期原样照抄 jwc 的 `开始于` / `结束于`，写法和源文件一致，
/// 将来要手工补一年时可以近乎逐字粘过来。
macro_rules! 校历 {
    ($(
        学年 $year:literal {
            第一学期 $t1:tt $(期中考 $t1m:tt)? $(期末考 $t1f:tt)?
            寒假     $winter:tt
            第二学期 $t2:tt $(期中考 $t2m:tt)? $(期末考 $t2f:tt)?
            第三学期 $t3:tt
            暑假     $summer:tt
        }
    )*) => {
        vec![$(
            AcademicYear {
                name: $year,
                first: Some(Term {
                    span: 区间! $t1,
                    midterm: 可选区间!($(区间! $t1m)?),
                    finals: 可选区间!($(区间! $t1f)?),
                }),
                winter: Some(区间! $winter),
                second: Some(Term {
                    span: 区间! $t2,
                    midterm: 可选区间!($(区间! $t2m)?),
                    finals: 可选区间!($(区间! $t2f)?),
                }),
                third: Some(区间! $t3),
                summer: Some(区间! $summer),
            },
        )*]
    };
}

macro_rules! 节假日 {
    ($($name:literal $span:tt $(调休 $makeups:tt)?),* $(,)?) => {
        vec![$(
            Holiday {
                name: $name.to_owned(),
                span: 区间! $span,
                makeups: 可选日期表!($(日期表! $makeups)?),
            },
        )*]
    };
}

/// 手抄的兜底校历，内容对齐 2026-06-15 那一版 `school.js`。
///
/// 它有两个用处：线上抓不到时顶上；以及给解析器当靶子——
/// 测试会拿它和解析 `testdata/school.js` 的结果逐字段比对。
pub fn fallback_calendar() -> Calendar {
    // jwc 的 2027 块是占位符：只有 `名称`、块级起止和空的 `节假日: {}`，一个学期都没有。
    let mut years = vec![AcademicYear::placeholder(2027)];
    years.extend(校历! {
        学年 2026 {
            第一学期 (2026-9-6 ..= 2027-1-9)   期末考 (2026-12-27 ..= 2027-1-9)
            寒假     (2027-1-10 ..= 2027-2-13)
            第二学期 (2027-2-14 ..= 2027-6-12) 期末考 (2027-5-30 ..= 2027-6-12)
            第三学期 (2027-6-13 ..= 2027-7-3)
            暑假     (2027-7-4 ..= 2027-8-31)
        }
        学年 2025 {
            第一学期 (2025-8-31 ..= 2026-1-10) 期末考 (2025-12-28 ..= 2026-1-10)
            寒假     (2026-1-11 ..= 2026-2-28)
            第二学期 (2026-3-1 ..= 2026-6-27)  期末考 (2026-6-14 ..= 2026-6-27)
            第三学期 (2026-6-28 ..= 2026-7-18)
            暑假     (2026-7-19 ..= 2026-9-5)
        }
        学年 2024 {
            第一学期 (2024-9-1 ..= 2025-1-11)  期末考 (2024-12-29 ..= 2025-1-11)
            寒假     (2025-1-12 ..= 2025-2-15)
            第二学期 (2025-2-16 ..= 2025-6-21) 期末考 (2025-6-8 ..= 2025-6-21)
            第三学期 (2025-6-22 ..= 2025-7-12)
            暑假     (2025-7-13 ..= 2025-8-30)
        }
        学年 2023 {
            // 期末考结束日 2024-1-14 比学期结束日 2024-1-13 晚一天，且和寒假第一天重合，
            // jwc 原文如此。
            第一学期 (2023-9-10 ..= 2024-1-13) 期末考 (2023-12-31 ..= 2024-1-14)
            寒假     (2024-1-14 ..= 2024-2-24)
            第二学期 (2024-2-25 ..= 2024-6-22) 期末考 (2024-6-9 ..= 2024-6-22)
            第三学期 (2024-6-23 ..= 2024-7-20)
            暑假     (2024-7-21 ..= 2024-8-31)
        }
        学年 2022 {
            第一学期 (2022-9-11 ..= 2023-1-14) 期末考 (2023-1-1 ..= 2023-1-14)
            寒假     (2023-1-15 ..= 2023-2-11)
            // 期中考开始日 2023-4-3 在源文件里标着「必须是周日」，实际是周一。原样保留。
            第二学期 (2023-2-12 ..= 2023-7-1)  期中考 (2023-4-3 ..= 2023-4-16) 期末考 (2023-6-18 ..= 2023-7-1)
            第三学期 (2023-7-2 ..= 2023-7-29)
            暑假     (2023-7-30 ..= 2023-9-9)
        }
        学年 2021 {
            // 期末考结束日 2022-2-15 落在寒假里（学期 2022-1-15 就结束了），jwc 原文如此。
            第一学期 (2021-9-12 ..= 2022-1-15) 期末考 (2022-1-2 ..= 2022-2-15)
            寒假     (2022-1-16 ..= 2022-2-19)
            第二学期 (2022-2-20 ..= 2022-6-18) 期中考 (2022-4-3 ..= 2022-4-16) 期末考 (2022-6-5 ..= 2022-6-18)
            第三学期 (2022-6-19 ..= 2022-7-23)
            暑假     (2022-7-24 ..= 2022-9-10)
        }
    });

    let holidays = 节假日! {
        // 以下两条挂在 jwc 的 2021 学年块下（按学年归属）。
        "劳动节" (2022-4-30 ..= 2022-5-4)   调休 (2022-4-24, 2022-5-7),
        "端午节" (2022-6-3 ..= 2022-6-5),
        // 以下七条挂在 2022 学年块下（按学年归属，含 2023 年春天的假）。
        "中秋节" (2022-9-10 ..= 2022-9-12),
        "国庆节" (2022-10-1 ..= 2022-10-7)  调休 (2022-10-8, 2022-10-9),
        "元旦"   (2022-12-31 ..= 2023-1-2),
        "春节"   (2023-1-21 ..= 2023-1-27)  调休 (2023-1-28, 2023-1-29),
        "清明节" (2023-4-5 ..= 2023-4-5),
        "劳动节" (2023-4-29 ..= 2023-5-3)   调休 (2023-4-23, 2023-5-6),
        "端午节" (2023-6-22 ..= 2023-6-24)  调休 (2023-6-25),
        // 以下两条挂在 2023 学年块下。
        "中秋节" (2023-9-29 ..= 2023-9-29),
        "国庆节" (2023-9-30 ..= 2023-10-6)  调休 (2023-10-7, 2023-10-8),
        // 以下七条挂在 2024 学年块下（从这里开始改成按自然年归属）。
        "元旦"   (2024-1-1 ..= 2024-1-1),
        "春节"   (2024-2-10 ..= 2024-2-17)  调休 (2024-2-4, 2024-2-18),
        "清明节" (2024-4-4 ..= 2024-4-6)    调休 (2024-4-7),
        "劳动节" (2024-5-1 ..= 2024-5-5)    调休 (2024-4-28, 2024-5-11),
        "端午节" (2024-6-10 ..= 2024-6-10),
        "中秋节" (2024-9-15 ..= 2024-9-17)  调休 (2024-9-14),
        "国庆节" (2024-10-1 ..= 2024-10-7)  调休 (2024-9-29, 2024-10-12),
        // 以下六条挂在 2025 学年块下（2025 年中秋并进了国庆，所以只有六条）。
        "元旦"   (2025-1-1 ..= 2025-1-1),
        "春节"   (2025-1-28 ..= 2025-2-4)   调休 (2025-1-26, 2025-2-8),
        "清明节" (2025-4-4 ..= 2025-4-6),
        "劳动节" (2025-5-1 ..= 2025-5-5)    调休 (2025-4-27),
        "端午节" (2025-5-31 ..= 2025-6-2),
        "国庆节" (2025-10-1 ..= 2025-10-8)  调休 (2025-9-28, 2025-10-11),
        // 以下七条挂在 2026 学年块下，是这一版覆盖的最后一批。
        "元旦"   (2026-1-1 ..= 2026-1-3)    调休 (2026-1-4),
        "春节"   (2026-2-15 ..= 2026-2-23)  调休 (2026-2-14, 2026-2-28),
        "清明节" (2026-4-4 ..= 2026-4-6),
        "劳动节" (2026-5-1 ..= 2026-5-5)    调休 (2026-5-9),
        "端午节" (2026-6-19 ..= 2026-6-21),
        "中秋节" (2026-9-25 ..= 2026-9-27)  调休 (2026-9-20),
        "国庆节" (2026-10-1 ..= 2026-10-7)  调休 (2026-10-10),
    };

    Calendar { years, holidays }
}

// ---------------------------------------------------------------------------
// 当前生效的那一份
// ---------------------------------------------------------------------------

static FALLBACK: LazyLock<Arc<Calendar>> = LazyLock::new(|| Arc::new(fallback_calendar()));

/// 线上抓回来的那一份；没抓到过就是空，取值回落到 `FALLBACK`。
static LIVE: LazyLock<ArcSwapOption<Calendar>> = LazyLock::new(ArcSwapOption::empty);

/// 当前生效的校历。
pub fn current() -> Arc<Calendar> {
    LIVE.load_full().unwrap_or_else(|| FALLBACK.clone())
}

pub fn day_kind(date: NaiveDate) -> DayKind {
    current().day_kind(date)
}

/// 这一天所属的假期（要名字时用，会克隆一份；不在热路径上调）。
pub fn holiday_on(date: NaiveDate) -> Option<Holiday> {
    current().holiday_on(date).cloned()
}

/// 这一天是哪个假期的调休上课日。
pub fn makeup_on(date: NaiveDate) -> Option<Holiday> {
    current().makeup_on(date).cloned()
}

pub fn is_covered(date: NaiveDate) -> bool {
    current().is_covered(date)
}

/// 节假日表管到哪一天。
pub fn coverage_end() -> Option<NaiveDate> {
    current().coverage().map(|s| s.end)
}

// ---------------------------------------------------------------------------
// 解析：school.js -> Calendar
// ---------------------------------------------------------------------------

/// 扫出来的值。源文件只用到这四种。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Int(i64),
    Str(String),
    Arr(Vec<Value>),
    /// 保留源文件里的键序。
    Obj(Vec<(String, Value)>),
}

impl Value {
    fn as_int(&self) -> Result<i64> {
        match self {
            Value::Int(n) => Ok(*n),
            other => bail!("期望一个整数，拿到 {other:?}"),
        }
    }

    fn as_str(&self) -> Result<&str> {
        match self {
            Value::Str(s) => Ok(s),
            other => bail!("期望一个字符串，拿到 {other:?}"),
        }
    }

    fn as_obj(&self) -> Result<&[(String, Value)]> {
        match self {
            Value::Obj(kv) => Ok(kv),
            other => bail!("期望一个对象，拿到 {other:?}"),
        }
    }

    fn as_arr(&self) -> Result<&[Value]> {
        match self {
            Value::Arr(v) => Ok(v),
            other => bail!("期望一个数组，拿到 {other:?}"),
        }
    }
}

/// 一个够用就好的 JS 字面量扫描器。
///
/// 按字节走：中文标识符是多字节 UTF-8，但 UTF-8 的续字节全都 >= 0x80，
/// 永远不会和这里比较的 ASCII 分隔符撞上，所以按字节切片是安全的。
struct Scanner<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a [u8]) -> Self {
        Self { src, pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// 空白和注释一起跳掉。整个解析器只有这一处认注释。
    fn skip_trivia(&mut self) {
        loop {
            while matches!(self.peek(), Some(b) if b.is_ascii_whitespace()) {
                self.pos += 1;
            }
            if self.src[self.pos..].starts_with(b"//") {
                while !matches!(self.peek(), None | Some(b'\n')) {
                    self.pos += 1;
                }
                continue;
            }
            if self.src[self.pos..].starts_with(b"/*") {
                self.pos += 2;
                while self.pos < self.src.len() && !self.src[self.pos..].starts_with(b"*/") {
                    self.pos += 1;
                }
                self.pos = (self.pos + 2).min(self.src.len());
                continue;
            }
            return;
        }
    }

    fn expect(&mut self, want: u8) -> Result<()> {
        self.skip_trivia();
        match self.peek() {
            Some(b) if b == want => {
                self.pos += 1;
                Ok(())
            }
            other => bail!(
                "第 {} 字节处期望 {:?}，拿到 {:?}",
                self.pos,
                want as char,
                other.map(|b| b as char)
            ),
        }
    }

    /// 单引号双引号都收。源文件里两种都有，而且没有转义。
    fn read_string(&mut self) -> Result<String> {
        self.skip_trivia();
        let quote = match self.peek() {
            Some(q @ (b'"' | b'\'')) => q,
            other => bail!("第 {} 字节处期望引号，拿到 {other:?}", self.pos),
        };
        self.pos += 1;
        let start = self.pos;
        let mut closed = false;
        while let Some(b) = self.peek() {
            if b == quote {
                closed = true;
                break;
            }
            self.pos += 1;
        }
        if !closed {
            bail!("字符串没有收尾");
        }
        let s = std::str::from_utf8(&self.src[start..self.pos])?.to_owned();
        self.pos += 1;
        Ok(s)
    }

    fn read_int(&mut self) -> Result<i64> {
        self.skip_trivia();
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b) if b.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            bail!("第 {} 字节处期望一个数字", self.pos);
        }
        Ok(std::str::from_utf8(&self.src[start..self.pos])?.parse()?)
    }

    /// 键：裸标识符（含中文）或带引号的字符串，后面跟冒号。
    fn read_key(&mut self) -> Result<String> {
        self.skip_trivia();
        if matches!(self.peek(), Some(b'"' | b'\'')) {
            let k = self.read_string()?;
            self.expect(b':')?;
            return Ok(k);
        }
        let start = self.pos;
        while !matches!(
            self.peek(),
            None | Some(b':' | b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n')
        ) {
            self.pos += 1;
        }
        if self.pos == start {
            bail!("第 {} 字节处期望一个键名", self.pos);
        }
        let k = std::str::from_utf8(&self.src[start..self.pos])?.to_owned();
        self.expect(b':')?;
        Ok(k)
    }

    fn read_value(&mut self) -> Result<Value> {
        self.skip_trivia();
        match self.peek() {
            Some(b'{') => Ok(Value::Obj(self.read_object()?)),
            Some(b'[') => Ok(Value::Arr(self.read_array()?)),
            Some(b'"' | b'\'') => Ok(Value::Str(self.read_string()?)),
            Some(_) => Ok(Value::Int(self.read_int()?)),
            None => bail!("值读到一半就没了"),
        }
    }

    /// `{ k: v, k: v, }` —— 尾逗号只在这里和 [`Self::read_array`] 里处理。
    fn read_object(&mut self) -> Result<Vec<(String, Value)>> {
        self.expect(b'{')?;
        let mut out = Vec::new();
        loop {
            self.skip_trivia();
            if self.peek() == Some(b'}') {
                self.pos += 1;
                return Ok(out);
            }
            let key = self.read_key()?;
            let value = self.read_value()?;
            out.push((key, value));
            self.skip_trivia();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {}
                other => bail!("第 {} 字节处对象里期望 , 或 }}，拿到 {other:?}", self.pos),
            }
        }
    }

    fn read_array(&mut self) -> Result<Vec<Value>> {
        self.expect(b'[')?;
        let mut out = Vec::new();
        loop {
            self.skip_trivia();
            if self.peek() == Some(b']') {
                self.pos += 1;
                return Ok(out);
            }
            out.push(self.read_value()?);
            self.skip_trivia();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {}
                other => bail!("第 {} 字节处数组里期望 , 或 ]，拿到 {other:?}", self.pos),
            }
        }
    }
}

/// `"2026-9-6"` —— 月日都不补零，所以不走 `chrono` 的格式串。
fn parse_date(s: &str) -> Result<NaiveDate> {
    let mut it = s.split('-');
    let mut next = |what: &str| -> Result<i32> {
        it.next()
            .ok_or_else(|| anyhow!("日期 {s:?} 缺少{what}"))?
            .trim()
            .parse::<i32>()
            .map_err(|e| anyhow!("日期 {s:?} 的{what}不是数字: {e}"))
    };
    let (y, m, d) = (next("年")?, next("月")?, next("日")?);
    if it.next().is_some() {
        bail!("日期 {s:?} 多了一段");
    }
    NaiveDate::from_ymd_opt(y, m as u32, d as u32).ok_or_else(|| anyhow!("日期 {s:?} 不存在"))
}

fn span_from(kv: &[(String, Value)]) -> Result<Span> {
    let pick = |want: &str| -> Result<NaiveDate> {
        let entry = kv
            .iter()
            .find(|(k, _)| k == want)
            .ok_or_else(|| anyhow!("缺少 {want}"))?;
        parse_date(entry.1.as_str()?)
    };
    Ok(Span {
        start: pick("开始于")?,
        end: pick("结束于")?,
    })
}

fn term_from(kv: &[(String, Value)]) -> Result<Term> {
    let sub = |want: &str| -> Result<Option<Span>> {
        match kv.iter().find(|(k, _)| k == want) {
            Some((_, v)) => Ok(Some(span_from(v.as_obj()?)?)),
            None => Ok(None),
        }
    };
    Ok(Term {
        span: span_from(kv)?,
        midterm: sub("期中考")?,
        finals: sub("期末考")?,
    })
}

fn holiday_from(name: &str, kv: &[(String, Value)]) -> Result<Holiday> {
    let makeups = match kv.iter().find(|(k, _)| k == "调休") {
        Some((_, v)) => v
            .as_arr()?
            .iter()
            .map(|d| parse_date(d.as_str()?))
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    Ok(Holiday {
        name: name.to_owned(),
        span: span_from(kv)?,
        makeups,
    })
}

fn year_from(kv: &[(String, Value)]) -> Result<(AcademicYear, Vec<Holiday>)> {
    let mut year = AcademicYear::placeholder(0);
    let mut holidays = Vec::new();
    let mut named = false;
    for (key, value) in kv {
        match key.as_str() {
            "名称" => {
                year.name = value.as_int()? as i32;
                named = true;
            }
            "第一学期" => year.first = Some(term_from(value.as_obj()?)?),
            "寒假" => year.winter = Some(span_from(value.as_obj()?)?),
            "第二学期" => year.second = Some(term_from(value.as_obj()?)?),
            "第三学期" => year.third = Some(span_from(value.as_obj()?)?),
            "暑假" => year.summer = Some(span_from(value.as_obj()?)?),
            "节假日" => {
                for (name, h) in value.as_obj()? {
                    holidays.push(holiday_from(name, h.as_obj()?)?);
                }
            }
            // 块级 `开始于` / `结束于` 语义不一致，直接丢（见模块文档第 1 条）；
            // 将来 jwc 加了别的键也从这里静静走掉。
            _ => {}
        }
    }
    if !named {
        bail!("年份块里没有 名称");
    }
    Ok((year, holidays))
}

/// 把 `school.js` 的正文解析成一份校历。
///
/// 有任何一个年份块坏掉就整体失败：宁可继续用上一份，也不要装进去半张表。
pub fn parse(src: &str) -> Result<Calendar> {
    const PUSH: &str = ".push(";
    let mut years = Vec::new();
    let mut holidays = Vec::new();
    let mut rest = src;

    while let Some(at) = rest.find(PUSH) {
        let body = &rest[at + PUSH.len()..];
        let mut sc = Scanner::new(body.as_bytes());
        let obj = sc.read_object()?;
        let (year, mut hs) = year_from(&obj)?;
        years.push(year);
        holidays.append(&mut hs);
        rest = &body[sc.pos..];
    }

    if years.is_empty() {
        bail!("没有解析出任何年份块");
    }

    years.sort_by(|a, b| b.name.cmp(&a.name));
    holidays.sort_by_key(|h| h.span.start);
    holidays.dedup();
    Ok(Calendar { years, holidays })
}

// ---------------------------------------------------------------------------
// 每天刷一次
// ---------------------------------------------------------------------------

pub struct SchoolCalendarTask;

#[async_trait]
impl TimeTask for SchoolCalendarTask {
    type Output = Arc<Calendar>;

    fn name(&self) -> &'static str {
        "SchoolCalendarTask"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(24 * 3600)
    }

    /// **永远返回 `Ok`。**
    ///
    /// [`TaskRunner`] 一旦收到 `Err` 就转进每 5 秒一次的重试循环，而且不会退出——
    /// jwc 挂一天就是一万七千次请求打到学校网站上。校历抓不到并不紧急（兜底表还在），
    /// 所以这里把失败咽掉、记一条 WARN，让调度器照常睡满 24 小时。
    async fn run(&self) -> Result<Self::Output> {
        match fetch_and_install().await {
            Ok(calendar) => Ok(calendar),
            Err(e) => {
                warn!(error = ?e, url = SCHOOL_JS_URL, "校历抓取失败，继续用现有的一份");
                Ok(current())
            }
        }
    }
}

pub static SCHOOL_CALENDAR_TASK: LazyLock<Arc<TaskRunner<SchoolCalendarTask>>> =
    LazyLock::new(|| TaskRunner::new(SchoolCalendarTask));

/// 由 `main` 在启动时调用：立刻抓一次校历，之后每 24 小时一次。
///
/// `LazyLock::force` 会触发 `TaskRunner::new`，后者先跑一次再睡。
pub fn spawn_refresh_task() {
    LazyLock::force(&SCHOOL_CALENDAR_TASK);
}

async fn fetch_and_install() -> Result<Arc<Calendar>> {
    let client = SessionClient::new();
    let text = client.get(SCHOOL_JS_URL).await?.text().await?;
    let parsed = parse(&text)?;

    let Some(new_span) = parsed.coverage() else {
        bail!("抓回来的校历一个节假日都没有");
    };

    // 覆盖范围只许往后走。jwc 手抖把某一年删掉时这里挡住，
    // 不让线上那份把已经知道的假期弄丢。
    let live = current();
    if let Some(old_span) = live.coverage()
        && new_span.end < old_span.end
    {
        warn!(
            new_end = %new_span.end,
            current_end = %old_span.end,
            "抓回来的校历比手上这份还旧，不采用"
        );
        return Ok(live);
    }

    let parsed = Arc::new(parsed);
    LIVE.store(Some(parsed.clone()));
    info!(
        years = parsed.years.len(),
        holidays = parsed.holidays.len(),
        covered_until = %new_span.end,
        "校历已更新"
    );
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Weekday};

    /// 2026-06-15 那一版 `school.js` 的原文。
    const FIXTURE: &str = include_str!("testdata/school.js");

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// 解析器的靶子：拿 `school.js` 原文解析出来的东西，必须和手抄的兜底表一模一样。
    ///
    /// 这一条同时验两件事——解析器对不对，以及手抄的那份有没有抄错。
    #[test]
    fn 解析结果与手抄的兜底表逐字相同() {
        let parsed = parse(FIXTURE).expect("fixture 应当能解析");
        let fallback = fallback_calendar();
        assert_eq!(parsed.years, fallback.years, "学年表不一致");
        assert_eq!(parsed.holidays, fallback.holidays, "节假日表不一致");
    }

    /// jwc 的 2027 块只有 `名称`，各段必须都是 `None`，不能因此解析失败。
    #[test]
    fn 占位年份块不会让解析失败() {
        let parsed = parse(FIXTURE).unwrap();
        let y2027 = parsed
            .years
            .iter()
            .find(|y| y.name == 2027)
            .expect("2027 占位块应当在");
        assert_eq!(*y2027, AcademicYear::placeholder(2027));
        assert!(y2027.span().is_none());
        assert!(y2027.teaching_monday().is_none());
    }

    /// 源文件的脏写法：行注释、尾逗号、单双引号混用、月日不补零、块级字段要丢掉。
    #[test]
    fn 解析器吃得下源文件的脏写法() {
        let src = r#"
window["school-calendars"].push({
    名称: 2099, // 行注释
    开始于: "2099-9-6", // 块级字段应当被丢掉
    第一学期: {
        开始于: "2099-9-6",
        结束于: "2100-1-9",
    },
    节假日: {
        国庆节: {
            开始于: "2099-10-1",
            结束于: "2099-10-7",
            调休: [
                '2099-9-26',
            ]
        },
    }
});
"#;
        let c = parse(src).unwrap();
        assert_eq!(c.years.len(), 1);
        assert_eq!(c.years[0].name, 2099);
        assert_eq!(c.years[0].first.unwrap().span.end, d(2100, 1, 9));
        assert!(c.years[0].winter.is_none(), "块级字段不该被当成学期");
        assert_eq!(c.holidays.len(), 1);
        assert_eq!(c.holidays[0].name, "国庆节");
        assert_eq!(c.holidays[0].makeups, vec![d(2099, 9, 26)]);
    }

    #[test]
    fn 解析失败不会给出半份校历() {
        assert!(parse(r#"window["school-calendars"].push({ 名称: });"#).is_err());
        assert!(parse("完全不是那个文件").is_err());
        // 收尾的 `}` 换成中文字符：扫描器按字节走，这里要的是干净的 Err，
        // 不能因为切在 UTF-8 字符中间而 panic。
        assert!(parse(r#"window["school-calendars"].push({ 名称: 2099，"#).is_err());
        assert!(parse(r#"window["school-calendars"].push({ 名称: 2099, 备注: "未完"#).is_err());
    }

    /// jwc 在每个学期/假期的开始日上都注释了「这一天必须是周日」。这条断言是整张表的
    /// 抄写校验：抄错一位数字，基本都会在这里露馅。
    ///
    /// 唯一的例外是 jwc 自己写错的 2023-4-3（2022 学年第二学期期中考），实际是周一。
    #[test]
    fn 每个学期和假期都从周日开始() {
        let 已知笔误 = d(2023, 4, 3);
        for year in &fallback_calendar().years {
            let mut starts = Vec::new();
            for term in [year.first, year.second].into_iter().flatten() {
                starts.push(term.span.start);
                starts.extend(term.midterm.map(|s| s.start));
                starts.extend(term.finals.map(|s| s.start));
            }
            starts.extend(
                [year.winter, year.third, year.summer]
                    .into_iter()
                    .flatten()
                    .map(|s| s.start),
            );
            for start in starts {
                if start == 已知笔误 {
                    continue;
                }
                assert_eq!(
                    start.weekday(),
                    Weekday::Sun,
                    "{} 学年的 {} 不是周日",
                    year.name,
                    start
                );
            }
        }
    }

    /// 学年内部必须首尾相接、不留空洞。占位块（各段都缺）跳过。
    #[test]
    fn 学年内的各段首尾相接() {
        for year in &fallback_calendar().years {
            let (Some(first), Some(winter), Some(second), Some(third), Some(summer)) = (
                year.first,
                year.winter,
                year.second,
                year.third,
                year.summer,
            ) else {
                continue;
            };
            let 各段 = [
                ("第一学期->寒假", first.span.end, winter.start),
                ("寒假->第二学期", winter.end, second.span.start),
                ("第二学期->第三学期", second.span.end, third.start),
                ("第三学期->暑假", third.end, summer.start),
            ];
            for (名, 前段结束, 后段开始) in 各段 {
                assert_eq!(
                    后段开始 - 前段结束,
                    chrono::Duration::days(1),
                    "{} 学年 {} 之间不连续：{} -> {}",
                    year.name,
                    名,
                    前段结束,
                    后段开始
                );
            }
        }
    }

    /// 相邻学年之间也必须首尾相接（上一年的暑假结束 = 下一年开学的前一天）。
    #[test]
    fn 相邻学年首尾相接() {
        let calendar = fallback_calendar();
        let 有课的: Vec<&AcademicYear> = calendar
            .years
            .iter()
            .filter(|y| y.span().is_some())
            .collect();
        for pair in 有课的.windows(2) {
            let (新, 旧) = (pair[0], pair[1]);
            assert_eq!(新.name, 旧.name + 1, "学年表必须按 name 降序且连续");
            assert_eq!(
                新.span().unwrap().start - 旧.span().unwrap().end,
                chrono::Duration::days(1),
                "{} 学年的暑假结束于 {}，{} 学年却从 {} 开学",
                旧.name,
                旧.span().unwrap().end,
                新.name,
                新.span().unwrap().start
            );
        }
    }

    /// 摊平后的节假日表必须按日期升序、区间合法、互不重叠。
    #[test]
    fn 节假日表有序且不重叠() {
        let calendar = fallback_calendar();
        for h in &calendar.holidays {
            assert!(h.span.start <= h.span.end, "{} 的区间是反的", h.name);
        }
        for pair in calendar.holidays.windows(2) {
            let (前, 后) = (&pair[0], &pair[1]);
            assert!(
                前.span.end < 后.span.start,
                "{}（{} 结束）和 {}（{} 开始）重叠或乱序",
                前.name,
                前.span.end,
                后.name,
                后.span.start
            );
        }
    }

    /// 调休日必须是周六或周日，而且不能落在任何假期里。
    #[test]
    fn 调休日都是周末且不与假期冲突() {
        let calendar = fallback_calendar();
        for h in &calendar.holidays {
            for &m in &h.makeups {
                assert!(
                    matches!(m.weekday(), Weekday::Sat | Weekday::Sun),
                    "{} 的调休日 {} 是 {:?}，不是周末",
                    h.name,
                    m,
                    m.weekday()
                );
                assert!(
                    calendar.holiday_on(m).is_none(),
                    "{} 的调休日 {} 被算成了放假",
                    h.name,
                    m
                );
            }
        }
    }

    /// 本学期（2026 秋）的判定钉死，防止以后改表改坏。
    #[test]
    fn 本学期的关键日子判定正确() {
        let c = fallback_calendar();

        assert_eq!(c.day_kind(d(2026, 9, 20)), DayKind::Makeup);
        assert_eq!(c.makeup_on(d(2026, 9, 20)).unwrap().name, "中秋节");
        assert_eq!(c.day_kind(d(2026, 9, 25)), DayKind::Holiday);
        assert_eq!(c.holiday_on(d(2026, 9, 25)).unwrap().name, "中秋节");
        assert_eq!(c.day_kind(d(2026, 9, 27)), DayKind::Holiday);
        assert_eq!(c.day_kind(d(2026, 10, 1)), DayKind::Holiday);
        assert_eq!(c.day_kind(d(2026, 10, 7)), DayKind::Holiday);
        assert_eq!(c.day_kind(d(2026, 10, 10)), DayKind::Makeup);
        assert_eq!(c.makeup_on(d(2026, 10, 10)).unwrap().name, "国庆节");
        assert_eq!(c.day_kind(d(2026, 9, 21)), DayKind::Normal);

        // 覆盖范围之外：按普通日处理，但 is_covered 要说实话。
        assert_eq!(c.day_kind(d(2027, 1, 1)), DayKind::Normal);
        assert!(!c.is_covered(d(2027, 1, 1)));
        assert!(c.is_covered(d(2026, 10, 10)));
    }

    /// 覆盖范围两头都得把调休日算进来：2022 劳动节的调休 4-24 比放假 4-30 还早，
    /// 2026 国庆的调休 10-10 比放假 10-7 还晚。
    #[test]
    fn 覆盖范围含调休日() {
        let span = fallback_calendar().coverage().unwrap();
        assert_eq!(span.start, d(2022, 4, 24));
        assert_eq!(span.end, d(2026, 10, 10));
    }

    /// 摊平之后，按日期查学年必须能查到正确的那一个（这是不按学年挂节假日的理由）。
    #[test]
    fn 按日期查学年() {
        let c = fallback_calendar();
        assert_eq!(c.academic_year_of(d(2026, 9, 20)).unwrap().name, 2026);
        // 2026 年春天属于 2025 学年，但它的假期挂在 jwc 的 2026 块里。
        assert_eq!(c.academic_year_of(d(2026, 4, 4)).unwrap().name, 2025);
        assert_eq!(c.holiday_on(d(2026, 4, 4)).unwrap().name, "清明节");
        // 2023 年春天属于 2022 学年，假期也挂在 2022 块里。
        assert_eq!(c.academic_year_of(d(2023, 4, 5)).unwrap().name, 2022);
        assert_eq!(c.holiday_on(d(2023, 4, 5)).unwrap().name, "清明节");
    }

    /// 真的去 jwc 拉一次，确认线上那份还能解析。
    ///
    /// 这是唯一能发现「jwc 改了文件写法、解析器从此静默回落到兜底表」的办法——
    /// 线上失败只会留一条 WARN，不会有人盯着。默认跳过，
    /// `XMU_TEST_NETWORK=1 cargo test --lib -- --test-threads=1` 打开。
    #[tokio::test(flavor = "multi_thread")]
    async fn 线上的校历还能解析() -> Result<()> {
        if !crate::api::xmu_service::testenv::network_enabled() {
            return crate::api::xmu_service::testenv::skipped_network(module_path!());
        }
        let text = SessionClient::new()
            .get(SCHOOL_JS_URL)
            .await?
            .text()
            .await?;
        let live = parse(&text)?;
        let span = live.coverage().expect("线上校历应当有节假日");
        println!(
            "线上校历：{} 个学年，{} 个节假日，覆盖到 {}",
            live.years.len(),
            live.holidays.len(),
            span.end
        );
        // 线上那份只会比手抄的更新，不该更旧。
        let fallback_end = fallback_calendar().coverage().unwrap().end;
        assert!(
            span.end >= fallback_end,
            "线上校历覆盖到 {}，比手抄的 {} 还早",
            span.end,
            fallback_end
        );
        Ok(())
    }

    /// 教学周一比 jwc 的开学日晚一天，喂给 `set_semester_start` 的必须是前者。
    #[test]
    fn 教学周一是开学日的后一天() {
        let c = fallback_calendar();
        let 本学年 = c.years.iter().find(|y| y.name == 2026).unwrap();
        assert_eq!(本学年.first.unwrap().span.start.weekday(), Weekday::Sun);
        assert_eq!(本学年.teaching_monday().unwrap(), d(2026, 9, 7));
        assert_eq!(本学年.teaching_monday().unwrap().weekday(), Weekday::Mon);
    }
}
