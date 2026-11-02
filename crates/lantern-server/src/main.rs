//! Single-user loopback web/PWA host. Secrets stay on the Rust side.
mod chain;
mod mongo_runtime;
mod pipeline;
mod research_mirror;
mod scout_runtime;
mod scout_store;

use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use chrono::{Duration, NaiveDate, Utc};
use lantern_connectors::{ChatMessage, Config, Connector};
use lantern_core::{Company, Investigation, Message, Store};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::IpAddr, path::PathBuf, sync::Arc};
use tokio::sync::Semaphore;
use tower_http::compression::CompressionLayer;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct App {
    store: Store,
    connector: Option<Arc<Connector>>,
    provider: Value,
    data_dir: PathBuf,
    allowed_hosts: Arc<Vec<String>>,
    provider_slots: Arc<Semaphore>,
    registry: Option<String>,
    rpc: String,
    scout: Option<Arc<scout_runtime::ScoutRuntime>>,
    mongo: Option<Arc<mongo_runtime::MongoRuntime>>,
    research_mirror: Option<Arc<research_mirror::ResearchMirror>>,
}

#[derive(Debug)]
struct ApiError(StatusCode, String);
type ApiResult<T> = std::result::Result<T, ApiError>;
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"ok":false,"error":self.1}))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        // Store errors contain no provider response body or credentials.
        Self(StatusCode::BAD_REQUEST, e.to_string())
    }
}

fn today() -> String {
    (Utc::now() + Duration::hours(7)).date_naive().to_string()
}
fn now() -> String {
    Utc::now().to_rfc3339()
}
fn bounded(text: &str, max: usize, field: &str) -> ApiResult<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > max {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!("{field}: nhập từ 1 đến {max} ký tự."),
        ));
    }
    Ok(text.to_string())
}
fn valid_date(value: Option<&str>) -> ApiResult<Option<String>> {
    let Some(value) = value.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "Ngày phải có dạng YYYY-MM-DD.".into(),
        )
    })?;
    if date.to_string().as_str() > today().as_str() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Không thể biết dữ liệu của ngày tương lai.".into(),
        ));
    }
    Ok(Some(date.to_string()))
}
fn connector(app: &App) -> ApiResult<Arc<Connector>> {
    app.connector.clone().ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE,
        "Chưa có cấu hình AI hợp lệ. Cấu hình file riêng phía máy chủ; không nhập API key trong chat.".into()))
}
fn add_message(app: &App, role: &str, content: &str) -> Result<()> {
    app.store.save_message(&Message {
        id: Uuid::new_v4().to_string(),
        role: role.into(),
        content: content.chars().take(12_000).collect(),
        created_at: now(),
    })
}

