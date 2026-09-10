use crate::abi::utils::SmartJsonExt;
use helper::lnt_get_api;
use serde::{Deserialize, Serialize};

/// 学期。`sort` 是全校统一、随时间单调递增的序号（2024-1=9 … 2026-1=15），
/// 用它挑“最新的那个学期”比解析 code 字符串稳。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Semester {
    pub code: String,
    pub sort: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Course {
    pub id: i64,
    pub name: String,
    /// 教学班代码，与教务课表的 `BJDM` 逐字符相同（实测同一账号 11/11 完全对上），
    /// 需要把课表条目精确对到某个教学班时用它，不要去比课程名。
    ///
    /// 只反序列化不序列化：这个结构体会被整个塞进 `choose_course` 给 LLM 的提示词里，
    /// 二十多位的代码对选课判断毫无帮助，只会白白占 token。
    #[serde(skip_serializing)]
    pub course_code: String,
    /// 有的历史课程没有学期信息，缺了就当它不属于当前学期。
    /// 同样不序列化，理由见 `course_code`。
    #[serde(skip_serializing, default)]
    pub semester: Option<Semester>,
    //pub academic_year: IgnoredAny,
    //pub compulsory: IgnoredAny,
    //pub course_attributes: IgnoredAny,
    //pub course_type: IgnoredAny,
    //pub credit: IgnoredAny,
    //pub department: IgnoredAny,
    //pub end_date: IgnoredAny,
    //pub grade: IgnoredAny,
    //pub instructors: IgnoredAny,
    //pub is_mute: IgnoredAny,
    //pub klass: IgnoredAny,
    //pub org: IgnoredAny,
    //pub org_id: IgnoredAny,
    //pub start_date: IgnoredAny,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MyCourseResponse {
    pub courses: Vec<Course>,
}

#[lnt_get_api(MyCourseResponse, "https://lnt.xmu.edu.cn/api/my-courses")]
pub struct MyCourses;

#[cfg(test)]
mod tests {
    use crate::api::xmu_service::login::castgc_get_session;
    use crate::api::xmu_service::testenv;

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let Some(castgc) = testenv::castgc() else {
            return testenv::skipped(module_path!());
        };
        let session = castgc_get_session(castgc).await?;
        let data = MyCourses::get(&session).await?;
        println!("MyCourses: {:?}", data);
        println!("JSON: {}", serde_json::to_string(&data)?);
        Ok(())
    }
}
