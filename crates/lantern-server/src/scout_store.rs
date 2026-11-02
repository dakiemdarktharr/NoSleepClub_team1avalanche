//! Durable single-host Scout adapter. Separate SQLite file, no raw remote document storage.
//! Outbox consumers must apply idempotently to their destination before acknowledging.
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use lantern_scout::{
    Change, FetchRecord, GleifCandidate, Lease, ScoutStore, SourcePolicy, WikidataCandidate,
    parse_gleif_vn_json, parse_wikidata_vn_json,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Mutex},
};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct SqliteScoutStore {
    conn: Arc<Mutex<Connection>>,
    user_agent_contact: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub source_id: String,
    pub enabled: bool,
    pub state: String,
    pub next_due: Option<DateTime<Utc>>,
    pub last_observed: Option<DateTime<Utc>>,
    pub content_hash: Option<String>,
    pub attempts: u32,
    pub origin_requests_today: u32,
    pub origin_daily_limit: u32,
    pub origin_suspended: bool,
    pub outbox_pending: u64,
    pub cursor: u32,
    pub last_error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboxItem {
    pub id: String,
    pub source_id: String,
    pub job_id: String,
    pub observed_at: DateTime<Utc>,
    pub content_hash: String,
    pub candidates: Vec<GleifCandidate>,
    #[serde(default)]
    pub wikidata_candidates: Vec<WikidataCandidate>,
}
fn ms(t: DateTime<Utc>) -> i64 {
    t.timestamp_millis()
}
fn deadline_ms(t: DateTime<Utc>) -> i64 {
    let n = t.timestamp_millis();
    if t.timestamp_subsec_nanos().is_multiple_of(1_000_000) {
        n
    } else {
        n.saturating_add(1)
    }
}
fn time(n: i64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp_millis(n).context("invalid stored time")
}
fn origin(policy: &SourcePolicy) -> Result<String> {
    let url = Url::parse(&policy.source_url)?;
    let host = url.host_str().context("missing source host")?.to_owned();
    ensure!(
        policy.allowed_hosts.len() == 1 && policy.allowed_hosts[0] == host,
        "SQLite adapter requires one exact origin to enforce quotas across redirects"
    );
    Ok(host)
}
fn validate_policy(p: &SourcePolicy) -> Result<()> {
    let mut check = p.clone();
    if !p.enabled {
        check.access_authorized = true;
        check.robots_and_terms_checked = true;
    }
    check.enabled = true;
    check.validate()?;
    origin(&check)?;
    Ok(())
}
fn day(t: DateTime<Utc>) -> String {
    t.date_naive().to_string()
}
fn next_day_ms(t: DateTime<Utc>) -> Result<i64> {
    let d = t.date_naive().succ_opt().context("date overflow")?;
    Ok(ms(d
        .and_hms_opt(0, 0, 0)
        .context("invalid date")?
        .and_utc()))
}
type SourceRow = (String, SourcePolicy, String, Option<i64>, i64, u32);
fn sources(tx: &Transaction<'_>) -> Result<Vec<SourceRow>> {
    let mut st=tx.prepare("SELECT source_id,policy,state,next_due_ms,protected_until_ms,attempts FROM scout_sources ORDER BY COALESCE(next_due_ms,0),source_id")?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, u32>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, json, state, due, protected, attempts) = row?;
        out.push((
            id,
            serde_json::from_str(&json)?,
            state,
            due,
            protected,
            attempts,
        ));
    }
    Ok(out)
}
fn origin_limits(rows: &[SourceRow], host: &str) -> Result<(u64, u32)> {
    let mut interval = 0;
    let mut limit = u32::MAX;
    for (_, p, _, _, _, _) in rows {
        if p.enabled && origin(p)? == host {
            interval = interval.max(p.min_interval_seconds);
            limit = limit.min(p.max_requests_per_day);
        }
    }
    ensure!(limit != u32::MAX, "no enabled policy for origin");
    Ok((interval, limit))
}
impl SqliteScoutStore {
    pub fn open(
        path: impl AsRef<Path>,
        policies: Vec<SourcePolicy>,
        contact: Option<String>,
    ) -> Result<Self> {
        let conn = Connection::open(path)?;
        let mut store = Self::from_conn(conn, policies)?;
        store.user_agent_contact = contact;
        Ok(store)
    }
    fn from_conn(mut conn: Connection, policies: Vec<SourcePolicy>) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS scout_sources(source_id TEXT PRIMARY KEY,policy TEXT NOT NULL,state TEXT NOT NULL,next_due_ms INTEGER,protected_until_ms INTEGER NOT NULL DEFAULT 0,attempts INTEGER NOT NULL DEFAULT 0,etag TEXT,last_modified TEXT,content_hash TEXT,last_observed_ms INTEGER,last_error TEXT,job_id TEXT,lease_token TEXT,lease_until_ms INTEGER);
CREATE TABLE IF NOT EXISTS scout_cursors(source_id TEXT PRIMARY KEY, page INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS scout_origins(origin TEXT PRIMARY KEY,utc_day TEXT NOT NULL,requests INTEGER NOT NULL,next_allowed_ms INTEGER NOT NULL DEFAULT 0,lease_source TEXT,lease_token TEXT,lease_until_ms INTEGER);
CREATE TABLE IF NOT EXISTS scout_outbox(id TEXT PRIMARY KEY,source_id TEXT NOT NULL REFERENCES scout_sources(source_id),job_id TEXT UNIQUE NOT NULL,observed_at_ms INTEGER NOT NULL,content_hash TEXT NOT NULL,payload TEXT NOT NULL,acked INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS scout_outbox_pending ON scout_outbox(acked,observed_at_ms);
CREATE TABLE IF NOT EXISTS scout_events(job_id TEXT PRIMARY KEY,source_id TEXT NOT NULL REFERENCES scout_sources(source_id),lease_token TEXT NOT NULL,payload TEXT NOT NULL);")?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for p in policies {
            validate_policy(&p)?;
            let state = if p.enabled { "idle" } else { "disabled" };
            tx.execute("INSERT OR IGNORE INTO scout_sources(source_id,policy,state,next_due_ms) VALUES(?1,?2,?3,?4)",params![p.source_id,serde_json::to_string(&p)?,state,ms(Utc::now())])?;
            if matches!(
                p.source_id.as_str(),
                "gleif-vn-latest-100-v1" | "wikidata-vn-business-v1"
            ) {
                let created = tx.execute(
                    "INSERT OR IGNORE INTO scout_cursors(source_id,page) VALUES(?1,0)",
                    [&p.source_id],
                )?;
                // Existing versions belonged to the old unpaged URL and must not condition the first sweep.
                if created == 1 {
                    // An old process may hold a lease for the previous URL: fence it without
                    // refunding the already consumed origin request or shortening its cooldown.
                    tx.execute("UPDATE scout_origins SET lease_source=NULL,lease_token=NULL,lease_until_ms=NULL WHERE lease_source=?1 AND lease_token=(SELECT lease_token FROM scout_sources WHERE source_id=?1 AND state='leased')", [&p.source_id])?;
                    tx.execute("UPDATE scout_sources SET etag=NULL,last_modified=NULL,content_hash=NULL,state=CASE WHEN state='leased' THEN 'idle' ELSE state END,job_id=NULL,lease_token=NULL,lease_until_ms=NULL WHERE source_id=?1", [&p.source_id])?;
                }
            }
        }
        tx.commit()?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            user_agent_contact: None,
        })
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("scout store lock poisoned"))
    }
    pub fn set_enabled(&self, source_id: &str, enabled: bool) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (raw, token): (String, Option<String>) = tx
            .query_row(
                "SELECT policy,lease_token FROM scout_sources WHERE source_id=?1",
                [source_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .context("source not found")?;
        let mut p: SourcePolicy = serde_json::from_str(&raw)?;
        if p.enabled == enabled {
            tx.commit()?;
            return Ok(());
        }
        p.enabled = enabled;
        if enabled {
            validate_policy(&p)?;
        }
        // Disable fences any worker but leaves quota and cooldown intact. Explicit enabling is operator action.
        let state = if enabled { "idle" } else { "disabled" };
        tx.execute("UPDATE scout_sources SET policy=?1,state=?2,job_id=NULL,lease_token=NULL,lease_until_ms=NULL,attempts=CASE WHEN ?2='idle' THEN 0 ELSE attempts END,next_due_ms=COALESCE(next_due_ms,?4) WHERE source_id=?3",params![serde_json::to_string(&p)?,state,source_id,ms(Utc::now())])?;
        if let Some(token) = token {
            tx.execute("UPDATE scout_origins SET lease_source=NULL,lease_token=NULL,lease_until_ms=NULL WHERE lease_source=?1 AND lease_token=?2",params![source_id,token])?;
        }
        tx.commit()?;
        Ok(())
    }

    fn claim_at(&self, now: DateTime<Utc>) -> Result<Option<Lease>> {
        let mut c = self.lock()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n = ms(now);
        // Expiry is counted as a failed attempt; the origin request was already charged at claim.
        let stale: Vec<(String, String, u32)> = {
            let mut st=tx.prepare("SELECT source_id,policy,attempts FROM scout_sources WHERE state='leased' AND lease_until_ms<=?1")?;
            let it = st.query_map([n], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            it.collect::<rusqlite::Result<_>>()?
        };
        for (id, raw, attempts) in stale {
            let p: SourcePolicy = serde_json::from_str(&raw)?;
            let attempts = attempts.saturating_add(1);
            let state = if attempts >= p.max_attempts {
                "exhausted"
            } else {
                "idle"
            };
            let due = if state == "idle" {
                Some(n.saturating_add((p.min_interval_seconds as i64) * 1000))
            } else {
                None
            };
            tx.execute("UPDATE scout_sources SET state=?1,attempts=?2,next_due_ms=?3,job_id=NULL,lease_token=NULL,lease_until_ms=NULL WHERE source_id=?4",params![state,attempts,due,id])?;
        }
        tx.execute("UPDATE scout_origins SET lease_source=NULL,lease_token=NULL,lease_until_ms=NULL WHERE lease_until_ms<=?1",[n])?;
        let all = sources(&tx)?;
        for (id, p, state, due, protected, attempts) in &all {
            if !p.enabled || state != "idle" || due.is_none_or(|d| d > n) || *protected > n {
                continue;
            }
            validate_policy(p)?;
            let host = origin(p)?;
            let (interval, limit) = origin_limits(&all, &host)?;
            let today = day(now);
            tx.execute("INSERT OR IGNORE INTO scout_origins(origin,utc_day,requests,next_allowed_ms) VALUES(?1,?2,0,0)",params![host,today])?;
            let (old_day,count,until,occupied):(String,u32,i64,Option<String>)=tx.query_row("SELECT utc_day,requests,next_allowed_ms,lease_token FROM scout_origins WHERE origin=?1",[&host],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            let count = if old_day == today { count } else { 0 };
            if occupied.is_some() || until > n || count >= limit {
                continue;
            }
            let token = Uuid::new_v4().to_string();
            let job = Uuid::new_v4().to_string();
            let expires = n.saturating_add((p.timeout_seconds as i64 + 30) * 1000);
            let (etag, last, hash): (Option<String>, Option<String>, Option<String>) = tx
                .query_row(
                    "SELECT etag,last_modified,content_hash FROM scout_sources WHERE source_id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
            tx.execute("UPDATE scout_origins SET utc_day=?1,requests=?2,next_allowed_ms=?3,lease_source=?4,lease_token=?5,lease_until_ms=?6 WHERE origin=?7",params![today,count+1,deadline_ms(now).saturating_add(interval as i64*1000),id,token,expires,host])?;
            tx.execute("UPDATE scout_sources SET state='leased',job_id=?1,lease_token=?2,lease_until_ms=?3 WHERE source_id=?4",params![job,token,expires,id])?;
            let page: u32 = tx
                .query_row(
                    "SELECT page FROM scout_cursors WHERE source_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(0);
            let request_url = match id.as_str() {
                "gleif-vn-latest-100-v1" => {
                    let mut u = Url::parse("https://api.gleif.org/api/v1/lei-records")?;
                    u.query_pairs_mut()
                        .append_pair("filter[entity.legalAddress.country]", "VN")
                        .append_pair("page[size]", "100")
                        .append_pair("sort", "lei")
                        .append_pair("page[number]", &(page + 1).to_string());
                    Some(u.to_string())
                }
                "wikidata-vn-business-v1" => {
                    let query = format!(
                        "SELECT DISTINCT ?item ?name ?website ?ticker WHERE {{ ?item wdt:P17 wd:Q881; wdt:P31 wd:Q4830453; rdfs:label ?name. FILTER(LANG(?name) = \"vi\") OPTIONAL {{ ?item wdt:P856 ?website }} OPTIONAL {{ ?item wdt:P249 ?ticker }} }} ORDER BY ?item ?name ?website ?ticker LIMIT 100 OFFSET {}",
                        page * 100
                    );
                    let mut u = Url::parse("https://query.wikidata.org/sparql")?;
                    u.query_pairs_mut()
                        .append_pair("query", &query)
                        .append_pair("format", "json");
                    Some(u.to_string())
                }
                _ => None,
            };
            let lease = Lease {
                request_url,
                user_agent_contact: self.user_agent_contact.clone(),
                job_id: job,
                fencing_token: token,
                policy: p.clone(),
                attempts: *attempts,
                previous_etag: etag,
                previous_last_modified: last,
                previous_hash: hash,
                not_before: now,
                expires_at: time(expires)?,
            };
            tx.commit()?;
            return Ok(Some(lease));
        }
        tx.commit()?;
        Ok(None)
    }
    /// Returns the earliest permitted time, never bypassing source backoff, origin quota, or lease.
    pub fn run_now(&self, source_id: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
        let mut c = self.lock()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let all = sources(&tx)?;
        let (_, p, state, _, protected, _) = all
            .iter()
            .find(|(id, _, _, _, _, _)| id == source_id)
            .context("source not found")?;
        ensure!(
            p.enabled && state == "idle",
            "source is disabled, blocked, exhausted, or leased"
        );
        let host = origin(p)?;
        let (_, limit) = origin_limits(&all, &host)?;
        let origin_state:Option<(String,u32,i64,Option<i64>)>=tx.query_row("SELECT utc_day,requests,next_allowed_ms,lease_until_ms FROM scout_origins WHERE origin=?1",[&host],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if origin_state
            .as_ref()
            .is_some_and(|(_, _, next, _)| *next == i64::MAX)
        {
            bail!("origin suspended after rate limit; operator review required")
        }
        let mut due = deadline_ms(now).max(*protected);
        if let Some((d, count, next, lease)) = origin_state {
            due = due.max(next);
            if let Some(exp) = lease {
                due = due.max(exp);
            }
            if d == day(now) && count >= limit {
                due = due.max(next_day_ms(now)?);
            }
        }
        tx.execute(
            "UPDATE scout_sources SET next_due_ms=?1 WHERE source_id=?2",
            params![due, source_id],
        )?;
        tx.commit()?;
        time(due)
    }
    /// Review resumes a blocked/exhausted source and/or a rate-limited origin atomically.
    /// Never resets quota, an existing cooldown, or a source checkpoint.
    pub fn resume_origin_after_review(&self, source_id: &str, now: DateTime<Utc>) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let all = sources(&tx)?;
        let (_, p, state, _, protected, _) = all
            .iter()
            .find(|(id, _, _, _, _, _)| id == source_id)
            .context("source not found")?;
        ensure!(p.enabled, "source must be explicitly enabled");
        validate_policy(p)?;
        let host = origin(p)?;
        let (interval, _) = origin_limits(&all, &host)?;
        let origin_state: Option<(i64, Option<String>)> = tx
            .query_row(
                "SELECT next_allowed_ms,lease_token FROM scout_origins WHERE origin=?1",
                [&host],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let suspended = origin_state
            .as_ref()
            .is_some_and(|(until, _)| *until == i64::MAX);
        ensure!(
            matches!(state.as_str(), "blocked" | "exhausted") || suspended,
            "source does not need review"
        );
        ensure!(
            origin_state
                .as_ref()
                .is_none_or(|(_, token)| token.is_none()),
            "origin has an active lease"
        );
        let mut origin_due = deadline_ms(now).saturating_add(interval as i64 * 1000);
        for (_, policy, _, _, until, _) in &all {
            if origin(policy)? == host {
                origin_due = origin_due.max(*until);
            }
        }
        if suspended {
            tx.execute("UPDATE scout_origins SET next_allowed_ms=?1 WHERE origin=?2 AND next_allowed_ms=?3 AND lease_token IS NULL", params![origin_due,host,i64::MAX])?;
        } else if let Some((until, _)) = origin_state {
            origin_due = origin_due.max(until);
        }
        if matches!(state.as_str(), "blocked" | "exhausted") {
            let due = origin_due.max(*protected);
            tx.execute("UPDATE scout_sources SET state='idle',next_due_ms=?1,protected_until_ms=?1,attempts=0 WHERE source_id=?2", params![due,source_id])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn pending_outbox(&self, limit: u32) -> Result<Vec<OutboxItem>> {
        let c = self.lock()?;
        let mut st = c.prepare(
            "SELECT payload FROM scout_outbox WHERE acked=0 ORDER BY observed_at_ms,id LIMIT ?1",
        )?;
        let it = st.query_map([limit.clamp(1, 100)], |r| r.get::<_, String>(0))?;
        it.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    /// Ack only after the downstream consumer has durably and idempotently applied this item.
    pub fn ack_outbox(&self, id: &str) -> Result<()> {
        let c = self.lock()?;
        let changed = c.execute("UPDATE scout_outbox SET acked=1 WHERE id=?1", [id])?;
        ensure!(changed == 1, "outbox item not found");
        Ok(())
    }

    pub fn snapshot(&self) -> Result<Vec<SourceSnapshot>> {
        self.snapshot_at(Utc::now())
    }
    fn snapshot_at(&self, now: DateTime<Utc>) -> Result<Vec<SourceSnapshot>> {
        let mut c = self.lock()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let all = sources(&tx)?;
        let mut out = Vec::new();
        for (id, p, state, due, protected, attempts) in &all {
            let host = origin(p)?;
            let limit = if p.enabled {
                origin_limits(&all, &host)?.1
            } else {
                p.max_requests_per_day
            };
            let orig: Option<(String, u32, i64, Option<i64>)> = tx
                .query_row(
                    "SELECT utc_day,requests,next_allowed_ms,lease_until_ms FROM scout_origins WHERE origin=?1",
                    [host],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            let requests = orig
                .as_ref()
                .filter(|(d, _, _, _)| *d == day(now))
                .map_or(0, |(_, n, _, _)| *n);
            let origin_suspended = orig
                .as_ref()
                .is_some_and(|(_, _, next, _)| *next == i64::MAX);
            let next_due = if !p.enabled || state != "idle" || origin_suspended {
                None
            } else if let Some(mut next) = *due {
                next = next.max(*protected);
                if let Some((_, _, origin_next, lease_until)) = &orig {
                    next = next.max(*origin_next);
                    if let Some(lease_until) = lease_until {
                        next = next.max(*lease_until);
                    }
                }
                if requests >= limit {
                    next = next.max(next_day_ms(now)?);
                }
                Some(time(next)?)
            } else {
                None
            };
            let (last, hash): (Option<i64>, Option<String>) = tx.query_row(
                "SELECT last_observed_ms,content_hash FROM scout_sources WHERE source_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let pending: i64 = tx.query_row(
                "SELECT COUNT(*) FROM scout_outbox WHERE source_id=?1 AND acked=0",
                [id],
                |r| r.get(0),
            )?;
            let cursor = tx
                .query_row(
                    "SELECT page FROM scout_cursors WHERE source_id=?1",
                    [&id],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(0);
            let last_error = tx.query_row(
                "SELECT last_error FROM scout_sources WHERE source_id=?1",
                [&id],
                |r| r.get(0),
            )?;
            out.push(SourceSnapshot {
                cursor,
                last_error,
                source_id: id.clone(),
                enabled: p.enabled,
                state: state.clone(),
                next_due,
                last_observed: last.map(time).transpose()?,
                content_hash: hash,
                attempts: *attempts,
                origin_requests_today: requests,
                origin_daily_limit: limit,
                origin_suspended,
                outbox_pending: u64::try_from(pending).context("invalid outbox count")?,
            });
        }
        tx.commit()?;
        Ok(out)
    }
    fn commit_at(
        &self,
        lease: &Lease,
        record: &FetchRecord,
        content: Option<&[u8]>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        ensure!(
            record.source_id == lease.policy.source_id && record.job_id == lease.job_id,
            "observation does not match lease"
        );
        let serialized = serde_json::to_string(record)?;
        let mut c = self.lock()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<(String, String)> = tx
            .query_row(
                "SELECT lease_token,payload FROM scout_events WHERE job_id=?1",
                [&lease.job_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((token, payload)) = previous {
            ensure!(
                token == lease.fencing_token && payload == serialized,
                "conflicting duplicate commit"
            );
            tx.commit()?;
            return Ok(());
        }
        let (raw,state,job,token,expires,stored_hash):(String,String,Option<String>,Option<String>,Option<i64>,Option<String>)=tx.query_row("SELECT policy,state,job_id,lease_token,lease_until_ms,content_hash FROM scout_sources WHERE source_id=?1",[&record.source_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).context("source not found")?;
        let p: SourcePolicy = serde_json::from_str(&raw)?;
        ensure!(
            p.enabled
                && state == "leased"
                && job.as_deref() == Some(&lease.job_id)
                && token.as_deref() == Some(&lease.fencing_token)
                && expires.is_some_and(|x| x > ms(now)),
            "stale, expired, or disabled lease"
        );
        ensure!(
            raw == serde_json::to_string(&lease.policy)? && stored_hash == lease.previous_hash,
            "policy or source version changed during fetch"
        );
        ensure!(
            record.previous_hash == lease.previous_hash,
            "previous hash does not match lease"
        );
        ensure!(
            record.attempts == lease.attempts.saturating_add(1),
            "invalid attempt count"
        );
        let host = origin(&p)?;
        let origin_token: Option<String> = tx.query_row(
            "SELECT lease_token FROM scout_origins WHERE origin=?1",
            [&host],
            |r| r.get(0),
        )?;
        ensure!(
            origin_token.as_deref() == Some(&lease.fencing_token),
            "origin lease fenced"
        );
        let mut candidates = None;
        let mut wikidata_candidates = None;
        let mut next_page = None;
        if matches!(record.change, Change::FirstSeen | Change::Changed) {
            ensure!(
                record.status_code == Some(200),
                "changed source must be a complete response"
            );
            let hash = record
                .content_hash
                .as_ref()
                .context("missing content hash")?;
            ensure!(
                matches!(
                    (&record.change, &stored_hash),
                    (Change::FirstSeen, None) | (Change::Changed, Some(_))
                ),
                "invalid source transition"
            );
            ensure!(
                stored_hash.as_ref() != Some(hash),
                "changed content must differ from previous version"
            );
            if p.process_content_authorized {
                let bytes = content.context("authorized content missing")?;
                ensure!(
                    bytes.len() <= p.max_bytes && hex::encode(Sha256::digest(bytes)) == *hash,
                    "content hash mismatch"
                );
                let parsed_url = Url::parse(&p.source_url)?;
                let parsed =
                    if host == "api.gleif.org" && parsed_url.path() == "/api/v1/lei-records" {
                        parse_gleif_vn_json(bytes).and_then(|items| {
                            let mut unique = HashSet::new();
                            ensure!(
                                items.iter().all(|item| unique.insert(item.lei.clone())),
                                "duplicate LEI"
                            );
                            let root: serde_json::Value = serde_json::from_slice(bytes)?;
                            let count = root
                                .get("data")
                                .and_then(|v| v.as_array())
                                .map_or(1, |v| v.len());
                            if p.source_id == "gleif-vn-latest-100-v1" {
                                ensure!(count <= 100, "oversized page");
                            }
                            candidates = Some(items);
                            Ok(count)
                        })
                    } else if host == "query.wikidata.org"
                        && parsed_url.path() == "/sparql"
                        && p.source_id == "wikidata-vn-business-v1"
                    {
                        parse_wikidata_vn_json(bytes).and_then(|items| {
                            let root: serde_json::Value = serde_json::from_slice(bytes)?;
                            let count = root
                                .pointer("/results/bindings")
                                .and_then(|v| v.as_array())
                                .context("missing results")?
                                .len();
                            wikidata_candidates = Some(items);
                            Ok(count)
                        })
                    } else {
                        bail!("no approved content parser for this source")
                    };
                match parsed {
                    Ok(count) => {
                        if matches!(
                            p.source_id.as_str(),
                            "gleif-vn-latest-100-v1" | "wikidata-vn-business-v1"
                        ) {
                            let page: u32 = tx
                                .query_row(
                                    "SELECT page FROM scout_cursors WHERE source_id=?1",
                                    [&p.source_id],
                                    |r| r.get(0),
                                )
                                .optional()?
                                .unwrap_or(0);
                            next_page = Some(if count < 100 || page >= 999 {
                                0
                            } else {
                                page + 1
                            });
                        }
                    }
                    Err(_) => {
                        self.fail_parse(&tx, &p, &lease.fencing_token, lease.attempts, now)?;
                        tx.commit()?;
                        bail!("source parse failed; checkpoint unchanged")
                    }
                }
            } else {
                ensure!(content.is_none(), "content processing not authorized");
            }
        } else if record.change == Change::Unchanged {
            ensure!(
                stored_hash.is_some() && record.content_hash == stored_hash,
                "unchanged version mismatch"
            );
            ensure!(
                record.status_code == Some(200) || record.status_code == Some(304),
                "invalid unchanged response"
            );
            if let Some(bytes) = content {
                ensure!(
                    p.process_content_authorized
                        && hex::encode(Sha256::digest(bytes)) == stored_hash.clone().unwrap(),
                    "unchanged content hash mismatch"
                );
            }
        } else {
            ensure!(
                content.is_none(),
                "unexpected content for non-success outcome"
            );
        }
        if candidates.is_some() || wikidata_candidates.is_some() {
            let item = OutboxItem {
                id: lease.job_id.clone(),
                source_id: p.source_id.clone(),
                job_id: lease.job_id.clone(),
                observed_at: record.observed_at,
                content_hash: record.content_hash.clone().unwrap_or_default(),
                candidates: candidates.unwrap_or_default(),
                wikidata_candidates: wikidata_candidates.unwrap_or_default(),
            };
            tx.execute("INSERT INTO scout_outbox(id,source_id,job_id,observed_at_ms,content_hash,payload) VALUES(?1,?2,?3,?4,?5,?6)",params![item.id,item.source_id,item.job_id,ms(item.observed_at),item.content_hash,serde_json::to_string(&item)?])?;
        }
        if let Some(page) = next_page {
            tx.execute("INSERT INTO scout_cursors(source_id,page) VALUES(?1,?2) ON CONFLICT(source_id) DO UPDATE SET page=excluded.page", params![p.source_id, page])?;
        }
        let origin_next: i64 = tx.query_row(
            "SELECT next_allowed_ms FROM scout_origins WHERE origin=?1",
            [&host],
            |r| r.get(0),
        )?;
        let (state, due, protected, attempts) = match record.change {
            Change::Blocked => ("blocked", None, 0, record.attempts),
            Change::Exhausted => (
                "exhausted",
                None,
                record.next_due.map(deadline_ms).unwrap_or(0),
                record.attempts,
            ),
            Change::Retry => {
                let due =
                    deadline_ms(record.next_due.context("missing retry time")?).max(origin_next);
                ("idle", Some(due), due, record.attempts)
            }
            _ => {
                let due =
                    deadline_ms(record.next_due.context("missing next time")?).max(origin_next);
                ("idle", Some(due), 0, 0)
            }
        };
        ensure!(
            due.is_none_or(|x| x >= ms(now)),
            "next due cannot precede current time"
        );
        let success = matches!(
            record.change,
            Change::FirstSeen | Change::Changed | Change::Unchanged
        );
        tx.execute("UPDATE scout_sources SET state=?1,next_due_ms=?2,protected_until_ms=?3,attempts=?4,etag=CASE WHEN ?5 THEN ?6 ELSE etag END,last_modified=CASE WHEN ?5 THEN ?7 ELSE last_modified END,content_hash=CASE WHEN ?5 THEN ?8 ELSE content_hash END,last_observed_ms=?9,last_error=?10,job_id=NULL,lease_token=NULL,lease_until_ms=NULL WHERE source_id=?11",params![state,due,protected,attempts,success,record.etag,record.last_modified,record.content_hash,ms(record.observed_at),if success{None}else{Some(record.reason.as_str())},p.source_id])?;
        if next_page.is_some() {
            tx.execute("UPDATE scout_sources SET etag=NULL,last_modified=NULL,content_hash=NULL WHERE source_id=?1", [&p.source_id])?;
        }
        // A 429 applies to the whole origin, not just this source. If the retry deadline is
        // unavailable or the retry budget is exhausted, suspend the origin until reviewed.
        if record.status_code == Some(429) {
            let origin_cooldown = if record.change == Change::Retry {
                due.context("missing retry deadline")?
            } else {
                i64::MAX
            };
            tx.execute(
                "UPDATE scout_origins SET next_allowed_ms=MAX(next_allowed_ms,?1) WHERE origin=?2",
                params![origin_cooldown, host],
            )?;
        }
        tx.execute("UPDATE scout_origins SET lease_source=NULL,lease_token=NULL,lease_until_ms=NULL WHERE origin=?1 AND lease_token=?2",params![host,lease.fencing_token])?;
        tx.execute(
            "INSERT INTO scout_events(job_id,source_id,lease_token,payload) VALUES(?1,?2,?3,?4)",
            params![lease.job_id, p.source_id, lease.fencing_token, serialized],
        )?;
        tx.commit()?;
        Ok(())
    }
    fn fail_parse(
        &self,
        tx: &Transaction<'_>,
        p: &SourcePolicy,
        token: &str,
        previous_attempts: u32,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let attempts = previous_attempts.saturating_add(1);
        let state = if attempts >= p.max_attempts {
            "exhausted"
        } else {
            "idle"
        };
        let due = if state == "idle" {
            Some(deadline_ms(now).saturating_add(p.min_interval_seconds as i64 * 1000))
        } else {
            None
        };
        tx.execute("UPDATE scout_sources SET state=?1,next_due_ms=?2,protected_until_ms=?3,attempts=?4,last_error='parse_failed',job_id=NULL,lease_token=NULL,lease_until_ms=NULL WHERE source_id=?5",params![state,due,due.unwrap_or(0),attempts,p.source_id])?;
        tx.execute("UPDATE scout_origins SET lease_source=NULL,lease_token=NULL,lease_until_ms=NULL WHERE origin=?1 AND lease_token=?2",params![origin(p)?,token])?;
        Ok(())
    }
}
impl ScoutStore for SqliteScoutStore {
    async fn claim_due(&self, now: DateTime<Utc>) -> Result<Option<Lease>> {
        self.claim_at(now)
    }
    async fn commit(
        &self,
        lease: &Lease,
        record: &FetchRecord,
        content: Option<&[u8]>,
    ) -> Result<()> {
        self.commit_at(lease, record, content, Utc::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    fn policy(id: &str) -> SourcePolicy {
        SourcePolicy{source_id:id.into(),source_url:"https://api.gleif.org/api/v1/lei-records?filter%5Bentity.legalAddress.country%5D=VN".into(),enabled:true,access_authorized:true,robots_and_terms_checked:true,allowed_hosts:vec!["api.gleif.org".into()],min_interval_seconds:300,max_requests_per_day:10,process_content_authorized:true,..SourcePolicy::default()}
    }
    fn memory(policies: Vec<SourcePolicy>) -> SqliteScoutStore {
        SqliteScoutStore::from_conn(Connection::open_in_memory().unwrap(), policies).unwrap()
    }
    fn payload() -> Vec<u8> {
        br#"{"data":[{"id":"11111111111111111111","attributes":{"entity":{"legalName":{"name":"Synthetic Entity"},"legalAddress":{"country":"VN"},"registeredAs":"REG-1","status":"ACTIVE"},"registration":{"status":"ISSUED"}}}]}"#.to_vec()
    }
    fn outcome(l: &Lease, t: DateTime<Utc>, body: &[u8]) -> FetchRecord {
        FetchRecord {
            source_id: l.policy.source_id.clone(),
            job_id: l.job_id.clone(),
            observed_at: t,
            change: if l.previous_hash.is_none() {
                Change::FirstSeen
            } else {
                Change::Changed
            },
            status_code: Some(200),
            content_type: Some("application/json".into()),
            content_hash: Some(hex::encode(Sha256::digest(body))),
            previous_hash: l.previous_hash.clone(),
            etag: Some("v1".into()),
            last_modified: None,
            next_due: Some(t + Duration::seconds(300)),
            attempts: l.attempts + 1,
            reason: "success".into(),
        }
    }
    #[test]
    fn paginated_commit_advances_cursor_atomically_and_resets_after_last_page() -> Result<()> {
        let s = memory(vec![policy("gleif-vn-latest-100-v1")]);
        let now = Utc::now() + Duration::seconds(1);
        let lease = s.claim_at(now)?.unwrap();
        assert!(
            lease
                .request_url
                .as_deref()
                .unwrap()
                .contains("page%5Bnumber%5D=1")
        );
        let data: Vec<serde_json::Value> = (0..100).map(|n| serde_json::json!({"id": format!("{n:020}"), "attributes":{"entity":{"legalName":{"name":"Synthetic"},"legalAddress":{"country":"VN"}},"registration":{}}})).collect();
        let bytes = serde_json::to_vec(&serde_json::json!({"data": data}))?;
        let rec = outcome(&lease, now, &bytes);
        s.commit_at(&lease, &rec, Some(&bytes), now)?;
        s.commit_at(&lease, &rec, Some(&bytes), now)?;
        assert_eq!(s.pending_outbox(10)?.len(), 1);
        assert_eq!(s.snapshot_at(now)?[0].cursor, 1);
        let later = now + Duration::seconds(301);
        let next = s.claim_at(later)?.unwrap();
        assert!(
            next.request_url
                .as_deref()
                .unwrap()
                .contains("page%5Bnumber%5D=2")
        );
        assert!(next.previous_hash.is_none());
        let bad = b"bad";
        assert!(
            s.commit_at(&next, &outcome(&next, later, bad), Some(bad), later)
                .is_err()
        );
        assert_eq!(s.snapshot_at(later)?[0].cursor, 1);
        let retry_at = later + Duration::seconds(301);
        let retry = s.claim_at(retry_at)?.unwrap();
        let last = payload();
        s.commit_at(
            &retry,
            &outcome(&retry, retry_at, &last),
            Some(&last),
            retry_at,
        )?;
        assert_eq!(s.snapshot_at(retry_at)?[0].cursor, 0);
        assert_eq!(s.pending_outbox(10)?.len(), 2);
        Ok(())
    }
    #[test]
    fn wikidata_page_produces_separate_outbox_and_checkpoint() -> Result<()> {
        let p = SourcePolicy {
            source_id: "wikidata-vn-business-v1".into(),
            source_url: "https://query.wikidata.org/sparql".into(),
            enabled: true,
            access_authorized: true,
            robots_and_terms_checked: true,
            allowed_hosts: vec!["query.wikidata.org".into()],
            min_interval_seconds: 300,
            max_requests_per_day: 10,
            process_content_authorized: true,
            ..SourcePolicy::default()
        };
        let s = memory(vec![p]);
        let now = Utc::now() + Duration::seconds(1);
        let lease = s.claim_at(now)?.unwrap();
        assert!(
            lease
                .request_url
                .as_ref()
                .unwrap()
                .starts_with("https://query.wikidata.org/sparql?")
        );
        let data = br#"{"results":{"bindings":[{"item":{"value":"http://www.wikidata.org/entity/Q123"},"name":{"value":"Synthetic"}}]}}"#;
        s.commit_at(&lease, &outcome(&lease, now, data), Some(data), now)?;
        let pending = s.pending_outbox(10)?;
        assert!(pending[0].candidates.is_empty());
        assert_eq!(pending[0].wikidata_candidates[0].qid, "Q123");
        assert_eq!(s.snapshot_at(now)?[0].cursor, 0);
        Ok(())
    }
    #[test]
    fn unsupported_source_never_processes_content() -> Result<()> {
        let mut p = policy("unknown");
        p.source_url = "https://api.gleif.org/unsupported".into();
        let s = memory(vec![p]);
        let now = Utc::now() + Duration::seconds(1);
        let lease = s.claim_at(now)?.unwrap();
        let bytes = payload();
        assert!(
            s.commit_at(&lease, &outcome(&lease, now, &bytes), Some(&bytes), now)
                .is_err()
        );
        assert!(s.pending_outbox(10)?.is_empty());
        Ok(())
    }
    #[test]
    fn outbox_and_validators_commit_atomically_and_are_idempotent() -> Result<()> {
        let s = memory(vec![policy("s")]);
        let now = Utc::now() + Duration::seconds(1);
        let lease = s.claim_at(now)?.unwrap();
        let bytes = payload();
        let rec = outcome(&lease, now, &bytes);
        s.commit_at(&lease, &rec, Some(&bytes), now)?;
        s.commit_at(&lease, &rec, Some(&bytes), now)?;
        let pending = s.pending_outbox(10)?;
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].candidates[0].registered_as.as_deref(),
            Some("REG-1")
        );
        assert_eq!(s.snapshot_at(now)?[0].content_hash, rec.content_hash);
        let mut tampered = rec.clone();
        tampered.reason = "different".into();
        assert!(s.commit_at(&lease, &tampered, Some(&bytes), now).is_err());
        s.ack_outbox(&pending[0].id)?;
        s.ack_outbox(&pending[0].id)?;
        assert!(s.pending_outbox(10)?.is_empty());
        Ok(())
    }
    #[test]
    fn parse_failure_never_advances_etag_or_creates_outbox() -> Result<()> {
        let s = memory(vec![policy("s")]);
        let now = Utc::now() + Duration::seconds(1);
        let first = s.claim_at(now)?.unwrap();
        let data = payload();
        let record = outcome(&first, now, &data);
        s.commit_at(&first, &record, Some(&data), now)?;
        let old = s.pending_outbox(1)?.remove(0);
        s.ack_outbox(&old.id)?;
        let later = now + Duration::seconds(301);
        let lease = s.claim_at(later)?.unwrap();
        let bad = b"not-json";
        let mut rec = outcome(&lease, later, bad);
        rec.etag = Some("bad-new".into());
        assert!(s.commit_at(&lease, &rec, Some(bad), later).is_err());
        assert!(s.pending_outbox(10)?.is_empty());
        let snap = s.snapshot_at(later)?.remove(0);
        assert_eq!(snap.content_hash, record.content_hash);
        assert_eq!(snap.attempts, 1);
        let next = s.claim_at(later + Duration::seconds(301))?.unwrap();
        assert_eq!(next.previous_etag.as_deref(), Some("v1"));
        Ok(())
    }
    #[test]
    fn stale_disabled_leases_cannot_commit_and_origins_are_coordinated() -> Result<()> {
        let mut p = policy("s");
        p.max_requests_per_day = 1;
        let mut p2 = policy("s2");
        p2.max_requests_per_day = 5;
        let s = memory(vec![p, p2]);
        let now = Utc::now() + Duration::seconds(1);
        let lease = s.claim_at(now)?.unwrap();
        assert!(s.claim_at(now)?.is_none());
        let data = payload();
        let rec = outcome(&lease, now, &data);
        s.set_enabled(&lease.policy.source_id, false)?;
        assert!(s.commit_at(&lease, &rec, Some(&data), now).is_err());
        assert!(s.pending_outbox(10)?.is_empty());
        // Disable fences stale work but cannot undo the consumed origin slot or minimum interval.
        assert!(s.claim_at(now)?.is_none());
        Ok(())
    }
    #[test]
    fn run_now_respects_shared_origin_daily_quota_and_interval() -> Result<()> {
        let mut p = policy("s1");
        p.max_requests_per_day = 1;
        let mut p2 = policy("s2");
        p2.max_requests_per_day = 5;
        let s = memory(vec![p, p2]);
        let next = Utc::now()
            .date_naive()
            .succ_opt()
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc();
        let lease = s.claim_at(next)?.unwrap();
        let data = payload();
        let rec = outcome(&lease, next, &data);
        s.commit_at(&lease, &rec, Some(&data), next)?;
        let other = if lease.policy.source_id == "s1" {
            "s2"
        } else {
            "s1"
        };
        let snapshot = s.snapshot_at(next + Duration::seconds(1))?;
        let waiting = snapshot.iter().find(|row| row.source_id == other).unwrap();
        assert!(waiting.next_due.unwrap() >= time(next_day_ms(next)?)?);
        assert!(!waiting.origin_suspended);
        let early = s.run_now(other, next + Duration::seconds(1))?;
        assert!(early >= time(next_day_ms(next)?)?);
        assert!(s.claim_at(next + Duration::seconds(301))?.is_none());
        assert_eq!(s.snapshot_at(next)?[0].origin_requests_today, 1);
        Ok(())
    }
    #[test]
    fn rate_limit_blocks_the_whole_origin_and_exhaustion_requires_review() -> Result<()> {
        let now = time(ms(Utc::now() + Duration::seconds(1)))?;
        let s = memory(vec![policy("s1"), policy("s2")]);
        let lease = s.claim_at(now)?.unwrap();
        let other = if lease.policy.source_id == "s1" {
            "s2"
        } else {
            "s1"
        };
        let mut rec = outcome(&lease, now, &payload());
        rec.change = Change::Retry;
        rec.status_code = Some(429);
        rec.content_hash = None;
        rec.next_due = Some(now + Duration::seconds(3600));
        s.commit_at(&lease, &rec, None, now)?;
        let snapshot = s.snapshot_at(now + Duration::seconds(301))?;
        let waiting = snapshot.iter().find(|row| row.source_id == other).unwrap();
        assert_eq!(waiting.next_due, Some(now + Duration::seconds(3600)));
        assert!(!waiting.origin_suspended);
        assert!(s.run_now(other, now + Duration::seconds(301))? >= now + Duration::seconds(3600));
        assert!(s.claim_at(now + Duration::seconds(301))?.is_none());
        let isolated = memory(vec![policy("x"), policy("y")]);
        let lease = isolated.claim_at(now)?.unwrap();
        let other = if lease.policy.source_id == "x" {
            "y"
        } else {
            "x"
        };
        let mut rec = outcome(&lease, now, &payload());
        rec.change = Change::Exhausted;
        rec.status_code = Some(429);
        rec.content_hash = None;
        rec.next_due = None;
        isolated.commit_at(&lease, &rec, None, now)?;
        for row in isolated.snapshot_at(now + Duration::seconds(301))? {
            assert!(row.origin_suspended);
            assert!(row.next_due.is_none());
        }
        assert!(
            isolated
                .run_now(other, now + Duration::seconds(301))
                .is_err()
        );
        isolated.resume_origin_after_review(other, now + Duration::seconds(301))?;
        let snapshot = isolated.snapshot_at(now + Duration::seconds(301))?;
        let waiting = snapshot.iter().find(|row| row.source_id == other).unwrap();
        assert!(!waiting.origin_suspended);
        assert!(waiting.next_due.unwrap() >= now + Duration::seconds(601));
        assert!(
            isolated.run_now(other, now + Duration::seconds(301))? >= now + Duration::seconds(601)
        );
        Ok(())
    }
    #[test]
    fn review_recovers_forbidden_source_without_resetting_quota_or_checkpoint() -> Result<()> {
        let mut p = policy("gleif-vn-latest-100-v1");
        p.max_requests_per_day = 1;
        let s = memory(vec![p]);
        let now = time(ms(Utc::now() + Duration::seconds(1)))?;
        let lease = s.claim_at(now)?.unwrap();
        let mut record = outcome(&lease, now, &payload());
        record.change = Change::Blocked;
        record.status_code = Some(403);
        record.reason = "access_denied_reapproval_required".into();
        record.content_hash = None;
        record.next_due = None;
        s.commit_at(&lease, &record, None, now)?;
        assert_eq!(s.snapshot_at(now)?[0].state, "blocked");
        s.resume_origin_after_review(&lease.policy.source_id, now + Duration::seconds(10))?;
        let snap = s.snapshot_at(now + Duration::seconds(10))?.remove(0);
        assert_eq!(snap.state, "idle");
        assert_eq!(snap.cursor, 0);
        assert_eq!(snap.origin_requests_today, 1);
        assert!(snap.next_due.unwrap() >= time(next_day_ms(now)?)?);
        assert!(s.claim_at(now + Duration::seconds(311))?.is_none());
        Ok(())
    }
    #[test]
    fn exhausted_rate_limit_preserves_retry_after_across_origin_review() -> Result<()> {
        let s = memory(vec![policy("s1"), policy("s2")]);
        let now = time(ms(Utc::now() + Duration::seconds(1)))?;
        let lease = s.claim_at(now)?.unwrap();
        let other = if lease.policy.source_id == "s1" {
            "s2"
        } else {
            "s1"
        };
        let mut rec = outcome(&lease, now, &payload());
        rec.change = Change::Exhausted;
        rec.status_code = Some(429);
        rec.content_hash = None;
        rec.next_due = Some(now + Duration::seconds(7200));
        s.commit_at(&lease, &rec, None, now)?;
        s.resume_origin_after_review(other, now + Duration::seconds(1))?;
        let snap = s.snapshot_at(now + Duration::seconds(2))?;
        assert!(
            snap.iter()
                .find(|x| x.source_id == other)
                .unwrap()
                .next_due
                .unwrap()
                >= now + Duration::seconds(7200)
        );
        s.resume_origin_after_review(&lease.policy.source_id, now + Duration::seconds(3))?;
        assert!(
            s.snapshot_at(now + Duration::seconds(4))?
                .iter()
                .all(|x| x.next_due.unwrap() >= now + Duration::seconds(7200))
        );
        assert!(s.claim_at(now + Duration::seconds(7199))?.is_none());
        Ok(())
    }
    #[test]
    fn concurrent_workers_get_only_one_origin_lease() -> Result<()> {
        let s = memory(vec![policy("s1"), policy("s2")]);
        let now = Utc::now() + Duration::seconds(1);
        let mut workers = Vec::new();
        for _ in 0..8 {
            let store = s.clone();
            workers.push(std::thread::spawn(move || {
                store.claim_at(now).unwrap().is_some()
            }));
        }
        assert_eq!(
            workers
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|v| *v)
                .count(),
            1
        );
        Ok(())
    }
    #[test]
    fn pagination_migration_fences_old_leases_without_refunding_quota() -> Result<()> {
        let path = std::env::temp_dir().join(format!("scout-migrate-{}.db", Uuid::new_v4()));
        let id = "gleif-vn-latest-100-v1";
        let now = Utc::now() + Duration::seconds(1);
        let first = SqliteScoutStore::open(&path, vec![policy(id)], None)?;
        let lease = first.claim_at(now)?.unwrap();
        {
            let c = first.lock()?;
            c.execute("DELETE FROM scout_cursors WHERE source_id=?1", [id])?;
        }
        let migrated = SqliteScoutStore::open(&path, vec![policy(id)], None)?;
        let bytes = payload();
        assert!(
            first
                .commit_at(&lease, &outcome(&lease, now, &bytes), Some(&bytes), now)
                .is_err()
        );
        let snapshot = migrated.snapshot_at(now)?.remove(0);
        assert_eq!(snapshot.state, "idle");
        assert_eq!(snapshot.origin_requests_today, 1);
        assert!(migrated.claim_at(now + Duration::seconds(1))?.is_none());
        drop(first);
        drop(migrated);
        std::fs::remove_file(path)?;
        Ok(())
    }
    #[test]
    fn expiry_and_restart_preserve_state_and_quota() -> Result<()> {
        let path = std::env::temp_dir().join(format!("scout-store-{}.db", Uuid::new_v4()));
        let now = Utc::now() + Duration::seconds(1);
        let lease = {
            let s = SqliteScoutStore::open(&path, vec![policy("s")], None)?;
            s.claim_at(now)?.unwrap()
        };
        {
            let s = SqliteScoutStore::open(&path, vec![policy("s")], None)?;
            assert!(s.claim_at(now + Duration::seconds(60))?.is_none());
            let stale = outcome(&lease, now, &payload());
            assert!(
                s.commit_at(
                    &lease,
                    &stale,
                    Some(&payload()),
                    now + Duration::seconds(60)
                )
                .is_err()
            );
            assert_eq!(s.snapshot_at(now + Duration::seconds(60))?[0].attempts, 1);
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
}