#[derive(Deserialize, Default)]
struct ViewQuery {
    view: Option<String>,
    selected: Option<String>,
    ids: Option<String>,
    q: Option<String>,
    as_of: Option<String>,
}
fn snapshot(app: &App, q: &ViewQuery) -> Result<Value> {
    let as_of = valid_date(q.as_of.as_deref())
        .map_err(|e| anyhow::anyhow!(e.1))?
        .unwrap_or_else(today);
    // Company profiles and job state are not yet bitemporal. Do not present
    // today's values as though they were known at an earlier date.
    let historical_unavailable = as_of < today();
    let mut companies = if historical_unavailable {
        vec![]
    } else {
        app.store.list_companies("", 300, 0)?
    };
    if !historical_unavailable {
        let mut seen: std::collections::HashSet<String> =
            companies.iter().map(|c| c.id.clone()).collect();
        if let Some(query) = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            for company in app.store.list_companies(query, 200, 0)? {
                if seen.insert(company.id.clone()) {
                    companies.push(company);
                }
            }
        }
        for id in q
            .selected
            .iter()
            .map(String::as_str)
            .chain(q.ids.as_deref().unwrap_or("").split(',').take(4))
            .filter(|id| !id.is_empty())
        {
            if !seen.contains(id)
                && let Some(company) = app.store.get_company(id)?
            {
                seen.insert(company.id.clone());
                companies.push(company);
            }
        }
    }
    let mut evidence = Vec::new();
    for company in &companies {
        evidence.extend(app.store.list_evidence(&company.id, Some(&as_of))?);
    }
    evidence.truncate(1000);
    let investigations = if historical_unavailable {
        vec![]
    } else {
        app.store.list_investigations(100)?
    };
    let mut events = Vec::new();
    for job in &investigations {
        events.extend(app.store.events(&job.id)?);
    }
    events.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    if events.len() > 500 {
        events.drain(..events.len() - 500);
    }
    let budget = app.store.budget()?;
    let mut config = app.provider.clone();
    config["configured"] = json!(app.connector.is_some());
    config["budget_usd"] = json!(budget.cap_cents as f64 / 100.0);
    config["reserved_usd"] = json!(budget.reserved_cents as f64 / 100.0);
    config["spent_usd"] = json!(budget.charged_cents as f64 / 100.0);
    config["remaining_usd"] = json!(budget.available_cents as f64 / 100.0);
    config["registry_configured"] = json!(app.registry.is_some());
    config["budget_note"] = json!("Hạn mức đặt giữ bảo thủ, không phải hóa đơn của nhà cung cấp.");
    let ids: Vec<&str> = q
        .ids
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .take(4)
        .collect();
    let query = q.q.as_deref().unwrap_or("").trim();
    let needle = lantern_core::normalize_search(query);
    let mut linked_candidates = std::collections::HashSet::new();
    if !query.is_empty() {
        for job in investigations.iter().filter(|j| {
            j.company_id.is_none()
                && j.query.trim() == query
                && matches!(j.status.as_str(), "completed" | "partial")
        }) {
            linked_candidates.extend(app.store.candidate_ids(&job.id)?);
        }
    }
    let filtered: Vec<&Company> = companies
        .iter()
        .filter(|c| {
            needle.is_empty()
                || linked_candidates.contains(&c.id)
                || lantern_core::normalize_search(&format!(
                    "{} {} {}",
                    c.name,
                    c.tax_id.as_deref().unwrap_or(""),
                    c.ticker.as_deref().unwrap_or("")
                ))
                .contains(&needle)
        })
        .collect();
    let mut report_ready_ids = Vec::new();
    for job in &investigations {
        if job.company_id.is_some()
            && matches!(job.status.as_str(), "completed" | "partial")
            && !job.summary.trim().is_empty()
            && app
                .store
                .evidence_for_investigation(&job.id)?
                .iter()
                .any(|item| item.status == "subject_confirmed")
        {
            report_ready_ids.push(job.id.clone());
        }
    }
    let mut reservations = serde_json::to_value(app.store.reservations(100)?)?;
    if let Some(items) = reservations.as_array_mut() {
        for item in items {
            if item["status"] == "reserved"
                && let Some(id) = item["id"].as_str()
                && let Ok(cents) = chat_upper_bound(app, id)
            {
                item["bounded_estimate_cents"] = json!(cents);
            }
        }
    }
    let scout = app
        .scout
        .as_ref()
        .and_then(|s| s.status().ok())
        .unwrap_or_else(|| json!({"configured":false,"enabled":false}));
    let mut storage = app.mongo.as_ref().map(|m| {
        let status = m.status_json();
        let ready = status["ready"].as_bool().unwrap_or(false);
        let failed = status["failed_cluster"].as_str();
        let cluster = |id: &str| if ready {"Đã thiết lập kết nối"} else if failed == Some(id) {"Kết nối không khả dụng"} else {"Chưa xác minh"};
        json!({"configured":status["configured"],"mode":"MongoDB Atlas (SQLite cục bộ)",
            "status":if ready {"Đã thiết lập kết nối"} else {"Chưa sẵn sàng"},
            "clusters":[{"name":"A","status":cluster("a"),"region":null},{"name":"B","status":cluster("b"),"region":null}]})
    }).unwrap_or_else(|| json!({"configured":true,"mode":"SQLite cục bộ","status":"Đang dùng cục bộ","clusters":[]}));
    if let Some(mirror) = &app.research_mirror {
        storage["research_mirror"] = mirror.status().unwrap_or(Value::Null);
    }
    Ok(
        json!({"companies":companies,"search_results":filtered,"search_query":q.q.as_deref().unwrap_or(""),
        "selected_ids":ids,"investigations":investigations,"evidence":evidence,
        "messages":app.store.messages(40)?,"events":events,"sources":app.store.list_sources()?,
        "config":config,"reservations":reservations,"as_of":as_of,"temporal_mode":"system_known",
        "historical_unavailable":historical_unavailable,"report_ready_ids":report_ready_ids,"scout":scout,"storage":storage}),
    )
}
fn checked_view(q: &ViewQuery) -> &str {
    match q.view.as_deref().unwrap_or("overview") {
        v @ ("overview" | "companies" | "dossier" | "compare" | "watchlist" | "jobs"
        | "sources" | "settings") => v,
        _ => "overview",
    }
}
async fn page(State(app): State<App>, Query(q): Query<ViewQuery>) -> ApiResult<Html<String>> {
    Ok(Html(
        lantern_ui::render_page(
            &snapshot(&app, &q)?,
            checked_view(&q),
            q.selected.as_deref(),
        )
        .into_string(),
    ))
}
async fn fragment(State(app): State<App>, Query(q): Query<ViewQuery>) -> ApiResult<Html<String>> {
    Ok(Html(
        lantern_ui::render_workspace(
            &snapshot(&app, &q)?,
            checked_view(&q),
            q.selected.as_deref(),
        )
        .into_string(),
    ))
}
async fn state(State(app): State<App>, Query(q): Query<ViewQuery>) -> ApiResult<Json<Value>> {
    Ok(Json(snapshot(&app, &q)?))
}
async fn health(State(app): State<App>) -> Json<Value> {
    Json(
        json!({"ok":true,"app":"lantern","version":env!("CARGO_PKG_VERSION"),"storage":"sqlite",
        "configured":app.connector.is_some(),"rust_first":true,"network":"local-only"}),
    )
}

