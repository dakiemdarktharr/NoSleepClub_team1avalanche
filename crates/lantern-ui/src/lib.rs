//! Server-rendered Vietnamese research workbench. Data enters as a server snapshot;
//! missing data is represented explicitly rather than inferred.
use maud::{DOCTYPE, Markup, html};
use serde_json::Value;

pub const CSS: &str = include_str!("../assets/app.css");
pub const JS: &str = include_str!("../assets/app.js");
pub const MANIFEST: &str = include_str!("../assets/manifest.webmanifest");
pub const SERVICE_WORKER: &str = include_str!("../assets/sw.js");
pub const ICON_SVG: &str = include_str!("../assets/icon.svg");

fn field<'a>(v: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .unwrap_or("")
}
fn items<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn title(v: &Value) -> &str {
    field(v, &["name", "legal_name", "company_name", "title", "query"])
}
fn id(v: &Value) -> &str {
    field(v, &["id", "company_id", "job_id"])
}
fn text_or<'a>(s: &'a str, default: &'a str) -> &'a str {
    if s.is_empty() { default } else { s }
}
fn identity_label(value: &str) -> &'static str {
    match value {
        "candidate_unverified" => "Ứng viên · chưa xác minh",
        "unverified" => "Chưa xác minh",
        "verified" => "Đã được đánh dấu xác minh",
        _ => "Chưa rõ trạng thái định danh",
    }
}
fn evidence_status_label(value: &str) -> &'static str {
    match value {
        "unverified" => "Nguồn gợi ý, chưa xác định chủ thể",
        "subject_confirmed" => "Chủ thể do người dùng xác nhận · nội dung chưa kiểm chứng",
        "not_about_company" => "Không nói về doanh nghiệp này",
        "verified" => "Trạng thái cũ · cần đối chiếu nguồn",
        "reported" => "Đã được ghi nhận",
        "rejected" => "Đã bị loại",
        _ => "Chưa rõ",
    }
}
fn job_status_label(value: &str) -> &'static str {
    match value {
        "queued" | "pending" => "Đang chờ xử lý",
        "running" => "Đang thực hiện",
        "verifying" => "Đang đối chiếu nguồn",
        "completed" => "Đã hoàn tất tác vụ",
        "partial" => "Chưa hoàn tất",
        "quarantined_test" => "Đã cách ly kết quả kiểm thử",
        "failed" => "Không hoàn tất do lỗi",
        "cancelled" => "Đã hủy",
        _ => "Chưa rõ trạng thái",
    }
}
fn job_stage_label(value: &str) -> &'static str {
    match value {
        "queued_lookup" => "Chờ tìm doanh nghiệp",
        "queued_research" => "Chờ nghiên cứu",
        "queued" => "Đang chờ",
        "resolving" => "Đang xác định phạm vi",
        "collecting" => "Đang tìm nguồn",
        "checking_sources" | "verify" => "Đang đối chiếu nguồn",
        "provider_incomplete" => "Model chưa hoàn tất",
        "historical_gap" => "Thiếu dữ liệu lịch sử",
        "reconciliation_required" => "Cần đối soát sau khởi động lại",
        "quarantined_test" => "Đã cách ly kết quả kiểm thử",
        "finished" | "done" => "Đã kết thúc xử lý",
        "cancelled" => "Đã hủy",
        "failed" => "Gặp lỗi",
        _ => "Chưa rõ bước xử lý",
    }
}
fn budget_kind_label(value: &str) -> &'static str {
    match value {
        "reasoning" => "Suy luận AI",
        "audio" => "Xử lý âm thanh",
        _ => "Chưa rõ loại",
    }
}
fn nav_link(view: &str, current: &str, icon: &str, label: &str) -> Markup {
    html! { a class=(if current == view { "nav-link active" } else { "nav-link" }) href=(format!("/?view={view}")) data-view=(view) aria-current=[(current == view).then_some("page")] title=(label) { span class="nav-icon" aria-hidden="true" { (icon) } span class="nav-label" { (label) } } }
}
fn badge(s: &str) -> Markup {
    html! { span class="badge" { (s) } }
}
fn section_head(kicker: &str, heading: &str, description: &str) -> Markup {
    let _ = (kicker, description);
    html! { div class="page-heading" { h1 { (heading) } } }
}

