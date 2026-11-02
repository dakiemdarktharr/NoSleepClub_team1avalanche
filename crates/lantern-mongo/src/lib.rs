//! Explicitly routed, append-only MongoDB persistence. No cross-cluster atomicity.
//!
//! A caller MUST durably pin `RoutingTable` outside the clusters and supply that
//! same trusted table on every start. This crate never performs bucket migration,
//! automatic failover, or a routing-table update. See the crate README.
#![forbid(unsafe_code)]

use futures_util::TryStreamExt;
use mongodb::{
    Client, Database, IndexModel,
    bson::{Document, doc},
    options::{Acknowledgment, ClientOptions, WriteConcern},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    time::Duration,
};
use thiserror::Error;
use tokio::time::timeout;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Configuration(&'static str),
    #[error("routing table differs from the externally persisted pin")]
    RoutingChanged,
    #[error("routing metadata differs or is missing on cluster {0}")]
    RoutingMetadata(String),
    #[error("cluster {cluster}: {operation} failed")]
    Cluster {
        cluster: String,
        operation: &'static str,
    },
    #[error("cluster {cluster}: {operation} timed out; write outcome may be unknown")]
    Timeout {
        cluster: String,
        operation: &'static str,
    },
    #[error("idempotency conflict: same record identity has different content")]
    Conflict,
    #[error("serialization failed")]
    Serialization,
}
pub type Result<T> = std::result::Result<T, Error>;

/// Mapping position is the virtual bucket; the key algorithm is fixed for v1.
/// Treat the entire table as immutable until an explicit audited migration exists.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutingTable {
    pub version: u64,
    pub algorithm: String,
    pub owners: Vec<String>,
}
impl RoutingTable {
    pub fn validate(&self) -> Result<()> {
        if self.version == 0
            || self.version > i64::MAX as u64
            || self.algorithm != "sha256-u64be-mod-v1"
            || self.owners.is_empty()
            || self.owners.len() > 65536
            || self.owners.iter().any(|x| x.trim().is_empty())
        {
            return Err(Error::Configuration("invalid routing table"));
        }
        Ok(())
    }
    pub fn bucket(&self, company_id: &str) -> Result<usize> {
        self.validate()?;
        required(company_id)?;
        let hash = Sha256::digest(company_id.as_bytes());
        let mut prefix = [0; 8];
        prefix.copy_from_slice(&hash[..8]);
        Ok((u64::from_be_bytes(prefix) % self.owners.len() as u64) as usize)
    }
    pub fn owner(&self, company_id: &str) -> Result<&str> {
        Ok(&self.owners[self.bucket(company_id)?])
    }
    pub fn fingerprint(&self) -> Result<String> {
        self.validate()?;
        Ok(hex_digest(
            &serde_json::to_vec(self).map_err(|_| Error::Serialization)?,
        ))
    }
}

/// URI is loaded only from the named environment variable; never serialize a URI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub id: String,
    pub uri_env: String,
    pub database: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub clusters: Vec<ClusterConfig>,
    pub routing: RoutingTable,
    pub timeout_ms: u64,
}
impl Config {
    pub fn validate_against_pin(&self, pinned: &RoutingTable) -> Result<()> {
        self.routing.validate()?;
        pinned.validate()?;
        if &self.routing != pinned {
            return Err(Error::RoutingChanged);
        }
        if !(100..=120_000).contains(&self.timeout_ms) || self.clusters.is_empty() {
            return Err(Error::Configuration("timeout or clusters"));
        }
        let mut ids = BTreeSet::new();
        for c in &self.clusters {
            if c.id.trim().is_empty()
                || c.uri_env.is_empty()
                || !c
                    .uri_env
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                || c.database.trim().is_empty()
                || !ids.insert(c.id.as_str())
            {
                return Err(Error::Configuration("cluster id, env or database"));
            }
        }
        if self
            .routing
            .owners
            .iter()
            .any(|owner| !ids.contains(owner.as_str()))
        {
            return Err(Error::Configuration("bucket owner is not configured"));
        }
        Ok(())
    }
}

