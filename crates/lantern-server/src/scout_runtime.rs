//! Opt-in GLEIF Scout runtime. No raw documents or unverified MSTs are imported.
use crate::{
    mongo_runtime::MongoRuntime,
    scout_store::{OutboxItem, SqliteScoutStore},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use lantern_core::{Company, Store};
use lantern_scout::{GleifCandidate, SourcePolicy, poll_once};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;

const SOURCE_ID: &str = "gleif-vn-latest-100-v1";
const COVERAGE: &str = "Tối đa 100 hồ sơ LEI có địa chỉ pháp lý tại Việt Nam được cập nhật gần đây; không bao phủ toàn bộ doanh nghiệp và chưa có phân trang lịch sử.";
fn policy(enabled: bool) -> SourcePolicy {
    SourcePolicy {source_id:SOURCE_ID.into(),
        source_url:"https://api.gleif.org/api/v1/lei-records?filter%5Bentity.legalAddress.country%5D=VN&page%5Bsize%5D=100&sort=-registration.lastUpdateDate".into(),
        enabled,access_authorized:true,robots_and_terms_checked:true,
        allowed_hosts:vec!["api.gleif.org".into()],min_interval_seconds:21_600,
        max_requests_per_day:4,timeout_seconds:30,max_bytes:5*1024*1024,max_attempts:3,
        allowed_content_types:vec!["application/vnd.api+json".into(),"application/json".into()],
        process_content_authorized:true}
}
#[derive(Default)]
struct RuntimeState {
    last_error: Option<&'static str>,
}
pub struct ScoutRuntime {
    scout: SqliteScoutStore,
    store: Store,
    mongo: Option<Arc<MongoRuntime>>,
    wake: Notify,
    state: Mutex<RuntimeState>,
}
impl ScoutRuntime {
    /// `enabled` is an explicit opt-in used only on first creation; existing state is never reset.
    pub fn open(
        data_dir: &Path,
        store: Store,
        mongo: Option<Arc<MongoRuntime>>,
        enabled: bool,
    ) -> Result<Self> {
        let scout = SqliteScoutStore::open(data_dir.join("scout.db"), vec![policy(enabled)])?;
        Ok(Self {
            scout,
            store,
            mongo,
            wake: Notify::new(),
            state: Mutex::new(RuntimeState::default()),
        })
    }
    fn error(&self, code: Option<&'static str>) {
        if let Ok(mut state) = self.state.lock() {
            state.last_error = code;
        }
    }
    /// The runtime is independent of chat. Dropping its task stops the scheduler;
    /// any interrupted lease expires and is fenced before another worker can claim it.
    pub async fn run(self: Arc<Self>) {
        self.run_loop().await;
    }
    async fn run_loop(&self) {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! { _=tick.tick()=>{}, _=self.wake.notified()=>{} }
            // Do not collect further source documents while downstream work cannot be stored.
            if self.drain_outbox().await.is_err() {
                self.error(Some("outbox_pending"));
                continue;
            }
            self.error(None);
            match poll_once(&self.scout).await {
                Ok(Some(_)) => self.error(None),
                Ok(None) => {}
                Err(_) => {
                    self.error(Some("scout_or_outbox_error"));
                    continue;
                }
            }
            if self.drain_outbox().await.is_err() {
                self.error(Some("outbox_pending"));
            }
        }
    }
    async fn drain_outbox(&self) -> Result<()> {
        for item in self.scout.pending_outbox(10)? {
            self.apply_item(&item).await?;
            self.scout.ack_outbox(&item.id)?;
        }
        Ok(())
    }
    async fn apply_item(&self, item: &OutboxItem) -> Result<()> {
        for candidate in &item.candidates {
            let id = format!("gleif:{}", candidate.lei);
            if let Some(mongo) = &self.mongo {
                // Exactly the same payload/version is retried after a timeout; no observation
                // timestamps or local watch flags in immutable content-addressed Mongo versions.
                let payload = serde_json::to_value(candidate)?;
                mongo
                    .put_candidate(&id, &item.content_hash, payload.clone())
                    .await
                    .map_err(|_| anyhow::anyhow!("mongo candidate write failed"))?;
                mongo
                    .put_source_snapshot(&id, SOURCE_ID, &item.content_hash, payload)
                    .await
                    .map_err(|_| anyhow::anyhow!("mongo source snapshot failed"))?;
            }
            let existing = self.store.get_company(&id)?;
            if let Some(company) = map_candidate(candidate, item.observed_at, existing.as_ref()) {
                self.store.save_company(&company)?;
            }
        }
        Ok(())
    }
    pub fn set_enabled(&self, enabled: bool) -> Result<()> {
        self.scout.set_enabled(SOURCE_ID, enabled)?;
        self.wake.notify_one();
        Ok(())
    }
    pub fn run_now(&self) -> Result<Value> {
        let due = self.scout.run_now(SOURCE_ID, Utc::now())?;
        self.wake.notify_one();
        Ok(
            json!({"ok":true,"scheduled_at":due,"message":"Lịch được giới hạn bởi hạn mức và thời gian chờ của nguồn."}),
        )
    }
    /// Explicit operator action after reviewing a suspended 429 source; never called automatically.
    pub fn resume_after_review(&self) -> Result<Value> {
        let source = self
            .scout
            .snapshot()?
            .into_iter()
            .find(|s| s.source_id == SOURCE_ID)
            .context("Scout source missing")?;
        self.scout
            .resume_origin_after_review(SOURCE_ID, Utc::now())?;
        if matches!(source.state.as_str(), "blocked" | "exhausted") {
            self.scout.set_enabled(SOURCE_ID, false)?;
            self.scout.set_enabled(SOURCE_ID, true)?;
        }
        self.wake.notify_one();
        Ok(
            json!({"ok":true,"message":"Đã ghi nhận rà soát; hạn mức và khoảng chờ nguồn vẫn được giữ."}),
        )
    }
    pub fn status(&self) -> Result<Value> {
        let source = self
            .scout
            .snapshot()?
            .into_iter()
            .find(|s| s.source_id == SOURCE_ID)
            .context("Scout source missing")?;
        let runtime_error = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Scout status unavailable"))?
            .last_error;
        let mongo_ready = self
            .mongo
            .as_ref()
            .is_none_or(|mongo| mongo.status_json()["ready"] == true);
        let state = if !source.enabled {
            "disabled"
        } else if source.origin_suspended
            || matches!(source.state.as_str(), "blocked" | "exhausted")
        {
            "needs_review"
        } else if source.state == "leased" {
            "running"
        } else if source.outbox_pending > 0 && !mongo_ready {
            "waiting_mongo"
        } else if runtime_error.is_some() {
            "attention"
        } else {
            "scheduled"
        };
        Ok(
            json!({"configured":true,"enabled":source.enabled,"status":state,
            "last_run_at":source.last_observed,"next_run_at":source.next_due,"coverage":COVERAGE,
            "origin_suspended":source.origin_suspended,
            "outbox_pending":source.outbox_pending,"message":if source.origin_suspended {Some("Nguồn tạm dừng; cần người vận hành rà soát.")}else if runtime_error.is_some(){Some("Có công việc cần thử lại; không xóa outbox.")}else{None},
            "sources":[{"name":"GLEIF LEI Việt Nam","status":state,"origin_suspended":source.origin_suspended,"last_checked_at":source.last_observed,"coverage":COVERAGE}]}),
        )
    }
}
/// LEI stays its own namespace. `registered_as` is shown as an unverified registry identifier,
/// never mapped to MST. No cross-entity name matching. Preserve newer local/user edits.
fn map_candidate(
    candidate: &GleifCandidate,
    observed: DateTime<Utc>,
    existing: Option<&Company>,
) -> Option<Company> {
    if let Some(old) = existing {
        if old.identity_status != "candidate_unverified" {
            return None;
        }
        if DateTime::parse_from_rfc3339(&old.updated_at)
            .ok()
            .is_some_and(|t| t.with_timezone(&Utc) > observed)
        {
            return None;
        }
    }
    let registered = candidate.registered_as.as_deref().unwrap_or("chưa có");
    let description = format!(
        "Nguồn GLEIF LEI {}; mã đăng ký theo nguồn: {}; trạng thái LEI: {}; trạng thái pháp nhân theo nguồn: {}. Chưa đối chiếu mã số thuế hoặc xác nhận danh tính.",
        candidate.lei,
        registered,
        candidate.lei_status.as_deref().unwrap_or("chưa rõ"),
        candidate
            .legal_entity_status
            .as_deref()
            .unwrap_or("chưa rõ")
    );
    let mut company = existing.cloned().unwrap_or_else(|| Company {
        id: format!("gleif:{}", candidate.lei),
        name: String::new(),
        tax_id: None,
        ticker: None,
        sector: None,
        location: Some("VN".into()),
        website: None,
        source_url: None,
        description: None,
        identity_status: "candidate_unverified".into(),
        watched: false,
        updated_at: observed.to_rfc3339(),
    });
    company.name = candidate.legal_name.clone();
    company.source_url = Some(candidate.source_url.clone());
    company.description = Some(description);
    company.updated_at = observed.to_rfc3339();
    Some(company)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(lei: &str) -> GleifCandidate {
        GleifCandidate {
            lei: lei.into(),
            legal_name: "Synthetic Legal Entity".into(),
            registered_as: Some("REG-ONLY".into()),
            registration_authority: Some("RA000000".into()),
            lei_status: Some("LAPSED".into()),
            legal_entity_status: Some("ACTIVE".into()),
            country: "VN".into(),
            source_url: format!("https://api.gleif.org/api/v1/lei-records/{lei}"),
            last_updated: None,
        }
    }
    #[test]
    fn mapping_keeps_lei_and_mst_distinct_and_preserves_watch() -> Result<()> {
        let date = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")?.with_timezone(&Utc);
        let c = candidate("11111111111111111111");
        let first = map_candidate(&c, date, None).unwrap();
        assert_eq!(first.id, "gleif:11111111111111111111");
        assert_eq!(first.tax_id, None);
        assert!(first.description.as_ref().unwrap().contains("REG-ONLY"));
        assert_eq!(first.identity_status, "candidate_unverified");
        let mut existing = first.clone();
        existing.watched = true;
        existing.tax_id = Some("0123456789".into());
        let replay = map_candidate(&c, date, Some(&existing)).unwrap();
        assert!(replay.watched);
        assert_eq!(replay.tax_id, existing.tax_id);
        existing.identity_status = "confirmed".into();
        assert!(map_candidate(&c, date, Some(&existing)).is_none());
        assert_ne!(
            first.id,
            map_candidate(&candidate("22222222222222222222"), date, None)
                .unwrap()
                .id
        );
        Ok(())
    }
    #[test]
    fn supervisor_future_is_spawnable_and_opted_out() -> Result<()> {
        fn require_send<T: Send>(_: T) {}
        let dir = std::env::temp_dir().join(format!("scout-runtime-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let runtime = Arc::new(ScoutRuntime::open(&dir, Store::memory()?, None, false)?);
        assert_eq!(runtime.status()?["enabled"], false);
        assert_eq!(runtime.status()?["origin_suspended"], false);
        assert!(runtime.status()?["next_run_at"].is_null());
        require_send(runtime.run());
        Ok(())
    }
    #[test]
    fn suspended_origin_is_visible_and_has_no_next_run() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("scout-runtime-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir)?;
        let runtime = ScoutRuntime::open(&dir, Store::memory()?, None, true)?;
        let conn = rusqlite::Connection::open(dir.join("scout.db"))?;
        conn.execute(
            "INSERT INTO scout_origins(origin,utc_day,requests,next_allowed_ms) VALUES(?1,?2,1,?3)",
            rusqlite::params![
                "api.gleif.org",
                Utc::now().date_naive().to_string(),
                i64::MAX
            ],
        )?;
        let status = runtime.status()?;
        assert_eq!(status["status"], "needs_review");
        assert_eq!(status["origin_suspended"], true);
        assert_eq!(status["sources"][0]["origin_suspended"], true);
        assert!(status["next_run_at"].is_null());
        Ok(())
    }
    #[test]
    fn policy_is_opt_in_and_page_scope_is_explicit() {
        let p = policy(false);
        assert!(!p.enabled);
        assert_eq!(p.max_requests_per_day, 4);
        assert_eq!(p.min_interval_seconds, 21600);
        assert!(p.source_url.contains("page%5Bsize%5D=100"));
        assert!(
            p.allowed_content_types
                .contains(&"application/vnd.api+json".to_string())
        );
        assert!(COVERAGE.contains("100"));
    }
}
