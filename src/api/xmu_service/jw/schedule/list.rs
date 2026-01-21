use super::JwAPI;
use crate::{abi::utils::SmartJsonExt, api::network::SessionClient};
use anyhow::Result;
use helper::{castgc_client_helper, jw_api};
use serde::{Deserialize, Serialize};

#[jw_api(
    url = "https://jw.xmu.edu.cn/gsapp/sys/wdkbapp/modules/xskcb/kfdxnxqcx.do",
    app = "https://jw.xmu.edu.cn/appShow?appId=4979568947762216"
)]
pub struct ScheduleList {
    pub xnxqdm: String, // 学年学期代码
    pub xnxqdm_display: String, // 学年学期代码显示
                        //pub by1: IgnoredAny,         // 备用1
                        //pub by2: IgnoredAny,         // 备用2
                        //pub by3: IgnoredAny,         // 备用3
                        //pub by4: IgnoredAny,         // 备用4
                        //pub by5: IgnoredAny,         // 备用5
                        //pub by6: IgnoredAny,         // 备用6
                        //pub by7: IgnoredAny,         // 备用7
                        //pub by8: IgnoredAny,         // 备用8
                        //pub by9: IgnoredAny,         // 备用9
                        //pub by10: IgnoredAny,        // 备用10
                        //pub kbkfrq: IgnoredAny,      // 课表开发日期
                        //pub orderfilter: IgnoredAny, // 订单过滤器
                        //pub rzlbdm: IgnoredAny,      // 认证类别代码
                        //pub wid: IgnoredAny,         // 唯一ID
                        //pub xnxqymc: IgnoredAny,     // 学年学期英文名称
}

impl ScheduleList {
    #[castgc_client_helper]
    pub async fn get_from_client(client: &SessionClient) -> Result<ScheduleList> {
        let data = ScheduleListRequest {};
        let schedule_list = Self::call_client(client, &data).await?;
        Ok(schedule_list)
    }
}

#[derive(Serialize, Debug)]
pub struct ScheduleListRequest {}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-3689523-tqSGK8uMKkyZVNAjG5H1ss4yc0Rsbdeac8Cwq7T5YKUxMQ3XU2L0cCe5FGiYHO6Z7EUnull_main";
        let data = ScheduleListRequest {};
        let schedule_list = ScheduleList::call(castgc, &data).await?;
        println!("ScheduleList API Response: {:?}", schedule_list);
        Ok(())
    }
}
