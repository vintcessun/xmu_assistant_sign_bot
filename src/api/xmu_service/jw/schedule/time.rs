use super::JwAPI;
use crate::abi::utils::SmartJsonExt;
use anyhow::Result;
use helper::jw_api;
use serde::{Deserialize, Serialize};

#[jw_api(
    url = "https://jw.xmu.edu.cn/gsapp/sys/wdkbapp/wdkcb/queryXsskjc.do",
    app = "https://jw.xmu.edu.cn/appShow?appId=4979568947762216"
)]
pub struct ScheduleTime {
    pub jssj: u16,  // 结束时间
    pub kssj: u16,  // 开始时间
    pub mc: String, // 名称
    pub px: i64,    // 排序
                    //pub dm: IgnoredAny,     // 代码
                    //pub jcfadm: IgnoredAny, // 教室分配代码
                    //pub ywmc: IgnoredAny,   // 英文名称
}

#[derive(Serialize, Debug)]
pub struct ScheduleTimeRequest<'a> {
    #[serde(rename = "XNXQDM")]
    pub semester: &'a str, // 学年学期代码
    #[serde(rename = "XH")]
    student_id: &'a str, // 学号
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-3689174-yqV2dqeExUYDOIabL8BdMNCkm-EUrfWocQ18HO03gqA4EUdlwOCCgO9UlWhoSi48p4gnull_main";
        let data = ScheduleTimeRequest {
            semester: "20252",
            student_id: "",
        };
        let schedule_time = ScheduleTime::call(castgc, &data).await?;
        println!("ScheduleTime API Response: {:?}", schedule_time);
        Ok(())
    }
}