/// Every record version is immutable; `version` is a stable event/source-version ID.
/// Supplying the same identity and payload is a no-op; differing payload is conflict.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Company {
    pub company_id: String,
    pub version: String,
    pub payload: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourceSnapshot {
    pub company_id: String,
    pub source_id: String,
    pub version: String,
    pub payload: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Claim {
    pub company_id: String,
    pub claim_id: String,
    pub version: String,
    pub payload: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Job {
    pub company_id: String,
    pub job_id: String,
    pub version: String,
    pub payload: Value,
}

pub trait Record: Serialize + DeserializeOwned + Send + Sync {
    const COLLECTION: &'static str;
    fn company_id(&self) -> &str;
    fn entity_id(&self) -> &str;
    fn version(&self) -> &str;
}
macro_rules! record {
    ($ty:ty, $collection:literal, $field:ident) => {
        impl Record for $ty {
            const COLLECTION: &'static str = $collection;
            fn company_id(&self) -> &str {
                &self.company_id
            }
            fn entity_id(&self) -> &str {
                &self.$field
            }
            fn version(&self) -> &str {
                &self.version
            }
        }
    };
}
record!(Company, "companies", company_id);
record!(SourceSnapshot, "source_snapshots", source_id);
record!(Claim, "claims", claim_id);
record!(Job, "jobs", job_id);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterFailure {
    pub cluster: String,
    pub reason: &'static str,
}
#[derive(Clone, Debug)]
pub struct PartialResults<T> {
    pub items: Vec<T>,
    pub failures: Vec<ClusterFailure>,
    pub truncated_clusters: Vec<String>,
}
impl<T> PartialResults<T> {
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty() && self.truncated_clusters.is_empty()
    }
}
#[derive(Clone, Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub truncated: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterHealth {
    pub cluster: String,
    pub ok: bool,
}

pub struct MongoStore {
    routing: RoutingTable,
    databases: BTreeMap<String, Database>,
    deadline: Duration,
}

fn required(s: &str) -> Result<()> {
    if s.trim().is_empty() {
        Err(Error::Configuration("record identity cannot be empty"))
    } else {
        Ok(())
    }
}
fn enforce_write_policy(options: &mut ClientOptions, deadline: Duration) {
    options.server_selection_timeout = Some(deadline);
    options.connect_timeout = Some(deadline);
    // Overrides URI/server choices such as w=1. A write-concern error or
    // timeout must never be converted into a successful outbox acknowledgement.
    options.write_concern = Some(WriteConcern::builder().w(Acknowledgment::Majority).build());
}
fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn record_key<T: Record>(v: &T) -> Result<String> {
    required(v.company_id())?;
    required(v.entity_id())?;
    required(v.version())?;
    let framed = serde_json::to_vec(&(T::COLLECTION, v.company_id(), v.entity_id(), v.version()))
        .map_err(|_| Error::Serialization)?;
    Ok(hex_digest(&framed))
}
fn item_document<T: Record>(v: &T, table: &RoutingTable) -> Result<Document> {
    let json = serde_json::to_string(v).map_err(|_| Error::Serialization)?;
    Ok(
        doc! { "_id": record_key(v)?, "company_id": v.company_id(), "entity_id": v.entity_id(), "version": v.version(), "bucket": table.bucket(v.company_id())? as i64, "routing_version": table.version as i64, "digest": hex_digest(json.as_bytes()), "payload_json": json },
    )
}
fn verify_replay(proposed: &Document, existing: &Document) -> Result<()> {
    if proposed.get_str("digest").ok() == existing.get_str("digest").ok()
        && proposed.get_str("payload_json").ok() == existing.get_str("payload_json").ok()
    {
        Ok(())
    } else {
        Err(Error::Conflict)
    }
}
fn decode<T: Record>(d: Document) -> Result<T> {
    serde_json::from_str(
        d.get_str("payload_json")
            .map_err(|_| Error::Serialization)?,
    )
    .map_err(|_| Error::Serialization)
}

fn decode_checked<T: Record>(d: Document, owner: &str, table: &RoutingTable) -> Result<T> {
    let record: T = decode(d.clone())?;
    if table.owner(record.company_id())? != owner {
        return Err(Error::Serialization);
    }
    let expected = item_document(&record, table)?;
    for field in [
        "_id",
        "company_id",
        "entity_id",
        "version",
        "bucket",
        "routing_version",
        "digest",
        "payload_json",
    ] {
        if d.get(field) != expected.get(field) {
            return Err(Error::Serialization);
        }
    }
    Ok(record)
}

