//! Optional, explicitly initialized MongoDB mirror. SQLite remains authoritative.
use lantern_mongo::{
    Claim, ClusterConfig, Company, Config, Error, MongoStore, PartialResults, RoutingTable,
    SourceSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha3::{Digest, Sha3_256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

const PIN_FILE: &str = "mongo-routing-v1.json";
const BOOTSTRAP_FILE: &str = "mongo-bootstrap-complete-v1.json";
const A: &str = "LANTERN_MONGO_URI_A";
const B: &str = "LANTERN_MONGO_URI_B";

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct RoutingPin {
    routing: RoutingTable,
    endpoint_a: String,
    endpoint_b: String,
}

/// A fingerprint of scheme and authority hosts, excluding credentials, path,
/// and options. It is a configuration guard, not a verified cluster identity.
fn endpoint_hash(uri: &str) -> Result<String, &'static str> {
    let uri = uri.trim();
    let (scheme, tail) = uri.split_once("://").ok_or("invalid_cluster_uri")?;
    if scheme != "mongodb" && scheme != "mongodb+srv" {
        return Err("invalid_cluster_uri");
    }
    let authority = tail.split(['/', '?', '#']).next().unwrap_or("");
    let hosts = authority.rsplit('@').next().unwrap_or("");
    if hosts.is_empty() || hosts.chars().any(char::is_whitespace) {
        return Err("invalid_cluster_uri");
    }
    let canonical = format!("{}://{}", scheme, hosts.to_ascii_lowercase());
    Ok(hex::encode(Sha3_256::digest(canonical.as_bytes())))
}

/// Status contains no connection strings, environment values, or driver errors.
#[derive(Clone)]
pub struct MongoRuntime {
    state: Arc<RwLock<RuntimeState>>,
    retry_gate: Arc<tokio::sync::Mutex<()>>,
    last_retry: Arc<Mutex<Option<Instant>>>,
}
#[derive(Clone)]
struct RuntimeState {
    store: Option<Arc<MongoStore>>,
    enabled: bool,
    configured: bool,
    code: &'static str,
    fingerprint: Option<String>,
    failed_cluster: Option<&'static str>,
}

fn setting(settings: &BTreeMap<String, String>, key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .or_else(|| settings.get(key).cloned())
}
fn enabled_setting(settings: &BTreeMap<String, String>, key: &str) -> bool {
    setting(settings, key).is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}
fn routing() -> RoutingTable {
    RoutingTable {
        version: 1,
        algorithm: "sha256-u64be-mod-v1".into(),
        owners: (0..256)
            .map(|i| if i % 2 == 0 { "a".into() } else { "b".into() })
            .collect(),
    }
}
fn config() -> Config {
    Config {
        clusters: vec![
            ClusterConfig {
                id: "a".into(),
                uri_env: A.into(),
                database: "lantern".into(),
            },
            ClusterConfig {
                id: "b".into(),
                uri_env: B.into(),
                database: "lantern".into(),
            },
        ],
        routing: routing(),
        timeout_ms: 5000,
    }
}
fn code_for_error(e: &Error) -> &'static str {
    match e {
        Error::Timeout { .. } => "timeout_outcome_unknown",
        Error::RoutingChanged | Error::RoutingMetadata(_) => "routing_pin_mismatch_or_missing",
        Error::Conflict => "idempotency_conflict",
        Error::Configuration(_) => "invalid_configuration",
        Error::Cluster { .. } => "cluster_unavailable_or_unauthorized",
        Error::Serialization => "invalid_data",
    }
}

fn candidate_response(result: PartialResults<Company>, limit: u32) -> Value {
    let complete = result.is_complete();
    json!({
        "scope": ["a", "b"],
        "limit_per_cluster": limit,
        "complete": complete,
        "candidates": result.items,
        "failed_clusters": result.failures.into_iter().map(|failure| json!({
            "id": failure.cluster,
            "reason": "cluster_read_failed"
        })).collect::<Vec<_>>(),
        "truncated_clusters": result.truncated_clusters,
    })
}