async fn boundary(State(app): State<App>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !app.allowed_hosts.iter().any(|h| h == host) {
        return ApiError(
            StatusCode::FORBIDDEN,
            "Máy chủ chỉ nhận địa chỉ loopback được cấu hình.".into(),
        )
        .into_response();
    }
    if request.method() != axum::http::Method::GET && request.method() != axum::http::Method::HEAD {
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|h| h.to_str().ok());
        let cross_site = request
            .headers()
            .get("sec-fetch-site")
            .is_some_and(|h| h == "cross-site");
        let matches =
            origin.is_some_and(|o| app.allowed_hosts.iter().any(|h| o == format!("http://{h}")));
        if cross_site || (origin.is_some() && !matches) {
            return ApiError(
                StatusCode::FORBIDDEN,
                "Yêu cầu khác nguồn bị từ chối.".into(),
            )
            .into_response();
        }
    }
    let path = request.uri().path().to_string();
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), geolocation=(), microphone=(self)"),
    );
    h.insert("content-security-policy", HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; media-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'"));
    if !path.starts_with("/assets/") {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}

#[derive(Deserialize)]
struct SearchInput {
    query: String,
    as_of: Option<String>,
}
#[derive(Deserialize)]
struct InvestigationInput {
    company_id: String,
    query: Option<String>,
    as_of: Option<String>,
}
fn create_job(
    app: &App,
    query: String,
    company_id: Option<String>,
    as_of: Option<String>,
    lookup: bool,
) -> ApiResult<Investigation> {
    if as_of.as_deref().is_some_and(|date| date < today().as_str()) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Chưa có kho lịch sử đầy đủ cho ngày này; chọn ngày hiện tại để tạo nghiên cứu mới."
                .into(),
        ));
    }
    connector(app)?;
    if app.store.budget()?.available_cents < 100 {
        return Err(ApiError(
            StatusCode::PAYMENT_REQUIRED,
            "Đã hết hạn mức đặt giữ cho phiên này; chưa gửi thêm request AI.".into(),
        ));
    }
    let jobs = app.store.list_investigations(100)?;
    if jobs
        .iter()
        .filter(|j| j.status == "queued" || j.status == "running" || j.status == "verifying")
        .count()
        >= 8
    {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "Hàng chờ đã đủ 8 tác vụ. Hãy hủy hoặc chờ tác vụ đang chạy.".into(),
        ));
    }
    let job = Investigation {
        id: Uuid::new_v4().to_string(),
        company_id,
        query,
        as_of,
        status: "queued".into(),
        stage: if lookup {
            "queued_lookup"
        } else {
            "queued_research"
        }
        .into(),
        summary: String::new(),
        limitations: vec![],
        created_at: now(),
        updated_at: now(),
    };
    app.store.create_investigation(&job)?;
    Ok(job)
}
async fn search(State(app): State<App>, Json(input): Json<SearchInput>) -> ApiResult<Json<Value>> {
    let query = bounded(&input.query, 1000, "Tên hoặc mã doanh nghiệp")?;
    let job = create_job(
        &app,
        query.clone(),
        None,
        valid_date(input.as_of.as_deref())?,
        true,
    )?;
    add_message(
        &app,
        "assistant",
        &format!(
            "Tôi đã nhận yêu cầu tìm “{query}”. Tác vụ được lưu; kết quả sẽ hiện ở Doanh nghiệp và Tác vụ."
        ),
    )?;
    Ok(Json(
        json!({"ok":true,"job_id":job.id,"message":"Đã tạo tác vụ tra cứu có lưu tiến độ."}),
    ))
}
async fn investigate(
    State(app): State<App>,
    Json(input): Json<InvestigationInput>,
) -> ApiResult<Json<Value>> {
    let c = app.store.get_company(&input.company_id)?.ok_or_else(|| {
        ApiError(
            StatusCode::NOT_FOUND,
            "Không có doanh nghiệp này trong workspace.".into(),
        )
    })?;
    let requested=input.query.filter(|s|!s.trim().is_empty()).unwrap_or_else(||
        "Thẩm định danh tính, hoạt động, công bố tài chính, pháp lý và thuế; chỉ dữ kiện có nguồn, nêu khoảng trống.".into());
    let query = bounded(&requested, 1400, "Yêu cầu điều tra")?;
    let job = create_job(
        &app,
        query,
        Some(c.id),
        valid_date(input.as_of.as_deref())?,
        false,
    )?;
    Ok(Json(
        json!({"ok":true,"job_id":job.id,"message":"Đã đưa điều tra vào hàng chờ."}),
    ))
}
#[derive(Deserialize)]
struct WatchInput {
    company_id: String,
    watched: bool,
}
async fn watch(State(app): State<App>, Json(input): Json<WatchInput>) -> ApiResult<Json<Value>> {
    app.store.set_watch(&input.company_id, input.watched)?;
    Ok(Json(
        json!({"ok":true,"message":if input.watched{"Đã thêm vào danh sách theo dõi."}else{"Đã bỏ theo dõi."}}),
    ))
}
#[derive(Deserialize)]
struct EvidenceReviewInput {
    decision: String,
}
async fn review_evidence(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<EvidenceReviewInput>,
) -> ApiResult<Json<Value>> {
    Uuid::parse_str(&id).map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "Mã bằng chứng không hợp lệ.".into(),
        )
    })?;
    if !matches!(
        input.decision.as_str(),
        "subject_confirmed" | "not_about_company"
    ) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Lựa chọn rà soát không hợp lệ.".into(),
        ));
    }
    let _mirror_guard = research_mirror::review_guard().await;
    app.store.review_evidence(&id, &input.decision)?;
    Ok(Json(
        json!({"ok":true,"message":"Đã lưu rà soát chủ thể. Việc này không chứng nhận nội dung nguồn hoặc kết luận rủi ro."}),
    ))
}
#[derive(Deserialize)]
struct BudgetLimitInput {
    cents: i64,
    confirmed: bool,
}
async fn budget_limit(
    State(app): State<App>,
    Json(input): Json<BudgetLimitInput>,
) -> ApiResult<Json<Value>> {
    if !input.confirmed {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Xác nhận mức ngân sách mới trước khi lưu.".into(),
        ));
    }
    app.store.set_budget_limit(input.cents)?;
    Ok(Json(
        json!({"ok":true,"message":"Đã lưu giới hạn ngân sách cục bộ. Các khoản đặt giữ và số đã đối chiếu vẫn được tính."}),
    ))
}
#[derive(Deserialize)]
struct BudgetSettlementInput {
    actual_cents: i64,
    confirmed: bool,
}
async fn budget_settle(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<BudgetSettlementInput>,
) -> ApiResult<Json<Value>> {
    Uuid::parse_str(&id)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Mã khoản giữ không hợp lệ.".into()))?;
    if !input.confirmed || !(0..=1_000_000).contains(&input.actual_cents) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Chỉ đối chiếu sau khi xác nhận số tiền từ nhà cung cấp.".into(),
        ));
    }
    app.store.settle_budget(&id, input.actual_cents)?;
    Ok(Json(
        json!({"ok":true,"message":"Đã lưu số tiền bạn đối chiếu; khoản giữ trước đó không còn được tính riêng."}),
    ))
}
fn chat_upper_bound(app: &App, id: &str) -> ApiResult<i64> {
    Uuid::parse_str(id)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Mã khoản giữ không hợp lệ.".into()))?;
    let bytes =
        std::fs::read(app.data_dir.join("usage").join(format!("{id}.json"))).map_err(|_| {
            ApiError(
                StatusCode::NOT_FOUND,
                "Chưa có usage receipt cho khoản này.".into(),
            )
        })?;
    if bytes.len() > 64 * 1024 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Usage receipt không hợp lệ.".into(),
        ));
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(anyhow::Error::from)?;
    if data["reservation_id"] != id
        || data["kind"] != "chat"
        || data["model"] != "gpt-6-luna"
        || data["endpoint_origin"] != "https://api.openai.com"
        || data["usage"]["usage_complete"] != true
        || data["usage"]["no_tool_calls_confirmed"] != true
        || !data["recorded_at"]
            .as_str()
            .is_some_and(|t| t.starts_with("2026-11-01"))
        || data["usage"]["web_calls"] != 0
    {
        return Err(ApiError(
            StatusCode::PRECONDITION_FAILED,
            "Chỉ ước tính chat không dùng web theo bảng giá đã kiểm tra ngày 01/11/2026.".into(),
        ));
    }
    let input = data["usage"]["input_tokens"]
        .as_u64()
        .filter(|v| *v <= 1_050_000)
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "Usage input không hợp lệ.".into()))?;
    let output = data["usage"]["output_tokens"]
        .as_u64()
        .filter(|v| *v <= 16_384)
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "Usage output không hợp lệ.".into()))?;
    // Upper allowance uses the published 2026-11-01 Luna rates, 2x fast,
    // 2x long-input, 1.25x cache writes, 1.5x long-output and two 10% buffers.
    // This is a conservative budget allocation, never an asserted invoice.
    let numerator = (input as u128 * 55 + output as u128 * 165) * 121;
    let cents = numerator.div_ceil(100_000_000).max(1);
    if cents > 100 {
        return Err(ApiError(
            StatusCode::PRECONDITION_FAILED,
            "Mức dự phòng lớn hơn khoản giữ; không điều chỉnh tự động.".into(),
        ));
    }
    Ok(cents as i64)
}
async fn budget_bound(State(app): State<App>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let cents = chat_upper_bound(&app, &id)?;
    app.store.bound_budget(&id, cents)?;
    Ok(Json(json!({"ok":true,"bounded_cents":cents,
        "message":"Đã chuyển khoản giữ chat sang mức dự phòng theo usage và bảng giá đã kiểm tra; đây không phải số tiền trên hóa đơn."})))
}
async fn cancel(State(app): State<App>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    app.store.transition_investigation(&id,"cancelled","cancelled","Người dùng hủy tác vụ. Request đã gửi có thể vẫn bị tính phí; kết quả về sau không được nhập.")?;
    Ok(Json(
        json!({"ok":true,"message":"Đã hủy. Kết quả từ request đang bay sẽ không được nhập; phí đã phát sinh vẫn được giữ."}),
    ))
}

