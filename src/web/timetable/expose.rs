use crate::{
    api::xmu_service::jw::{
        BitField32, CourseTime, LocationStore, ScheduleCourseTime, TimeBitMap, Weekday,
    },
    logic::rollcall::{
        is_sign_time_active_now, query_sign_group, query_sign_time, update_sign_time,
    },
    web::timetable::task,
};
use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

include!(concat!(env!("OUT_DIR"), "/web_data.rs"));

#[derive(Deserialize)]
struct TaskPath {
    id: String,
}

#[derive(Deserialize)]
struct QqPath {
    qq: i64,
}

#[derive(Serialize, Deserialize, Clone)]
struct CourseView {
    name: String,
    location: Option<String>,
    /// 距午夜 0:00 的分钟数
    start: u16,
    /// 距午夜 0:00 的分钟数
    end: u16,
    /// 星期（英文枚举名，如 "Monday"）
    day: String,
    /// 周次位掩码，第 i 位为 1 表示第 i+1 周有课
    week_mask: u32,
}

#[derive(Serialize)]
struct TaskDetailResponse {
    task_id: String,
    qq: i64,
    expire_at: u64,
    seconds_left: u64,
    courses: Vec<CourseView>,
}

#[derive(Deserialize)]
struct SaveTaskRequest {
    courses: Vec<CourseView>,
}

#[derive(Serialize)]
struct QqQueryResponse {
    qq: i64,
    enabled: bool,
    group_id: Option<i64>,
    course_count: usize,
    is_active_now: bool,
    courses: Vec<CourseView>,
}

pub fn task_router(router: Router) -> Router {
    router
        // 编辑页面：通过 UUID 编辑一次性课表任务
        .route("/edit/{id}", get(edit_page_handler))
        // 查询页面：输入 QQ 查看当前自动签到课表状态
        .route("/query", get(query_page_handler))
        // 任务数据读取
        .route("/api/task/{id}", get(task_detail_handler))
        // 任务保存并失效
        .route("/api/task/{id}/save", post(task_save_handler))
        // 按 QQ 查询当前自动签到课表状态
        .route("/api/query/{qq}", get(query_by_qq_handler))
}

async fn edit_page_handler() -> Html<&'static str> {
    Html(TIMETABLE_HTML)
}

async fn query_page_handler() -> Html<&'static str> {
    Html(TIMETABLE_QUERY_HTML)
}

async fn task_detail_handler(Path(params): Path<TaskPath>) -> impl IntoResponse {
    match task::get_valid_task(&params.id) {
        Some(task_data) => {
            let courses = task_data
                .course_time
                .times
                .iter()
                .map(to_course_view)
                .collect::<Vec<_>>();
            Json(TaskDetailResponse {
                task_id: task_data.id.clone(),
                qq: task_data.qq,
                expire_at: task_data.expire_at,
                seconds_left: task::seconds_left(&task_data),
                courses,
            })
            .into_response()
        }
        None => (
            StatusCode::GONE,
            Json(serde_json::json!({ "detail": "编辑任务不存在或已过期" })),
        )
            .into_response(),
    }
}

async fn task_save_handler(
    Path(params): Path<TaskPath>,
    Json(payload): Json<SaveTaskRequest>,
) -> impl IntoResponse {
    let task_data = match task::take_valid_task(&params.id) {
        Some(task_data) => task_data,
        None => {
            return (
                StatusCode::GONE,
                Json(serde_json::json!({ "detail": "编辑任务不存在、已提交或已过期" })),
            )
                .into_response();
        }
    };

    let mut times = Vec::with_capacity(payload.courses.len());
    for course in &payload.courses {
        match parse_course_view(course) {
            Ok(c) => times.push(c),
            Err(msg) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "detail": msg })),
                )
                    .into_response();
            }
        }
    }

    match update_sign_time(task_data.qq, ScheduleCourseTime { times }).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "detail": format!("保存失败: {}", e) })),
        )
            .into_response(),
    }
}

async fn query_by_qq_handler(Path(params): Path<QqPath>) -> impl IntoResponse {
    let data = query_sign_time(params.qq);
    let group_id = query_sign_group(params.qq);

    match data {
        Some(d) => {
            let courses = d.times.iter().map(to_course_view).collect::<Vec<_>>();
            Json(QqQueryResponse {
                qq: params.qq,
                enabled: true,
                group_id,
                course_count: courses.len(),
                is_active_now: is_sign_time_active_now(params.qq),
                courses,
            })
            .into_response()
        }
        None => Json(QqQueryResponse {
            qq: params.qq,
            enabled: false,
            group_id,
            course_count: 0,
            is_active_now: false,
            courses: vec![],
        })
        .into_response(),
    }
}

fn to_course_view(course: &CourseTime) -> CourseView {
    let (sh, sm) = course.start.to_hm();
    let (eh, em) = course.end.to_hm();
    CourseView {
        name: course.name.clone(),
        location: course.location.location_str.clone(),
        start: (sh as u16) * 60 + (sm as u16),
        end: (eh as u16) * 60 + (em as u16),
        day: format!("{:?}", course.day),
        week_mask: course.week_mask.0,
    }
}

fn parse_day(day: &str) -> Option<Weekday> {
    match day {
        "Monday" => Some(Weekday::Monday),
        "Tuesday" => Some(Weekday::Tuesday),
        "Wednesday" => Some(Weekday::Wednesday),
        "Thursday" => Some(Weekday::Thursday),
        "Friday" => Some(Weekday::Friday),
        "Saturday" => Some(Weekday::Saturday),
        "Sunday" => Some(Weekday::Sunday),
        _ => None,
    }
}

fn parse_course_view(course: &CourseView) -> Result<CourseTime, String> {
    if course.start >= 1440 || course.end > 1440 || course.start >= course.end {
        return Err(format!("课程「{}」时间范围无效", course.name));
    }

    let start_h = (course.start / 60) as u8;
    let start_m = (course.start % 60) as u8;
    let end_h = (course.end / 60) as u8;
    let end_m = (course.end % 60) as u8;

    let start = crate::api::xmu_service::jw::ClockTime::new(start_h, start_m);
    let end = crate::api::xmu_service::jw::ClockTime::new(end_h, end_m);
    let day =
        parse_day(&course.day).ok_or_else(|| format!("课程「{}」星期字段无效", course.name))?;

    Ok(CourseTime {
        name: course.name.clone(),
        location: Arc::new(LocationStore::from(course.location.clone())),
        start,
        end,
        time_bitmap: TimeBitMap::from_range(start, end),
        week_mask: BitField32::new(course.week_mask),
        day,
    })
}
