mod expose;

use axum::{Router, routing::get};

use crate::web::URL;

pub fn rollcall_router() -> Router {
    let router = Router::new();
    let router = expose::task_router(router);
    main_router(router)
}

fn main_router(router: Router) -> Router {
    router.route("/rollcall", get(status_handler))
}

async fn status_handler() -> &'static str {
    "Web API Rollcall Module Is Running"
}

pub fn get_url() -> &'static str {
    const_format::formatcp!("{}/rollcall/info", URL)
}
