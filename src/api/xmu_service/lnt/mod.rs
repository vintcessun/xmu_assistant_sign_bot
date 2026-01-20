mod activities;
mod classroom;
mod classroom_subject;
mod distribute;
mod exams;
mod file_url;
mod html;
mod my_courses;
mod profile;
mod recently_visited_courses;
mod submissions;
mod submissions_id;

pub use activities::Activities;
pub use classroom::ClassroomExams;
pub use classroom::ClassroomList;
pub use classroom_subject::ClassroomSubject;
pub use distribute::Distribute;
pub use exams::Exams;
pub use file_url::FileUrl;
pub use my_courses::MyCourses;
pub use profile::Profile;
pub use recently_visited_courses::RecentlyVisitedCourses;
pub use submissions::Submissions;
pub use submissions_id::SubmissionsId;

use crate::api::network::SessionClient;
use std::sync::LazyLock;
use url::Url;
use url_macro::url;

pub static LNT_URL: LazyLock<Url> = LazyLock::new(|| url!("https://lnt.xmu.edu.cn"));

pub fn get_session_client(session: &str) -> SessionClient {
    let client = SessionClient::new();
    client.set_cookie("session", session, &LNT_URL);
    client
}
