//! 绩点分排名查询页：受口令保护地列出成绩范围、申请绩点计算、
//! 下载绩点证明 PDF 并把正文回显到网页。
//!
//! 口令由 [`crate::web::guard`] 模块负责，本模块只在每个接口入口校验访问令牌。

mod expose;
pub mod task;

use axum::{Router, routing::get};

pub fn jdfpm_router() -> Router {
    let router = Router::new();
    let router = expose::task_router(router);
    main_router(router)
}

fn main_router(router: Router) -> Router {
    router.route("/status", get(status_handler))
}

async fn status_handler() -> &'static str {
    "Web API Jdfpm Module Is Running"
}

#[cfg(test)]
mod tests {
    use super::jdfpm_router;
    use crate::web::guard::guard_router;
    use axum::Router;

    /// 路由冲突（如 `/{id}` 与 `/status` 撞车）在 axum 里是构造期 panic，
    /// 会直接打挂整个 Web 服务，所以在测试里先把路由装配一遍。
    #[test]
    fn routers_assemble_without_conflict() {
        let _router: Router = Router::new()
            .nest("/jdfpm", jdfpm_router())
            .nest("/guard", guard_router());
    }
}
