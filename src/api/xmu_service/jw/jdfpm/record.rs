use super::JwAPI;
use crate::{abi::utils::SmartJsonExt, api::network::SessionClient};
use anyhow::Result;
use helper::{castgc_client_helper, jw_api};
use serde::{Deserialize, Serialize};

/// 绩点计算申请记录。与其它教务接口不同，`datas.getJdjssq` 直接是数组，
/// 因此用 `list = true` 让宏跳过 `{rows: [...]}` 那层包装。
#[jw_api(
    url = "https://jw.xmu.edu.cn/jwapp/sys/jdfpm/api/jdjs/getJdjssq.do",
    app = "https://jw.xmu.edu.cn/appShow?appId=4767629148181062",
    list = true
)]
pub struct GpaRecord {
    pub sqsj: String,    // 申请时间
    pub sxsj: String,    // 失效时间
    pub sqjlwid: String, // 申请记录唯一ID
    pub sfyx: bool,      // 是否有效
    pub sfsx: bool,      // 是否已失效
    #[serde(default)]
    pub zx: Option<GpaRecordDetail>, // 计算明细；结果尚未生成时可能缺失
}

/// `getJdjssq` 每条记录里的 `ZX` 明细：绩点、均分、排名与打印证明所需的 `WID`。
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "UPPERCASE", default)]
pub struct GpaRecordDetail {
    pub wid: String,       // 计算结果唯一ID（打印绩点证明用的 wid）
    pub sqjlwid: String,   // 申请记录唯一ID
    pub xh: String,        // 学号
    pub cjfwwid: String,   // 成绩范围唯一ID
    pub cjfwmc: String,    // 成绩范围名称
    pub gpa: String,       // 平均学分绩点
    pub jqpjf: String,     // 加权平均分
    pub sspjf: String,     // 算术平均分
    // 排名未公开时教务给的是 "*"；这三个字段最有可能出现 null，统一退化成空串。
    #[serde(deserialize_with = "crate::abi::utils::null_to_default")]
    pub zygpapm: String, // 专业绩点排名
    #[serde(deserialize_with = "crate::abi::utils::null_to_default")]
    pub zyjqpjfpm: String, // 专业加权平均分排名
    #[serde(deserialize_with = "crate::abi::utils::null_to_default")]
    pub zysspjfpm: String, // 专业算术平均分排名
    pub cyjszyrs: String,  // 参与计算的专业人数
    pub cyjscjms: f64,     // 参与计算的成绩门数
    pub jssj: String,      // 成绩截止日期
    pub sqsj: String,      // 申请时间
    pub sxsj: String,      // 失效时间
    pub sfyx: bool,        // 是否有效
    pub sfzx: String,      // 是否在校
    pub sfxspm: bool,      // 是否显示排名
}

impl GpaRecordResponse {
    /// 打印绩点证明所需的 `wid`（在明细里，而不是 `SQJLWID`）。
    pub fn print_wid(&self) -> Option<&str> {
        self.zx.as_ref().map(|zx| zx.wid.as_str())
    }

    /// 该记录是否是指定成绩范围下当前有效的计算结果。
    pub fn is_valid_for(&self, range_wid: &str) -> bool {
        self.sfyx
            && self
                .zx
                .as_ref()
                .is_some_and(|zx| zx.cjfwwid == range_wid && !zx.wid.is_empty())
    }
}

impl GpaRecord {
    #[castgc_client_helper]
    pub async fn get_from_client(client: &SessionClient) -> Result<Vec<GpaRecordResponse>> {
        let resp = Self::call_client(client, &GpaRecordRequest {}).await?;
        resp.ensure_ok()?;
        Ok(resp.datas.getJdjssq)
    }

    /// 取指定成绩范围下当前有效的计算结果。
    pub fn find_valid<'a>(
        rows: &'a [GpaRecordResponse],
        range_wid: &str,
    ) -> Option<&'a GpaRecordResponse> {
        rows.iter().find(|row| row.is_valid_for(range_wid))
    }
}

/// 该接口的请求体为空（浏览器抓包里 body 为 null）。
#[derive(Serialize, Debug)]
pub struct GpaRecordRequest {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "需要有效 CASTGC(TGT)，网络+凭证依赖，手动运行:                 XMU_TEST_CASTGC=TGT-... cargo test -- --ignored"]
    async fn test() -> Result<()> {
        let castgc = super::super::test_castgc();
        let rows = GpaRecord::get(&castgc).await?;
        println!("GpaRecord rows: {:#?}", rows);
        Ok(())
    }

    #[test]
    fn parse_captured_response() -> Result<()> {
        let raw = r#"{ "code": "0", "datas": { "getJdjssq": [{ "SQSJ": "2026-08-19 11:49:12", "SFYX": true, "SQJLWID": "1e695aa20b714426bda7f18a131e3dd0", "SXSJ": "2026-08-21 00:00:00", "ZX": { "JSSJ": "2026-08-14", "ZYSSPJFPM": "*", "CYJSZYRS": "173", "ZYJQPJFPM": "*", "CJFWMC": "自入学以来 含校选", "SQJLWID": "1e695aa20b714426bda7f18a131e3dd0", "ZYGPAPM": "*", "SFXSPM": false, "XH": "34520242201240", "CJFWWID": "67465a77314b4110a3a35901165ba41f", "SQSJ": "2026-08-19 11:49:12", "WID": "595D0B9C75D637D8E06302431BACC8D8", "SSPJF": "88", "SFYX": true, "SFZX": "1", "GPA": "3.73", "SXSJ": "2026-08-21 00:00:00", "CYJSCJMS": 38.0, "JQPJF": "88.7" }, "SFSX": false }] } }"#;

        let parsed: GpaRecord = serde_json::from_str(raw)?;
        parsed.ensure_ok()?;
        let rows = parsed.datas.getJdjssq;
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_valid_for("67465a77314b4110a3a35901165ba41f"));
        // 打印证明用的是 ZX.WID，而不是 SQJLWID。
        assert_eq!(rows[0].print_wid(), Some("595D0B9C75D637D8E06302431BACC8D8"));

        let zx = rows[0].zx.as_ref().expect("ZX 应存在");
        assert_eq!(zx.gpa, "3.73");
        assert_eq!(zx.cyjscjms, 38.0);
        assert_eq!(zx.cyjszyrs, "173");
        assert!(!zx.sfxspm);
        Ok(())
    }
}
