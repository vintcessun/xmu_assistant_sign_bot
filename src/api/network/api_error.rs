//! 接口返回非 2xx 时的类型化错误。
//!
//! 以前这类失败只是一句 `anyhow!("HTTP Error: API returned status {} ...")`，
//! 调用方拿到的是纯字符串，没法区分「重试有用」和「重试多少次都没用」，
//! 于是 403（没权限）也要陪着重试三次，最后还报一句听起来像网络抖动的
//! "多次尝试后下载失败"。现在把状态码带出来，调用方可以自己判断。

use std::fmt;

/// 接口以非 2xx 状态码拒绝了请求。
#[derive(Debug, Clone)]
pub struct ApiStatusError {
    pub status: reqwest::StatusCode,
    pub url: String,
}

impl ApiStatusError {
    pub fn new(status: reqwest::StatusCode, url: impl Into<String>) -> Self {
        Self {
            status,
            url: url.into(),
        }
    }

    /// 再试多少次都不会变的失败。这类错误应当立刻放弃并如实告诉用户原因。
    ///
    /// 只挑确定性的几个：401/403 是权限问题，404/410 是东西不存在，
    /// 429（限流）和 5xx 反而值得重试，所以不算在内。
    pub fn is_permanent(&self) -> bool {
        matches!(
            self.status,
            reqwest::StatusCode::UNAUTHORIZED
                | reqwest::StatusCode::FORBIDDEN
                | reqwest::StatusCode::NOT_FOUND
                | reqwest::StatusCode::GONE
        )
    }

    /// 给用户看的原因，说人话。
    pub fn user_reason(&self) -> &'static str {
        match self.status {
            reqwest::StatusCode::UNAUTHORIZED => "登录已失效",
            reqwest::StatusCode::FORBIDDEN => "没有访问权限",
            reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE => "文件不存在或已被删除",
            _ => "服务器拒绝了请求",
        }
    }
}

impl fmt::Display for ApiStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 保持与改动前一致的措辞，日志/排查习惯不用跟着变。
        write!(
            f,
            "HTTP Error: API returned status {} for URL: {}",
            self.status, self.url
        )
    }
}

impl std::error::Error for ApiStatusError {}

/// 从任意错误链里找出接口状态错误。
pub fn api_status_of(error: &anyhow::Error) -> Option<&ApiStatusError> {
    error.downcast_ref::<ApiStatusError>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn permanent_statuses_are_not_worth_retrying() {
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::NOT_FOUND,
            reqwest::StatusCode::GONE,
        ] {
            assert!(ApiStatusError::new(status, "u").is_permanent(), "{status}");
        }
    }

    /// 限流和服务端故障是会自己好的，必须继续重试。
    #[test]
    fn transient_statuses_still_retry() {
        for status in [
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            reqwest::StatusCode::BAD_GATEWAY,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            reqwest::StatusCode::GATEWAY_TIMEOUT,
        ] {
            assert!(!ApiStatusError::new(status, "u").is_permanent(), "{status}");
        }
    }

    /// 线上那批「专题报告集锦」下载失败就是这个：403 + 您没有权限完成此操作。
    #[test]
    fn forbidden_reads_as_permission_problem() {
        let e = ApiStatusError::new(reqwest::StatusCode::FORBIDDEN, "https://lnt/x");
        assert_eq!(e.user_reason(), "没有访问权限");
        assert!(e.to_string().contains("403"));
    }

    #[test]
    fn status_survives_anyhow_context() {
        let err = anyhow::Error::new(ApiStatusError::new(
            reqwest::StatusCode::FORBIDDEN,
            "https://lnt/x",
        ))
        .context("取下载地址失败");

        let found = api_status_of(&err).expect("套了 context 之后仍然要能取出状态码");
        assert_eq!(found.status, reqwest::StatusCode::FORBIDDEN);
    }

    #[test]
    fn unrelated_errors_have_no_status() {
        assert!(api_status_of(&anyhow!("随便什么错误")).is_none());
    }
}