#[derive(Deserialize)]
struct ChatInput {
    message: String,
    as_of: Option<String>,
}
async fn chat(State(app): State<App>, Json(input): Json<ChatInput>) -> ApiResult<Json<Value>> {
    let message = bounded(&input.message, 2000, "Lời nhắn")?;
    let as_of = valid_date(input.as_of.as_deref())?;
    let client = connector(&app)?;
    let _slot = app
        .provider_slots
        .acquire()
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "AI đang dừng.".into()))?;
    let hold = app.store.reserve_budget("reasoning", 100)?;
    let history: Vec<ChatMessage> = app
        .store
        .messages(12)?
        .into_iter()
        .map(|m| ChatMessage {
            role: m.role,
            content: m.content.chars().take(1200).collect(),
        })
        .collect();
    add_message(&app, "user", &message)?;
    let reply = match client.chat(&message, &history).await {
        Ok(r) => r,
        Err(e) => {
            add_message(
                &app,
                "assistant",
                &format!("Chưa nhận được câu trả lời AI: {e}. Hạn mức đã đặt giữ, không tự retry."),
            )?;
            return Err(ApiError(
                StatusCode::BAD_GATEWAY,
                format!(
                    "AI chưa trả lời được: {e}. Khoản đặt giữ {} vẫn được tính tới khi đối soát.",
                    hold.id
                ),
            ));
        }
    };
    pipeline::save_usage(&app, &hold.id, "chat", &reply)?;
    if reply.status != "completed" || reply.refusal.is_some() {
        let msg=reply.refusal.as_deref().unwrap_or("Model chưa hoàn thành trong giới hạn cấu hình. Không tự đổi model hoặc tăng trần token.");
        add_message(&app, "assistant", msg)?;
        return Err(ApiError(StatusCode::BAD_GATEWAY, msg.into()));
    }
    let value = pipeline::parse_json(&reply.text)
        .unwrap_or_else(|_| json!({"action":"answer","answer":reply.text}));
    let answer = value
        .get("answer")
        .and_then(Value::as_str)
        .unwrap_or("Tôi chưa có đủ thông tin để trả lời.");
    add_message(&app, "assistant", answer)?;
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("answer");
    let query = value
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or(&message);
    let mut job_id = None;
    if matches!(action, "search" | "investigate") {
        match create_job(&app, bounded(query, 2000, "Truy vấn")?, None, as_of, true) {
            Ok(j) => {
                job_id = Some(j.id);
                add_message(
                    &app,
                    "assistant",
                    "Tôi đã lưu tác vụ tra cứu. Hãy chọn đúng pháp nhân trong kết quả trước khi điều tra sâu.",
                )?;
            }
            Err(e) => add_message(&app, "assistant", &e.1)?,
        }
    }
    if action == "compare" {
        add_message(
            &app,
            "assistant",
            "Mở So sánh và chọn tối đa bốn doanh nghiệp đã tra cứu; tôi không tự gộp tên hoặc xếp hạng khi thiếu bằng chứng.",
        )?;
    }
    Ok(Json(json!({"ok":true,"message":answer,"job_id":job_id})))
}

