use super::JwAPI;
use crate::{abi::utils::SmartJsonExt, api::network::SessionClient};
use anyhow::{Result, bail};
use helper::{castgc_client_helper, jw_api};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// 申请一次绩点计算。成功时 `datas.addJdjssq` 是 `null`，
/// 重复申请时教务改用 `{"code":"1","msg":"..."}` 回复，两种形态都由宏统一处理。
#[jw_api(
    url = "https://jw.xmu.edu.cn/jwapp/sys/jdfpm/api/jdjs/addJdjssq.do",
    app = "https://jw.xmu.edu.cn/appShow?appId=4767629148181062",
    auto_row = false
)]
pub struct GpaApply {}

/// 申请结果：新建了一次计算，还是教务判定“已有有效结果”。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpaApplyOutcome {
    /// 本次真的提交了新的计算申请。
    Created,
    /// 该成绩范围已有有效结果，无需重复申请（同样可以直接去取结果）。
    AlreadyExists,
}

impl GpaApplyOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "已提交绩点计算申请",
            Self::AlreadyExists => "教务不允许重算：上一次的计算结果仍在有效期内，本次沿用",
        }
    }
}

impl GpaApply {
    #[castgc_client_helper]
    pub async fn submit_from_client(
        client: &SessionClient,
        range_wid: &str,
        range_name: &str,
    ) -> Result<GpaApplyOutcome> {
        debug!(range_wid = range_wid, range_name = range_name, "提交绩点计算申请");
        let resp = Self::call_client(
            client,
            &GpaApplyRequest {
                range_name,
                range_wid,
            },
        )
        .await?;

        match resp.error_message() {
            None => {
                info!(range_wid = range_wid, "绩点计算申请提交成功");
                Ok(GpaApplyOutcome::Created)
            }
            // 教务把“重复申请”当成错误码返回，但对调用方而言结果同样可用。
            Some(msg) if msg.contains("无需重复申请") || msg.contains("已有有效") => {
                info!(range_wid = range_wid, msg = msg, "绩点计算结果已存在，跳过申请");
                Ok(GpaApplyOutcome::AlreadyExists)
            }
            Some(msg) => bail!("申请绩点计算失败: {msg}"),
        }
    }
}

#[derive(Serialize, Debug)]
pub struct GpaApplyRequest<'a> {
    #[serde(rename = "CJFWWID_DISPLAY")]
    range_name: &'a str, // 成绩范围显示名称
    #[serde(rename = "CJFWWID")]
    range_wid: &'a str, // 成绩范围唯一ID
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_success_response() -> Result<()> {
        // 成功时 datas.addJdjssq 是显式 null。
        let raw = r#"{ "code": "0", "datas": { "addJdjssq": null } }"#;
        let parsed: GpaApply = serde_json::from_str(raw)?;
        assert!(parsed.is_ok());
        assert_eq!(parsed.error_message(), None);
        Ok(())
    }

    #[test]
    fn parse_duplicate_response() -> Result<()> {
        // 重复申请时整个 datas 字段都不存在。
        let raw = r#"{ "msg": "该成绩范围已有有效的绩点计算结果，无需重复申请！", "code": "1" }"#;
        let parsed: GpaApply = serde_json::from_str(raw)?;
        assert!(!parsed.is_ok());
        assert!(parsed.error_message().unwrap().contains("无需重复申请"));
        assert!(parsed.ensure_ok().is_err());
        Ok(())
    }
}