/// Full document. All dynamic values are escaped by Maud.
pub fn render_page(snapshot: &Value, route: &str, selected: Option<&str>) -> Markup {
    let config = snapshot.get("config").unwrap_or(&Value::Null);
    let as_of = field(snapshot, &["as_of"]);
    let configured = config
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let recipient = if configured {
        field(config, &["endpoint_origin"])
    } else {
        ""
    };
    html! {
        (DOCTYPE) html lang="vi" { head { meta charset="utf-8"; meta name="viewport" content="width=device-width, initial-scale=1"; meta name="theme-color" content="#0b1522"; meta name="description" content="Không gian nghiên cứu doanh nghiệp cùng MIRA."; link rel="icon" href="/assets/icon.svg" type="image/svg+xml"; link rel="manifest" href="/manifest.webmanifest"; link rel="stylesheet" href="/assets/app.css?v=6"; title { "LANTERN VN · MIRA" } }
        body { a class="skip-link" href="#main-content" { "Chuyển tới nội dung" }
            header class="topbar" {
                button type="button" class="mobile-menu icon-button" data-action="menu" aria-label="Mở điều hướng" aria-expanded="false" { "☰" }
                a class="wordmark" href="/?view=overview" data-view="overview" aria-label="LANTERN VN, trang tổng quan" { span class="brand-mark" aria-hidden="true" { "✦" } span { strong { "LANTERN" } small { "VN" } } }
                div class="top-divider" aria-hidden="true" {}
                form id="command-form" class="command" data-form="search" { label class="sr-only" for="command-input" { "Tìm doanh nghiệp theo tên hoặc mã số thuế" } span class="search-glyph" aria-hidden="true" { "⌕" } input id="command-input" name="query" type="search" autocomplete="off" placeholder="Tìm tên doanh nghiệp hoặc mã số thuế" required; kbd { "Ctrl K" } button type="submit" class="command-submit" { "Tìm" } }
                div class="top-actions" { label class="date-control" title="Ngày chốt theo dữ liệu hệ thống đã lưu; không phải tái dựng toàn bộ thông tin công khai trong quá khứ" { span { "Hệ thống đã biết đến" } input id="as-of" type="date" value=(as_of) aria-label="Ngày chốt dữ liệu hệ thống đã lưu"; } span class=(if configured { "connection ok" } else { "connection" }) { span class="status-dot" {} (if configured { "MIRA đã cấu hình" } else { "MIRA chưa cấu hình" }) } button type="button" class="icon-button top-mic" data-action="record" aria-label="Ghi âm cho MIRA" title="Ghi âm cho MIRA" { "◉" } button type="button" class="mira-mobile icon-button" data-action="mira" aria-label="Mở MIRA" aria-expanded="false" aria-controls="mira-dock" { "M" } }
            }
            div class="mobile-date" { label for="as-of-mobile" { "Dữ liệu hệ thống đã biết đến" } input id="as-of-mobile" type="date" value=(as_of); }
            div class="app-shell" {
                nav class="rail" id="navigation" aria-label="Điều hướng chính" {
                    div class="rail-group" {
                        (nav_link("overview", route, "◫", "Tổng quan"))
                        (nav_link("companies", route, "⌕", "Doanh nghiệp"))
                        (nav_link("compare", route, "⇄", "So sánh"))
                        (nav_link("watchlist", route, "☆", "Theo dõi"))
                        (nav_link("jobs", route, "◷", "Tác vụ"))
                        (nav_link("sources", route, "▤", "Nguồn"))
                    }
                    div class="rail-bottom" { (nav_link("settings", route, "⚙", "Cài đặt")) span class="rail-caption" { "LANTERN" } }
                }
                div id="workspace" class="workspace" { (render_workspace(snapshot, route, selected)) }
            }
            div id="toast" class="toast" role="status" aria-live="polite" hidden {}
            dialog id="voice-consent" data-recipient=(recipient) { div class="dialog-content" { span class="dialog-icon" { "◉" } h2 { "Cho phép gửi giọng nói?" } p { "Khi ghi âm, âm thanh được gửi để chuyển thành văn bản tới nhà cung cấp tại: " strong { (text_or(recipient, "Chưa cấu hình")) } ". Bạn sẽ xem và chỉnh bản chép trước khi gửi yêu cầu." } p class="muted" { "Tối đa 30 giây hoặc 5 MB mỗi lần. Micro chỉ bật sau khi bạn đồng ý và trình duyệt cho phép." } div class="dialog-actions" { button type="button" class="button subtle" data-action="voice-decline" { "Để sau" } button type="button" class="button primary" data-action="voice-accept" disabled[recipient.is_empty()] { "Đồng ý và ghi âm" } } } }
            dialog id="report-dialog" aria-labelledby="report-dialog-title" { div class="dialog-content report-dialog-content" { div class="dialog-title-row" { div { p class="context-label" { "Kiểm chứng phiên bản" } h2 id="report-dialog-title" { "Cam kết báo cáo" } } button type="button" class="icon-button" data-action="report-close" aria-label="Đóng" { "×" } } p { "Cam kết xác nhận tính toàn vẹn của một phiên bản, không chứng nhận nguồn đúng, danh tính tổ chức hay chất lượng kết luận." } div class="commitment-card" { div { span { "Trạng thái" } strong id="commitment-status" { "Chưa tạo" } } div { span { "Digest" } code id="commitment-digest" { "—" } } } p class="setting-note" { "Bản xuất chứa báo cáo, salt và nguồn. Chỉ chia sẻ khi bạn có quyền." } a id="report-export-link" class="button subtle" href="#" { "Tải bản báo cáo JSON" }
                div class="dialog-section" { h3 { "Kiểm tra bản đã xuất" } p { "Chọn file JSON hoặc dán nội dung. Phép kiểm tra so sánh nội dung với cam kết trong chính bản bạn đưa vào. Một người có thể thay cả hai; hãy đối chiếu digest với nguồn đáng tin hoặc receipt đã xác thực." } label for="report-file" { "Chọn bản JSON" } input type="file" id="report-file" accept="application/json,.json"; label for="report-json" { "Hoặc dán JSON" } textarea id="report-json" rows="3" placeholder="Dán bản báo cáo và commitment" {} button type="button" class="button subtle" data-action="report-verify" { "Kiểm tra tính toàn vẹn" } p id="verify-result" class="dialog-result" role="status" aria-live="polite" {} }
                div class="dialog-section" { h3 { "Neo trên Avalanche Fuji" } p { "Bước này tùy chọn và cần ví của bạn. Cam kết cục bộ chưa được neo; chỉ receipt đúng contract và digest mới xác nhận giao dịch. Chuỗi không chứng minh nội dung là sự thật." } button type="button" class="button subtle" data-action="chain-prepare" { "Chuẩn bị neo Fuji" } div id="chain-prepared" hidden { p class="setting-note" { "Mạng: Fuji (chain ID 43113). Contract:" } code id="chain-contract" class="digest-line" {} p { "Bạn sẽ kiểm tra dữ liệu và phí trong ví trước khi xác nhận." } button type="button" class="button subtle" data-action="chain-connect" { "Kết nối ví và chọn Fuji" } } div id="chain-connected" hidden { p class="setting-note" { "Ví: " code id="chain-account" {} } button type="button" class="button primary" data-action="chain-send" { "Xác nhận gửi giao dịch" } } label for="transaction-hash" { "Transaction hash để kiểm tra receipt" } input id="transaction-hash" type="text" autocomplete="off" placeholder="0x…"; button type="button" class="button subtle" data-action="chain-retry" { "Kiểm tra lại receipt" } p id="chain-result" class="dialog-result" role="status" aria-live="polite" {} }
                div class="dialog-actions" { button type="button" class="button subtle" data-action="report-close" { "Đóng" } }
            } }
            script src="/assets/app.js?v=6" defer {} }
        }
    }
}

/// Replaces the private workspace after navigation or data refresh.
pub fn render_workspace(snapshot: &Value, route: &str, selected: Option<&str>) -> Markup {
    html! { main id="main-content" class="main-content" tabindex="-1" { div class="main-inner" {
        @if snapshot.get("historical_unavailable").and_then(Value::as_bool).unwrap_or(false) { div class="history-warning" role="status" { strong { "Chưa có bản lịch sử đầy đủ, hồ sơ và tác vụ được ẩn; hội thoại hiển thị hiện tại." } } }
        @match route {
            "companies" => (companies(snapshot)),
            "dossier" => (dossier(snapshot, selected)),
            "compare" => (compare(snapshot)),
            "watchlist" => (watchlist(snapshot)),
            "jobs" => (jobs(snapshot)),
            "sources" => (sources(snapshot)),
            "settings" => (settings(snapshot)),
            _ => (overview(snapshot)),
        }
    } } (mira(snapshot)) }
}