impl MongoStore {
    /// Connects lazily, verifies the caller's trusted durable pin, and verifies
    /// existing metadata on every configured cluster. Never initializes metadata.
    pub async fn connect(config: Config, pinned: &RoutingTable) -> Result<Self> {
        Self::connect_with_resolver(config, pinned, |name| env::var(name).ok()).await
    }
    /// Resolve connection URIs from caller-owned private settings without modifying
    /// process environment. The resolver and values are never logged or returned.
    pub async fn connect_with_resolver(
        config: Config,
        pinned: &RoutingTable,
        resolver: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        config.validate_against_pin(pinned)?;
        let deadline = Duration::from_millis(config.timeout_ms);
        let mut databases = BTreeMap::new();
        for c in &config.clusters {
            let uri = resolver(&c.uri_env)
                .filter(|uri| !uri.trim().is_empty())
                .ok_or(Error::Configuration("missing MongoDB URI setting"))?;
            let mut options = timeout(deadline, ClientOptions::parse(&uri))
                .await
                .map_err(|_| Error::Timeout {
                    cluster: c.id.clone(),
                    operation: "parse URI",
                })?
                .map_err(|_| Error::Cluster {
                    cluster: c.id.clone(),
                    operation: "parse URI",
                })?;
            enforce_write_policy(&mut options, deadline);
            let client = Client::with_options(options).map_err(|_| Error::Cluster {
                cluster: c.id.clone(),
                operation: "configure client",
            })?;
            databases.insert(c.id.clone(), client.database(&c.database));
        }
        let result = Self {
            routing: config.routing,
            databases,
            deadline,
        };
        result.verify_metadata().await?;
        Ok(result)
    }

    async fn bounded<T>(
        &self,
        cluster: &str,
        operation: &'static str,
        future: impl std::future::IntoFuture<Output = mongodb::error::Result<T>>,
    ) -> Result<T> {
        timeout(self.deadline, future.into_future())
            .await
            .map_err(|_| Error::Timeout {
                cluster: cluster.into(),
                operation,
            })?
            .map_err(|_| Error::Cluster {
                cluster: cluster.into(),
                operation,
            })
    }
    fn db(&self, cluster: &str) -> Result<&Database> {
        self.databases
            .get(cluster)
            .ok_or(Error::Configuration("unknown cluster"))
    }

