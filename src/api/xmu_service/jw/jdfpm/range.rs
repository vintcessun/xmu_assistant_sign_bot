use super::JwAPI;
use crate::{abi::utils::SmartJsonExt, api::network::SessionClient};
use anyhow::Result;
use helper::{castgc_client_helper, jw_api};
use serde::{Deserialize, Serialize};

#[jw_api(
    url = "https://jw.xmu.edu.cn/jwapp/sys/jdfpm/modules/jdjs/cxxskxcjfw.do",
    app = "https://jw.xmu.edu.cn/appShow?appId=4767629148181062"
)]
pub struct GpaRange {
    pub wid: String,                 // 成绩范围唯一ID（申请绩点计算时的 CJFWWID）
    pub xsmc: String,                // 显示名称，如“自入学以来 含校选”
    pub jsxnxq: String,              // 结束学年学期
    pub jsxnxqdm: Option<String>,    // 结束学年学期代码（“自入学以来”为 null）
    pub ksxnxqdm: Option<String>,    // 开始学年学期代码（“自入学以来”为 null）
    pub sfxgxk: String,              // 是否含校选（"1"/"0"）
    pub sfxgxk_display: String,      // 是否含校选显示（"是"/"否"）
}

impl GpaRange {
    #[castgc_client_helper]
    pub async fn get_from_client(client: &SessionClient) -> Result<Vec<GpaRangeResponse>> {
        let list = Self::call_client(client, &GpaRangeRequest {}).await?;
        list.ensure_ok()?;
        Ok(list.datas.cxxskxcjfw.rows)
    }
}

/// 该接口的请求体为空（浏览器抓包里 body 为 null）。
#[derive(Serialize, Debug)]
pub struct GpaRangeRequest {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "需要有效 CASTGC(TGT)，网络+凭证依赖，手动运行:                 XMU_TEST_CASTGC=TGT-... cargo test -- --ignored"]
    async fn test() -> Result<()> {
        let castgc = super::super::test_castgc();
        let rows = GpaRange::get(&castgc).await?;
        println!("GpaRange rows: {:#?}", rows);
        Ok(())
    }

    #[test]
    fn parse_captured_response() -> Result<()> {
        // 抓包样本：注意“自入学以来”两行的 JSXNXQDM / KSXNXQDM 为 null。
        let raw = r#"{"code":"0","datas":{"cxxskxcjfw":{"totalSize":2,"pageSize":1000,"rows":[
            {"SFXGXK":"1","WID":"67ba8c883d3b43c2bb43bffc84ecc78b","XSMC":"2025-2026学年 第一学期至2025-2026学年 第三学期 含校选","JSXNXQ":"2025-2026学年 第一学期至2025-2026学年 第三学期","JSXNXQDM":"2025-2026-3","KSXNXQDM":"2025-2026-1","SFXGXK_DISPLAY":"是"},
            {"SFXGXK":"1","WID":"67465a77314b4110a3a35901165ba41f","XSMC":"自入学以来 含校选","JSXNXQ":"自入学以来","JSXNXQDM":null,"KSXNXQDM":null,"SFXGXK_DISPLAY":"是"}]}}}"#;

        let parsed: GpaRange = serde_json::from_str(raw)?;
        parsed.ensure_ok()?;
        let rows = parsed.datas.cxxskxcjfw.rows;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].wid, "67ba8c883d3b43c2bb43bffc84ecc78b");
        assert_eq!(rows[1].jsxnxqdm, None);
        assert_eq!(rows[1].xsmc, "自入学以来 含校选");
        Ok(())
    }
}