fn overview(snapshot: &Value) -> Markup {
    let cs = items(snapshot, "companies");
    let jobs = items(snapshot, "investigations");
    html! {
        div class="overview-title" { h1 { "Nghiên cứu doanh nghiệp" } button type="button" class="button primary" data-action="focus-search" { "Tìm doanh nghiệp" } }
        (monitoring_status(snapshot))
        div class="section-title" { h2 { "Doanh nghiệp" } a href="/?view=companies" data-view="companies" class="text-link" { "Tất cả doanh nghiệp" } }
        div class="dashboard-grid" {
            section class="panel wide-panel" aria-labelledby="recent-title" { div class="panel-head" { div { h3 id="recent-title" { "Hồ sơ" } } span class="panel-symbol" aria-hidden="true" { "◫" } }
                @if cs.is_empty() { div class="empty compact" { div class="empty-icon" aria-hidden="true" { "⌕" } div { strong { "Chưa có hồ sơ doanh nghiệp" } p { "Tìm theo tên hoặc mã số thuế để bắt đầu." } button type="button" class="inline-button" data-action="focus-search" { "Bắt đầu tìm kiếm" } } } }
                @else { (company_rows(&cs[..cs.len().min(6)], false)) }
            }
            section class="panel" aria-labelledby="active-title" { div class="panel-head" { div { h3 id="active-title" { "Tác vụ nghiên cứu" } p { "Cập nhật khi bạn giao việc" } } span class="panel-symbol" aria-hidden="true" { "◷" } }
                @if jobs.is_empty() { div class="empty compact" { div class="empty-icon" aria-hidden="true" { "◷" } div { strong { "Chưa có tác vụ" } p { "MIRA sẽ hiển thị tiến trình khi bắt đầu nghiên cứu." } } } }
                @else { (job_rows(jobs, snapshot)) }
                a href="/?view=jobs" data-view="jobs" class="panel-footer" { "Mở danh sách tác vụ" span aria-hidden="true" { "↗" } }
            }
        }
    }
}

fn monitoring_status(snapshot: &Value) -> Markup {
    let scout = snapshot.get("scout").unwrap_or(&Value::Null);
    let storage = snapshot.get("storage").unwrap_or(&Value::Null);
    let scout_configured = scout
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let scout_enabled = scout
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let storage_configured = storage
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let sources = items(scout, "sources");
    let clusters = items(storage, "clusters");
    let scout_status = |value: &str| match value {
        "scheduled" => "Đã lên lịch",
        "running" => "Đang chạy",
        "disabled" => "Đã dừng",
        "needs_review" => "Cần rà soát",
        "waiting_mongo" => "Chờ MongoDB",
        "attention" => "Cần kiểm tra",
        _ => "Chưa xác minh",
    };
    html! {
        section class="monitor-panel panel" aria-labelledby="monitor-title" {
            div class="monitor-heading" { h2 id="monitor-title" { "Theo dõi dữ liệu" } div { button type="button" class="button subtle small" data-action="scout-toggle" data-enabled=(scout_enabled) disabled[!scout_configured] { @if scout_enabled { "Dừng lấy mới" } @else { "Bật Scout" } } button type="button" class="button subtle small" data-action="scout-run" disabled[!(scout_configured && scout_enabled)] { "Chạy Scout" } @if scout.get("origin_suspended").and_then(Value::as_bool).unwrap_or(false) { button type="button" class="button subtle small" data-action="scout-resume" disabled[!scout_enabled] { "Tiếp tục sau rà soát" } } } }
            div class="monitor-grid" {
                div { span { "Scout" } strong { @if scout_configured { (scout_status(field(scout, &["status"]))) } @else { "Chưa cấu hình" } }
                    small { "Lần kiểm tra: " (text_or(field(scout, &["last_run_at"]), "Chưa có")) }
                    small { "Dự kiến: " (text_or(field(scout, &["next_run_at"]), "Chưa có")) }
                    small { "Độ phủ: " (text_or(field(scout, &["coverage"]), "Chưa xác minh")) }
                }
                div { span { "Lưu trữ" } strong { @if storage_configured { (text_or(field(storage, &["status"]), "Chưa xác minh")) } @else { "Chưa cấu hình" } }
                    small { "Chế độ: " (text_or(field(storage, &["mode"]), "Chưa xác minh")) }
                    @if let Some(pending) = storage.get("research_mirror").and_then(|m| m.get("pending")).and_then(Value::as_i64).filter(|n| *n > 0) { small { "Chờ đồng bộ: " (pending) } }
                    @if clusters.is_empty() { small { "Cluster: chưa có thông tin" } }
                    @for cluster in clusters { small { (text_or(field(cluster, &["name"]), "Cluster chưa có tên")) " · " (text_or(field(cluster, &["status"]), "Chưa xác minh")) @if !field(cluster, &["region"]).is_empty() { " · " (field(cluster, &["region"])) } } }
                }
            }
            @if !sources.is_empty() { div class="monitor-sources" { @for source in sources { div { strong { (text_or(field(source, &["name"]), "Nguồn chưa có tên")) } } } } }
        }
    }
}

