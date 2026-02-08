use super::JwAPI;
use crate::abi::utils::SmartJsonExt;
use crate::api::network::SessionClient;
use anyhow::Result;
use anyhow::bail;
use helper::castgc_client_helper;
use helper::jw_api;
use serde::{Deserialize, Serialize};

#[jw_api(
    url = "https://jw.xmu.edu.cn/jwapp/sys/zzygl/modules/xszzysq/cxxszzybmsq.do",
    app = "https://jw.xmu.edu.cn/appShow?appId=4939740894443498"
)]
pub struct Zzy {
    pub xznj_display: String, // 学制年级显示
    pub sfzzlq: String,       // 是否最终录取
    pub sqyx_display: String, // 申请院系显示
                              // lqzt: IgnoredAny,           // 录取状态
                              // yxdm_display: IgnoredAny,   // 院系代码显示
                              // pclbdm_display: IgnoredAny, // 批次类别代码显示
                              // lqzt_display: IgnoredAny,   // 录取状态显示
                              // zyfxdm: IgnoredAny,         // 专业方向代码
                              // zczy: IgnoredAny,           // 专业主业
                              // zcnj: IgnoredAny,           // 注册年级
                              // czsj: IgnoredAny,           // 操作时间
                              // sqzy_display: IgnoredAny,   // 申请专业显示
                              // sqsj: IgnoredAny,           // 申请时间
                              // zbdm_display: IgnoredAny,   // 招办代码显示
                              // kskssj: IgnoredAny,         // 考试开始时间
                              // xznj: IgnoredAny,           // 学制年级
                              // zbdm: IgnoredAny,           // 招办代码
                              // zcbj: IgnoredAny,           // 注册标记
                              // pcdm: IgnoredAny,           // 批次代码
                              //ggkcj: IgnoredAny,          // 公共课成绩
                              //msjssj: IgnoredAny,         // 面试结束时间
                              //zcyx: IgnoredAny,           // 专业意向
                              //zydm: IgnoredAny,           // 专业代码
                              //pclbdm: IgnoredAny,         // 批次类别代码
                              //zylcxbzt: IgnoredAny,       // 专业录取选报状态
                              //by10: IgnoredAny,           // 备用10
                              //czrxm: IgnoredAny,          // 操作人姓名
                              //sfsx_display: IgnoredAny,   // 是否生效显示
                              //bzsm: IgnoredAny,           // 备注说明
                              //zyshzt: IgnoredAny,         // 专业审核状态
                              //wid: IgnoredAny,            // 唯一ID
                              //zyfxdm_display: IgnoredAny, // 专业方向代码显示
                              //czr: IgnoredAny,            // 操作人
                              //by2: IgnoredAny,            // 备用2
                              //sqzy: IgnoredAny,           // 申请专业
                              //by1: IgnoredAny,            // 备用1
                              //ydxnxq: IgnoredAny,         // 已读学年学期
                              //by4: IgnoredAny,            // 备用4
                              //sqjg: IgnoredAny,           // 申请结果
                              //by3: IgnoredAny,            // 备用3
                              //sqzt: IgnoredAny,           // 申请状态
                              //by6: IgnoredAny,            // 备用6
                              //lqsj: IgnoredAny,           // 录取时间
                              //by5: IgnoredAny,            // 备用5
                              //zcj: IgnoredAny,            // 总成绩
                              //by8: IgnoredAny,            // 备用8
                              //yxdm: IgnoredAny,           // 院系代码
                              //bjmc: IgnoredAny,           // 班级名称
                              //czip: IgnoredAny,           // 操作IP
                              //by7: IgnoredAny,            // 备用7
                              //rzlbdm: IgnoredAny,         // 入至类别代码
                              //by9: IgnoredAny,            // 备用9
                              //fj: IgnoredAny,             // 附加
                              //zyxh: IgnoredAny,           // 专业学号
                              //xsbmkssj: IgnoredAny,       // 学生报名开始时间
                              //ydlbdm: IgnoredAny,         // 已读类别代码
                              //sqlx: IgnoredAny,           // 申请类型
                              //sqly: IgnoredAny,           // 申请理由
                              //zykcj: IgnoredAny,          // 专业课成绩
                              //bjdm: IgnoredAny,           // 班级代码
                              //ydxnxq_display: IgnoredAny, // 已读学年学期显示
                              //tsxslx: IgnoredAny,         // 特殊学生类型
                              //sqyx: IgnoredAny,           // 申请院系
                              //sfdsq: IgnoredAny,          // 是否待申请
                              //sfsx: IgnoredAny,           // 是否生效
                              //zylb: IgnoredAny,           // 专业类别
                              //zydm_display: IgnoredAny,   // 专业代码显示
                              //ksjssj: IgnoredAny,         // 考试结束时间
                              //pcdm_display: IgnoredAny,   // 批次代码显示
                              //xsbmjssj: IgnoredAny,       // 学生报名结束时间
                              //sqzyfx: IgnoredAny,         // 申请专业方向
                              //xh: IgnoredAny,             // 学号
                              //kch: IgnoredAny,            // 课程号
                              //kcm: IgnoredAny,            // 课程名
                              //orderfilter: IgnoredAny,    // 排序过滤
                              //xm: IgnoredAny,             // 姓名
                              //sfzzlq_display: IgnoredAny, // 是否最终录取显示
                              //lxfs: IgnoredAny,           // 联系方式
                              //pm: IgnoredAny,             // 排名
                              //mskssj: IgnoredAny,         // 面试开始时间
}