async fn transcribe(
    State(app): State<App>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    if body.is_empty() || body.len() > 5 * 1024 * 1024 {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Âm thanh rỗng hoặc vượt 5 MiB.".into(),
        ));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if mime != "audio/wav" {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Chỉ nhận WAV PCM16 tối đa 30 giây; trình duyệt sẽ chuyển đổi trước khi gửi.".into(),
        ));
    }
    validate_audio(&body)?;
    let client = connector(&app)?;
    let _slot = app
        .provider_slots
        .acquire()
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "AI đang dừng.".into()))?;
    let hold = app.store.reserve_budget("audio", 25)?;
    match client.transcribe(body.to_vec(), mime).await {
        Ok(text) => Ok(Json(
            json!({"ok":true,"text":text,"reservation_id":hold.id}),
        )),
        Err(e) => Err(ApiError(
            StatusCode::BAD_GATEWAY,
            format!("Chưa phiên âm được: {e}. Không tự retry."),
        )),
    }
}
#[derive(Deserialize)]
struct SpeechInput {
    text: String,
}
fn validate_audio(bytes: &[u8]) -> ApiResult<()> {
    let mut wav = hound::WavReader::new(std::io::Cursor::new(bytes))
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "WAV không hợp lệ.".into()))?;
    let spec = wav.spec();
    if spec.sample_format != hound::SampleFormat::Int
        || spec.bits_per_sample != 16
        || spec.channels != 1
        || !(8000..=48000).contains(&spec.sample_rate)
        || wav.duration() == 0
        || wav.duration() > spec.sample_rate * 30
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Âm thanh phải là PCM16 mono, 8–48 kHz, dài không quá 30 giây.".into(),
        ));
    }
    for sample in wav.samples::<i16>() {
        sample.map_err(|_| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "Dữ liệu mẫu WAV bị cắt hoặc hỏng.".into(),
            )
        })?;
    }
    Ok(())
}
async fn speech(State(app): State<App>, Json(input): Json<SpeechInput>) -> ApiResult<Response> {
    let text = bounded(&input.text, 1200, "Nội dung đọc")?;
    let client = connector(&app)?;
    let _slot = app
        .provider_slots
        .acquire()
        .await
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "AI đang dừng.".into()))?;
    app.store.reserve_budget("audio", 25)?;
    let bytes = client.synthesize(&text).await.map_err(|e| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            format!("Chưa tạo được giọng nói: {e}. Không tự retry."),
        )
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, "audio/mpeg"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response())
}

async fn scout_run(State(app): State<App>) -> ApiResult<Json<Value>> {
    let scout = app.scout.as_ref().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Scout chưa cấu hình.".into(),
        )
    })?;
    scout.run_now().map(Json).map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "Scout chưa sẵn sàng; kiểm tra cấu hình và lịch nguồn.".into(),
        )
    })
}
#[derive(Deserialize)]
struct ScoutEnabled {
    enabled: bool,
}
async fn scout_enabled(
    State(app): State<App>,
    Json(input): Json<ScoutEnabled>,
) -> ApiResult<Json<Value>> {
    let scout = app.scout.as_ref().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Scout chưa cấu hình.".into(),
        )
    })?;
    scout.set_enabled(input.enabled).map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "Không đổi được trạng thái Scout.".into(),
        )
    })?;
    Ok(Json(
        json!({"message":if input.enabled {"Đã bật Scout."} else {"Đã dừng lấy nguồn mới; dữ liệu chờ vẫn tiếp tục được lưu."}}),
    ))
}
async fn scout_resume(State(app): State<App>) -> ApiResult<Json<Value>> {
    let scout = app.scout.as_ref().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Scout chưa cấu hình.".into(),
        )
    })?;
    scout.resume_after_review().map(Json).map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "Chưa thể tiếp tục Scout; kiểm tra chính sách nguồn.".into(),
        )
    })
}
async fn storage_health(State(app): State<App>) -> Json<Value> {
    let mut result = match &app.mongo {
        Some(mongo) => mongo.health().await,
        None => json!({"enabled":false,"status":"sqlite_only"}),
    };
    if let Some(mirror) = &app.research_mirror {
        result["research_mirror"] = mirror.status().unwrap_or(Value::Null);
    }
    Json(result)
}
async fn storage_candidates(State(app): State<App>) -> ApiResult<Json<Value>> {
    let mongo = app
        .mongo
        .as_ref()
        .ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE, "MongoDB chưa bật.".into()))?;
    mongo.read_candidates(100).await.map(Json).map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Chưa thể đọc MongoDB.".into(),
        )
    })
}

