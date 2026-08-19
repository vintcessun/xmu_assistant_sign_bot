//! 绩点分排名（jdfpm）：查询成绩范围、申请绩点计算、读取计算结果、下载绩点证明 PDF。
//!
//! 对应教务应用 <https://jw.xmu.edu.cn/appShow?appId=4767629148181062>。

mod apply;
mod certificate;
mod range;
mod record;

pub use apply::*;
pub use certificate::*;
pub use range::*;
pub use record::*;

use super::JwAPI;

/// 该应用的入口页；`jw_api` 会先 GET 它换取 jwapp 会话再打业务接口。
pub const JDFPM_APP: &str = "https://jw.xmu.edu.cn/appShow?appId=4767629148181062";
