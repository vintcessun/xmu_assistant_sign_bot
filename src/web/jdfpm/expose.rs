use crate::{
    api::xmu_service::jw::{
        CertificateFacts, GpaApply, GpaCertificate, GpaRange, GpaRangeResponse, GpaRecord,
        GpaRecordResponse, extract_facts,
    },
    web::{
        guard::{GuardToken, require},
        jdfpm::task::{CachedCertificate, JdfpmSession, get_session},
    },
};
use axum::{
    Json, Router,
    extract::{Path, Query},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tracing::{info, trace, warn};

include!(concat!(env!("OUT_DIR"), "/web_data.rs"));

/// 提交申请后等待教务算出结果的轮询次数与间隔。
const RESULT_POLL_TRIES: u32 = 3;
const RESULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Deserialize)]
struct SessionPath {
    id: String,
}

#[derive(Deserialize)]
struct QueryRequest {
    /// 成绩范围唯一ID（CJFWWID）
    range_wid: String,
    /// 成绩范围显示名称（CJFWWID_DISPLAY）
    range_name: String,
}

#[derive(Deserialize)]
struct CertificateRequest {
    /// 计算结果唯一ID（GpaRecordDetail::wid）
    wid: String,
    #[serde(default = "default_show_rank")]
    show_rank: bool,
}

#[derive(Deserialize)]
struct PdfQuery {
    wid: String,
}

const fn default_show_rank() -> bool {
    true
}

#[derive(Serialize)]
struct SessionInfo {
    qq: i64,
    seconds_left: u64,
}

#[derive(Serialize)]
struct RangeView {
    wid: String,
    name: String,
    semester: String,
    with_elective: bool,
}

#[derive(Serialize)]
struct DetailView {
    wid: String,
    range_name: String,
    gpa: String,
    weighted_average: String,
    arithmetic_average: String,
    gpa_rank: String,
    weighted_rank: String,
    arithmetic_rank: String,
    major_size: String,
    course_count: f64,
    data_deadline: String,
    rank_visible: bool,
}

#[derive(Serialize)]
struct RecordView {
    apply_time: String,
    expire_time: String,
    valid: bool,
    expired: bool,
    detail: Option<DetailView>,
}

#[derive(Serialize)]
struct QueryResponse {
    message: String,
    record: RecordView,
}

#[derive(Serialize)]
struct CertificateResponse {
    wid: String,
    /// 证明 PDF 的完整正文
    text: String,
    /// 从正文里抽出的关键字段（排名等只有证明里才有的信息）
    facts: CertificateFacts,
    pdf_url: String,
}

/// 教务侧调用失败：区别于本地 4xx，用 502 表达“上游出错”。
fn upstream(context: &str, e: anyhow::Error) -> Response {
    warn!(context = context, error = ?e, "教务接口调用失败");
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({ "detail": format!("{context}: {e}") })),
    )
        .into_response()
}

fn session_gone() -> Response {
    (
        StatusCode::GONE,
        Json(serde_json::json!({ "detail": "查询会话不存在或已过期，请重新发送 /jdpm" })),
    )
        .into_response()
}

/// 会话 + 令牌双重校验：两者都通过才返回会话。
///
/// 错误分支装箱：`axum::Response` 体积较大，直接放进 `Result` 会让每个调用点都背上这份开销。
fn authorize(id: &str, token: &GuardToken) -> Result<Arc<JdfpmSession>, Box<Response>> {
    let session = get_session(id).ok_or_else(|| Box::new(session_gone()))?;
    require(id, &token.0).map_err(|e| Box::new(e.into_response()))?;
    Ok(session)
}

async fn page_handler(Path(params): Path<SessionPath>) -> impl IntoResponse {
    trace!(session_id = params.id, "请求绩点查询页面");
    match get_session(&params.id) {
        Some(session) => Html(render_page(&session)).into_response(),
        None => (StatusCode::NOT_FOUND, Html(NOT_FOUND_HTML)).into_response(),
    }
}

