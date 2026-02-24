use super::JwAPI;
use crate::{
    abi::utils::SmartJsonExt,
    api::{network::SessionClient, xmu_service::jw::ScheduleListResponse},
};
use anyhow::Result;
use helper::{castgc_client_helper, jw_api};
use serde::{Deserialize, Serialize};

#[jw_api(
    url = "https://jw.xmu.edu.cn/gsapp/sys/wdkbapp/wdkcb/queryXspkjg.do",
    app = "https://jw.xmu.edu.cn/appShow?appId=4979568947762216",
    wrapper_name = "pkjgList"
)]
pub struct Schedule {
    pub jasmc: Option<String>, // 教室名称
    pub zcbh: String,          // 周次编号
    pub kcmc: String,          // 课程名称
    pub jssj: u16,             // 结束时间
    pub kssj: u16,             // 开始时间
    pub jsjcdm: i64,           // 结束节次代码
    pub ksjcdm: i64,           // 开始节次代码
    pub xq: i64,               // 星期
                               //pub bjdm: IgnoredAny,    // 班级代码
                               //pub bjmc: IgnoredAny,    // 班级名称
                               //pub jasdm: IgnoredAny,   // 教室代码
                               //pub jasywmc: IgnoredAny, // 教室英文名称
                               //pub jcfadm: IgnoredAny,  // 教室分配代码
                               //pub jsxm: IgnoredAny,    // 教师姓名
                               //pub jsywm: IgnoredAny,   // 教师英文名
                               //pub kbbz: IgnoredAny,    // 课表标志
                               //pub kcdm: IgnoredAny,    // 课程代码
                               //pub kcywmc: IgnoredAny,  // 课程英文名称
                               //pub sjly: IgnoredAny,    // 时间来源
                               //pub wid: IgnoredAny,     // 唯一ID
                               //pub xh: IgnoredAny,      // 学号
                               //pub xkrs: IgnoredAny,    // 选课人数
                               //pub zcmc: IgnoredAny,    // 周次名称
}

impl Schedule {
    #[castgc_client_helper]
    pub async fn get_from_client(
        client: &SessionClient,
        schedule_time: &ScheduleListResponse,
    ) -> Result<Schedule> {
        Self::get_by_code_from_client(client, &schedule_time.xnxqdm).await
    }

    #[castgc_client_helper]
    pub async fn get_by_code_from_client(
        client: &SessionClient,
        semester_code: &str,
    ) -> Result<Schedule> {
        let data = ScheduleRequest {
            semester: semester_code,
            student_id: "",
        };
        let schedule = Self::call_client(client, &data).await?;
        Ok(schedule)
    }
}

#[derive(Serialize, Debug)]
pub struct ScheduleRequest<'a> {
    #[serde(rename = "XNXQDM")]
    pub semester: &'a str, // 学年学期代码
    #[serde(rename = "XH")]
    student_id: &'a str, // 学号
}

#[cfg(test)]
mod tests {
    use crate::{
        abi::utils::SmartJsonExt,
        api::xmu_service::jw::{ScheduleCourseTime, ScheduleList, ScheduleListRequest},
    };

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test_location_vintcessun() -> Result<()> {
        let castgc = "TGT-4506885-hZ1EnCYrj6-6nx7Q7StF6WeTJye89IKSJcnGaCirrYRAbbO7-AQLV0Iz5Xin-5jChZMnull_main";
        let data = ScheduleListRequest {};
        let schedule_list = ScheduleList::call(castgc, &data).await?;
        for item in schedule_list.datas.kfdxnxqcx.rows {
            println!("Schedule Item: {:?}", item);
            let data = ScheduleRequest {
                semester: &item.xnxqdm,
                student_id: "",
            };
            let schedule = Schedule::call(castgc, &data).await?;
            println!("{} Schedule API Response: {:?}\n\n", &item.xnxqdm, schedule);
            let parse_result = ScheduleCourseTime::new_partial(schedule);
            println!("Parsed Schedule Course Time: {:?}\n\n", parse_result);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_location_dianzige() -> Result<()> {
        let castgc = "TGT-3689884-PTrQv9OKBhrc2RNRIoUz6fYzZqC08Zc9utxS0wxeMHDCMWKQ-KRSUxEjjVTcKiPQSf0null_main";
        let data = ScheduleListRequest {};
        let schedule_list = ScheduleList::call(castgc, &data).await?;
        for item in schedule_list.datas.kfdxnxqcx.rows {
            println!("Schedule Item: {:?}", item);
            let data = ScheduleRequest {
                semester: &item.xnxqdm,
                student_id: "",
            };
            let schedule = Schedule::call(castgc, &data).await?;
            println!("{} Schedule API Response: {:?}\n\n", &item.xnxqdm, schedule);
            let parse_result = ScheduleCourseTime::new_partial(schedule);
            println!("Parsed Schedule Course Time: {:?}\n\n", parse_result);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_location_lih() -> Result<()> {
        let castgc = "TGT-3689952-fGlR02k8pDvqfPNL-F-lyY8-bpus2ZHBZRqFbmEcmomCsp0D9laQQItngiL-K3UAumonull_main";
        let data = ScheduleListRequest {};
        let schedule_list = ScheduleList::call(castgc, &data).await?;
        for item in schedule_list.datas.kfdxnxqcx.rows {
            println!("Schedule Item: {:?}", item);
            let data = ScheduleRequest {
                semester: &item.xnxqdm,
                student_id: "",
            };
            let schedule = Schedule::call(castgc, &data).await?;
            println!("{} Schedule API Response: {:?}\n\n", &item.xnxqdm, schedule);
            let parse_result = ScheduleCourseTime::new_partial(schedule);
            println!("Parsed Schedule Course Time: {:?}\n\n", parse_result);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_location_axol() -> Result<()> {
        let castgc = "TGT-2288325-P177wkI8xWr8WNjs6QfA23HpnFTZH6Ac8U-zUHtVbWsxx5vVxWOTZ3VifZiELK0EDSInull_main";
        let data = ScheduleListRequest {};
        let schedule_list = ScheduleList::call(castgc, &data).await?;
        for item in schedule_list.datas.kfdxnxqcx.rows {
            println!("Schedule Item: {:?}", item);
            let data = ScheduleRequest {
                semester: &item.xnxqdm,
                student_id: "",
            };
            let schedule = Schedule::call(castgc, &data).await?;
            println!("{} Schedule API Response: {:?}\n\n", &item.xnxqdm, schedule);
            let parse_result = ScheduleCourseTime::new_partial(schedule);
            println!("Parsed Schedule Course Time: {:?}\n\n", parse_result);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_detail() -> Result<()> {
        let castgc = "TGT-3689523-tqSGK8uMKkyZVNAjG5H1ss4yc0Rsbdeac8Cwq7T5YKUxMQ3XU2L0cCe5FGiYHO6Z7EUnull_main";
        let client = crate::api::xmu_service::jw::get_castgc_client(castgc);
        client.get(Schedule::APP_ENTRANCE).await?;
        let data = ScheduleRequest {
            semester: "20251",
            student_id: "",
        };
        let resp = client
            .post(Schedule::URL_DATA, &data)
            .await?
            .json_smart::<Schedule>()
            .await?;
        println!("Schedule Detail Response: {:?}", resp);
        Ok(())
    }
}