    async fn verify_metadata(&self) -> Result<()> {
        let fingerprint = self.routing.fingerprint()?;
        for (id, db) in &self.databases {
            let row = self
                .bounded(
                    id,
                    "read routing metadata",
                    db.collection::<Document>("_lantern_metadata")
                        .find_one(doc! {"_id":"routing-v1"}),
                )
                .await?;
            if row.as_ref().and_then(|d| d.get_str("fingerprint").ok())
                != Some(fingerprint.as_str())
            {
                return Err(Error::RoutingMetadata(id.clone()));
            }
        }
        Ok(())
    }
    /// EXPLICIT first-use initialization only. Call after a separately persisted,
    /// operator-reviewed pin; partially initialized clusters must be retried with
    /// exactly the same pin. Never changes an existing routing table.
    pub async fn initialize_empty(config: Config, pinned: &RoutingTable) -> Result<Self> {
        Self::initialize_empty_with_resolver(config, pinned, |name| env::var(name).ok()).await
    }
    pub async fn initialize_empty_with_resolver(
        config: Config,
        pinned: &RoutingTable,
        resolver: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        config.validate_against_pin(pinned)?;
        let deadline = Duration::from_millis(config.timeout_ms);
        let mut databases = BTreeMap::new();
        for c in &config.clusters {
            let uri = resolver(&c.uri_env)
                .filter(|uri| !uri.trim().is_empty())
                .ok_or(Error::Configuration("missing MongoDB URI setting"))?;
            let mut options = timeout(deadline, ClientOptions::parse(&uri))
                .await
                .map_err(|_| Error::Timeout {
                    cluster: c.id.clone(),
                    operation: "parse URI",
                })?
                .map_err(|_| Error::Cluster {
                    cluster: c.id.clone(),
                    operation: "parse URI",
                })?;
            enforce_write_policy(&mut options, deadline);
            let client = Client::with_options(options).map_err(|_| Error::Cluster {
                cluster: c.id.clone(),
                operation: "configure client",
            })?;
            databases.insert(c.id.clone(), client.database(&c.database));
        }
        let store = Self {
            routing: config.routing,
            databases,
            deadline,
        };
        let fingerprint = store.routing.fingerprint()?;
        // Refuse to attach a new pin to a database that already has data.
        // Preflight all clusters before the first metadata write.
        for (id, db) in &store.databases {
            let metadata = store
                .bounded(
                    id,
                    "read routing metadata",
                    db.collection::<Document>("_lantern_metadata")
                        .find_one(doc! {"_id":"routing-v1"}),
                )
                .await?;
            if let Some(row) = metadata {
                if row.get_str("fingerprint").ok() != Some(fingerprint.as_str()) {
                    return Err(Error::RoutingMetadata(id.clone()));
                }
            } else {
                for name in [
                    Company::COLLECTION,
                    SourceSnapshot::COLLECTION,
                    Claim::COLLECTION,
                    Job::COLLECTION,
                ] {
                    let found = store
                        .bounded(
                            id,
                            "check existing data",
                            db.collection::<Document>(name).find_one(doc! {}),
                        )
                        .await?;
                    if found.is_some() {
                        return Err(Error::RoutingMetadata(id.clone()));
                    }
                }
            }
        }
        for (id, db) in &store.databases {
            let c = db.collection::<Document>("_lantern_metadata");
            store
                .bounded(
                    id,
                    "initialize routing metadata",
                    c.update_one(
                        doc! {"_id":"routing-v1"},
                        doc! {"$setOnInsert":{"fingerprint": &fingerprint}},
                    )
                    .upsert(true),
                )
                .await?;
        }
        store.verify_metadata().await?;
        Ok(store)
    }
    /// This adapter requires unsharded collections; indexes below intentionally
    /// do not assume `_id` globally unique on an Atlas-sharded collection.
    pub async fn ensure_indexes(&self) -> Result<()> {
        for (id, db) in &self.databases {
            for collection in [
                Company::COLLECTION,
                SourceSnapshot::COLLECTION,
                Claim::COLLECTION,
                Job::COLLECTION,
            ] {
                let index = IndexModel::builder()
                    .keys(doc! {"company_id":1,"entity_id":1,"version":1})
                    .build();
                self.bounded(
                    id,
                    "create index",
                    db.collection::<Document>(collection).create_index(index),
                )
                .await?;
            }
        }
        Ok(())
    }
    pub async fn put<T: Record>(&self, record: &T) -> Result<()> {
        let owner = self.routing.owner(record.company_id())?;
        let coll = self.db(owner)?.collection::<Document>(T::COLLECTION);
        let proposed = item_document(record, &self.routing)?;
        let key = proposed
            .get_str("_id")
            .map_err(|_| Error::Serialization)?
            .to_owned();
        self.bounded(
            owner,
            "upsert record",
            coll.update_one(doc! {"_id": &key}, doc! {"$setOnInsert": proposed.clone()})
                .upsert(true),
        )
        .await?;
        // Do not turn a majority acknowledgement failure into success merely
        // because a primary read finds the record. Retry the identical version.
        let existing = self
            .bounded(owner, "read back record", coll.find_one(doc! {"_id": &key}))
            .await?
            .ok_or(Error::Cluster {
                cluster: owner.into(),
                operation: "read back record",
            })?;
        verify_replay(&proposed, &existing)
    }
    pub async fn get<T: Record>(
        &self,
        company_id: &str,
        entity_id: &str,
        version: &str,
    ) -> Result<Option<T>> {
        required(company_id)?;
        required(entity_id)?;
        required(version)?;
        let owner = self.routing.owner(company_id)?;
        let coll = self.db(owner)?.collection::<Document>(T::COLLECTION);
        let value = self
            .bounded(
                owner,
                "get record",
                coll.find_one(
                    doc! {"company_id":company_id,"entity_id":entity_id,"version":version},
                ),
            )
            .await?;
        value
            .map(|d| decode_checked(d, owner, &self.routing))
            .transpose()
    }
    /// A single company is always read from its configured owner; never fail over.
    pub async fn list_company<T: Record>(&self, company_id: &str, limit: u32) -> Result<Page<T>> {
        required(company_id)?;
        if limit == 0 || limit > 10_000 {
            return Err(Error::Configuration("invalid read limit"));
        }
        let owner = self.routing.owner(company_id)?;
        let coll = self.db(owner)?.collection::<Document>(T::COLLECTION);
        let cursor = self
            .bounded(
                owner,
                "list records",
                coll.find(doc! {"company_id":company_id})
                    .sort(doc! {"_id":1})
                    .limit(i64::from(limit) + 1),
            )
            .await?;
        let mut rows: Vec<Document> = self
            .bounded(owner, "read cursor", cursor.try_collect())
            .await?;
        let truncated = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        Ok(Page {
            items: rows
                .into_iter()
                .map(|d| decode_checked(d, owner, &self.routing))
                .collect::<Result<Vec<_>>>()?,
            truncated,
        })
    }
    /// Explicit cluster scope; results are never presented as complete on failure.
    /// `limit_per_cluster` caps memory and does not implement global pagination.
    pub async fn list_clusters<T: Record>(
        &self,
        scope: &[String],
        limit_per_cluster: u32,
    ) -> Result<PartialResults<T>> {
        if scope.is_empty() || limit_per_cluster == 0 || limit_per_cluster > 10_000 {
            return Err(Error::Configuration("invalid scope or limit"));
        }
        let unique: BTreeSet<_> = scope.iter().collect();
        if unique.len() != scope.len() || unique.iter().any(|id| !self.databases.contains_key(*id))
        {
            return Err(Error::Configuration(
                "scope contains duplicate or unknown cluster",
            ));
        }
        let mut result = PartialResults {
            items: Vec::new(),
            failures: Vec::new(),
            truncated_clusters: Vec::new(),
        };
        for id in scope {
            let coll = self.db(id)?.collection::<Document>(T::COLLECTION);
            let fetched = async {
                let cursor = self
                    .bounded(
                        id,
                        "list records",
                        coll.find(doc! {})
                            .sort(doc! {"_id":1})
                            .limit(i64::from(limit_per_cluster) + 1),
                    )
                    .await?;
                let mut docs: Vec<Document> = self
                    .bounded(id, "read cursor", cursor.try_collect())
                    .await?;
                let truncated = docs.len() > limit_per_cluster as usize;
                docs.truncate(limit_per_cluster as usize);
                Ok::<_, Error>((
                    docs.into_iter()
                        .map(|d| decode_checked(d, id, &self.routing))
                        .collect::<Result<Vec<T>>>()?,
                    truncated,
                ))
            }
            .await;
            match fetched {
                Ok((items, truncated)) => {
                    result.items.extend(items);
                    if truncated {
                        result.truncated_clusters.push(id.clone());
                    }
                }
                Err(_) => result.failures.push(ClusterFailure {
                    cluster: id.clone(),
                    reason: "cluster read failed, timed out, or returned invalid data",
                }),
            }
        }
        Ok(result)
    }
    pub async fn health(&self) -> Vec<ClusterHealth> {
        let mut result = Vec::new();
        for (id, db) in &self.databases {
            result.push(ClusterHealth {
                cluster: id.clone(),
                ok: self
                    .bounded(id, "ping", db.run_command(doc! {"ping":1}))
                    .await
                    .is_ok(),
            });
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table() -> RoutingTable {
        RoutingTable {
            version: 1,
            algorithm: "sha256-u64be-mod-v1".into(),
            owners: vec!["a".into(), "b".into(), "a".into()],
        }
    }
    fn config() -> Config {
        Config {
            clusters: vec![
                ClusterConfig {
                    id: "a".into(),
                    uri_env: "URI_A".into(),
                    database: "lantern".into(),
                },
                ClusterConfig {
                    id: "b".into(),
                    uri_env: "URI_B".into(),
                    database: "lantern".into(),
                },
            ],
            routing: table(),
            timeout_ms: 1000,
        }
    }
    #[test]
    fn deterministic_ownership_and_pin() {
        let t = table();
        assert_eq!(t.bucket("0100109106").unwrap(), 2);
        assert_eq!(t.owner("0100109106").unwrap(), "a");
        let mut changed = t.clone();
        changed.owners[0] = "b".into();
        assert_eq!(
            config().validate_against_pin(&changed),
            Err(Error::RoutingChanged)
        );
        assert!(t.owner("").is_err());
    }
    #[test]
    fn unknown_owner_is_rejected() {
        let mut c = config();
        c.routing.owners[0] = "unknown".into();
        assert!(matches!(
            c.validate_against_pin(&c.routing),
            Err(Error::Configuration(_))
        ));
    }
    #[test]
    fn retry_matches_but_changed_payload_conflicts() {
        let v = Claim {
            company_id: "c".into(),
            claim_id: "claim".into(),
            version: "v1".into(),
            payload: serde_json::json!({"fact":1}),
        };
        let d = item_document(&v, &table()).unwrap();
        assert_eq!(verify_replay(&d, &d), Ok(()));
        let mut changed = v.clone();
        changed.payload = serde_json::json!({"fact":2});
        let d2 = item_document(&changed, &table()).unwrap();
        assert_eq!(d.get("_id"), d2.get("_id"));
        assert_eq!(verify_replay(&d, &d2), Err(Error::Conflict));
    }
    #[test]
    fn identities_do_not_collide_on_ambiguous_boundaries() {
        let v = Claim {
            company_id: "ab".into(),
            claim_id: "c".into(),
            version: "d".into(),
            payload: Value::Null,
        };
        let mut w = v.clone();
        w.company_id = "a".into();
        w.claim_id = "bc".into();
        assert_ne!(record_key(&v).unwrap(), record_key(&w).unwrap());
    }
    #[test]
    fn reads_reject_wrong_owner_and_inconsistent_document() {
        let v = Claim {
            company_id: "0100109106".into(),
            claim_id: "x".into(),
            version: "1".into(),
            payload: Value::Null,
        };
        let mut d = item_document(&v, &table()).unwrap();
        assert!(decode_checked::<Claim>(d.clone(), "a", &table()).is_ok());
        assert!(decode_checked::<Claim>(d.clone(), "b", &table()).is_err());
        d.insert("bucket", 0i64);
        assert!(decode_checked::<Claim>(d, "a", &table()).is_err());
    }
    #[tokio::test]
    async fn resolver_failure_never_returns_sensitive_uri() {
        let err = match MongoStore::connect_with_resolver(config(), &table(), |_| {
            Some("secret-that-is-not-a-mongodb-uri".into())
        })
        .await
        {
            Ok(_) => panic!("invalid URI unexpectedly accepted"),
            Err(e) => e,
        };
        assert!(!format!("{err:?} {err}").contains("secret-that-is-not-a-mongodb-uri"));
    }
    #[tokio::test]
    async fn write_policy_overrides_uri_w_one() {
        let mut options = ClientOptions::parse("mongodb://localhost:27017/?w=1")
            .await
            .unwrap();
        assert_eq!(
            options.write_concern.as_ref().and_then(|w| w.w.as_ref()),
            Some(&Acknowledgment::Nodes(1))
        );
        enforce_write_policy(&mut options, Duration::from_secs(3));
        assert_eq!(
            options.write_concern.as_ref().and_then(|w| w.w.as_ref()),
            Some(&Acknowledgment::Majority)
        );
    }
    #[test]
    fn partial_never_claims_complete() {
        let p: PartialResults<Claim> = PartialResults {
            items: vec![],
            failures: vec![ClusterFailure {
                cluster: "b".into(),
                reason: "failed",
            }],
            truncated_clusters: vec![],
        };
        assert!(!p.is_complete());
        let truncated: PartialResults<Claim> = PartialResults {
            items: vec![],
            failures: vec![],
            truncated_clusters: vec!["a".into()],
        };
        assert!(!truncated.is_complete());
    }
}