fn company_rows(companies: &[Value], watched_only: bool) -> Markup {
    html! { div class="data-list" { @for c in companies.iter().filter(|c| !watched_only || c.get("watched").and_then(Value::as_bool).unwrap_or(false)) {
        @let name = text_or(title(c), "Doanh nghiệp chưa có tên"); @let cid = id(c); @let tax = field(c, &["tax_id", "mst", "tax_code"]);
        div class="company-row" { div class="company-monogram" aria-hidden="true" { (name.chars().next().unwrap_or('D').to_uppercase()) } div class="company-info" { @if !cid.is_empty() { a href=(format!("/?view=dossier&selected={cid}")) data-view="dossier" data-selected=(cid) { (name) } } @else { strong { (name) } } p { @if !tax.is_empty() { "MST " (tax) } @else { "Chưa có mã số thuế" } } } span class="row-meta" { (identity_label(field(c, &["identity_status", "status", "state"]))) } @if !cid.is_empty() { button class="icon-button star" type="button" data-action="watch" data-id=(cid) data-watched=(c.get("watched").and_then(Value::as_bool).unwrap_or(false)) aria-label=(format!("Theo dõi {name}")) { @if c.get("watched").and_then(Value::as_bool).unwrap_or(false) { "★" } @else { "☆" } } } }
    } } }
}
fn companies(snapshot: &Value) -> Markup {
    let results = snapshot
        .get("search_results")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_else(|| items(snapshot, "companies"));
    let query = field(snapshot, &["search_query"]).trim();
    let normalized_query = query.to_lowercase();
    let pending = !query.is_empty()
        && items(snapshot, "investigations").iter().any(|j| {
            field(j, &["query"]).trim().to_lowercase() == normalized_query
                && ["queued", "running", "verifying"].contains(&field(j, &["status"]))
        });
    html! { (section_head("Danh mục", "Doanh nghiệp", "Tìm đúng pháp nhân trước khi bắt đầu thẩm định."))
        section class="panel full-panel" { div class="panel-head" { div { h3 { "Kết quả tìm kiếm" } } @if !query.is_empty() && !pending { button type="button" class="button subtle small" data-action="search-web" data-query=(query) { "Tìm bằng AI" } } }
        @if pending { div class="search-pending" role="status" { span class="status-dot" {} span { "Đang tìm, " a href="/?view=jobs" data-view="jobs" { "xem Tác vụ" } } } }
        @if results.is_empty() && !pending { div class="empty large" { div class="empty-icon" { "⌕" } div { h3 { "Chưa có kết quả" } p { "Nhập tên hoặc mã số thuế trong thanh tìm kiếm. Kết quả phụ thuộc nguồn dữ liệu được cấu hình." } button class="button primary" type="button" data-action="focus-search" { "Tìm doanh nghiệp" } } } } @else if !results.is_empty() { (company_rows(results, false)) } }
    }
}
fn dossier(snapshot: &Value, selected: Option<&str>) -> Markup {
    let cs = items(snapshot, "companies");
    let company = selected
        .and_then(|s| cs.iter().find(|c| id(c) == s))
        .or_else(|| snapshot.get("company").filter(|c| c.is_object()));
    html! { @if let Some(c) = company { @let name = text_or(title(c), "Hồ sơ doanh nghiệp"); @let cid = id(c); @let tax = field(c, &["tax_id", "mst", "tax_code"]);
        div class="backline" { a href="/?view=companies" data-view="companies" { "← Doanh nghiệp" } span { "Hồ sơ" } }
        div class="dossier-heading" { div class="company-monogram large-monogram" aria-hidden="true" { (name.chars().next().unwrap_or('D').to_uppercase()) } div class="dossier-title" { p class="context-label" { "Hồ sơ pháp nhân" } h1 { (name) } p { @if tax.is_empty() { "Mã số thuế chưa được xác minh" } @else { "MST " (tax) } "  ·  " (identity_label(field(c, &["identity_status", "status", "state"]))) } } @if !cid.is_empty() { button type="button" class="button subtle" data-action="watch" data-id=(cid) data-watched=(c.get("watched").and_then(Value::as_bool).unwrap_or(false)) { @if c.get("watched").and_then(Value::as_bool).unwrap_or(false) { "★ Đang theo dõi" } @else { "☆ Theo dõi" } } } }
        section class="panel company-details" { div class="panel-head" { div { h3 { "Thông tin pháp nhân" } p { "Thông tin đã có trong hồ sơ" } } @let url = field(c, &["source_url"]); @if url.starts_with("https://") || url.starts_with("http://") { a href=(url) target="_blank" rel="noopener noreferrer" class="text-link" { "Mở nguồn ↗" } } } div class="company-detail-grid" { div { span { "Lĩnh vực" } strong { (text_or(field(c, &["sector"]), "Chưa có")) } } div { span { "Địa phương" } strong { (text_or(field(c, &["location"]), "Chưa có")) } } div { span { "Mã giao dịch" } strong { (text_or(field(c, &["ticker"]), "Chưa có")) } } @let website = field(c, &["website"]); div { span { "Trang doanh nghiệp" } @if website.starts_with("https://") || website.starts_with("http://") { a href=(website) target="_blank" rel="noopener noreferrer" { "Mở trang ↗" } } @else { strong { "Chưa có" } } } } @if !field(c, &["description"]).is_empty() { p class="company-description" { (field(c, &["description"])) } } }
        section class="panel research-prompt" { div { h3 { "Bắt đầu một cuộc điều tra" } p { "Nêu câu hỏi. MIRA sẽ lưu tác vụ và trình bày những gì có thể kiểm chứng." } } @if !cid.is_empty() { form data-form="investigation" class="investigate-form" { input type="hidden" name="company_id" value=(cid); label class="sr-only" for="investigate-query" { "Câu hỏi điều tra" } input id="investigate-query" name="query" placeholder="Ví dụ: Kiểm tra tình trạng pháp lý" required; button type="submit" class="button primary" { "Giao MIRA" } } } }
        div class="dossier-grid" { section class="panel" { div class="panel-head" { div { h3 { "Bằng chứng" } p { "Nguồn và nội dung đã thu thập" } } (badge(&format!("{} mục", items(snapshot, "evidence").iter().filter(|e| field(e, &["company_id"]) == cid && field(e, &["status"]) != "not_about_company").count()))) } (evidence_rows(snapshot, cid)) }
        section class="panel" { div class="panel-head" { div { h3 { "Diễn tiến nghiên cứu" } p { "Các bước đã được ghi nhận" } } } (event_rows(snapshot, cid)) }
        section class="panel span-two" { div class="panel-head" { div { h3 { "Các chiều cần xem xét" } p { "Số mục không phải đánh giá an toàn hoặc rủi ro." } } } div class="dimension-grid" { @for (name, caption, aliases) in [("Pháp lý", "Trạng thái và sự kiện", &["legal", "pháp lý"][..]), ("Tài chính", "Báo cáo và chỉ tiêu", &["financial", "finance", "tài chính"][..]), ("Hoạt động", "Thông tin công bố", &["operations", "operational", "hoạt động"][..]), ("Quan hệ", "Liên kết có bằng chứng", &["relationship", "relationships", "quan hệ"][..])] { @let count = items(snapshot, "evidence").iter().filter(|e| field(e, &["company_id"]) == cid && field(e, &["status"]) != "not_about_company" && aliases.contains(&field(e, &["dimension"]))).count(); div { strong { (name) } span { (caption) } small { @if count == 0 { "Chưa có bằng chứng" } @else { (count) " mục cần xem xét" } } } } } }
        }
    } @else { (section_head("Hồ sơ", "Chọn doanh nghiệp", "Chưa xác định pháp nhân cần mở.")) (empty_action("◫", "Chưa có hồ sơ để hiển thị", "Tìm doanh nghiệp rồi chọn một kết quả để mở hồ sơ.", "Tìm doanh nghiệp", "focus-search")) } }
}
fn evidence_rows(snapshot: &Value, company_id: &str) -> Markup {
    let entries: Vec<_> = items(snapshot, "evidence")
        .iter()
        .filter(|e| field(e, &["company_id"]) == company_id)
        .collect();
    html! {
        @if entries.is_empty() { div class="empty compact" { div class="empty-icon" { "▤" } div { strong { "Chưa có bằng chứng" } p { "Bằng chứng có nguồn sẽ xuất hiện tại đây." } } } }
        @else { p class="evidence-explanation" { "Chỉ xác nhận nguồn nói về đúng chủ thể; nội dung và kết luận pháp lý vẫn chưa được kiểm chứng." } div class="data-list" {
            @for e in entries { div class="evidence-row" { div { strong { (text_or(field(e, &["title", "claim", "text", "content"]), "Tài liệu")) } p { (text_or(field(e, &["publisher", "source", "source_name", "url"]), "Chưa có nguồn")) } @if !field(e, &["claim"]).is_empty() { p { (field(e, &["claim"])) } } p { "Trạng thái: " (evidence_status_label(field(e, &["status"]))) } @if !field(e, &["published_at"]).is_empty() { p { "Công bố: " (field(e, &["published_at"])) } } @if !field(e, &["observed_at"]).is_empty() { p { "Hệ thống ghi nhận: " (field(e, &["observed_at"])) } }
                @if !id(e).is_empty() { div class="evidence-actions" { button type="button" class="button subtle small" data-action="review-evidence" data-id=(id(e)) data-decision="subject_confirmed" { "Tôi đã đối chiếu đúng chủ thể" } button type="button" class="button subtle small" data-action="review-evidence" data-id=(id(e)) data-decision="not_about_company" { "Không nói về doanh nghiệp này" } } }
            } @let url = field(e, &["url", "source_url"]); @if url.starts_with("https://") || url.starts_with("http://") { a href=(url) target="_blank" rel="noopener noreferrer" class="text-link" { "Mở nguồn ↗" } } } }
        } }
    }
}
fn event_rows(snapshot: &Value, company_id: &str) -> Markup {
    let investigations: Vec<&str> = items(snapshot, "investigations")
        .iter()
        .filter(|i| field(i, &["company_id"]) == company_id)
        .map(id)
        .collect();
    let entries: Vec<_> = items(snapshot, "events")
        .iter()
        .filter(|e| {
            field(e, &["company_id"]) == company_id
                || investigations.contains(&field(e, &["investigation_id"]))
        })
        .collect();
    html! { @if entries.is_empty() { div class="empty compact" { div class="empty-icon" { "◷" } div { strong { "Chưa có sự kiện" } p { "Tiến trình nghiên cứu sẽ hiển thị tại đây." } } } } @else { div class="timeline" { @for e in entries { div class="timeline-item" { time { (text_or(field(e, &["created_at", "date", "occurred_at", "effective_at"]), "Chưa rõ ngày")) } strong { (text_or(field(e, &["message", "title", "description", "event"]), "Sự kiện")) } } } } } }
}
fn empty_action(icon: &str, heading: &str, desc: &str, label: &str, action: &str) -> Markup {
    html! { section class="panel full-panel" { div class="empty large" { div class="empty-icon" aria-hidden="true" { (icon) } div { h3 { (heading) } p { (desc) } button type="button" class="button primary" data-action=(action) { (label) } } } } }
}
fn compare(snapshot: &Value) -> Markup {
    let companies = items(snapshot, "companies");
    let selected: Vec<&str> = items(snapshot, "selected_ids")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let selected_companies: Vec<&Value> = companies
        .iter()
        .filter(|c| selected.contains(&id(c)))
        .collect();
    html! { (section_head("Phân tích", "So sánh doanh nghiệp", "Đặt các hồ sơ cạnh nhau để xem bằng chứng và khoảng trống."))
        @if companies.len() < 2 { (empty_action("⇄", "Cần ít nhất hai doanh nghiệp", "Tìm và lưu hồ sơ trước khi chọn các doanh nghiệp cần so sánh.", "Tìm doanh nghiệp", "focus-search")) }
        @else { @if selected_companies.len() >= 2 { section class="panel comparison-result" { div class="panel-head" { div { h3 { "Các hồ sơ đã chọn" } p { "Dữ liệu có sẵn theo từng pháp nhân; thiếu dữ liệu không có nghĩa là an toàn." } } } div class="comparison-table" { @for c in selected_companies { div class="comparison-col" { a href=(format!("/?view=dossier&selected={}", id(c))) data-view="dossier" data-selected=(id(c)) { (text_or(title(c), "Chưa có tên")) } dl { dt { "Mã số thuế" } dd { (text_or(field(c, &["tax_id", "mst", "tax_code"]), "Chưa có")) } dt { "Định danh" } dd { (identity_label(field(c, &["identity_status", "status"]))) } dt { "Địa phương" } dd { (text_or(field(c, &["location"]), "Chưa có")) } dt { "Lĩnh vực" } dd { (text_or(field(c, &["sector"]), "Chưa có")) } } } } } } } section class="panel full-panel" { div class="panel-head" { div { h3 { "Chọn hồ sơ" } p { "Chọn ít nhất hai doanh nghiệp đã có trong không gian làm việc." } } } form data-form="compare" class="compare-form" { @for c in companies { @if !id(c).is_empty() { label class="check-row" { input type="checkbox" name="ids" value=(id(c)) checked[selected.contains(&id(c))]; span { (text_or(title(c), "Chưa có tên")) } small { (field(c, &["tax_id", "mst", "tax_code"])) } } } } button type="submit" class="button primary" { "So sánh hồ sơ" } } } }
    }
}
fn watchlist(snapshot: &Value) -> Markup {
    let companies = items(snapshot, "companies");
    let count = companies
        .iter()
        .filter(|c| c.get("watched").and_then(Value::as_bool).unwrap_or(false))
        .count();
    html! { (section_head("Danh mục", "Theo dõi", "Những doanh nghiệp bạn muốn quay lại kiểm tra.")) @if count == 0 { (empty_action("☆", "Danh sách đang trống", "Mở một hồ sơ và chọn Theo dõi để lưu vào đây.", "Tìm doanh nghiệp", "focus-search")) } @else { section class="panel full-panel" { div class="panel-head" { div { h3 { "Danh sách theo dõi" } p { "Chọn doanh nghiệp để mở hồ sơ." } } (badge(&format!("{count} hồ sơ"))) } (company_rows(companies, true)) } } }
}
fn job_rows(jobs: &[Value], snapshot: &Value) -> Markup {
    html! { div class="data-list" { @for j in jobs { div class="job-row" { div { strong { (text_or(field(j, &["query", "title", "name"]), "Tác vụ nghiên cứu")) } p { (job_status_label(field(j, &["status", "state"]))) @if !field(j, &["stage"]).is_empty() { " · " (job_stage_label(field(j, &["stage"]))) } } @if !field(j, &["summary"]).is_empty() { p { "Tóm tắt AI, chưa được chuyên viên xác minh: " (field(j, &["summary"])) } } } @let jid = id(j); @let state = field(j, &["status", "state"]); @if !jid.is_empty() && ["queued", "running", "verifying", "pending"].contains(&state) { button type="button" class="button subtle small" data-action="cancel-job" data-id=(jid) { "Hủy tác vụ" } } @let ready = items(snapshot, "report_ready_ids").iter().any(|value| value.as_str() == Some(jid)); @if !jid.is_empty() && ready { div class="job-actions" { button type="button" class="button subtle small" data-action="report-commitment" data-id=(jid) { "Kiểm chứng báo cáo" } } } } } } }
}
fn jobs(snapshot: &Value) -> Markup {
    let jobs = items(snapshot, "investigations");
    html! { (section_head("Hoạt động", "Tác vụ nghiên cứu", "Theo dõi tiến trình và kết quả những việc đã giao.")) section class="panel full-panel" { div class="panel-head" { div { h3 { "Tác vụ" } p { "Hủy tác vụ là hành động riêng với dừng phát âm thanh." } } } @if jobs.is_empty() { div class="empty large" { div class="empty-icon" { "◷" } div { h3 { "Chưa có tác vụ" } p { "Chọn một doanh nghiệp để giao MIRA câu hỏi nghiên cứu." } a class="button primary" href="/?view=companies" data-view="companies" { "Mở doanh nghiệp" } } } } @else { (job_rows(jobs, snapshot)) } } }
}
fn sources(snapshot: &Value) -> Markup {
    let sources = items(snapshot, "sources");
    html! { (section_head("Dữ liệu", "Nguồn thông tin", "Quyền truy cập và nội dung phụ thuộc từng nguồn.")) section class="panel full-panel" { div class="panel-head" { div { h3 { "Danh mục nguồn" } p { "Danh mục tham khảo; kiểm tra quyền truy cập từng nguồn." } } } @if sources.is_empty() { div class="empty large" { div class="empty-icon" { "▤" } div { h3 { "Chưa có nguồn được liệt kê" } p { "Khi có nguồn được cấu hình, thông tin và trạng thái sẽ xuất hiện tại đây. Không có nguồn không có nghĩa là doanh nghiệp không có rủi ro." } a href="/?view=settings" data-view="settings" class="button subtle" { "Mở cài đặt" } } } } @else { div class="data-list" { @for source in sources { div class="source-row" { div class="source-icon" { "▤" } div { strong { (text_or(title(source), "Nguồn dữ liệu")) } p { (text_or(field(source, &["description", "url"]), "Chưa có mô tả")) } } @let url = field(source, &["url"]); @if url.starts_with("https://") || url.starts_with("http://") { a href=(url) target="_blank" rel="noopener noreferrer" class="text-link" { "Mở nguồn ↗" } } } } } } } }
}
fn settings(snapshot: &Value) -> Markup {
    let config = snapshot.get("config").unwrap_or(&Value::Null);
    let configured = config
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let min_budget = config
        .get("reserved_usd")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        + config
            .get("spent_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
    let reservations_loaded = snapshot
        .get("reservations")
        .and_then(Value::as_array)
        .is_some();
    let holds: Vec<_> = items(snapshot, "reservations")
        .iter()
        .filter(|r| field(r, &["status"]) == "reserved")
        .collect();
    let estimates: Vec<_> = items(snapshot, "reservations")
        .iter()
        .filter(|r| field(r, &["status"]) == "bounded")
        .collect();
    html! { (section_head("Không gian làm việc", "Cài đặt & quyền riêng tư", "Kiểm tra cấu hình và cách bạn sử dụng giọng nói."))
        div class="settings-grid" {
            section class="panel settings-panel" { h3 { "Kết nối MIRA" } div class="setting-line" { span { "Trạng thái" } strong { (if configured { "Đã cấu hình" } else { "Chưa cấu hình" }) } } div class="setting-line" { span { "Mô hình" } strong { (text_or(field(config, &["model"]), "Chưa cấu hình")) } } div class="setting-line" { span { "Mức suy luận" } strong { (text_or(field(config, &["reasoning_effort"]), "Chưa cấu hình")) } } p class="setting-note" { "Cấu hình được quản lý ở máy chủ. Không hiển thị khóa trong giao diện." } }
            section class="panel settings-panel" { h3 { "Giọng nói & quyền riêng tư" } p { "Ghi âm chỉ bắt đầu sau khi bạn đồng ý. Âm thanh được gửi tới nhà cung cấp đã cấu hình để phiên âm; bạn có thể xem địa chỉ và xác nhận trước khi ghi." } p class="setting-note" { "Dừng phát âm thanh không hủy tác vụ nghiên cứu." } button class="button subtle" type="button" data-action="forget-voice" { "Đặt lại lựa chọn ghi âm" } }
            section class="panel settings-panel" { h3 { "Ngân sách" } p { "Khoản giữ chỗ là giới hạn tạm thời, không phải hóa đơn của nhà cung cấp." }
                @for (key, label) in [("budget_usd", "Giới hạn"), ("reserved_usd", "Đang giữ chỗ"), ("spent_usd", "Đã đối soát / dự phòng"), ("remaining_usd", "Còn khả dụng")] { @if let Some(v) = config.get(key).and_then(Value::as_f64) { div class="setting-line" { span { (label) } strong { (format!("${v:.2}")) } } } }
                form class="budget-form" data-form="budget-limit" { label { "Giới hạn mới (USD)" input name="amount_usd" type="number" min=(format!("{min_budget:.2}")) step="0.01" inputmode="decimal" required; } label class="budget-confirm" { input name="confirmed" type="checkbox" required; span { "Tôi xác nhận thay đổi giới hạn ngân sách lưu trên máy." } } button type="submit" class="button subtle" { "Lưu giới hạn" } p class="setting-note" { "Không thể hạ thấp hơn tổng khoản đã đối soát và đang giữ chỗ." } }
            }
            section class="panel settings-panel" { h3 { "Cài đặt ứng dụng" } p { "Có thể thêm LANTERN vào màn hình chính khi trình duyệt hỗ trợ." } button type="button" class="button subtle" id="install-button" data-action="install" disabled title="Trình duyệt chưa cung cấp tùy chọn cài đặt" { "Cài ứng dụng" } p class="setting-note" id="install-hint" { "Tùy chọn cài đặt chưa khả dụng trên trình duyệt này." } }
            section class="panel settings-panel budget-holds" { h3 { "Đối soát khoản giữ chỗ" } p { "Nhập số tiền từ hóa đơn nhà cung cấp, làm tròn lên đến cent. Chỉ nhập 0 khi nhà cung cấp xác nhận không tính phí. Nếu chưa chắc, hãy giữ nguyên khoản này để đối soát sau; hệ thống không tự đặt lại." }
                @if !reservations_loaded { p class="setting-note" { "Chưa tải được dữ liệu khoản giữ chỗ từ máy chủ." } }
                @else if holds.is_empty() { p class="setting-note" { "Không có khoản đang giữ chỗ." } }
                @else { div class="hold-list" { @for hold in holds { @let rid = id(hold); @let cents = hold.get("reserved_cents").and_then(Value::as_i64).unwrap_or(0); @let short: String = rid.chars().take(8).collect(); div class="hold-entry" { div class="hold-meta" { strong { (budget_kind_label(field(hold, &["kind"]))) } span { (format!("${:.2}", cents as f64 / 100.0)) } } p class="setting-note" { "Mã " (short) " · " (text_or(field(hold, &["created_at"]), "Chưa rõ thời gian")) } @if !rid.is_empty() { @if let Some(estimate) = hold.get("bounded_estimate_cents").and_then(Value::as_i64).filter(|v| *v >= 0) { p class="setting-note" { "Dự phòng thận trọng dựa trên usage do nhà cung cấp báo và bảng giá công bố ngày 01/11; đây không phải hóa đơn." } button type="button" class="button subtle small" data-action="budget-bound" data-id=(rid) { "Dự phòng ước tính " (format!("${:.2}", estimate as f64 / 100.0)) } } form class="budget-form" data-form="reconcile-budget" { input type="hidden" name="reservation_id" value=(rid); label { "Số tiền theo hóa đơn (USD)" input name="actual_usd" type="number" min="0" step="0.01" inputmode="decimal" required; } label class="budget-confirm" { input name="confirmed" type="checkbox" required; span { "Tôi đã kiểm tra hóa đơn nhà cung cấp và xác nhận số tiền này." } } button type="submit" class="button subtle" { "Xác nhận đối soát" } } } } } } }
                @if !estimates.is_empty() { h3 class="estimate-heading" { "Dự phòng ước tính" } p class="setting-note" { "Các khoản dưới đây là dự phòng, không phải số tiền trên hóa đơn." } div class="hold-list" { @for estimate in estimates { @let short: String = id(estimate).chars().take(8).collect(); @let cents = estimate.get("actual_cents").and_then(Value::as_i64); div class="hold-entry" { div class="hold-meta" { strong { "Dự phòng ước tính · " (budget_kind_label(field(estimate, &["kind"]))) } span { @if let Some(cents) = cents { (format!("${:.2}", cents as f64 / 100.0)) } @else { "Chưa rõ" } } } p class="setting-note" { "Mã " (short) " · " (text_or(field(estimate, &["created_at"]), "Chưa rõ thời gian")) } } } } }
            }
        }
    }
}
fn mira(snapshot: &Value) -> Markup {
    let messages = items(snapshot, "messages");
    html! { aside class="mira-dock" id="mira-dock" aria-label="Trợ lý MIRA" { div class="mira-head" { div class="mira-avatar" aria-hidden="true" { "✦" } div class="mira-title" { strong { "MIRA" } span { @if snapshot.get("historical_unavailable").and_then(Value::as_bool).unwrap_or(false) { "Hội thoại hiện tại" } @else { "Trợ lý nghiên cứu" } } } button type="button" class="icon-button mobile-close" data-action="mira-close" aria-label="Đóng MIRA" { "×" } }
        div class="mira-body" id="mira-messages" aria-live="polite" { @if messages.is_empty() { div class="mira-welcome" { h2 { "Chào bạn, mình là MIRA." } p { "Bạn muốn tìm hiểu doanh nghiệp nào?" } div class="suggestion-list" { button type="button" data-action="suggest" data-text="Mình muốn tìm hiểu một doanh nghiệp." { "Tìm hiểu doanh nghiệp" span { "↗" } } button type="button" data-action="suggest" data-text="Bạn kiểm tra nguồn thông tin như thế nào?" { "Cách kiểm tra nguồn" span { "↗" } } } } } @else { div class="messages" { @for m in messages { @let role = field(m, &["role", "speaker"]); div class=(if role == "user" { "message user-message" } else { "message assistant-message" }) { span class="message-role" { (if role == "user" { "Bạn" } else { "MIRA" }) } p { (text_or(field(m, &["content", "text", "message"]), "")) } @if role != "user" && !field(m, &["content", "text", "message"]).is_empty() { div class="voice-actions" { button type="button" class="speak-button" data-action="speak" data-text=(field(m, &["content", "text", "message"])) { "▶ Nghe từ dịch vụ" } button type="button" class="speak-button" data-action="device-speak" data-text=(field(m, &["content", "text", "message"])) { "Giọng thiết bị" } } } } } } } }
        div class="mira-compose" { div id="recording-state" class="recording-state" hidden { span class="record-dot" {} span { "Đang ghi âm" } button type="button" class="inline-button" data-action="stop-recording" { "Dừng ghi" } } form data-form="chat" { label class="sr-only" for="chat-input" { "Nhắn cho MIRA" } textarea id="chat-input" name="message" placeholder="Nhắn hoặc nói với MIRA..." rows="2" required {} div class="compose-actions" { button type="button" class="icon-button mic-button" data-action="record" aria-label="Ghi âm" title="Ghi âm" { "◉" } button type="button" class="inline-button" data-action="stop-audio" { "Dừng âm thanh" } button type="submit" class="send-button" aria-label="Gửi tin nhắn" { "↑" } } } p { "Kiểm tra nguồn trước khi đưa ra quyết định." } }
    } }
}

#[cfg(test)]
mod tests {
    use super::render_workspace;
    use serde_json::json;

    #[test]
    fn renders_unverified_candidates_and_job_states_in_vietnamese() {
        let snapshot = json!({
            "companies": [{"id":"c1", "name":"Ví dụ", "identity_status":"candidate_unverified"}],
            "investigations": [{"id":"j1", "query":"Tra cứu", "status":"partial", "stage":"provider_incomplete"}],
            "evidence": [{"id":"e1", "company_id":"c1", "title":"Nguồn", "status":"unverified"}]
        });
        let company = render_workspace(&snapshot, "dossier", Some("c1")).into_string();
        assert!(company.contains("Ứng viên · chưa xác minh"));
        assert!(company.contains("Nguồn gợi ý, chưa xác định chủ thể"));
        assert!(company.contains("Tôi đã đối chiếu đúng chủ thể"));
        assert!(company.contains("Không nói về doanh nghiệp này"));
        assert!(!company.contains("candidate_unverified"));
        let jobs = render_workspace(&snapshot, "jobs", None).into_string();
        assert!(jobs.contains("Chưa hoàn tất"));
        assert!(jobs.contains("Model chưa hoàn tất"));
        assert!(!jobs.contains("provider_incomplete"));
    }

    #[test]
    fn report_button_comes_from_server_gate_and_history_is_labeled() {
        let mut snapshot = json!({
            "historical_unavailable": true,
            "companies": [{"id":"c1", "name":"Ví dụ"}],
            "investigations": [{"id":"j1", "company_id":"c1", "query":"Kiểm tra", "status":"completed", "summary":"Tóm tắt"}],
            "evidence": [{"id":"e1", "company_id":"c1", "url":"https://example.com", "status":"unverified"}],
            "report_ready_ids": []
        });
        let hidden = render_workspace(&snapshot, "jobs", None).into_string();
        assert!(!hidden.contains("Kiểm chứng báo cáo"));
        assert!(hidden.contains("Chưa có bản lịch sử đầy đủ"));
        assert!(hidden.contains("Hội thoại hiện tại"));
        snapshot["report_ready_ids"] = json!(["j1"]);
        let ready = render_workspace(&snapshot, "jobs", None).into_string();
        assert!(ready.contains("Kiểm chứng báo cáo"));
        snapshot["evidence"][0]["status"] = json!("subject_confirmed");
        let confirmed = render_workspace(&snapshot, "dossier", Some("c1")).into_string();
        assert!(confirmed.contains("Chủ thể do người dùng xác nhận · nội dung chưa kiểm chứng"));
    }

    #[test]
    fn estimate_action_requires_a_numeric_estimate() {
        let snapshot = json!({"reservations":[
            {"id":"a", "kind":"reasoning", "status":"reserved", "reserved_cents":100, "bounded_estimate_cents":1},
            {"id":"b", "kind":"reasoning", "status":"reserved", "reserved_cents":100},
            {"id":"c", "kind":"reasoning", "status":"bounded", "actual_cents":1}
        ]});
        let page = render_workspace(&snapshot, "settings", None).into_string();
        assert_eq!(page.matches("data-action=\"budget-bound\"").count(), 1);
        assert!(page.contains("Dự phòng ước tính $0.01"));
        assert!(page.contains("không phải hóa đơn"));
    }

    #[test]
    fn monitoring_never_invents_a_connection_or_allows_an_unconfigured_run() {
        let page = render_workspace(&json!({}), "overview", None).into_string();
        assert!(page.contains("Chưa cấu hình"));
        assert!(page.contains("Chưa xác minh"));
        assert!(page.contains("data-action=\"scout-run\" disabled"));
        let disabled = render_workspace(
            &json!({"scout":{"configured":true,"enabled":false}}),
            "overview",
            None,
        )
        .into_string();
        assert!(disabled.contains("data-action=\"scout-run\" disabled"));
    }

    #[test]
    fn monitoring_shows_only_snapshot_sources_clusters_and_coverage() {
        let snapshot = json!({
            "scout":{"configured":true,"enabled":true,"status":"Đang chờ", "coverage":"2 nguồn", "last_run_at":"2026-11-02", "sources":[{"name":"Nguồn A", "status":"Khả dụng", "last_checked_at":"2026-11-02", "coverage":"Giới hạn"}]},
            "storage":{"configured":true,"mode":"MongoDB Atlas", "status":"Chưa xác minh", "clusters":[{"name":"A <script>","status":"Đang kiểm tra","region":"ap-southeast-1"},{"name":"B","status":"Không khả dụng"}]}
        });
        let page = render_workspace(&snapshot, "overview", None).into_string();
        assert!(page.contains("data-action=\"scout-run\">"));
        for expected in [
            "2 nguồn",
            "Nguồn A",
            "2026-11-02",
            "MongoDB Atlas",
            "ap-southeast-1",
            "Không khả dụng",
            "A &lt;script&gt;",
        ] {
            assert!(page.contains(expected), "missing {expected}");
        }
        assert!(!page.contains("A <script>"));
    }
    #[test]
    fn web_search_requires_a_separate_explicit_action() {
        let page = render_workspace(
            &json!({"search_query":"FPT <script>", "search_results":[], "investigations":[]}),
            "companies",
            None,
        )
        .into_string();
        assert!(page.contains("data-action=\"search-web\""));
        assert!(page.contains("FPT &lt;script&gt;"));
        assert!(!page.contains("FPT <script>"));
    }
}
