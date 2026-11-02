//! Local analyst review and bounded operational learning. These routes never call a model.
use super::*;
use lantern_core::{
    Evidence,
    risk::{IdentityReviewInput, RiskAssessmentInput},
};

#[derive(Deserialize)]
pub(super) struct CohortInput {
    company_ids: Vec<String>,
    as_of: Option<String>,
}
pub(super) async fn save_snapshot(
    State(app): State<App>,
    Json(input): Json<CohortInput>,
) -> ApiResult<Json<Value>> {
    if input.company_ids.is_empty() || input.company_ids.len() > lantern_core::risk::MAX_RISK_COHORT
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Chọn từ 1 đến 1.000 hồ sơ cho một nhóm so sánh.".into(),
        ));
    }
    let cutoff = match input.as_of.as_deref() {
        None | Some("") => now(),
        Some(value) if value == today() => now(),
        Some(value) => {
            valid_date(Some(value))?;
            value.to_string()
        }
    };
    let result = app.store.risk_snapshot(&input.company_ids, &cutoff)?;
    Ok(Json(
        json!({"ok":true,"snapshot":result,"message":"Đã lưu phiên bản sàng lọc của nhóm hồ sơ đã chọn."}),
    ))
}
pub(super) async fn get_snapshot(
    State(app): State<App>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let result = app.store.risk_get_snapshot(&id)?.ok_or_else(|| {
        ApiError(
            StatusCode::NOT_FOUND,
            "Không có phiên bản sàng lọc này.".into(),
        )
    })?;
    Ok(Json(json!({"ok":true,"snapshot":result})))
}
pub(super) async fn assess(
    State(app): State<App>,
    Json(input): Json<RiskAssessmentInput>,
) -> ApiResult<Json<Value>> {
    let entry = app.store.risk_assess(input)?;
    Ok(Json(
        json!({"ok":true,"entry":entry,"message":"Đã lưu đánh giá của người rà soát cùng bằng chứng."}),
    ))
}
pub(super) async fn identity(
    State(app): State<App>,
    Json(input): Json<IdentityReviewInput>,
) -> ApiResult<Json<Value>> {
    let entry = app.store.risk_review_identity(input)?;
    Ok(Json(
        json!({"ok":true,"entry":entry,"message":"Đã lưu quyết định định danh."}),
    ))
}
#[derive(Deserialize)]
pub(super) struct RetractInput {
    analyst: String,
    reason: String,
}
pub(super) async fn retract(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<RetractInput>,
) -> ApiResult<Json<Value>> {
    let entry = app
        .store
        .risk_retract_assessment(&id, &input.analyst, &input.reason)?;
    Ok(Json(
        json!({"ok":true,"entry":entry,"message":"Đã ghi nhận thu hồi; lịch sử vẫn được giữ."}),
    ))
}
#[derive(Deserialize)]
pub(super) struct EvidenceInput {
    company_id: String,
    title: String,
    url: String,
    publisher: String,
    claim: String,
    dimension: String,
    published_at: Option<String>,
}
pub(super) async fn add_evidence(
    State(app): State<App>,
    Json(input): Json<EvidenceInput>,
) -> ApiResult<Json<Value>> {
    if app.store.get_company(&input.company_id)?.is_none() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            "Không có hồ sơ này.".into(),
        ));
    }
    let raw = bounded(&input.url, 2000, "URL")?;
    let url = url::Url::parse(&raw)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "URL không hợp lệ.".into()))?;
    let host = url.host_str().unwrap_or("");
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || host.is_empty()
        || host.parse::<IpAddr>().is_ok()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Dùng URL HTTPS công khai, không chứa thông tin đăng nhập.".into(),
        ));
    }
    if !["identity", "legal", "tax", "financial", "operations"].contains(&input.dimension.as_str())
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Chiều đánh giá không hợp lệ.".into(),
        ));
    }
    let item = Evidence {
        id: Uuid::new_v4().to_string(),
        company_id: input.company_id,
        title: bounded(&input.title, 500, "Tên tài liệu")?,
        url: url.to_string(),
        publisher: bounded(&input.publisher, 250, "Đơn vị công bố")?,
        claim: bounded(&input.claim, 2500, "Dữ kiện")?,
        dimension: input.dimension,
        status: "unverified".into(),
        published_at: valid_date(input.published_at.as_deref())?,
        observed_at: now(),
    };
    app.store.save_evidence(&item)?;
    Ok(Json(
        json!({"ok":true,"evidence":item,"message":"Đã lưu nguồn do bạn nhập. Hãy đối chiếu chủ thể trước khi đánh giá."}),
    ))
}
pub(super) async fn evolution_state(State(app): State<App>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(app.evolution.snapshot()?)))
}
pub(super) async fn evolution_tick(State(app): State<App>) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({"ok":true,"evolution":app.evolution.tick()?,"message":"Đã kiểm tra dữ liệu học; chưa tự áp dụng phiên bản mới."}),
    ))
}
#[derive(Deserialize)]
pub(super) struct VersionInput {
    version: i64,
}
pub(super) async fn promote(
    State(app): State<App>,
    Json(input): Json<VersionInput>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({"ok":true,"evolution":app.evolution.promote(input.version)?,"message":"Đã áp dụng cấu hình tìm nguồn cho các tác vụ mới."}),
    ))
}
pub(super) async fn rollback(
    State(app): State<App>,
    Json(input): Json<VersionInput>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({"ok":true,"evolution":app.evolution.rollback(input.version)?,"message":"Đã khôi phục cấu hình tìm nguồn."}),
    ))
}
#[derive(Deserialize)]
pub(super) struct FeedbackInput {
    feedback: evolution::Feedback,
}
pub(super) async fn feedback(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<FeedbackInput>,
) -> ApiResult<Json<Value>> {
    let job = app
        .store
        .get_investigation(&id)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Không có tác vụ này.".into()))?;
    if !matches!(job.status.as_str(), "completed" | "partial" | "failed") {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Chỉ đánh giá tác vụ đã kết thúc.".into(),
        ));
    }
    app.evolution.record_feedback(&id, input.feedback)?;
    app.evolution.tick()?;
    Ok(Json(
        json!({"ok":true,"message":"Đã lưu phản hồi cho lần tìm nguồn này."}),
    ))
}
pub(super) async fn revision(State(app): State<App>) -> ApiResult<Json<Value>> {
    let evo = app.evolution.snapshot()?;
    let stamp = json!({"db":app.store.local_revision()?,"scout":app.scout.as_ref().and_then(|s|s.status().ok()),
        "storage":app.mongo.as_ref().map(|m|m.status_json()),"mirror":app.research_mirror.as_ref().and_then(|m|m.status().ok()),
        "evolution":evo});
    use sha2::{Digest, Sha256};
    Ok(Json(
        json!({"revision":hex::encode(Sha256::digest(stamp.to_string().as_bytes()))}),
    ))
}
pub(super) async fn source_enabled(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<ScoutEnabled>,
) -> ApiResult<Json<Value>> {
    let scout = app.scout.as_ref().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Scout chưa cấu hình.".into(),
        )
    })?;
    scout.set_source_enabled(&id, input.enabled)?;
    Ok(Json(
        json!({"ok":true,"message":"Đã cập nhật nguồn; lịch vẫn tuân thủ hạn mức."}),
    ))
}
pub(super) async fn source_run(
    State(app): State<App>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let scout = app.scout.as_ref().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Scout chưa cấu hình.".into(),
        )
    })?;
    Ok(Json(scout.run_source_now(&id)?))
}
pub(super) async fn source_resume(
    State(app): State<App>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let scout = app.scout.as_ref().ok_or_else(|| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Scout chưa cấu hình.".into(),
        )
    })?;
    Ok(Json(scout.resume_source_after_review(&id)?))
}