fn routes(app: App) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/ui", get(fragment))
        .route("/api/state", get(state))
        .route("/api/health", get(health))
        .route("/api/scout/run", post(scout_run))
        .route("/api/scout/enabled", post(scout_enabled))
        .route("/api/scout/resume-after-review", post(scout_resume))
        .route("/api/storage/health", get(storage_health))
        .route("/api/storage/candidates", get(storage_candidates))
        .route("/health", get(health))
        .route("/api/search", post(search))
        .route("/api/investigations", post(investigate))
        .route("/api/chat", post(chat))
        .route("/api/watchlist", post(watch))
        .route("/api/jobs/{id}/cancel", post(cancel))
        .route("/api/evidence/{id}/review", post(review_evidence))
        .route("/api/budget/limit", post(budget_limit))
        .route("/api/budget/{id}/settle", post(budget_settle))
        .route("/api/budget/{id}/bound", post(budget_bound))
        .route("/api/transcribe", post(transcribe))
        .route("/api/speech", post(speech))
        .route("/api/reports/{id}/export", get(chain::export))
        .route("/api/reports/{id}/commitment", post(chain::commitment))
        .route("/api/reports/{id}/verify", post(chain::verify))
        .route("/api/reports/{id}/transaction", post(chain::transaction))
        .route("/api/reports/{id}/anchor", post(chain::anchor))
        .route("/api/chain", get(chain::status))
        .route(
            "/assets/app.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    lantern_ui::CSS,
                )
            }),
        )
        .route(
            "/assets/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    lantern_ui::JS,
                )
            }),
        )
        .route(
            "/assets/icon.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    lantern_ui::ICON_SVG,
                )
            }),
        )
        .route(
            "/manifest.webmanifest",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/manifest+json")],
                    lantern_ui::MANIFEST,
                )
            }),
        )
        .route(
            "/sw.js",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                        (header::CACHE_CONTROL, "no-cache"),
                    ],
                    lantern_ui::SERVICE_WORKER,
                )
            }),
        )
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
        .layer(CompressionLayer::new())
        .layer(middleware::from_fn_with_state(app.clone(), boundary))
        .with_state(app)
}