fn render_page(session: &Arc<JdfpmSession>) -> String {
    JDFPM_HTML
        .replace("__QQ_ID__", &session.qq.to_string())
        .replace(
            "__SESSION_ID_JS__",
            &serde_json::to_string(&session.id).unwrap_or_else(|_| "\"\"".into()),
        )
}

/// 解锁后拉取会话基本信息，用于页面标题栏。
async fn info_handler(Path(params): Path<SessionPath>, token: GuardToken) -> Response {
    let session = match authorize(&params.id, &token) {
        Ok(session) => session,
        Err(resp) => return *resp,
    };

    Json(SessionInfo {
        qq: session.qq,
        seconds_left: session.seconds_left(),
    })
    .into_response()
}

/// 可申请的成绩范围列表。
async fn ranges_handler(Path(params): Path<SessionPath>, token: GuardToken) -> Response {
    let session = match authorize(&params.id, &token) {
        Ok(session) => session,
        Err(resp) => return *resp,
    };

    match GpaRange::get_from_client(&session.client).await {
        Ok(rows) => Json(rows.iter().map(to_range_view).collect::<Vec<_>>()).into_response(),
        Err(e) => upstream("获取成绩范围失败", e),
    }
}

/// 历史申请记录。
async fn records_handler(Path(params): Path<SessionPath>, token: GuardToken) -> Response {
    let session = match authorize(&params.id, &token) {
        Ok(session) => session,
        Err(resp) => return *resp,
    };

    match GpaRecord::get_from_client(&session.client).await {
        Ok(rows) => Json(rows.iter().map(to_record_view).collect::<Vec<_>>()).into_response(),
        Err(e) => upstream("获取绩点计算记录失败", e),
    }
}

/// 查询指定成绩范围的绩点：已有有效结果就直接取，否则先申请再轮询。
async fn query_handler(
    Path(params): Path<SessionPath>,
    token: GuardToken,
    Json(payload): Json<QueryRequest>,
) -> Response {
    let session = match authorize(&params.id, &token) {
        Ok(session) => session,
        Err(resp) => return *resp,
    };

    let rows = match GpaRecord::get_from_client(&session.client).await {
        Ok(rows) => rows,
        Err(e) => return upstream("获取绩点计算记录失败", e),
    };

    // 已有有效结果时不再重复申请（教务对重复申请是直接报错的）。
    if let Some(row) = GpaRecord::find_valid(&rows, &payload.range_wid) {
        return Json(QueryResponse {
            message: "已有有效的绩点计算结果".to_owned(),
            record: to_record_view(row),
        })
        .into_response();
    }

    let outcome = match GpaApply::submit_from_client(
        &session.client,
        &payload.range_wid,
        &payload.range_name,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(e) => return upstream("申请绩点计算失败", e),
    };
    info!(session_id = %session.id, range_wid = %payload.range_wid, outcome = ?outcome, "绩点计算申请完成");

    // 教务算完结果需要一点时间，轮询几次再放弃。
    for attempt in 1..=RESULT_POLL_TRIES {
        tokio::time::sleep(RESULT_POLL_INTERVAL).await;
        let rows = match GpaRecord::get_from_client(&session.client).await {
            Ok(rows) => rows,
            Err(e) => return upstream("获取绩点计算记录失败", e),
        };
        if let Some(row) = GpaRecord::find_valid(&rows, &payload.range_wid) {
            return Json(QueryResponse {
                message: outcome.as_str().to_owned(),
                record: to_record_view(row),
            })
            .into_response();
        }
        trace!(attempt = attempt, "绩点计算结果尚未生成，继续等待");
    }

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "detail": "教务仍在计算，请稍后重新查询" })),
    )
        .into_response()
}

