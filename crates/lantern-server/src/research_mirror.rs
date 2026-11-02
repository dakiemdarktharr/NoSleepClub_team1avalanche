//! Durable, optional research mirror. SQLite remains the source of truth.
//! One worker per process; multiple server processes require external fencing.
//! Mongo versions are immutable historical suggestions. Review decisions after
//! delivery are not mirrored or retracted: consumers must check current SQLite
//! review state, and must not use these records directly for risk ranking.
use crate::mongo_runtime::MongoRuntime;
use lantern_core::{ResearchOutboxItem, Store};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, Notify};

static WORKER_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
static DRAIN_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());

/// Serialize evidence review with in-process delivery. Hold only while the
/// SQLite review transaction runs; a version already delivered stays immutable.
/// Multiple server processes require a separate cross-process fencing protocol.
pub async fn review_guard() -> tokio::sync::MutexGuard<'static, ()> {
    WORKER_LOCK.lock().await
}

#[derive(Default)]
struct State {
    last_error: Option<&'static str>,
}
#[derive(Debug, Clone, Copy)]
pub struct MirrorRun {
    pub delivered: usize,
    pub failed: usize,
}
pub struct ResearchMirror {
    store: Store,
    mongo: Option<Arc<MongoRuntime>>,
    wake: Notify,
    state: Mutex<State>,
}
impl ResearchMirror {
    pub fn new(store: Store, mongo: Option<Arc<MongoRuntime>>) -> Self {
        Self {
            store,
            mongo,
            wake: Notify::new(),
            state: Mutex::new(State::default()),
        }
    }
    pub fn wake(&self) {
        self.wake.notify_one();
    }
    fn record_error(&self, error: Option<&'static str>) {
        if let Ok(mut state) = self.state.lock() {
            state.last_error = error;
        }
    }
    /// A dropped task never marks a pending item delivered. Restart replays it.
    pub async fn run(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! { _ = interval.tick() => {}, _ = self.wake.notified() => {} }
            let _ = self.drain_once().await;
        }
    }
    /// Processes at most 50 items. On uncertainty, keep pending and retry the
    /// identical stable version. A successful ack follows the Mongo majority ack.
    pub async fn drain_once(&self) -> Result<MirrorRun, &'static str> {
        let _guard = DRAIN_LOCK.try_lock().map_err(|_| "mirror_busy")?;
        let Some(mongo) = &self.mongo else {
            self.record_error(Some("mongo_not_configured"));
            return Err("mongo_not_configured");
        };
        let mongo = Arc::clone(mongo);
        let result = drain_with(&self.store, move |item| {
            let mongo = Arc::clone(&mongo);
            async move { deliver(&mongo, &item).await }
        })
        .await?;
        self.record_error((result.failed > 0).then_some("mongo_delivery_pending"));
        Ok(result)
    }
    pub fn status(&self) -> Result<Value, &'static str> {
        let counts = self
            .store
            .research_outbox_counts()
            .map_err(|_| "outbox_unavailable")?;
        let error = self
            .state
            .lock()
            .map_err(|_| "mirror_state_unavailable")?
            .last_error;
        Ok(
            json!({"configured":self.mongo.is_some(),"pending":counts.pending,"delivered":counts.delivered,"last_error":error}),
        )
    }
}
async fn drain_with<F, Fut>(store: &Store, mut send: F) -> Result<MirrorRun, &'static str>
where
    F: FnMut(ResearchOutboxItem) -> Fut,
    Fut: std::future::Future<Output = Result<(), &'static str>>,
{
    let pending = store
        .research_outbox_pending(50)
        .map_err(|_| "outbox_unavailable")?;
    let mut result = MirrorRun {
        delivered: 0,
        failed: 0,
    };
    for item in pending {
        // A review may remove pending work after this batch was selected.
        // Serialize one item at a time and recheck under the same review lock.
        let _item_guard = WORKER_LOCK.lock().await;
        if !store
            .research_outbox_is_pending(&item.id)
            .map_err(|_| "outbox_unavailable")?
        {
            continue;
        }
        if send(item.clone()).await.is_ok() {
            store
                .research_outbox_delivered(&item.id)
                .map_err(|_| "outbox_ack_failed")?;
            result.delivered += 1;
        } else {
            store
                .research_outbox_failed(&item.id)
                .map_err(|_| "outbox_update_failed")?;
            result.failed += 1;
        }
    }
    Ok(result)
}
async fn deliver(mongo: &MongoRuntime, item: &ResearchOutboxItem) -> Result<(), &'static str> {
    match item.kind.as_str() {
        "company" => {
            mongo
                .put_candidate(&item.company_id, &item.version, item.payload.clone())
                .await
        }
        "claim" => {
            mongo
                .put_claim(
                    &item.company_id,
                    &item.entity_id,
                    &item.version,
                    item.payload.clone(),
                )
                .await
        }
        _ => Err("invalid_outbox_kind"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn no_mongo_does_not_ack_or_delete_outbox() {
        use lantern_core::{Company, Investigation};
        let store = Store::memory().unwrap();
        let job = Investigation {
            id: "job".into(),
            company_id: None,
            query: "private query".into(),
            as_of: None,
            status: "queued".into(),
            stage: "queued".into(),
            summary: String::new(),
            limitations: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        store.create_investigation(&job).unwrap();
        store
            .transition_investigation("job", "running", "fetch", "started")
            .unwrap();
        store
            .transition_investigation("job", "verifying", "verify", "checking")
            .unwrap();
        let candidate = Company {
            id: "c".into(),
            name: "Example".into(),
            tax_id: None,
            ticker: None,
            sector: None,
            location: None,
            website: None,
            source_url: Some("https://example.org/firm".into()),
            description: None,
            identity_status: "candidate_unverified".into(),
            watched: false,
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        store
            .finalize_import("job", &[candidate], &[], "", &[], "completed")
            .unwrap();
        let before = store.research_outbox_pending(10).unwrap();
        assert_eq!(before.len(), 1);
        let mirror = ResearchMirror::new(store.clone(), None);
        assert!(matches!(
            mirror.drain_once().await,
            Err("mongo_not_configured")
        ));
        assert_eq!(mirror.status().unwrap()["pending"], 1);
        assert_eq!(store.research_outbox_pending(10).unwrap(), before);
        let sent: Arc<Mutex<Vec<(String, String, Value)>>> = Arc::new(Mutex::new(vec![]));
        let writes = Arc::clone(&sent);
        let first = drain_with(&store, move |item| {
            writes
                .lock()
                .unwrap()
                .push((item.id, item.version, item.payload));
            async { Err("simulated_timeout") }
        })
        .await
        .unwrap();
        assert_eq!(first.failed, 1);
        assert_eq!(first.delivered, 0);
        let pending = store.research_outbox_pending(10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].version, before[0].version);
        let writes = Arc::clone(&sent);
        let second = drain_with(&store, move |item| {
            writes
                .lock()
                .unwrap()
                .push((item.id, item.version, item.payload));
            async { Ok(()) }
        })
        .await
        .unwrap();
        assert_eq!(second.failed, 0);
        assert_eq!(second.delivered, 1);
        let writes = sent.lock().unwrap();
        assert_eq!(writes[0], writes[1]);
        assert_eq!(store.research_outbox_counts().unwrap().pending, 0);
    }
    #[test]
    fn worker_is_spawnable() {
        fn require_send<T: Send>(_: T) {}
        require_send(Arc::new(ResearchMirror::new(Store::memory().unwrap(), None)).run());
    }
    #[tokio::test]
    async fn review_before_dispatch_suppresses_pending_claim() {
        use lantern_core::{Company, Evidence, Investigation};
        use std::sync::atomic::{AtomicBool, Ordering};
        let store = Store::memory().unwrap();
        let company = Company {
            id: "c".into(),
            name: "Example".into(),
            tax_id: None,
            ticker: None,
            sector: None,
            location: None,
            website: None,
            source_url: Some("https://example.org/c".into()),
            description: None,
            identity_status: "candidate_unverified".into(),
            watched: false,
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        store.save_company(&company).unwrap();
        let job = Investigation {
            id: "j".into(),
            company_id: Some("c".into()),
            query: "private".into(),
            as_of: None,
            status: "queued".into(),
            stage: "queued".into(),
            summary: String::new(),
            limitations: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        store.create_investigation(&job).unwrap();
        store
            .transition_investigation("j", "running", "fetch", "start")
            .unwrap();
        store
            .transition_investigation("j", "verifying", "verify", "check")
            .unwrap();
        let e = Evidence {
            id: "e".into(),
            company_id: "c".into(),
            title: "source".into(),
            url: "https://example.org/source".into(),
            publisher: "publisher".into(),
            claim: "claim".into(),
            dimension: "legal".into(),
            status: "unverified".into(),
            published_at: None,
            observed_at: "2026-01-01T00:00:00Z".into(),
        };
        store
            .finalize_import("j", &[], &[e], "", &[], "completed")
            .unwrap();
        let guard = review_guard().await;
        let called = Arc::new(AtomicBool::new(false));
        let sent = Arc::clone(&called);
        let worker_store = store.clone();
        let task = tokio::spawn(async move {
            drain_with(&worker_store, move |_| {
                sent.store(true, Ordering::SeqCst);
                async { Ok(()) }
            })
            .await
        });
        tokio::task::yield_now().await;
        store.review_evidence("e", "not_about_company").unwrap();
        drop(guard);
        let result = task.await.unwrap().unwrap();
        assert_eq!(result.delivered, 0);
        assert!(!called.load(Ordering::SeqCst));
        assert_eq!(store.research_outbox_counts().unwrap().pending, 0);
    }
}
