use super::*;

fn area(value: &str) -> &str {
    match value {
        "identity" => "Định danh",
        "legal" => "Pháp lý",
        "tax" => "Thuế",
        "financial" => "Tài chính",
        "operations" => "Hoạt động",
        _ => value,
    }
}
fn severity(value: Option<u64>) -> &'static str {
    match value {
        Some(0) => "0 · Chưa ghi nhận vấn đề trong nguồn",
        Some(1) => "1 · Quan ngại thấp",
        Some(2) => "2 · Quan ngại vừa",
        Some(3) => "3 · Quan ngại cao",
        _ => "Chưa đủ điều kiện",
    }
}
fn result_for<'a>(snapshot: &'a Value, cid: &str) -> Option<&'a Value> {
    items(&snapshot["risk"], "results")
        .iter()
        .find(|r| field(r, &["company_id"]) == cid)
}

pub(super) fn pagination(snapshot: &Value, view: &str) -> Markup {
    let p = &snapshot["catalogue"];
    let n = p["page"].as_u64().unwrap_or(0);
    let total = p["total"].as_u64().unwrap_or(0);
    html! { div class="pagination" { span { (total) " hồ sơ · trang " (n+1) }
        @if n>0 { button type="button" class="button subtle small" data-action="page" data-page=(n-1) data-target-view=(view) { "← Trước" } }
        @if p["has_next"].as_bool().unwrap_or(false) { button type="button" class="button subtle small" data-action="page" data-page=(n+1) data-target-view=(view) { "Sau →" } }
    } }
}
pub(super) fn risk(snapshot: &Value) -> Markup {
    let risk = &snapshot["risk"];
    let results = items(risk, "results");
    html! {
        (section_head("", "Sàng lọc rủi ro", ""))
        section class="panel full-panel" {
            input type="hidden" id="risk-cohort" value=(snapshot["risk_cohort"].to_string());
            div class="panel-head" { h3 { "Mức quan ngại · thấp → cao" }
                @if !results.is_empty() && !risk["saved"].as_bool().unwrap_or(false) { button type="button" class="button subtle small" data-action="risk-snapshot" { "Lưu phiên bản" } }
                @if risk["saved"].as_bool().unwrap_or(false) { a class="text-link" href=(format!("/api/risk/snapshots/{}",field(risk,&["id"]))) target="_blank" rel="noopener" { "Xuất JSON ↗" } }
            }
            p class="essential-note" { "Sàng lọc theo bằng chứng đã rà soát, không phải xác suất vỡ nợ hay tư vấn đầu tư. Xếp thứ tự toàn nhóm trước khi phân trang." }
            @if snapshot["catalogue"]["truncated"]==true { p class="essential-note" { "Giới hạn nhóm 1.000 hồ sơ; kho có " (snapshot["catalogue"]["available"].as_u64().unwrap_or(0)) ". Chưa xếp hạng phần còn lại." } }
            @if risk["saved"].as_bool().unwrap_or(false) { p class="essential-note" { "Bản đã lưu · dữ liệu biết đến " (field(risk,&["as_of"])) ". Tên được ghi nhận lúc tạo bản; nguồn có thể đã được sửa sau đó." } }
            @if results.is_empty() { div class="empty compact" { strong { "Chưa có hồ sơ để sàng lọc" } } }
            @for r in results {
                @let cid=field(r,&["company_id"]);
                div class="risk-row" data-risk-company=(cid) { div { a href=(format!("/?view=dossier&selected={cid}")) data-view="dossier" data-selected=(cid) {
                    (text_or(field(r,&["company_name"]),cid))
                } span class="risk-score" { (severity(r["score"].as_u64())) }
                small { "Độ phủ " (r["covered_dimensions"].as_u64().unwrap_or(0)) "/4 · " @if r["identity_confirmed"]==true { "Định danh đã rà soát" } @else { "Chưa xác nhận định danh" } }
                details { summary { "Căn cứ" }
                    @for reason in items(r,"reasons") { p { (reason.as_str().unwrap_or("")) } }
                    @for d in items(r,"dimensions") { p { (area(field(d,&["dimension"]))) ": " (severity(d["severity"].as_u64())) } }
                } }
                @if let Some(rank)=r["rank"].as_u64() { span class="badge" { "#" (rank) } }
                }
            }
            (pagination(snapshot,"risk"))
        }
        @if !items(snapshot,"risk_snapshots").is_empty() { details class="panel review-panel" { summary { "Phiên bản đã lưu" }
            @for item in items(snapshot,"risk_snapshots") { div class="setting-line" { button type="button" class="inline-button" data-action="risk-open" data-id=(field(item,&["id"])) { (field(item,&["created_at"])) } span { (item["scored_count"].as_u64().unwrap_or(0)) "/" (item["company_count"].as_u64().unwrap_or(0)) " đủ điều kiện" } } }
        } }
    }
}