/// 下载绩点证明 PDF 并提取正文；PDF 本身缓存在会话里供页面下载。
async fn certificate_handler(
    Path(params): Path<SessionPath>,
    token: GuardToken,
    Json(payload): Json<CertificateRequest>,
) -> Response {
    let session = match authorize(&params.id, &token) {
        Ok(session) => session,
        Err(resp) => return *resp,
    };

    let cached = match session.cached_certificate(&payload.wid) {
        Some(cached) => cached,
        None => {
            let pdf = match GpaCertificate::download_from_client(
                &session.client,
                &payload.wid,
                payload.show_rank,
            )
            .await
            {
                Ok(pdf) => pdf,
                Err(e) => return upstream("下载绩点证明失败", e),
            };

            let text = match GpaCertificate::extract_text(pdf.clone()).await {
                Ok(text) => text,
                Err(e) => return upstream("提取绩点证明正文失败", e),
            };

            let cached = CachedCertificate {
                wid: payload.wid.clone(),
                pdf,
                text,
            };
            session.cache_certificate(cached.clone());
            cached
        }
    };

    Json(CertificateResponse {
        facts: extract_facts(&cached.text),
        pdf_url: format!(
            "/jdfpm/{}/certificate.pdf?wid={}&token={}",
            session.id,
            urlencoding::encode(&cached.wid),
            urlencoding::encode(&token.0)
        ),
        wid: cached.wid,
        text: cached.text,
    })
    .into_response()
}

/// 直接下载已缓存的 PDF。浏览器导航加不了请求头，令牌走查询串。
async fn pdf_handler(
    Path(params): Path<SessionPath>,
    Query(query): Query<PdfQuery>,
    token: GuardToken,
) -> Response {
    let session = match authorize(&params.id, &token) {
        Ok(session) => session,
        Err(resp) => return *resp,
    };

    let Some(cached) = session.cached_certificate(&query.wid) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "detail": "该证明尚未生成，请先在页面上查询一次" })),
        )
            .into_response();
    };

    Response::builder()
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"gpa-{}.pdf\"", cached.wid),
        )
        .body(axum::body::Body::from(cached.pdf))
        .unwrap_or_default()
}

fn to_range_view(row: &GpaRangeResponse) -> RangeView {
    RangeView {
        wid: row.wid.clone(),
        name: row.xsmc.clone(),
        semester: row.jsxnxq.clone(),
        with_elective: row.sfxgxk == "1",
    }
}

fn to_record_view(row: &GpaRecordResponse) -> RecordView {
    RecordView {
        apply_time: row.sqsj.clone(),
        expire_time: row.sxsj.clone(),
        valid: row.sfyx,
        expired: row.sfsx,
        detail: row.zx.as_ref().map(|zx| DetailView {
            wid: zx.wid.clone(),
            range_name: zx.cjfwmc.clone(),
            gpa: zx.gpa.clone(),
            weighted_average: zx.jqpjf.clone(),
            arithmetic_average: zx.sspjf.clone(),
            gpa_rank: zx.zygpapm.clone(),
            weighted_rank: zx.zyjqpjfpm.clone(),
            arithmetic_rank: zx.zysspjfpm.clone(),
            major_size: zx.cyjszyrs.clone(),
            course_count: zx.cyjscjms,
            data_deadline: zx.jssj.clone(),
            rank_visible: zx.sfxspm,
        }),
    }
}

pub fn task_router(router: Router) -> Router {
    router
        // 查询页面（锁屏 + 数据都在这一页）
        .route("/{id}", get(page_handler))
        // 已缓存证明的 PDF 下载
        .route("/{id}/certificate.pdf", get(pdf_handler))
        // 会话基本信息
        .route("/api/{id}/info", get(info_handler))
        // 可选的成绩范围
        .route("/api/{id}/ranges", get(ranges_handler))
        // 历史申请记录
        .route("/api/{id}/records", get(records_handler))
        // 申请并查询指定成绩范围的绩点
        .route("/api/{id}/query", post(query_handler))
        // 生成绩点证明并提取正文
        .route("/api/{id}/certificate", post(certificate_handler))
}