impl Zzy {
    pub fn get_profile(self) -> Result<ZzyProfile> {
        let mut rows = self.datas.cxxszzybmsq.rows;

        if rows.is_empty() {
            bail!("未查询到转专业信息");
        }

        let first_row = rows.remove(0);
        let entry_year = first_row.xznj_display;

        let mut trans_dept: Vec<String> = rows
            .into_iter()
            .filter(|item| item.sfzzlq == "1")
            .map(|item| item.sqyx_display)
            .collect();

        if first_row.sfzzlq == "1" {
            trans_dept.insert(0, first_row.sqyx_display);
        }

        Ok(ZzyProfile {
            entry_year,
            trans_dept,
        })
    }

    #[castgc_client_helper]
    pub async fn get_from_client(client: &SessionClient, student_id: &str) -> Result<Self> {
        Self::call_client(
            client,
            &ZzyRequest {
                batch_code: "01",
                student_id,
                tag: "-CZSJ,+ZYXH",
                page_size: 10,
                page_number: 1,
            },
        )
        .await
    }
}

#[derive(Debug)]
pub struct ZzyProfile {
    pub entry_year: String,
    pub trans_dept: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct ZzyRequest<'a> {
    #[serde(rename = "PCLBDM")]
    batch_code: &'static str, // 批次类别代码
    #[serde(rename = "XH")]
    student_id: &'a str, // 学号
    #[serde(rename = "*order")]
    tag: &'static str, // 标签
    #[serde(rename = "pageSize")]
    page_size: usize, // 每页大小
    #[serde(rename = "pageNumber")]
    page_number: usize, // 页码
}

#[cfg(test)]
mod tests {

    use super::*;
    use anyhow::Result;

    #[tokio::test]
    async fn test() -> Result<()> {
        let castgc = "TGT-2435869-O8Wwbqik8mV2AiaFWm2RKkKG8nq1zARLvjuN2XWuYtBMaXNrSUaZDng4bJZj-3FfQrsnull_main";
        let data = ZzyRequest {
            batch_code: "01",
            student_id: "13720192200474",
            tag: "-CZSJ,+ZYXH",
            page_size: 10,
            page_number: 1,
        };
        let zzy_api = Zzy::call(castgc, &data).await?;
        println!("Zzy API Response: {:?}", zzy_api);
        let profile = zzy_api.get_profile()?;
        println!("Zzy Profile: {:?}", profile);
        Ok(())
    }
}