pub(super) fn dossier(snapshot: &Value, cid: &str) -> Markup {
    let evidence: Vec<_> = items(snapshot, "evidence")
        .iter()
        .filter(|e| field(e, &["company_id"]) == cid)
        .collect();
    let reviewed: Vec<_> = evidence
        .iter()
        .filter(|e| field(e, &["status"]) == "subject_confirmed")
        .collect();
    let journal = items(snapshot, "risk_journal");
    let retracted: Vec<_> = journal
        .iter()
        .filter_map(|j| j["target_id"].as_str())
        .collect();
    html! {
        section class="panel review-panel" {
            h3 { "Đánh giá bằng chứng" }
            @if let Some(r)=result_for(snapshot,cid) { p class="risk-score" { (severity(r["score"].as_u64())) } }
            details { summary { "Thêm nguồn" }
                form class="review-form" data-form="evidence-add" {
                    input type="hidden" name="company_id" value=(cid);
                    label { "Tài liệu" input name="title" maxlength="500" required; }
                    label { "URL nguồn" input name="url" type="url" placeholder="https://" maxlength="2000" required; }
                    label { "Đơn vị công bố" input name="publisher" maxlength="250" required; }
                    label { "Ngày công bố" input name="published_at" type="date" max=(field(snapshot,&["as_of"])); }
                    label { "Phạm vi" select name="dimension" { @for value in ["identity","legal","tax","financial","operations"] { option value=(value) { (area(value)) } } } }
                    label { "Dữ kiện cần đối chiếu" textarea name="claim" rows="2" maxlength="2500" required {} }
                    button type="submit" class="button subtle" { "Lưu nguồn" }
                }
            }
            @if !reviewed.is_empty() {
                details { summary { "Chấm mức quan ngại" }
                    form class="review-form" data-form="risk-assess" { input type="hidden" name="company_id" value=(cid);
                        label { "Bằng chứng đã đối chiếu" select name="evidence_id" required { option value="" { "Chọn nguồn" }
                            @for e in &reviewed { @if field(e,&["dimension"])!="identity" { option value=(id(e)) data-dimension=(field(e,&["dimension"])) { (area(field(e,&["dimension"]))) " · " (field(e,&["title"])) } } }
                        } }
                        label { "Mức quan ngại" select name="severity" { @for n in 0..=3 { option value=(n) { (severity(Some(n))) } } } }
                        (review_fields())
                        label class="review-check" { input type="checkbox" name="confirmed" required; "Tôi đã đọc nguồn, kiểm tra dữ kiện và phạm vi đánh giá." }
                        button type="submit" class="button subtle" { "Lưu đánh giá" }
                    }
                }
                @if reviewed.iter().any(|e|field(e,&["dimension"])=="identity") { details { summary { "Rà soát định danh" }
                    form class="review-form" data-form="risk-identity" { input type="hidden" name="company_id" value=(cid);
                        label { "Nguồn định danh" select name="evidence_id" required { option value="" { "Chọn nguồn" }
                            @for e in &reviewed { @if field(e,&["dimension"])=="identity" { option value=(id(e)) { (field(e,&["title"])) } } }
                        } }
                        input type="hidden" name="confirmed" value="true";
                        (review_fields())
                        button type="submit" class="button subtle" { "Lưu quyết định" }
                    }
                } }
            }
            @if let Some(previous)=journal.iter().rev().find(|j|field(j,&["kind"])=="identity_review").filter(|j|j["confirmed"]==true) {
                details { summary { "Rút xác nhận định danh" }
                    form class="review-form" data-form="risk-identity" { input type="hidden" name="company_id" value=(cid); input type="hidden" name="evidence_id" value=(field(&previous["evidence"],&["id"])); input type="hidden" name="confirmed" value="false";
                        (review_fields()) button type="submit" class="button subtle" { "Rút xác nhận" }
                    }
                }
            }
            @if !journal.is_empty() { details { summary { "Lịch sử rà soát" }
                @for j in journal { div class="journal-entry" { strong { (area(field(j,&["dimension"]))) " · " (field(j,&["recorded_at"])) } p { (field(j,&["analyst"])) ": " (field(j,&["rationale"])) }
                    @if field(j,&["kind"])=="assessment" && !retracted.contains(&field(j,&["id"])) {
                        form class="review-form" data-form="risk-retract" { input type="hidden" name="entry_id" value=(field(j,&["id"])); (review_fields()) button type="submit" class="button subtle small" { "Thu hồi đánh giá" } }
                    }
                } }
            } }
        }
    }
}
fn review_fields() -> Markup {
    html! { label { "Người rà soát" input name="analyst" maxlength="100" required; } label { "Căn cứ" textarea name="rationale" rows="2" minlength="10" maxlength="2000" required {} } }
}
fn recipe(value: &str) -> &str {
    match value {
        "baseline" => "Cơ sở",
        "identity" => "Đối chiếu pháp nhân",
        "coverage" => "Bổ sung nguồn",
        "identity_coverage" => "Pháp nhân + nguồn",
        _ => value,
    }
}
fn outcome(value: &str) -> &str {
    match value {
        "success" => "Có dữ liệu nhập, chưa xác minh",
        "no_evidence" => "Không có nguồn đủ điều kiện",
        "identity_mismatch" => "Sai pháp nhân",
        "source_failure" => "Lỗi nguồn / phản hồi",
        _ => value,
    }
}
pub(super) fn evolution(snapshot: &Value) -> Markup {
    let state = &snapshot["evolution"];
    let active = &state["active"];
    html! {
        (section_head("","Cải thiện agent",""))
        section class="panel review-panel" { div class="panel-head" { h3 { (recipe(field(active,&["recipe"]))) " · v" (active["id"].as_i64().unwrap_or(1)) }
            button type="button" class="button subtle small" data-action="evolution-evaluate" { "Đánh giá" }
        }
            p class="essential-note" { "Học từ kết quả và phản hồi để đề xuất cách tìm nguồn. Kiểm thử quy tắc cục bộ chưa chứng minh AI chính xác hơn." }
            div class="setting-line" { span { "Lượt quan sát" } strong { (state["retained_runs"].as_i64().unwrap_or(0)) } }
            @if !items(state,"blocked_by_rollback").is_empty() { p class="essential-note" { "Đề xuất đã khôi phục đang được giữ lại; cần thêm lỗi mới để thử lại." } }
            @if state["shadow"].is_object() { @let candidate=&state["shadow"];
                div class="setting-line" { span { "Đề xuất" } strong { (recipe(field(candidate,&["recipe"]))) } }
                p class="essential-note" { "Đã qua kiểm thử quy tắc; chưa thử model đối chứng." }
                button type="button" class="button subtle" data-action="evolution-promote" data-version=(candidate["id"].as_i64().unwrap_or(0)) { "Áp dụng đề xuất" }
            } @else { p class="essential-note" { "Chưa có đề xuất đủ điều kiện." } }
            @if active["id"].as_i64().unwrap_or(1)!=1 { button type="button" class="button subtle" data-action="evolution-rollback" data-version="1" { "Khôi phục cấu hình cơ sở" } }
        }
        section class="panel review-panel" { h3 { "Phản hồi tác vụ" }
            @if items(state,"recent_runs").is_empty() { p class="essential-note" { "Chưa có lượt tìm nguồn được ghi nhận từ phiên bản này." } }
            @for run in items(state,"recent_runs") { div class="journal-entry" { strong { (outcome(field(run,&["outcome"]))) } small { "v" (run["version_id"].as_i64().unwrap_or(1)) " · " (field(run,&["run_id"])) }
                form class="feedback-form" data-form="evolution-feedback" { input type="hidden" name="run_id" value=(field(run,&["run_id"]));
                    select name="feedback" aria-label="Phản hồi tác vụ" {
                        @for (value,label) in [("helpful","Hữu ích"),("missing_evidence","Thiếu bằng chứng"),("wrong_entity","Sai pháp nhân"),("unhelpful","Chưa hữu ích")] { option value=(value) selected[field(run,&["feedback"])==value] { (label) } }
                    }
                    button type="submit" class="button subtle small" { "Lưu phản hồi" }
                }
            } }
        }
        @if !items(state,"recent_audit").is_empty() { details class="panel review-panel" { summary { "Nhật ký thay đổi" } @for a in items(state,"recent_audit") { p { (field(a,&["at"])) " · " (field(a,&["action"])) " · v" (a["version_id"].as_i64().unwrap_or(0)) } } } }
    }
}