/// Create only with explicit init opt-in. create_new prevents replacement and
/// sync_all flushes file contents. A partial pin after a crash is rejected; it is
/// never silently recreated. The caller must ensure the directory is protected.
fn load_or_create_pin(
    dir: &Path,
    allow_create: bool,
    expected: &RoutingPin,
) -> Result<RoutingTable, &'static str> {
    let path = dir.join(PIN_FILE);
    if !path.exists() {
        if !allow_create {
            return Err("routing_pin_missing");
        }
        let bytes = serde_json::to_vec(expected).map_err(|_| "pin_encoding_failed")?;
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return read_pin(&path, expected);
            }
            Err(_) => return Err("pin_create_failed"),
        };
        file.write_all(&bytes).map_err(|_| "pin_write_failed")?;
        file.sync_all().map_err(|_| "pin_sync_failed")?;
        #[cfg(unix)]
        fs::File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|_| "pin_sync_failed")?;
    }
    read_pin(&path, expected)
}
fn read_pin(path: &Path, expected: &RoutingPin) -> Result<RoutingTable, &'static str> {
    let metadata = fs::metadata(path).map_err(|_| "pin_read_failed")?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err("pin_invalid");
    }
    let bytes = fs::read(path).map_err(|_| "pin_read_failed")?;
    let pin: RoutingPin = serde_json::from_slice(&bytes).map_err(|_| "pin_invalid")?;
    if &pin != expected {
        return Err("routing_pin_mismatch");
    }
    Ok(pin.routing)
}

/// The marker uses the same secret-free topology record as the pin. Existing
/// malformed or mismatched markers fail closed and are never replaced.
fn bootstrap_complete(dir: &Path, expected: &RoutingPin) -> Result<bool, &'static str> {
    let path = dir.join(BOOTSTRAP_FILE);
    match fs::metadata(&path) {
        Ok(_) => {
            read_pin(&path, expected).map_err(|_| "bootstrap_marker_invalid")?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err("bootstrap_marker_read_failed"),
    }
}
fn mark_bootstrap_complete(dir: &Path, expected: &RoutingPin) -> Result<(), &'static str> {
    let path = dir.join(BOOTSTRAP_FILE);
    let bytes = serde_json::to_vec(expected).map_err(|_| "bootstrap_marker_invalid")?;
    let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            read_pin(&path, expected).map_err(|_| "bootstrap_marker_invalid")?;
            return Ok(());
        }
        Err(_) => return Err("bootstrap_marker_create_failed"),
    };
    file.write_all(&bytes)
        .map_err(|_| "bootstrap_marker_write_failed")?;
    file.sync_all()
        .map_err(|_| "bootstrap_marker_sync_failed")?;
    #[cfg(unix)]
    fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|_| "bootstrap_marker_sync_failed")?;
    Ok(())
}

