//! 访问口令模块：给需要保护的网页加一道“申请口令 → 输入口令 → 换取访问令牌”的门。
//!
//! 口令有两种来源：调用方自己指定，或由本模块随机生成（[`PasswordMode`]）。
//! 口令只以「随机盐 + 多轮 SHA-256」的摘要形式留在内存里，明文只在创建时返回一次。
//! 验证成功会轮换访问令牌，因此同一时刻只有最后一次输入口令的人能继续访问页面。

mod expose;
pub mod task;

use axum::{Router, routing::get};

pub use expose::{GuardToken, require};
pub use task::*;

pub fn guard_router() -> Router {
    let router = Router::new();
    let router = expose::task_router(router);
    main_router(router)
}

fn main_router(router: Router) -> Router {
    router.route("/status", get(status_handler))
}

async fn status_handler() -> &'static str {
    "Web API Guard Module Is Running"
}
