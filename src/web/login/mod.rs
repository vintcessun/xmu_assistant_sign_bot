pub mod expose;
pub mod task;

use axum::{Router, routing::get};

pub use expose::*;
pub use task::*;

pub fn login_router() -> Router {
    let router = Router::new();
    let router = expose::task_router(router);
    main_router(router)
}

fn main_router(router: Router) -> Router {
    router.route("/status", get(status_handler))
}

async fn status_handler() -> &'static str {
    "Web API Login Module Is Running"
}