impl MongoRuntime {
    fn from_state(state: RuntimeState) -> Self {
        Self {
            state: Arc::new(RwLock::new(state)),
            retry_gate: Arc::new(tokio::sync::Mutex::new(())),
            last_retry: Arc::new(Mutex::new(None)),
        }
    }
    fn current_store(&self) -> Option<Arc<MongoStore>> {
        self.state.read().ok().and_then(|state| state.store.clone())
    }
    /// Opens only the pinned two-cluster topology. `true` init is an explicit,
    /// operator-authorized write to a fresh dedicated `lantern` database.
    pub async fn open(settings: &BTreeMap<String, String>, data_dir: &Path) -> Self {
        Self::from_state(Self::open_state(settings, data_dir).await)
    }
    async fn open_state(settings: &BTreeMap<String, String>, data_dir: &Path) -> RuntimeState {
        let enabled = enabled_setting(settings, "LANTERN_MONGO_ENABLED");
        let uri_a = setting(settings, A).filter(|s| !s.trim().is_empty());
        let uri_b = setting(settings, B).filter(|s| !s.trim().is_empty());
        let configured = uri_a.is_some() && uri_b.is_some();
        let status = |code, fingerprint| RuntimeState {
            store: None,
            enabled,
            configured,
            code,
            fingerprint,
            failed_cluster: None,
        };
        if !enabled {
            return status("disabled", None);
        }
        if !configured {
            return status(
                if uri_a.is_some() || uri_b.is_some() {
                    "partial_configuration"
                } else {
                    "configuration_missing"
                },
                None,
            );
        }
        let endpoint_a = match endpoint_hash(uri_a.as_deref().unwrap_or("")) {
            Ok(v) => v,
            Err(code) => return status(code, None),
        };
        let endpoint_b = match endpoint_hash(uri_b.as_deref().unwrap_or("")) {
            Ok(v) => v,
            Err(code) => return status(code, None),
        };
        if endpoint_a == endpoint_b {
            return status("duplicate_cluster_uri", None);
        }
        let init = enabled_setting(settings, "LANTERN_MONGO_INIT_EMPTY");
        let expected = RoutingPin {
            routing: routing(),
            endpoint_a,
            endpoint_b,
        };
        let was_complete = match bootstrap_complete(data_dir, &expected) {
            Ok(value) => value,
            Err(code) => return status(code, None),
        };
        let pin = match load_or_create_pin(data_dir, init && !was_complete, &expected) {
            Ok(pin) => pin,
            Err(code) => return status(code, None),
        };
        let fingerprint = match pin.fingerprint() {
            Ok(v) => Some(v),
            Err(_) => return status("pin_invalid", None),
        };
        let resolver = |key: &str| match key {
            A => uri_a.clone(),
            B => uri_b.clone(),
            _ => None,
        };
        // Attempt existing metadata first, so installations from before the
        // marker can upgrade without reinitializing an existing database.
        let mut opened = MongoStore::connect_with_resolver(config(), &pin, resolver).await;
        if !was_complete && init && matches!(&opened, Err(Error::RoutingMetadata(_))) {
            opened = MongoStore::initialize_empty_with_resolver(config(), &pin, resolver).await;
        }
        match opened {
            Ok(store) => {
                if !was_complete && let Err(code) = mark_bootstrap_complete(data_dir, &expected) {
                    return status(code, fingerprint);
                }
                RuntimeState {
                    store: Some(Arc::new(store)),
                    enabled,
                    configured,
                    code: "ready",
                    fingerprint,
                    failed_cluster: None,
                }
            }
            Err(e) => {
                let mut failed = status(code_for_error(&e), fingerprint);
                let id = match &e {
                    Error::Cluster { cluster, .. }
                    | Error::Timeout { cluster, .. }
                    | Error::RoutingMetadata(cluster) => Some(cluster.as_str()),
                    _ => None,
                };
                failed.failed_cluster = match id {
                    Some("a") => Some("a"),
                    Some("b") => Some("b"),
                    _ => None,
                };
                failed
            }
        }
    }
    /// Retry a failed initial connection without retaining settings or secrets.
    /// Concurrent calls coalesce; at most one attempt is started per 30 seconds.
    /// A retained client handles later connectivity outages via the Mongo driver.
    pub async fn try_reconnect(
        &self,
        settings: &BTreeMap<String, String>,
        data_dir: &Path,
    ) -> Value {
        let Ok(_guard) = self.retry_gate.try_lock() else {
            return self.status_json();
        };
        if self.current_store().is_some() {
            return self.status_json();
        }
        {
            let Ok(mut last) = self.last_retry.lock() else {
                return self.status_json();
            };
            if last.is_some_and(|at| at.elapsed() < Duration::from_secs(30)) {
                return self.status_json();
            }
            *last = Some(Instant::now());
        }
        let candidate = tokio::time::timeout(
            Duration::from_secs(30),
            Self::open_state(settings, data_dir),
        )
        .await;
        if let Ok(mut state) = self.state.write() {
            match candidate {
                Ok(next) => *state = next,
                Err(_) => {
                    state.code = "reconnect_timeout";
                    state.failed_cluster = None;
                }
            }
        }
        self.status_json()
    }
    pub fn status_json(&self) -> Value {
        let Ok(state) = self.state.read() else {
            return json!({"enabled":false,"configured":false,"ready":false,"status":"state_unavailable","clusters":[]});
        };
        json!({"enabled":state.enabled,"configured":state.configured,"ready":state.store.is_some(),"status":state.code,"failed_cluster":state.failed_cluster,"routing_fingerprint":state.fingerprint,"clusters":[{"id":"a"},{"id":"b"}]})
    }
    pub async fn health(&self) -> Value {
        let Some(store) = self.current_store() else {
            return self.status_json();
        };
        let clusters = store.health().await;
        let all_ok = clusters.iter().all(|c| c.ok);
        let fingerprint = self
            .state
            .read()
            .ok()
            .and_then(|state| state.fingerprint.clone());
        json!({"enabled":true,"configured":true,"ready":all_ok,"status":if all_ok {"ready"} else {"partial_unavailable"},"routing_fingerprint":fingerprint,"clusters":clusters.into_iter().map(|c|json!({"id":c.cluster,"ok":c.ok})).collect::<Vec<_>>()})
    }
    /// Read every candidate version from both configured clusters, bounded per
    /// cluster. An incomplete or truncated response must not be treated as a
    /// full snapshot. This does not select a latest version or paginate.
    pub async fn read_candidates(&self, per_cluster_limit: u32) -> Result<Value, &'static str> {
        let store = self.current_store().ok_or("mongo_not_ready")?;
        let scope = ["a".to_owned(), "b".to_owned()];
        let result = store
            .list_clusters::<Company>(&scope, per_cluster_limit)
            .await
            .map_err(|error| code_for_error(&error))?;
        Ok(candidate_response(result, per_cluster_limit))
    }
    /// Caller supplies an immutable event version; retry same payload/version on timeout.
    pub async fn put_candidate(
        &self,
        company_id: &str,
        version: &str,
        payload: Value,
    ) -> Result<(), &'static str> {
        let store = self.current_store().ok_or("mongo_not_ready")?;
        store
            .put(&Company {
                company_id: company_id.into(),
                version: version.into(),
                payload,
            })
            .await
            .map_err(|e| code_for_error(&e))
    }
    pub async fn put_source_snapshot(
        &self,
        company_id: &str,
        source_id: &str,
        version: &str,
        payload: Value,
    ) -> Result<(), &'static str> {
        let store = self.current_store().ok_or("mongo_not_ready")?;
        store
            .put(&SourceSnapshot {
                company_id: company_id.into(),
                source_id: source_id.into(),
                version: version.into(),
                payload,
            })
            .await
            .map_err(|e| code_for_error(&e))
    }
    pub async fn put_claim(
        &self,
        company_id: &str,
        claim_id: &str,
        version: &str,
        payload: Value,
    ) -> Result<(), &'static str> {
        let store = self.current_store().ok_or("mongo_not_ready")?;
        store
            .put(&Claim {
                company_id: company_id.into(),
                claim_id: claim_id.into(),
                version: version.into(),
                payload,
            })
            .await
            .map_err(|e| code_for_error(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routing_is_stable_and_balanced() {
        let t = routing();
        assert_eq!(t.owners.len(), 256);
        assert_eq!(t.owners.iter().filter(|v| *v == "a").count(), 128);
        assert_eq!(t.owners[1], "b");
        assert!(config().validate_against_pin(&t).is_ok());
    }
    #[test]
    fn endpoint_fingerprint_excludes_password_and_options() {
        let a = endpoint_hash(
            "mongodb+srv://alice:secret@EXAMPLE.mongodb.net/lantern?retryWrites=true",
        )
        .unwrap();
        let b = endpoint_hash("mongodb+srv://bob:other@example.mongodb.net/other").unwrap();
        assert_eq!(a, b);
        assert_ne!(
            a,
            endpoint_hash("mongodb+srv://example2.mongodb.net/").unwrap()
        );
        assert!(!a.contains("secret"));
        assert!(endpoint_hash("https://example.com").is_err());
    }
    #[test]
    fn pin_rejects_swapped_clusters_and_never_contains_secrets() {
        let pin = RoutingPin {
            routing: routing(),
            endpoint_a: endpoint_hash("mongodb+srv://u:p@a.example/").unwrap(),
            endpoint_b: endpoint_hash("mongodb+srv://u:p@b.example/").unwrap(),
        };
        let text = serde_json::to_string(&pin).unwrap();
        assert!(!text.contains("u:p"));
        let mut swapped = serde_json::from_str::<RoutingPin>(&text).unwrap();
        std::mem::swap(&mut swapped.endpoint_a, &mut swapped.endpoint_b);
        assert!(pin != swapped);
    }
    #[test]
    fn pin_creation_is_explicit_and_existing_pin_is_immutable() {
        let unique = format!(
            "lantern-routing-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir(&dir).unwrap();
        let pin = RoutingPin {
            routing: routing(),
            endpoint_a: endpoint_hash("mongodb://u:secret@a.example").unwrap(),
            endpoint_b: endpoint_hash("mongodb://u:secret@b.example").unwrap(),
        };
        assert_eq!(
            load_or_create_pin(&dir, false, &pin),
            Err("routing_pin_missing")
        );
        assert!(load_or_create_pin(&dir, true, &pin).is_ok());
        assert!(load_or_create_pin(&dir, false, &pin).is_ok());
        let changed = RoutingPin {
            routing: routing(),
            endpoint_a: pin.endpoint_b.clone(),
            endpoint_b: pin.endpoint_a.clone(),
        };
        assert_eq!(
            load_or_create_pin(&dir, true, &changed),
            Err("routing_pin_mismatch")
        );
        let contents = fs::read_to_string(dir.join(PIN_FILE)).unwrap();
        assert!(!contents.contains("secret"));
        fs::remove_file(dir.join(PIN_FILE)).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn completed_marker_forbids_recreating_a_missing_pin() {
        let unique = format!(
            "lantern-bootstrap-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir(&dir).unwrap();
        let expected = RoutingPin {
            routing: routing(),
            endpoint_a: endpoint_hash("mongodb://a.example").unwrap(),
            endpoint_b: endpoint_hash("mongodb://b.example").unwrap(),
        };
        assert!(!bootstrap_complete(&dir, &expected).unwrap());
        mark_bootstrap_complete(&dir, &expected).unwrap();
        let complete = bootstrap_complete(&dir, &expected).unwrap();
        assert!(complete);
        assert_eq!(
            load_or_create_pin(&dir, true && !complete, &expected),
            Err("routing_pin_missing")
        );
        let wrong = RoutingPin {
            routing: routing(),
            endpoint_a: expected.endpoint_b.clone(),
            endpoint_b: expected.endpoint_a.clone(),
        };
        assert_eq!(
            bootstrap_complete(&dir, &wrong),
            Err("bootstrap_marker_invalid")
        );
        fs::remove_file(dir.join(BOOTSTRAP_FILE)).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn disabled_runtime_never_exposes_config_values() {
        let runtime = MongoRuntime::from_state(RuntimeState {
            store: None,
            enabled: false,
            configured: true,
            code: "disabled",
            fingerprint: None,
            failed_cluster: None,
        });
        let s = runtime.status_json().to_string();
        assert!(!s.contains("mongodb://"));
        assert_eq!(runtime.status_json()["status"], "disabled");
    }
    #[tokio::test]
    async fn reconnect_coalesces_and_honors_cooldown_without_network() {
        let runtime = MongoRuntime::from_state(RuntimeState {
            store: None,
            enabled: true,
            configured: true,
            code: "cluster_unavailable_or_unauthorized",
            fingerprint: None,
            failed_cluster: Some("b"),
        });
        let guard = runtime.retry_gate.lock().await;
        let result = runtime
            .try_reconnect(&BTreeMap::new(), Path::new("unused"))
            .await;
        assert_eq!(result["status"], "cluster_unavailable_or_unauthorized");
        drop(guard);
        *runtime.last_retry.lock().unwrap() = Some(Instant::now());
        let result = runtime
            .try_reconnect(&BTreeMap::new(), Path::new("unused"))
            .await;
        assert_eq!(result["failed_cluster"], "b");
        assert!(!result.to_string().contains("mongodb://"));
        assert_eq!(runtime.status_json()["ready"], false);
    }
    #[test]
    fn candidate_read_reports_cluster_failure_and_truncation() {
        use lantern_mongo::ClusterFailure;
        let row = Company {
            company_id: "company-1".into(),
            version: "event-1".into(),
            payload: json!({"name":"Example"}),
        };
        let partial = PartialResults {
            items: vec![row],
            failures: vec![ClusterFailure {
                cluster: "b".into(),
                reason: "sensitive underlying error must not be returned",
            }],
            truncated_clusters: vec!["a".into()],
        };
        let value = candidate_response(partial, 10);
        assert_eq!(value["complete"], false);
        assert_eq!(value["candidates"][0]["company_id"], "company-1");
        assert_eq!(value["failed_clusters"][0]["id"], "b");
        assert_eq!(value["truncated_clusters"][0], "a");
        assert!(!value.to_string().contains("sensitive underlying error"));
        let complete = PartialResults::<Company> {
            items: vec![],
            failures: vec![],
            truncated_clusters: vec![],
        };
        assert_eq!(candidate_response(complete, 10)["complete"], true);
    }
}