fn read_local_settings() -> Result<BTreeMap<String, String>> {
    let path = PathBuf::from(".env.local");
    let bytes = if path.is_file() {
        std::fs::read(&path)?
    } else {
        Vec::new()
    };
    ensure!(bytes.len() <= 64 * 1024, "local settings exceed 64 KiB");
    let text = std::str::from_utf8(&bytes)?.trim_start_matches('\u{feff}');
    let mut result = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim_start().starts_with('#')) {
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if key.starts_with("LANTERN_") {
                result.insert(
                    key.to_string(),
                    value.trim().trim_matches(['\'', '"']).to_string(),
                );
            }
        }
    }
    load_mongo_secrets(&mut result);
    Ok(result)
}
fn load_mongo_secrets(result: &mut BTreeMap<String, String>) {
    if !setting(result, "LANTERN_MONGO_ENABLED", "false").eq_ignore_ascii_case("true") {
        return;
    }
    if let Some(path) = std::env::var("LANTERN_MONGO_SECRETS_FILE")
        .ok()
        .or_else(|| result.get("LANTERN_MONGO_SECRETS_FILE").cloned())
        .filter(|v| !v.is_empty())
    {
        let Ok(secret) = std::fs::read(path) else {
            tracing::warn!("MongoDB secrets file unavailable");
            return;
        };
        if secret.len() > 64 * 1024 {
            tracing::warn!("MongoDB secrets file too large");
            return;
        }
        let Ok(secret) = std::str::from_utf8(&secret) else {
            tracing::warn!("MongoDB secrets file is not UTF-8");
            return;
        };
        let secret = secret.trim_start_matches('\u{feff}');
        for line in secret.lines().filter(|l| !l.trim_start().starts_with('#')) {
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                if matches!(key, "LANTERN_MONGO_URI_A" | "LANTERN_MONGO_URI_B") {
                    result.insert(
                        key.to_string(),
                        value.trim().trim_matches(['\'', '"']).to_string(),
                    );
                }
            }
        }
    }
}
fn setting(values: &BTreeMap<String, String>, name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .or_else(|| values.get(name).cloned())
        .unwrap_or_else(|| default.into())
}
fn lock_instance(data_dir: &std::path::Path) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(data_dir.join("lantern-instance.lock"))?;
    file.try_lock()
        .map_err(|_| anyhow::anyhow!("Another LANTERN instance is using this data directory"))?;
    Ok(file)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lantern_server=info,tower_http=warn".into()),
        )
        .init();
    let settings = read_local_settings()?;
    let host: IpAddr = setting(&settings, "LANTERN_HOST", "127.0.0.1").parse()?;
    ensure!(
        host.is_loopback(),
        "This release is single-user local-only. Non-loopback hosting requires a separate authenticated deployment."
    );
    let port: u16 = setting(&settings, "LANTERN_PORT", "3210").parse()?;
    let data_dir = PathBuf::from(setting(&settings, "LANTERN_DATA_DIR", "data"));
    std::fs::create_dir_all(data_dir.join("reports"))?;
    std::fs::create_dir_all(data_dir.join("usage"))?;
    let _instance_lock = lock_instance(&data_dir)?;
    let store = Store::open(data_dir.join("lantern.db"))?;
    let recovered = store.recover_interrupted()?;
    let config_path = setting(&settings, "LANTERN_NORA_CONFIG_PATH", "");
    let config_result = if !config_path.is_empty() {
        Config::load(Some(std::path::Path::new(&config_path)))
    } else if std::env::var_os("LANTERN_API_KEY").is_some() {
        Config::load(None)
    } else {
        Config::load(
            std::path::Path::new(".env.local")
                .is_file()
                .then(|| std::path::Path::new(".env.local")),
        )
    };
    let (client, summary) = match config_result {
        Ok((config, _)) => {
            let stt = setting(&settings, "LANTERN_STT_MODEL", "");
            let tts = setting(&settings, "LANTERN_TTS_MODEL", "");
            let limit: u32 = setting(&settings, "LANTERN_RESEARCH_MAX_OUTPUT_TOKENS", "8192")
                .parse()
                .context("LANTERN_RESEARCH_MAX_OUTPUT_TOKENS must be an integer")?;
            let config = config
                .with_audio_models(
                    (!stt.is_empty()).then_some(stt.as_str()),
                    (!tts.is_empty()).then_some(tts.as_str()),
                )?
                .with_research_output_limit(limit)?;
            let summary = config.summary();
            (
                Some(Arc::new(Connector::new(config)?)),
                serde_json::to_value(summary)?,
            )
        }
        Err(_) => (
            None,
            json!({"configured":false,"model":"Chưa cấu hình","reasoning_effort":"","endpoint_origin":""}),
        ),
    };
    let registry = setting(&settings, "LANTERN_REGISTRY_ADDRESS", "");
    if !registry.is_empty() {
        ensure!(chain::valid_address(&registry), "Invalid registry address");
    }
    let mongo_enabled =
        setting(&settings, "LANTERN_MONGO_ENABLED", "false").eq_ignore_ascii_case("true");
    let mongo = if mongo_enabled {
        Some(Arc::new(
            mongo_runtime::MongoRuntime::open(&settings, &data_dir).await,
        ))
    } else {
        None
    };
    if mongo_enabled && store.research_outbox_backfill().is_err() {
        tracing::warn!("Research backfill not completed; existing data remains local");
    }
    let scout_enabled =
        setting(&settings, "LANTERN_SCOUT_ENABLED", "false").eq_ignore_ascii_case("true");
    let scout = Some(Arc::new(scout_runtime::ScoutRuntime::open(
        &data_dir,
        store.clone(),
        mongo.clone(),
        scout_enabled,
    )?));
    let research_mirror = Some(Arc::new(research_mirror::ResearchMirror::new(
        store.clone(),
        mongo.clone(),
    )));
    let app = App {
        store,
        connector: client,
        provider: summary,
        data_dir,
        allowed_hosts: Arc::new(vec![
            format!("127.0.0.1:{port}"),
            format!("localhost:{port}"),
            format!("[::1]:{port}"),
        ]),
        provider_slots: Arc::new(Semaphore::new(1)),
        registry: (!registry.is_empty()).then_some(registry),
        rpc: setting(
            &settings,
            "LANTERN_FUJI_RPC_URL",
            "https://api.avax-test.network/ext/bc/C/rpc",
        ),
        scout,
        mongo,
        research_mirror,
    };
    let rpc = url::Url::parse(&app.rpc)?;
    ensure!(
        rpc.scheme() == "https" && rpc.username().is_empty() && rpc.password().is_none(),
        "RPC must use credential-free HTTPS"
    );
    let listener = tokio::net::TcpListener::bind((host, port))
        .await
        .context("Cannot bind local web server")?;
    tokio::spawn(pipeline::runner(app.clone()));
    if let Some(scout) = app.scout.clone() {
        tokio::spawn(scout.run());
    }
    if let Some(mirror) = app.research_mirror.clone() {
        mirror.wake();
        tokio::spawn(mirror.run());
    }
    if let Some(mongo) = app.mongo.clone() {
        let reconnect_settings = settings.clone();
        let reconnect_data_dir = app.data_dir.clone();
        let mirror = app.research_mirror.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                tick.tick().await;
                let state = mongo
                    .try_reconnect(&reconnect_settings, &reconnect_data_dir)
                    .await;
                if state["ready"] == true
                    && let Some(mirror) = &mirror
                {
                    mirror.wake();
                }
            }
        });
    }
    tracing::info!(%port,recovered,scout_enabled,"LANTERN ready on loopback; Scout may poll sources if enabled");
    println!("LANTERN VN: http://127.0.0.1:{port}");
    axum::serve(listener, routes(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;
    #[test]
    fn only_one_server_can_use_a_data_directory_at_once() {
        let dir = std::env::temp_dir().join(format!("lantern-lock-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = lock_instance(&dir).unwrap();
        assert!(lock_instance(&dir).is_err());
        drop(first);
        drop(lock_instance(&dir).unwrap());
        std::fs::remove_file(dir.join("lantern-instance.lock")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn search_and_selected_dossier_reach_beyond_first_page() {
        let app = app();
        for n in 0..205 {
            app.store
                .save_company(&Company {
                    id: format!("synthetic-{n}"),
                    name: if n == 204 {
                        "Zzz Synthetic Target".into()
                    } else {
                        format!("Aaa Synthetic {n:03}")
                    },
                    tax_id: None,
                    ticker: None,
                    sector: None,
                    location: None,
                    website: None,
                    source_url: None,
                    description: None,
                    identity_status: "candidate_unverified".into(),
                    watched: false,
                    updated_at: "2026-01-01T00:00:00Z".into(),
                })
                .unwrap();
        }
        let state = snapshot(
            &app,
            &ViewQuery {
                q: Some("Zzz Synthetic".into()),
                selected: Some("synthetic-204".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(state["search_results"].as_array().unwrap().len(), 1);
        assert_eq!(state["search_results"][0]["id"], "synthetic-204");
        assert!(
            state["companies"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "synthetic-204")
        );
    }
    fn app() -> App {
        App {
            store: Store::memory().unwrap(),
            connector: None,
            provider: json!({}),
            data_dir: std::env::temp_dir().join(format!("lantern-test-{}", Uuid::new_v4())),
            allowed_hosts: Arc::new(vec!["localhost:3210".into()]),
            provider_slots: Arc::new(Semaphore::new(1)),
            registry: None,
            rpc: "https://api.avax-test.network/ext/bc/C/rpc".into(),
            scout: None,
            mongo: None,
            research_mirror: None,
        }
    }
    #[tokio::test]
    async fn reject_cross_origin_and_rebinding_without_billing() {
        let routes = routes(app());
        for (host, origin) in [
            ("evil.example", "http://evil.example"),
            ("localhost:3210", "https://evil.example"),
        ] {
            let req = axum::http::Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("host", host)
                .header("origin", origin)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"message":"hello"}"#))
                .unwrap();
            assert_eq!(
                routes.clone().oneshot(req).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
    }
    #[tokio::test]
    async fn missing_provider_is_explicit_no_fake_reply() {
        let app = app();
        let store = app.store.clone();
        let routes = routes(app);
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/search")
            .header("host", "localhost:3210")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"query":"FPT"}"#))
            .unwrap();
        assert_eq!(
            routes.oneshot(req).await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(store.budget().unwrap().reserved_cents, 0);
    }
    #[test]
    fn unknown_and_future_dates_are_rejected() {
        assert!(valid_date(Some("2026-50-44")).is_err());
        assert!(valid_date(Some("2999-01-01")).is_err());
    }
    #[test]
    fn historical_snapshot_hides_current_records_and_rejects_jobs() {
        let app = app();
        let company = Company {
            id: Uuid::new_v4().to_string(),
            name: "Doanh nghiệp hiện tại".into(),
            tax_id: None,
            ticker: None,
            sector: None,
            location: None,
            website: None,
            source_url: None,
            description: None,
            identity_status: "candidate_unverified".into(),
            watched: false,
            updated_at: now(),
        };
        app.store.save_company(&company).unwrap();
        let q = ViewQuery {
            as_of: Some("2020-01-01".into()),
            ..Default::default()
        };
        let value = snapshot(&app, &q).unwrap();
        assert_eq!(value["historical_unavailable"], true);
        assert!(value["companies"].as_array().unwrap().is_empty());
        assert!(value["investigations"].as_array().unwrap().is_empty());
        assert!(value["evidence"].as_array().unwrap().is_empty());
        assert_eq!(
            create_job(&app, "old".into(), None, q.as_of, true)
                .unwrap_err()
                .0,
            StatusCode::CONFLICT
        );
        assert_eq!(app.store.budget().unwrap().reserved_cents, 0);
    }
    #[test]
    fn long_search_does_not_return_unrelated_companies() {
        let app = app();
        let company = Company {
            id: Uuid::new_v4().to_string(),
            name: "Một công ty khác".into(),
            tax_id: None,
            ticker: None,
            sector: None,
            location: None,
            website: None,
            source_url: None,
            description: None,
            identity_status: "candidate_unverified".into(),
            watched: false,
            updated_at: now(),
        };
        app.store.save_company(&company).unwrap();
        let q = ViewQuery {
            q: Some("Tìm giúp tôi một doanh nghiệp trong ngành logistics tại Hà Nội".into()),
            ..Default::default()
        };
        assert!(
            snapshot(&app, &q).unwrap()["search_results"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn bounded_chat_cost_requires_historical_receipt_provenance() {
        let app = app();
        let id = Uuid::new_v4().to_string();
        std::fs::create_dir_all(app.data_dir.join("usage")).unwrap();
        let path = app.data_dir.join("usage").join(format!("{id}.json"));
        let mut receipt = json!({"reservation_id":id,"kind":"chat","model":"gpt-6-luna","endpoint_origin":"https://api.openai.com",
            "recorded_at":"2026-11-01T12:00:00Z","usage":{"input_tokens":500,"output_tokens":200,"web_calls":0,
            "usage_complete":true,"no_tool_calls_confirmed":true}});
        std::fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert_eq!(chat_upper_bound(&app, &id).unwrap(), 1);
        receipt["usage"]["usage_complete"] = json!(false);
        std::fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(chat_upper_bound(&app, &id).is_err());
        receipt["usage"]["usage_complete"] = json!(true);
        receipt["usage"]["no_tool_calls_confirmed"] = json!(false);
        std::fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(chat_upper_bound(&app, &id).is_err());
        receipt["usage"]["no_tool_calls_confirmed"] = json!(true);
        receipt["endpoint_origin"] = json!("https://example.org");
        std::fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(chat_upper_bound(&app, &id).is_err());
    }
    #[test]
    fn audio_duration_and_format_are_enforced_before_provider() {
        fn wav(seconds: u32, channels: u16) -> Vec<u8> {
            let mut cursor = std::io::Cursor::new(Vec::new());
            let spec = hound::WavSpec {
                channels,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            {
                let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
                for _ in 0..(8000 * seconds * u32::from(channels)) {
                    writer.write_sample(0i16).unwrap();
                }
                writer.finalize().unwrap();
            }
            cursor.into_inner()
        }
        assert!(validate_audio(&wav(1, 1)).is_ok());
        assert!(validate_audio(&wav(31, 1)).is_err());
        assert!(validate_audio(&wav(1, 2)).is_err());
        assert!(validate_audio(b"not a wav").is_err());
    }
}
