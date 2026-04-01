use crate::{api::xmu_service::jw::ScheduleCourseTime, web::URL};
use dashmap::DashMap;
use std::{
    sync::{Arc, LazyLock},
    time::{SystemTime, UNIX_EPOCH},
};

const EDIT_EXPIRE_SECS: u64 = 20 * 60;

#[derive(Debug, Clone)]
pub struct TimetableEditTask {
    pub id: String,
    pub qq: i64,
    pub course_time: ScheduleCourseTime,
    pub expire_at: u64,
}

static TIMETABLE_EDIT_TASKS: LazyLock<DashMap<String, Arc<TimetableEditTask>>> =
    LazyLock::new(DashMap::new);

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn create_edit_task_url(qq: i64, course_time: &ScheduleCourseTime) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let task = TimetableEditTask {
        id: id.clone(),
        qq,
        course_time: course_time.clone(),
        expire_at: now_ts() + EDIT_EXPIRE_SECS,
    };
    TIMETABLE_EDIT_TASKS.insert(id.clone(), Arc::new(task));
    format!("{}/timetable/edit/{}", URL, id)
}

pub fn get_valid_task(id: &str) -> Option<Arc<TimetableEditTask>> {
    let task = TIMETABLE_EDIT_TASKS.get(id)?.clone();
    if task.expire_at <= now_ts() {
        TIMETABLE_EDIT_TASKS.remove(id);
        return None;
    }
    Some(task)
}

pub fn take_valid_task(id: &str) -> Option<Arc<TimetableEditTask>> {
    let (task_id, task) = TIMETABLE_EDIT_TASKS.remove(id)?;
    if task.expire_at <= now_ts() {
        return None;
    }
    // 利用一次性任务语义：提交后该 UUID 永久失效。
    debug_assert_eq!(task_id, id);
    Some(task)
}

pub fn seconds_left(task: &TimetableEditTask) -> u64 {
    task.expire_at.saturating_sub(now_ts())
}
