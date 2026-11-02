//! Durable domain storage for LANTERN. No external data is assumed to be complete.
mod catalog;
pub mod risk;
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Company {
    pub id: String,
    pub name: String,
    pub tax_id: Option<String>,
    pub ticker: Option<String>,
    pub sector: Option<String>,
    pub location: Option<String>,
    pub website: Option<String>,
    pub source_url: Option<String>,
    pub description: Option<String>,
    pub identity_status: String,
    pub watched: bool,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Evidence {
    pub id: String,
    pub company_id: String,
    pub title: String,
    pub url: String,
    pub publisher: String,
    pub claim: String,
    pub dimension: String,
    pub status: String,
    pub published_at: Option<String>,
    pub observed_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceReview {
    pub evidence_id: String,
    pub decision: String,
    pub reviewed_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Investigation {
    pub id: String,
    pub company_id: Option<String>,
    pub query: String,
    pub as_of: Option<String>,
    pub status: String,
    pub stage: String,
    pub summary: String,
    pub limitations: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    pub id: String,
    pub investigation_id: String,
    pub status: String,
    pub stage: String,
    pub message: String,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub url: String,
    pub description: String,
}
/// Durable local delivery work. Payload is an allowlisted, unverified research
/// record; it never includes chat, job query, summary, user watch flag, or keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResearchOutboxItem {
    pub id: String,
    pub kind: String,
    pub company_id: String,
    pub entity_id: String,
    pub version: String,
    pub payload: Value,
    pub attempts: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchOutboxCounts {
    pub pending: i64,
    pub delivered: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchBackfill {
    pub scanned: usize,
    pub enqueued: usize,
    pub skipped: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Reservation {
    pub id: String,
    pub kind: String,
    pub reserved_cents: i64,
    pub actual_cents: Option<i64>,
    pub status: String,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Budget {
    pub cap_cents: i64,
    pub charged_cents: i64,
    pub reserved_cents: i64,
    pub available_cents: i64,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TemporalMode {
    PublicKnowable,
    SystemKnown,
    Retrospective,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Commitment {
    pub algorithm: String,
    pub salt: String,
    pub digest: String,
}

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}
fn now() -> String {
    Utc::now().to_rfc3339()
}
fn lock(conn: &Mutex<Connection>) -> Result<MutexGuard<'_, Connection>> {
    conn.lock()
        .map_err(|_| anyhow::anyhow!("database lock poisoned"))
}
fn decode<T: DeserializeOwned>(s: String) -> Result<T> {
    Ok(serde_json::from_str(&s)?)
}
fn required(s: &str, what: &str) -> Result<()> {
    ensure!(!s.trim().is_empty(), "{what} is required");
    Ok(())
}
fn web_url(s: &str) -> Result<()> {
    let url = url::Url::parse(s).context("invalid source URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "URL must be HTTP(S)"
    );
    Ok(())
}
/// Normalize for Vietnamese name discovery only, never for automatic identity merging.
pub fn normalize_search(s: &str) -> String {
    s.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'đ' => 'd',
            _ => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
/// Syntax normalization only: a valid-looking number is not proof of registration.
pub fn normalize_tax_id(tax: &str) -> Result<String> {
    let t = tax.trim();
    let normalized = if t.len() == 14 && t.as_bytes().get(10) == Some(&b'-') {
        t.replace('-', "")
    } else {
        t.to_string()
    };
    ensure!(
        (normalized.len() == 10 || normalized.len() == 13)
            && normalized.bytes().all(|b| b.is_ascii_digit()),
        "tax ID must be 10 or 13 digits (a branch may use 10-3)"
    );
    Ok(normalized)
}
fn parse_time(s: &str, end_of_day: bool) -> Result<DateTime<Utc>> {
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let t = if end_of_day {
            d.and_hms_nano_opt(23, 59, 59, 999_999_999).unwrap()
        } else {
            d.and_hms_opt(0, 0, 0).unwrap()
        };
        let vietnam = FixedOffset::east_opt(7 * 3600).expect("valid UTC offset");
        Ok(vietnam
            .from_local_datetime(&t)
            .single()
            .expect("fixed offset has no ambiguity")
            .with_timezone(&Utc))
    } else {
        Ok(DateTime::parse_from_rfc3339(s)
            .context("expected ISO date or RFC3339 timestamp")?
            .with_timezone(&Utc))
    }
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }
    pub fn memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }
    fn from_connection(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(
            version <= 2,
            "database schema is newer than this application"
        );
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS companies(id TEXT PRIMARY KEY, tax_id TEXT UNIQUE, search_text TEXT NOT NULL, watched INTEGER NOT NULL, payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS evidence(id TEXT PRIMARY KEY, company_id TEXT NOT NULL REFERENCES companies(id), published_at TEXT, observed_at TEXT NOT NULL, payload TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS evidence_company ON evidence(company_id);
CREATE TABLE IF NOT EXISTS investigations(id TEXT PRIMARY KEY, company_id TEXT REFERENCES companies(id), status TEXT NOT NULL, created_at TEXT NOT NULL, payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY, investigation_id TEXT NOT NULL REFERENCES investigations(id), created_at TEXT NOT NULL, payload TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS events_investigation ON events(investigation_id,created_at);
CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY, created_at TEXT NOT NULL, payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sources(id TEXT PRIMARY KEY, payload TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS reservations(id TEXT PRIMARY KEY, kind TEXT NOT NULL, reserved_cents INTEGER NOT NULL CHECK(reserved_cents>0), actual_cents INTEGER, status TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS app_settings(key TEXT PRIMARY KEY, value INTEGER NOT NULL CHECK(value BETWEEN 1 AND 1000000));
CREATE TABLE IF NOT EXISTS investigation_candidates(investigation_id TEXT NOT NULL REFERENCES investigations(id), company_id TEXT NOT NULL REFERENCES companies(id), PRIMARY KEY(investigation_id,company_id));
CREATE TABLE IF NOT EXISTS investigation_evidence(investigation_id TEXT NOT NULL REFERENCES investigations(id), evidence_id TEXT NOT NULL REFERENCES evidence(id), PRIMARY KEY(investigation_id,evidence_id));
CREATE TABLE IF NOT EXISTS evidence_reviews(id INTEGER PRIMARY KEY AUTOINCREMENT, evidence_id TEXT NOT NULL REFERENCES evidence(id), decision TEXT NOT NULL CHECK(decision IN ('subject_confirmed','not_about_company')), reviewed_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS evidence_reviews_by_id ON evidence_reviews(evidence_id,id);
CREATE TABLE IF NOT EXISTS research_outbox(id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('company','claim')), company_id TEXT NOT NULL, entity_id TEXT NOT NULL, version TEXT NOT NULL, payload TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('pending','delivered')), attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0), created_at TEXT NOT NULL, delivered_at TEXT);
CREATE INDEX IF NOT EXISTS research_outbox_pending ON research_outbox(status,attempts,created_at,id);
PRAGMA user_version=2; COMMIT;")?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.seed_sources()?;
        Ok(store)
    }
    fn seed_sources(&self) -> Result<()> {
        let sources = [
            Source {
                id: "registry".into(),
                name: "Cổng đăng ký doanh nghiệp".into(),
                url: "https://dangkykinhdoanh.gov.vn/".into(),
                description: "Tra cứu đăng ký; quyền tự động hóa và dữ liệu lịch sử cần xác minh."
                    .into(),
            },
            Source {
                id: "tax".into(),
                name: "Tra cứu người nộp thuế".into(),
                url: "https://tracuunnt.gdt.gov.vn/".into(),
                description: "Tra cứu thuế; kết nối và quyền chưa được xác minh.".into(),
            },
            Source {
                id: "hnx".into(),
                name: "HNX / UPCoM".into(),
                url: "https://www.hnx.vn/".into(),
                description: "Công bố thông tin chứng khoán; không bao phủ mọi doanh nghiệp."
                    .into(),
            },
            Source {
                id: "vsdc".into(),
                name: "VSDC".into(),
                url: "https://www.vsd.vn/".into(),
                description: "Thông tin chứng khoán và thực hiện quyền.".into(),
            },
            Source {
                id: "court".into(),
                name: "Công bố bản án".into(),
                url: "https://congbobanan.toaan.gov.vn/".into(),
                description:
                    "Chỉ các bản án, quyết định được công bố; không thể suy ra không có tranh chấp."
                        .into(),
            },
            Source {
                id: "enforcement".into(),
                name: "Thi hành án dân sự".into(),
                url: "https://thads.moj.gov.vn/".into(),
                description: "Nguồn thông tin thi hành án, phạm vi công bố có giới hạn.".into(),
            },
            Source {
                id: "gleif".into(),
                name: "GLEIF LEI Việt Nam".into(),
                url: "https://www.gleif.org/en/lei-data/gleif-api/".into(),
                description: "Scout phân trang LEI có địa chỉ pháp lý Việt Nam; CC0, không xác nhận MST.".into(),
            },
            Source {
                id: "wikidata".into(),
                name: "Wikidata Việt Nam".into(),
                url: "https://www.wikidata.org/wiki/Wikidata:Licensing".into(),
                description: "Mục doanh nghiệp, website và ticker cộng đồng CC0; chưa xác minh, bật nguồn riêng.".into(),
            },
        ];
        let conn = lock(&self.conn)?;
        for s in sources {
            conn.execute(
                "INSERT OR IGNORE INTO sources(id,payload) VALUES(?1,?2)",
                params![s.id, serde_json::to_string(&s)?],
            )?;
        }
        Ok(())
    }
    pub fn list_sources(&self) -> Result<Vec<Source>> {
        let conn = lock(&self.conn)?;
        let mut st = conn.prepare("SELECT payload FROM sources ORDER BY id")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| decode(r?)).collect()
    }
    pub fn save_company(&self, c: &Company) -> Result<()> {
        let conn = lock(&self.conn)?;
        write_company(&conn, c)
    }
    pub fn get_company(&self, id: &str) -> Result<Option<Company>> {
        let conn = lock(&self.conn)?;
        let s: Option<String> = conn
            .query_row("SELECT payload FROM companies WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        s.map(decode).transpose()
    }
    pub fn list_companies(&self, query: &str, limit: u32, offset: u32) -> Result<Vec<Company>> {
        let conn = lock(&self.conn)?;
        let q = normalize_search(query);
        let mut st=conn.prepare("SELECT payload FROM companies WHERE instr(search_text,?1)>0 ORDER BY watched DESC,search_text,id LIMIT ?2 OFFSET ?3")?;
        let rows = st.query_map(params![q, limit.clamp(1, 200), offset], |r| {
            r.get::<_, String>(0)
        })?;
        rows.map(|r| decode(r?)).collect()
    }
    pub fn set_watch(&self, id: &str, watched: bool) -> Result<()> {
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let s: String = tx
            .query_row("SELECT payload FROM companies WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .context("company not found")?;
        let mut c: Company = decode(s)?;
        c.watched = watched;
        c.updated_at = now();
        tx.execute(
            "UPDATE companies SET watched=?1,payload=?2 WHERE id=?3",
            params![watched, serde_json::to_string(&c)?, id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn save_evidence(&self, e: &Evidence) -> Result<()> {
        let conn = lock(&self.conn)?;
        write_evidence(&conn, e)
    }
    /// System-known as-of requires observation <= T; a known publication date must also be <= T.
    /// This is not a retrospective reconstruction or a complete public archive.
    pub fn list_evidence(&self, company_id: &str, as_of: Option<&str>) -> Result<Vec<Evidence>> {
        match as_of {
            Some(t) => self.evidence_at(company_id, t, TemporalMode::SystemKnown),
            None => self.evidence_at(company_id, &now(), TemporalMode::Retrospective),
        }
    }
    /// PublicKnowable supplies dated candidates, not proof that the exact version was publicly
    /// accessible at T. Retrospective returns present evidence, not a reconstructed effective state:
    /// this schema does not record valid intervals. Callers must surface both limitations.
    pub fn evidence_at(
        &self,
        company_id: &str,
        as_of: &str,
        mode: TemporalMode,
    ) -> Result<Vec<Evidence>> {
        let cutoff = parse_time(as_of, true)?;
        let conn = lock(&self.conn)?;
        let mut st = conn.prepare(
            "SELECT payload FROM evidence WHERE company_id=?1 ORDER BY observed_at DESC LIMIT 500",
        )?;
        let rows = st.query_map([company_id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let mut e: Evidence = decode(row?)?;
            // Malformed legacy publication dates are unknown, not evidence of a specific day.
            let publication = e
                .published_at
                .as_deref()
                .and_then(|p| parse_time(p, true).ok());
            if publication.is_none() {
                e.published_at = None;
            }
            match mode {
                TemporalMode::PublicKnowable => {
                    if publication.is_none_or(|p| p > cutoff) {
                        continue;
                    }
                }
                TemporalMode::SystemKnown => {
                    if parse_time(&e.observed_at, true).map_or(true, |t| t > cutoff)
                        || publication.is_some_and(|p| p > cutoff)
                    {
                        continue;
                    }
                }
                TemporalMode::Retrospective => {}
            }
            out.push(e);
        }
        Ok(out)
    }
    pub fn create_investigation(&self, i: &Investigation) -> Result<()> {
        required(&i.id, "investigation id")?;
        required(&i.query, "query")?;
        ensure!(i.status == "queued", "new investigation must be queued");
        if let Some(t) = &i.as_of {
            parse_time(t, true)?;
        }
        parse_time(&i.created_at, false)?;
        parse_time(&i.updated_at, false)?;
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO investigations(id,company_id,status,created_at,payload) VALUES(?1,?2,?3,?4,?5)",params![i.id,i.company_id,i.status,i.created_at,serde_json::to_string(i)?])?;
        insert_event(
            &tx,
            &Event {
                id: Uuid::new_v4().to_string(),
                investigation_id: i.id.clone(),
                status: i.status.clone(),
                stage: i.stage.clone(),
                message: "Đã tạo yêu cầu".into(),
                created_at: i.created_at.clone(),
            },
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn get_investigation(&self, id: &str) -> Result<Option<Investigation>> {
        let conn = lock(&self.conn)?;
        let s: Option<String> = conn
            .query_row(
                "SELECT payload FROM investigations WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        s.map(decode).transpose()
    }
    pub fn list_investigations(&self, limit: u32) -> Result<Vec<Investigation>> {
        let conn = lock(&self.conn)?;
        let mut st = conn.prepare(
            "SELECT payload FROM investigations ORDER BY created_at DESC,id DESC LIMIT ?1",
        )?;
        let rows = st.query_map([limit.clamp(1, 200)], |r| r.get::<_, String>(0))?;
        rows.map(|r| decode(r?)).collect()
    }
    /// Only candidates imported by this particular lookup; legacy rows are not guessed.
    pub fn candidate_ids(&self, job_id: &str) -> Result<Vec<String>> {
        let conn = lock(&self.conn)?;
        let mut st=conn.prepare("SELECT company_id FROM investigation_candidates WHERE investigation_id=?1 ORDER BY rowid")?;
        let rows = st.query_map([job_id], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Only evidence explicitly imported by this investigation, including current review status.
    pub fn evidence_for_investigation(&self, job_id: &str) -> Result<Vec<Evidence>> {
        let conn = lock(&self.conn)?;
        let mut st=conn.prepare("SELECT e.payload FROM investigation_evidence ie JOIN evidence e ON e.id=ie.evidence_id WHERE ie.investigation_id=?1 ORDER BY ie.rowid")?;
        let rows = st.query_map([job_id], |r| r.get::<_, String>(0))?;
        rows.map(|r| decode(r?)).collect()
    }
    /// Review concerns subject association only. It does not verify the claim, credit, or risk.
    pub fn review_evidence(&self, evidence_id: &str, decision: &str) -> Result<()> {
        ensure!(
            matches!(decision, "subject_confirmed" | "not_about_company"),
            "invalid review decision"
        );
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let payload: String = tx
            .query_row(
                "SELECT payload FROM evidence WHERE id=?1",
                [evidence_id],
                |r| r.get(0),
            )
            .context("evidence not found")?;
        let mut item: Evidence = decode(payload)?;
        ensure!(
            matches!(
                item.status.as_str(),
                "unverified" | "subject_confirmed" | "not_about_company"
            ),
            "evidence status cannot be reviewed"
        );
        item.status = decision.into();
        tx.execute(
            "UPDATE evidence SET payload=?1 WHERE id=?2",
            params![serde_json::to_string(&item)?, evidence_id],
        )?;
        tx.execute(
            "INSERT INTO evidence_reviews(evidence_id,decision,reviewed_at) VALUES(?1,?2,?3)",
            params![evidence_id, decision, now()],
        )?;
        if decision == "not_about_company" {
            // Pending suggestions are no longer eligible for delivery. Already
            // delivered immutable Mongo versions are not retracted by this.
            tx.execute("DELETE FROM research_outbox WHERE kind='claim' AND company_id=?1 AND entity_id=?2 AND status='pending'", params![item.company_id,evidence_id])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn evidence_review_history(&self, evidence_id: &str) -> Result<Vec<EvidenceReview>> {
        let conn = lock(&self.conn)?;
        let mut st=conn.prepare("SELECT evidence_id,decision,reviewed_at FROM evidence_reviews WHERE evidence_id=?1 ORDER BY id")?;
        let rows = st.query_map([evidence_id], |r| {
            Ok(EvidenceReview {
                evidence_id: r.get(0)?,
                decision: r.get(1)?,
                reviewed_at: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Attach an explicitly selected identity before the investigation is terminal.
    /// Commit imports and terminal state together. Cancellation wins if it commits first.
    /// Lookup jobs have no selected entity and may import candidates, never attach evidence.
    pub fn finalize_import(
        &self,
        id: &str,
        companies: &[Company],
        evidence: &[Evidence],
        summary: &str,
        limitations: &[String],
        final_status: &str,
    ) -> Result<()> {
        ensure!(
            matches!(final_status, "completed" | "partial"),
            "final status must be completed or partial"
        );
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let s: String = tx
            .query_row(
                "SELECT payload FROM investigations WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .context("investigation not found")?;
        let mut job: Investigation = decode(s)?;
        ensure!(
            job.status == "verifying",
            "investigation is no longer verifying"
        );
        match job.company_id.as_deref() {
            None => ensure!(
                evidence.is_empty(),
                "select an entity before attaching evidence"
            ),
            Some(selected) => {
                ensure!(
                    companies.iter().all(|c| c.id == selected),
                    "company does not match selected entity"
                );
                ensure!(
                    evidence.iter().all(|e| e.company_id == selected),
                    "evidence does not match selected entity"
                );
            }
        }
        for company in companies {
            write_company(&tx, company)?;
            if let Some(payload) = public_company_payload(company) {
                enqueue_research_outbox(&tx, "company", &company.id, &company.id, payload)?;
            }
            if job.company_id.is_none() {
                tx.execute("INSERT OR IGNORE INTO investigation_candidates(investigation_id,company_id) VALUES(?1,?2)",params![id,company.id])?;
            }
        }
        for item in evidence {
            ensure!(
                item.status == "unverified",
                "new evidence must be reviewed by the user"
            );
            write_evidence(&tx, item)?;
            if let Some(payload) = public_claim_payload(item) {
                enqueue_research_outbox(&tx, "claim", &item.company_id, &item.id, payload)?;
            }
            tx.execute(
                "INSERT INTO investigation_evidence(investigation_id,evidence_id) VALUES(?1,?2)",
                params![id, item.id],
            )?;
        }
        job.status = final_status.into();
        job.stage = "finished".into();
        job.summary = summary.into();
        job.limitations = limitations.to_vec();
        job.updated_at = now();
        tx.execute(
            "UPDATE investigations SET status=?1,payload=?2 WHERE id=?3",
            params![job.status, serde_json::to_string(&job)?, id],
        )?;
        insert_event(
            &tx,
            &Event {
                id: Uuid::new_v4().to_string(),
                investigation_id: id.into(),
                status: final_status.into(),
                stage: "finished".into(),
                message: "Tác vụ đã kết thúc; dữ kiện chưa rà giữ nhãn chưa xác minh.".into(),
                created_at: job.updated_at.clone(),
            },
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn set_investigation_company(&self, id: &str, company_id: &str) -> Result<Investigation> {
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let s: String = tx
            .query_row(
                "SELECT payload FROM investigations WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .context("investigation not found")?;
        let mut i: Investigation = decode(s)?;
        ensure!(
            matches!(i.status.as_str(), "queued" | "running"),
            "cannot change identity on a terminal or verifying investigation"
        );
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM companies WHERE id=?1)",
            [company_id],
            |r| r.get(0),
        )?;
        ensure!(exists, "company not found");
        i.company_id = Some(company_id.into());
        i.updated_at = now();
        tx.execute(
            "UPDATE investigations SET company_id=?1,payload=?2 WHERE id=?3",
            params![company_id, serde_json::to_string(&i)?, id],
        )?;
        insert_event(
            &tx,
            &Event {
                id: Uuid::new_v4().to_string(),
                investigation_id: id.into(),
                status: i.status.clone(),
                stage: i.stage.clone(),
                message: "Đã chọn pháp nhân".into(),
                created_at: i.updated_at.clone(),
            },
        )?;
        tx.commit()?;
        Ok(i)
    }
    pub fn transition_investigation(
        &self,
        id: &str,
        status: &str,
        stage: &str,
        message: &str,
    ) -> Result<Investigation> {
        self.transition_with_result(id, status, stage, message, None, None)
    }
    pub fn transition_with_result(
        &self,
        id: &str,
        status: &str,
        stage: &str,
        message: &str,
        summary: Option<&str>,
        limitations: Option<Vec<String>>,
    ) -> Result<Investigation> {
        required(stage, "stage")?;
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let s: String = tx
            .query_row(
                "SELECT payload FROM investigations WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .context("investigation not found")?;
        let mut i: Investigation = decode(s)?;
        ensure!(
            valid_transition(&i.status, status),
            "invalid investigation transition {} -> {}",
            i.status,
            status
        );
        i.status = status.into();
        i.stage = stage.into();
        i.updated_at = now();
        if let Some(s) = summary {
            i.summary = s.into()
        }
        if let Some(l) = limitations {
            i.limitations = l
        }
        tx.execute(
            "UPDATE investigations SET status=?1,payload=?2 WHERE id=?3",
            params![i.status, serde_json::to_string(&i)?, id],
        )?;
        insert_event(
            &tx,
            &Event {
                id: Uuid::new_v4().to_string(),
                investigation_id: id.into(),
                status: status.into(),
                stage: stage.into(),
                message: message.into(),
                created_at: i.updated_at.clone(),
            },
        )?;
        tx.commit()?;
        Ok(i)
    }
    pub fn events(&self, id: &str) -> Result<Vec<Event>> {
        let conn = lock(&self.conn)?;
        let mut st = conn.prepare(
            "SELECT payload FROM events WHERE investigation_id=?1 ORDER BY rowid ASC LIMIT 500",
        )?;
        let rows = st.query_map([id], |r| r.get::<_, String>(0))?;
        rows.map(|r| decode(r?)).collect()
    }
    /// Mark in-flight work partial on restart. Outstanding budget holds remain reserved.
    pub fn recover_interrupted(&self) -> Result<usize> {
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let items: Vec<String> = {
            let mut st = tx.prepare(
                "SELECT payload FROM investigations WHERE status IN ('running','verifying')",
            )?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for s in &items {
            let mut i: Investigation = decode(s.clone())?;
            i.status = "partial".into();
            i.stage = "reconciliation_required".into();
            i.updated_at = now();
            i.limitations.push("Tiến trình bị gián đoạn; kiểm tra trạng thái bên ngoài trước khi thử lại. Ngân sách đang giữ chưa tự giải phóng.".into());
            tx.execute(
                "UPDATE investigations SET status=?1,payload=?2 WHERE id=?3",
                params![i.status, serde_json::to_string(&i)?, i.id],
            )?;
            insert_event(
                &tx,
                &Event {
                    id: Uuid::new_v4().to_string(),
                    investigation_id: i.id.clone(),
                    status: i.status.clone(),
                    stage: i.stage.clone(),
                    message: "Cần đối soát sau khởi động lại".into(),
                    created_at: i.updated_at.clone(),
                },
            )?;
        }
        tx.commit()?;
        Ok(items.len())
    }
    pub fn save_message(&self, m: &Message) -> Result<()> {
        required(&m.id, "message id")?;
        required(&m.content, "message content")?;
        ensure!(
            matches!(m.role.as_str(), "user" | "assistant" | "system"),
            "invalid message role"
        );
        parse_time(&m.created_at, false)?;
        lock(&self.conn)?.execute(
            "INSERT INTO messages(id,created_at,payload) VALUES(?1,?2,?3)",
            params![m.id, m.created_at, serde_json::to_string(m)?],
        )?;
        Ok(())
    }
    pub fn messages(&self, limit: u32) -> Result<Vec<Message>> {
        let conn = lock(&self.conn)?;
        let mut st=conn.prepare("SELECT payload FROM (SELECT rowid,payload FROM messages ORDER BY rowid DESC LIMIT ?1) ORDER BY rowid ASC")?;
        let rows = st.query_map([limit.clamp(1, 200)], |r| r.get::<_, String>(0))?;
        rows.map(|r| decode(r?)).collect()
    }
    /// All money is in integer US cents. A hold remains counted across process restarts.
    pub fn reserve_budget(&self, kind: &str, amount_cents: i64) -> Result<Reservation> {
        let minimum = match kind {
            "reasoning" => 100,
            "audio" => 25,
            _ => bail!("unknown budget category"),
        };
        ensure!(
            amount_cents >= minimum,
            "reservation below conservative minimum"
        );
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let snapshot = budget_snapshot(&tx)?;
        ensure!(
            snapshot
                .charged_cents
                .saturating_add(snapshot.reserved_cents)
                .saturating_add(amount_cents)
                <= snapshot.cap_cents,
            "configured budget exhausted"
        );
        let item = Reservation {
            id: Uuid::new_v4().to_string(),
            kind: kind.into(),
            reserved_cents: amount_cents,
            actual_cents: None,
            status: "reserved".into(),
            created_at: now(),
        };
        tx.execute("INSERT INTO reservations(id,kind,reserved_cents,actual_cents,status,created_at) VALUES(?1,?2,?3,NULL,?4,?5)",params![item.id,item.kind,item.reserved_cents,item.status,item.created_at])?;
        tx.commit()?;
        Ok(item)
    }
    /// Only call after authoritative reconciliation; actual may exceed hold and blocks further reservations.
    pub fn settle_budget(&self, id: &str, actual_cents: i64) -> Result<()> {
        ensure!(actual_cents >= 0, "negative charge");
        let conn = lock(&self.conn)?;
        let changed=conn.execute("UPDATE reservations SET status='settled',actual_cents=?1 WHERE id=?2 AND status='reserved'",params![actual_cents,id])?;
        ensure!(changed == 1, "hold missing or already reconciled");
        Ok(())
    }
    /// Record a documented conservative upper bound for a reasoning reservation.
    /// This is an estimate for budget accounting, not a provider invoice or final charge.
    pub fn bound_budget(&self, id: &str, upper_cents: i64) -> Result<()> {
        ensure!(upper_cents > 0, "upper bound must be positive");
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed=tx.execute("UPDATE reservations SET status='bounded',actual_cents=?1 WHERE id=?2 AND kind='reasoning' AND status='reserved' AND reserved_cents>=?1",params![upper_cents,id])?;
        ensure!(
            changed == 1,
            "hold missing, already reconciled, not reasoning, or bound exceeds reservation"
        );
        tx.commit()?;
        Ok(())
    }
    /// Only call after confirming that the provider did not bill the request.
    pub fn release_budget(&self, id: &str) -> Result<()> {
        let conn = lock(&self.conn)?;
        let changed=conn.execute("UPDATE reservations SET status='released',actual_cents=0 WHERE id=?1 AND status='reserved'",[id])?;
        ensure!(changed == 1, "hold missing or already reconciled");
        Ok(())
    }
    pub fn reservations(&self, limit: u32) -> Result<Vec<Reservation>> {
        let conn = lock(&self.conn)?;
        let mut st=conn.prepare("SELECT id,kind,reserved_cents,actual_cents,status,created_at FROM reservations ORDER BY rowid DESC LIMIT ?1")?;
        let rows = st.query_map([limit.clamp(1, 200)], |r| {
            Ok(Reservation {
                id: r.get(0)?,
                kind: r.get(1)?,
                reserved_cents: r.get(2)?,
                actual_cents: r.get(3)?,
                status: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// Explicitly set the durable lifetime budget cap in cents, without releasing existing holds.
    pub fn set_budget_limit(&self, cents: i64) -> Result<()> {
        ensure!(
            (1..=1_000_000).contains(&cents),
            "budget must be between 1 and 1000000 cents"
        );
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = budget_snapshot(&tx)?;
        ensure!(
            cents >= current.charged_cents.saturating_add(current.reserved_cents),
            "budget cannot be lower than charged plus reserved"
        );
        tx.execute("INSERT INTO app_settings(key,value) VALUES('budget_cap_cents',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[cents])?;
        tx.commit()?;
        Ok(())
    }
    pub fn budget(&self) -> Result<Budget> {
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let snapshot = budget_snapshot(&tx)?;
        tx.commit()?;
        Ok(snapshot)
    }
    /// Reads bounded pending work without deleting it. Failed items remain
    /// pending and are rotated behind lower-attempt work for fairness.
    pub fn research_outbox_pending(&self, limit: u32) -> Result<Vec<ResearchOutboxItem>> {
        ensure!((1..=1000).contains(&limit), "invalid research outbox limit");
        let conn = lock(&self.conn)?;
        let mut st = conn.prepare("SELECT id,kind,company_id,entity_id,version,payload,attempts FROM research_outbox WHERE status='pending' ORDER BY attempts,created_at,id LIMIT ?1")?;
        let rows = st.query_map([limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?;
        rows.map(|row| {
            let (id, kind, company_id, entity_id, version, payload, attempts) = row?;
            Ok(ResearchOutboxItem {
                id,
                kind,
                company_id,
                entity_id,
                version,
                payload: decode(payload)?,
                attempts,
            })
        })
        .collect()
    }
    /// Called only after the corresponding Mongo write has returned majority ack.
    pub fn research_outbox_delivered(&self, id: &str) -> Result<()> {
        let conn = lock(&self.conn)?;
        let changed = conn.execute("UPDATE research_outbox SET status='delivered',delivered_at=?1 WHERE id=?2 AND status='pending'", params![now(),id])?;
        if changed == 0 {
            let state: Option<String> = conn
                .query_row(
                    "SELECT status FROM research_outbox WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            ensure!(
                state.as_deref() == Some("delivered"),
                "research outbox item not found"
            );
        }
        Ok(())
    }
    pub fn research_outbox_is_pending(&self, id: &str) -> Result<bool> {
        let conn = lock(&self.conn)?;
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM research_outbox WHERE id=?1 AND status='pending')",
            [id],
            |r| r.get(0),
        )?)
    }
    pub fn research_outbox_failed(&self, id: &str) -> Result<()> {
        let conn = lock(&self.conn)?;
        let changed = conn.execute(
            "UPDATE research_outbox SET attempts=attempts+1 WHERE id=?1 AND status='pending'",
            [id],
        )?;
        ensure!(changed == 1, "research outbox item not pending");
        Ok(())
    }
    pub fn research_outbox_counts(&self) -> Result<ResearchOutboxCounts> {
        let conn = lock(&self.conn)?;
        Ok(conn.query_row("SELECT COALESCE(SUM(status='pending'),0),COALESCE(SUM(status='delivered'),0) FROM research_outbox",[],|r|Ok(ResearchOutboxCounts {pending:r.get(0)?,delivered:r.get(1)?}))?)
    }
    /// Explicit idempotent backfill of current records linked to completed or
    /// partial research jobs. This cannot reconstruct deleted prior versions.
    /// It refuses oversized databases rather than silently returning a partial scan.
    pub fn research_outbox_backfill(&self) -> Result<ResearchBackfill> {
        const MAX_RECORDS: usize = 10_000;
        let mut conn = lock(&self.conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let candidates: Vec<String> = {
            let mut stmt=tx.prepare("SELECT DISTINCT c.payload FROM companies c JOIN investigation_candidates x ON x.company_id=c.id JOIN investigations j ON j.id=x.investigation_id WHERE j.status IN ('completed','partial') LIMIT 10001")?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let evidence: Vec<String> = {
            let mut stmt=tx.prepare("SELECT DISTINCT e.payload FROM evidence e JOIN investigation_evidence x ON x.evidence_id=e.id JOIN investigations j ON j.id=x.investigation_id WHERE j.status IN ('completed','partial') LIMIT 10001")?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        ensure!(
            candidates.len() <= MAX_RECORDS && evidence.len() <= MAX_RECORDS,
            "research backfill exceeds bounded scan; pagination required"
        );
        let before: i64 = tx.query_row("SELECT COUNT(*) FROM research_outbox", [], |r| r.get(0))?;
        let mut skipped = 0;
        for encoded in &candidates {
            let c: Company = decode(encoded.clone())?;
            if let Some(payload) = public_company_payload(&c) {
                enqueue_research_outbox(&tx, "company", &c.id, &c.id, payload)?;
            } else {
                skipped += 1;
            }
        }
        for encoded in &evidence {
            let e: Evidence = decode(encoded.clone())?;
            if let Some(payload) = public_claim_payload(&e) {
                enqueue_research_outbox(&tx, "claim", &e.company_id, &e.id, payload)?;
            } else {
                skipped += 1;
            }
        }
        let after: i64 = tx.query_row("SELECT COUNT(*) FROM research_outbox", [], |r| r.get(0))?;
        tx.commit()?;
        Ok(ResearchBackfill {
            scanned: candidates.len() + evidence.len(),
            enqueued: (after - before) as usize,
            skipped,
        })
    }
}

/// Strip URL query/fragment, which can contain a user's search or access token.
/// Reject user-info; the sanitized locator may not reproduce query-based pages.
fn public_locator(raw: &str) -> Option<(String, bool)> {
    let mut url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return None;
    }
    let stripped = url.query().is_some() || url.fragment().is_some();
    url.set_query(None);
    url.set_fragment(None);
    Some((url.to_string(), stripped))
}
fn public_company_payload(company: &Company) -> Option<Value> {
    let (locator, query_removed) = public_locator(company.source_url.as_deref()?)?;
    Some(
        serde_json::json!({"company_id":company.id,"name":company.name,"tax_id":company.tax_id,"ticker":company.ticker,"sector":company.sector,"location":company.location,"source_locator":locator,"source_query_removed":query_removed,"identity_status":"candidate_unverified","provenance_status":"model_suggested_not_independently_verified"}),
    )
}
fn public_claim_payload(e: &Evidence) -> Option<Value> {
    if !matches!(e.status.as_str(), "unverified" | "subject_confirmed") {
        return None;
    }
    let (locator, query_removed) = public_locator(&e.url)?;
    Some(
        serde_json::json!({"claim_id":e.id,"company_id":e.company_id,"title":e.title,"source_locator":locator,"source_query_removed":query_removed,"publisher":e.publisher,"claim":e.claim,"dimension":e.dimension,"status":"unverified","published_at":e.published_at,"observed_at":e.observed_at,"provenance_status":"model_suggested_not_independently_verified"}),
    )
}
fn enqueue_research_outbox(
    tx: &rusqlite::Transaction<'_>,
    kind: &str,
    company_id: &str,
    entity_id: &str,
    payload: Value,
) -> Result<()> {
    let encoded = serde_json::to_string(&payload)?;
    let version = hex::encode(Sha256::digest(encoded.as_bytes()));
    let framed = serde_json::to_vec(&(kind, company_id, entity_id, &version))?;
    let id = hex::encode(Sha256::digest(framed));
    tx.execute("INSERT OR IGNORE INTO research_outbox(id,kind,company_id,entity_id,version,payload,status,attempts,created_at) VALUES(?1,?2,?3,?4,?5,?6,'pending',0,?7)",params![id,kind,company_id,entity_id,version,encoded,now()])?;
    Ok(())
}
fn budget_snapshot(conn: &Connection) -> Result<Budget> {
    let cap: i64 = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key='budget_cap_cents'",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(500);
    let (charged,reserved):(i64,i64)=conn.query_row("SELECT COALESCE(SUM(CASE WHEN status IN ('settled','bounded') THEN actual_cents ELSE 0 END),0),COALESCE(SUM(CASE WHEN status='reserved' THEN reserved_cents ELSE 0 END),0) FROM reservations",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    Ok(Budget {
        cap_cents: cap,
        charged_cents: charged,
        reserved_cents: reserved,
        available_cents: cap.saturating_sub(charged.saturating_add(reserved)).max(0),
    })
}
fn write_company(conn: &Connection, c: &Company) -> Result<()> {
    required(&c.id, "company id")?;
    required(&c.name, "company name")?;
    parse_time(&c.updated_at, false)?;
    let mut canonical = c.clone();
    // Watch state belongs to the user, not an asynchronously retrieved provider result.
    if let Some(watched) = conn
        .query_row("SELECT watched FROM companies WHERE id=?1", [&c.id], |r| {
            r.get::<_, bool>(0)
        })
        .optional()?
    {
        canonical.watched = watched;
    }
    if let Some(tax) = &c.tax_id {
        canonical.tax_id = Some(normalize_tax_id(tax)?);
    }
    if let Some(url) = &c.source_url {
        web_url(url)?;
    }
    if let Some(url) = &c.website {
        web_url(url)?;
    }
    let search = normalize_search(&format!(
        "{} {} {} {} {}",
        c.name,
        canonical.tax_id.as_deref().unwrap_or(""),
        c.ticker.as_deref().unwrap_or(""),
        c.sector.as_deref().unwrap_or(""),
        c.location.as_deref().unwrap_or("")
    ));
    conn.execute("INSERT INTO companies(id,tax_id,search_text,watched,payload) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET tax_id=excluded.tax_id,search_text=excluded.search_text,watched=excluded.watched,payload=excluded.payload",params![c.id,canonical.tax_id,search,canonical.watched,serde_json::to_string(&canonical)?]).context("company save failed; a tax ID may belong to another entity")?;
    Ok(())
}
fn write_evidence(conn: &Connection, e: &Evidence) -> Result<()> {
    required(&e.id, "evidence id")?;
    required(&e.company_id, "company id")?;
    required(&e.title, "title")?;
    required(&e.claim, "claim")?;
    web_url(&e.url)?;
    parse_time(&e.observed_at, false)?;
    if let Some(p) = &e.published_at {
        parse_time(p, false)?;
    }
    conn.execute("INSERT INTO evidence(id,company_id,published_at,observed_at,payload) VALUES(?1,?2,?3,?4,?5)",params![e.id,e.company_id,e.published_at,e.observed_at,serde_json::to_string(e)?])?;
    Ok(())
}
fn valid_transition(from: &str, to: &str) -> bool {
    matches!(
        (from, to),
        ("queued", "running" | "cancelled" | "failed")
            | (
                "running",
                "running" | "verifying" | "partial" | "failed" | "cancelled"
            )
            | (
                "verifying",
                "verifying" | "completed" | "partial" | "failed" | "cancelled"
            )
    )
}
fn insert_event(tx: &rusqlite::Transaction<'_>, e: &Event) -> Result<()> {
    tx.execute(
        "INSERT INTO events(id,investigation_id,created_at,payload) VALUES(?1,?2,?3,?4)",
        params![
            e.id,
            e.investigation_id,
            e.created_at,
            serde_json::to_string(e)?
        ],
    )?;
    Ok(())
}
/// Do not infer a risk ranking when required dimensions or evidence are absent.
pub fn risk_rankable(required_dimensions: &[&str], evidence: &[Evidence]) -> bool {
    !required_dimensions.is_empty()
        && required_dimensions.iter().all(|dim| {
            evidence.iter().any(|e| {
                e.dimension == *dim && e.status == "verified" && !e.claim.trim().is_empty()
            })
        })
}
/// Salted commitment to deterministic JSON. A hash does not authenticate the issuer or assert truth.
pub fn commit_report(report: &Value) -> Result<Commitment> {
    let salt = Uuid::new_v4().simple().to_string();
    Ok(Commitment {
        algorithm: "sha256:lantern-report-v1".into(),
        digest: report_digest(report, &salt)?,
        salt,
    })
}
pub fn verify_report(report: &Value, commitment: &Commitment) -> Result<bool> {
    ensure!(
        commitment.algorithm == "sha256:lantern-report-v1",
        "unsupported commitment algorithm"
    );
    ensure!(
        commitment.salt.len() == 32 && hex::decode(&commitment.salt).is_ok(),
        "invalid commitment salt"
    );
    Ok(report_digest(report, &commitment.salt)? == commitment.digest)
}
fn report_digest(report: &Value, salt: &str) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"lantern-report-v1\0");
    hash.update(hex::decode(salt)?);
    hash.update(serde_json::to_vec(&canonical_value(report))?);
    Ok(hex::encode(hash.finalize()))
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let mut sorted = serde_json::Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonical_value(&map[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn company(id: &str, tax: &str) -> Company {
        Company {
            id: id.into(),
            name: "Công ty Đầu Tư Việt".into(),
            tax_id: Some(tax.into()),
            ticker: None,
            sector: None,
            location: None,
            website: None,
            source_url: None,
            description: None,
            identity_status: "unverified".into(),
            watched: false,
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }
    fn investigation() -> Investigation {
        Investigation {
            id: "i".into(),
            company_id: None,
            query: "kiểm tra".into(),
            as_of: Some("2026-03-05".into()),
            status: "queued".into(),
            stage: "queued".into(),
            summary: String::new(),
            limitations: vec![],
            created_at: "2026-03-01T00:00:00Z".into(),
            updated_at: "2026-03-01T00:00:00Z".into(),
        }
    }
    #[test]
    fn research_outbox_migrates_v1_and_survives_restart() -> Result<()> {
        let path = std::env::temp_dir().join(format!("lantern-outbox-{}.sqlite", Uuid::new_v4()));
        {
            let conn = Connection::open(&path)?;
            conn.execute_batch("CREATE TABLE companies(id TEXT PRIMARY KEY,tax_id TEXT UNIQUE,search_text TEXT NOT NULL,watched INTEGER NOT NULL,payload TEXT NOT NULL); PRAGMA user_version=1;")?;
        }
        let pending_id;
        {
            let store = Store::open(&path)?;
            let mut job = investigation();
            job.query = "private-user-query".into();
            store.create_investigation(&job)?;
            store.transition_investigation("i", "running", "collecting", "start")?;
            store.transition_investigation("i", "verifying", "verify", "checking")?;
            let mut c = company("c", "0123456789");
            c.source_url = Some("https://example.org/profile?token=private-secret".into());
            c.description = Some("private-description".into());
            c.watched = true;
            store.finalize_import("i", &[c], &[], "private-summary", &[], "completed")?;
            let rows = store.research_outbox_pending(10)?;
            assert_eq!(rows.len(), 1);
            let payload = serde_json::to_string(&rows[0].payload)?;
            assert!(!payload.contains("private-"));
            assert_eq!(rows[0].payload["source_query_removed"], true);
            assert_eq!(rows[0].payload["identity_status"], "candidate_unverified");
            pending_id = rows[0].id.clone();
            store.research_outbox_failed(&pending_id)?;
        }
        {
            let store = Store::open(&path)?;
            let rows = store.research_outbox_pending(10)?;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].id, pending_id);
            assert_eq!(rows[0].attempts, 1);
            store.research_outbox_delivered(&pending_id)?;
            store.research_outbox_delivered(&pending_id)?;
            assert_eq!(
                store.research_outbox_counts()?,
                ResearchOutboxCounts {
                    pending: 0,
                    delivered: 1
                }
            );
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
    #[test]
    fn research_outbox_rolls_back_invalid_import_and_cancelled_job() -> Result<()> {
        let store = Store::memory()?;
        store.save_company(&company("c", "0123456789"))?;
        let mut job = investigation();
        job.company_id = Some("c".into());
        store.create_investigation(&job)?;
        store.transition_investigation("i", "running", "collecting", "start")?;
        store.transition_investigation("i", "verifying", "verify", "checking")?;
        let mut c = company("c", "0123456789");
        c.source_url = Some("https://example.org/profile".into());
        let mut bad = evidence("e", None, "2026-01-01");
        bad.status = "unverified".into();
        bad.url = "javascript:alert(1)".into();
        assert!(
            store
                .finalize_import("i", &[c.clone()], &[bad], "", &[], "completed")
                .is_err()
        );
        assert_eq!(store.research_outbox_counts()?.pending, 0);
        store.transition_investigation("i", "cancelled", "cancelled", "user")?;
        assert!(
            store
                .finalize_import("i", &[c], &[], "", &[], "completed")
                .is_err()
        );
        assert_eq!(store.research_outbox_counts()?.pending, 0);
        Ok(())
    }
    #[test]
    fn evidence_mirror_keeps_unverified_claim_and_strips_secret_url_query() -> Result<()> {
        let store = Store::memory()?;
        store.save_company(&company("c", "0123456789"))?;
        let mut job = investigation();
        job.company_id = Some("c".into());
        store.create_investigation(&job)?;
        store.transition_investigation("i", "running", "collecting", "start")?;
        store.transition_investigation("i", "verifying", "verify", "checking")?;
        let mut e = evidence("e", None, "2026-01-01");
        e.status = "unverified".into();
        e.url = "https://example.org/p?credential=hidden".into();
        store.finalize_import("i", &[], &[e], "", &[], "completed")?;
        let pending = store.research_outbox_pending(10)?;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, "claim");
        assert_eq!(pending[0].payload["status"], "unverified");
        assert!(!pending[0].payload.to_string().contains("credential"));
        // Simulate data imported before the outbox migration, then backfill twice.
        lock(&store.conn)?.execute("DELETE FROM research_outbox", [])?;
        let first = store.research_outbox_backfill()?;
        assert_eq!(first.scanned, 1);
        assert_eq!(first.enqueued, 1);
        assert_eq!(first.skipped, 0);
        let again = store.research_outbox_backfill()?;
        assert_eq!(again.scanned, 1);
        assert_eq!(again.enqueued, 0);
        assert_eq!(store.research_outbox_counts()?.pending, 1);
        store.review_evidence("e", "not_about_company")?;
        assert_eq!(store.research_outbox_counts()?.pending, 0);
        let excluded = store.research_outbox_backfill()?;
        assert_eq!(excluded.enqueued, 0);
        assert_eq!(excluded.skipped, 1);
        let mut rejected = evidence("other", None, "2026-01-01");
        rejected.status = "rejected".into();
        assert!(public_claim_payload(&rejected).is_none());
        rejected.status = "unknown_future_status".into();
        assert!(public_claim_payload(&rejected).is_none());
        Ok(())
    }
    fn evidence(id: &str, pub_at: Option<&str>, seen: &str) -> Evidence {
        Evidence {
            id: id.into(),
            company_id: "c".into(),
            title: "Tài liệu".into(),
            url: "https://example.org/document".into(),
            publisher: "Publisher".into(),
            claim: "Thông tin được công bố".into(),
            dimension: "legal".into(),
            status: "verified".into(),
            published_at: pub_at.map(str::to_owned),
            observed_at: seen.into(),
        }
    }
    #[test]
    fn reopen_persists_entities_holds_and_recovery() -> Result<()> {
        let path = std::env::temp_dir().join(format!("lantern-test-{}.sqlite", Uuid::new_v4()));
        {
            let s = Store::open(&path)?;
            s.save_company(&company("c", "0123456789"))?;
            s.create_investigation(&investigation())?;
            s.transition_investigation("i", "running", "lookup", "start")?;
            s.reserve_budget("reasoning", 100)?;
        }
        {
            let s = Store::open(&path)?;
            assert_eq!(s.list_companies("dau tu viet", 20, 0)?.len(), 1);
            assert_eq!(s.recover_interrupted()?, 1);
            assert_eq!(s.get_investigation("i")?.unwrap().status, "partial");
            assert_eq!(s.budget()?.reserved_cents, 100);
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
    #[test]
    fn budget_is_atomic_and_cancel_does_not_release_hold() -> Result<()> {
        let store = Store::memory()?;
        let mut tasks = Vec::new();
        for _ in 0..10 {
            let s = store.clone();
            tasks.push(std::thread::spawn(move || {
                s.reserve_budget("reasoning", 100).is_ok()
            }));
        }
        let count = tasks
            .into_iter()
            .map(|t| t.join().unwrap())
            .filter(|ok| *ok)
            .count();
        assert_eq!(count, 5);
        assert_eq!(store.budget()?.reserved_cents, 500);
        store.create_investigation(&investigation())?;
        store.transition_investigation("i", "cancelled", "cancelled", "user")?;
        assert_eq!(store.budget()?.available_cents, 0);
        Ok(())
    }
    #[test]
    fn conservative_bound_counts_toward_cap_and_is_not_an_invoice() -> Result<()> {
        let s = Store::memory()?;
        let hold = s.reserve_budget("reasoning", 100)?;
        for _ in 0..4 {
            s.reserve_budget("reasoning", 100)?;
        }
        assert!(s.bound_budget(&hold.id, 0).is_err());
        assert!(s.bound_budget(&hold.id, 101).is_err());
        s.bound_budget(&hold.id, 1)?;
        let budget = s.budget()?;
        assert_eq!(
            (
                budget.charged_cents,
                budget.reserved_cents,
                budget.available_cents
            ),
            (1, 400, 99)
        );
        let item = s
            .reservations(20)?
            .into_iter()
            .find(|r| r.id == hold.id)
            .unwrap();
        assert_eq!(item.status, "bounded");
        assert_eq!(item.actual_cents, Some(1));
        assert!(s.bound_budget(&hold.id, 1).is_err());
        assert!(s.settle_budget(&hold.id, 1).is_err());
        assert!(s.release_budget(&hold.id).is_err());
        assert!(s.reserve_budget("reasoning", 100).is_err());
        let audio = s.reserve_budget("audio", 25)?;
        assert!(s.bound_budget(&audio.id, 1).is_err());
        Ok(())
    }
    #[test]
    fn budget_limit_is_durable_and_cannot_undercut_holds() -> Result<()> {
        let path = std::env::temp_dir().join(format!("lantern-budget-{}.sqlite", Uuid::new_v4()));
        {
            let s = Store::open(&path)?;
            assert_eq!(s.budget()?.cap_cents, 500);
            s.set_budget_limit(250)?;
            let hold = s.reserve_budget("reasoning", 100)?;
            assert!(s.set_budget_limit(99).is_err());
            assert!(s.set_budget_limit(0).is_err());
            assert!(s.set_budget_limit(1_000_001).is_err());
            s.settle_budget(&hold.id, 80)?;
            s.reserve_budget("reasoning", 100)?;
            assert!(s.set_budget_limit(179).is_err());
            s.set_budget_limit(180)?;
            assert_eq!(s.budget()?.available_cents, 0);
        }
        {
            let s = Store::open(&path)?;
            assert_eq!(
                s.budget()?,
                Budget {
                    cap_cents: 180,
                    charged_cents: 80,
                    reserved_cents: 100,
                    available_cents: 0
                }
            );
            assert!(s.reserve_budget("audio", 25).is_err());
            s.set_budget_limit(300)?;
            assert_eq!(s.budget()?.available_cents, 120);
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
    #[test]
    fn concurrent_reservations_honor_configured_cap() -> Result<()> {
        let s = Store::memory()?;
        s.set_budget_limit(225)?;
        let mut threads = Vec::new();
        for _ in 0..10 {
            let store = s.clone();
            threads.push(std::thread::spawn(move || {
                store.reserve_budget("audio", 25).is_ok()
            }));
        }
        assert_eq!(
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .filter(|x| *x)
                .count(),
            9
        );
        assert_eq!(s.budget()?.reserved_cents, 225);
        Ok(())
    }
    #[test]
    fn terminal_cancellation_is_guarded_and_recorded() -> Result<()> {
        let s = Store::memory()?;
        s.create_investigation(&investigation())?;
        s.transition_investigation("i", "running", "collecting", "started")?;
        s.transition_investigation("i", "cancelled", "cancelled", "stopped")?;
        assert!(
            s.transition_investigation("i", "completed", "done", "late result")
                .is_err()
        );
        assert_eq!(s.events("i")?.len(), 3);
        assert_eq!(s.get_investigation("i")?.unwrap().status, "cancelled");
        Ok(())
    }
    #[test]
    fn historical_query_distinguishes_unknown_and_future_publication() -> Result<()> {
        let s = Store::memory()?;
        s.save_company(&company("c", "0123456789"))?;
        s.save_evidence(&evidence("before", Some("2026-03-01"), "2026-03-02"))?;
        s.save_evidence(&evidence("unknown", None, "2026-03-02"))?;
        s.save_evidence(&evidence("unknown_observed_late", None, "2026-03-12"))?;
        let mut legacy = evidence("invalid_date", None, "2026-03-02");
        legacy.published_at = Some("invalid".into());
        lock(&s.conn)?.execute("INSERT INTO evidence(id,company_id,published_at,observed_at,payload) VALUES(?1,?2,?3,?4,?5)",params![legacy.id,legacy.company_id,legacy.published_at,legacy.observed_at,serde_json::to_string(&legacy)?])?;
        s.save_evidence(&evidence("late", Some("2026-03-10"), "2026-03-12"))?;
        s.save_evidence(&evidence("observed_late", Some("2026-03-01"), "2026-03-12"))?;
        let known = s.list_evidence("c", Some("2026-03-05"))?;
        assert!(known.iter().any(|e| e.id == "before"));
        assert!(
            known
                .iter()
                .any(|e| e.id == "unknown" && e.published_at.is_none())
        );
        assert!(
            known
                .iter()
                .any(|e| e.id == "invalid_date" && e.published_at.is_none())
        );
        assert!(
            !known.iter().any(|e| e.id == "unknown_observed_late"
                || e.id == "late"
                || e.id == "observed_late")
        );
        let public = s.evidence_at("c", "2026-03-05", TemporalMode::PublicKnowable)?;
        assert!(public.iter().any(|e| e.id == "observed_late"));
        assert!(
            !public
                .iter()
                .any(|e| e.id == "unknown" || e.id == "late" || e.id == "invalid_date")
        );
        assert_eq!(
            s.evidence_at("c", "2026-03-05", TemporalMode::Retrospective)?
                .len(),
            6
        );
        assert!(
            s.save_evidence(&evidence("before", Some("2026-02-01"), "2026-02-01"))
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn date_as_of_ends_at_midnight_vietnam_and_current_allows_undated() -> Result<()> {
        let s = Store::memory()?;
        s.save_company(&company("c", "0123456789"))?;
        s.save_evidence(&evidence(
            "before_midnight",
            Some("2026-03-04"),
            "2026-03-05T16:59:59Z",
        ))?;
        s.save_evidence(&evidence(
            "after_midnight",
            Some("2026-03-04"),
            "2026-03-05T17:00:00Z",
        ))?;
        s.save_evidence(&evidence("undated", None, "2026-03-05T16:00:00Z"))?;
        let historical = s.list_evidence("c", Some("2026-03-05"))?;
        assert_eq!(
            historical.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            vec!["before_midnight", "undated"]
        );
        let current = s.list_evidence("c", None)?;
        assert!(current.iter().any(|e| e.id == "undated"));
        assert_eq!(
            parse_time("2026-03-05T17:00:00Z", true)?,
            DateTime::parse_from_rfc3339("2026-03-05T17:00:00Z")?.with_timezone(&Utc)
        );
        Ok(())
    }
    #[test]
    fn no_ranking_from_missing_data_and_no_identity_merge() -> Result<()> {
        let s = Store::memory()?;
        s.save_company(&company("parent", "0123456789"))?;
        s.save_company(&company("branch", "0123456789001"))?;
        assert_eq!(s.list_companies("cong ty dau tu viet", 20, 0)?.len(), 2);
        assert!(s.save_company(&company("collision", "0123456789")).is_err());
        assert!(!risk_rankable(
            &["legal", "financial"],
            &[evidence("x", Some("2026-01-01"), "2026-01-01")]
        ));
        assert!(!risk_rankable(&[], &[]));
        Ok(())
    }
    #[test]
    fn finalize_cancel_and_invalid_evidence_cannot_partially_import() -> Result<()> {
        let s = Store::memory()?;
        s.save_company(&company("c", "0123456789"))?;
        let mut job = investigation();
        job.company_id = Some("c".into());
        s.create_investigation(&job)?;
        s.transition_investigation("i", "running", "work", "started")?;
        s.transition_investigation("i", "verifying", "verify", "checking")?;
        let mut update = company("c", "0123456789");
        update.name = "Changed".into();
        let mut bad = evidence("new", Some("2026-01-01"), "2026-01-01");
        bad.url = "javascript:alert(1)".into();
        bad.status = "unverified".into();
        assert!(
            s.finalize_import("i", &[update.clone()], &[bad], "summary", &[], "completed")
                .is_err()
        );
        assert_eq!(s.get_company("c")?.unwrap().name, "Công ty Đầu Tư Việt");
        assert_eq!(s.get_investigation("i")?.unwrap().status, "verifying");
        s.transition_investigation("i", "cancelled", "cancelled", "user")?;
        assert!(
            s.finalize_import("i", &[update], &[], "summary", &[], "completed")
                .is_err()
        );
        assert_eq!(s.get_company("c")?.unwrap().name, "Công ty Đầu Tư Việt");
        assert_eq!(s.get_investigation("i")?.unwrap().status, "cancelled");
        Ok(())
    }
    #[test]
    fn job_links_are_isolated_and_review_history_survives_reopen() -> Result<()> {
        let path = std::env::temp_dir().join(format!("lantern-links-{}.sqlite", Uuid::new_v4()));
        {
            let s = Store::open(&path)?;
            s.save_company(&company("c", "0123456789"))?;
            let mut legacy = evidence("legacy", Some("2026-01-01"), "2026-01-01");
            legacy.status = "unverified".into();
            s.save_evidence(&legacy)?;
            let mut lookup = investigation();
            lookup.id = "lookup".into();
            s.create_investigation(&lookup)?;
            s.transition_investigation("lookup", "running", "collecting", "started")?;
            s.transition_investigation("lookup", "verifying", "verify", "checking")?;
            s.finalize_import(
                "lookup",
                &[company("candidate", "1123456789")],
                &[],
                "done",
                &[],
                "completed",
            )?;
            assert_eq!(s.candidate_ids("lookup")?, vec!["candidate"]);
            assert!(s.evidence_for_investigation("lookup")?.is_empty());
            for id in ["a", "b"] {
                let mut j = investigation();
                j.id = id.into();
                j.company_id = Some("c".into());
                s.create_investigation(&j)?;
                s.transition_investigation(id, "running", "collecting", "started")?;
                s.transition_investigation(id, "verifying", "verify", "checking")?;
                let mut e = evidence(&format!("e-{id}"), Some("2026-01-01"), "2026-01-01");
                e.status = "unverified".into();
                s.finalize_import(id, &[], &[e], "done", &[], "completed")?;
            }
            assert_eq!(
                s.evidence_for_investigation("a")?
                    .iter()
                    .map(|e| e.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["e-a"]
            );
            assert_eq!(
                s.evidence_for_investigation("b")?
                    .iter()
                    .map(|e| e.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["e-b"]
            );
            assert!(s.review_evidence("e-a", "verified").is_err());
            s.review_evidence("e-a", "subject_confirmed")?;
            assert_eq!(
                s.evidence_for_investigation("a")?[0].status,
                "subject_confirmed"
            );
            assert_eq!(s.evidence_for_investigation("b")?[0].status, "unverified");
            s.review_evidence("e-a", "not_about_company")?;
        }
        {
            let s = Store::open(&path)?;
            assert_eq!(s.candidate_ids("lookup")?, vec!["candidate"]);
            assert_eq!(
                s.evidence_for_investigation("a")?[0].status,
                "not_about_company"
            );
            assert_eq!(s.evidence_for_investigation("b")?[0].status, "unverified");
            let reviews = s.evidence_review_history("e-a")?;
            assert_eq!(
                reviews
                    .iter()
                    .map(|r| r.decision.as_str())
                    .collect::<Vec<_>>(),
                vec!["subject_confirmed", "not_about_company"]
            );
            assert!(s.evidence_review_history("e-b")?.is_empty());
        }
        std::fs::remove_file(path)?;
        Ok(())
    }
    #[test]
    fn cancellation_and_finalize_are_serialized() -> Result<()> {
        use std::sync::{Arc, Barrier};
        let s = Store::memory()?;
        let job = investigation();
        s.create_investigation(&job)?;
        s.transition_investigation("i", "running", "work", "started")?;
        s.transition_investigation("i", "verifying", "verify", "checking")?;
        let gate = Arc::new(Barrier::new(3));
        let a = s.clone();
        let ga = gate.clone();
        let finalize = std::thread::spawn(move || {
            ga.wait();
            a.finalize_import(
                "i",
                &[company("new", "0123456789")],
                &[],
                "done",
                &[],
                "completed",
            )
            .is_ok()
        });
        let b = s.clone();
        let gb = gate.clone();
        let cancel = std::thread::spawn(move || {
            gb.wait();
            b.transition_investigation("i", "cancelled", "cancelled", "user")
                .is_ok()
        });
        gate.wait();
        let completed = finalize.join().unwrap();
        let cancelled = cancel.join().unwrap();
        assert_ne!(completed, cancelled);
        assert_eq!(s.get_company("new")?.is_some(), completed);
        assert_eq!(
            s.get_investigation("i")?.unwrap().status,
            if completed { "completed" } else { "cancelled" }
        );
        Ok(())
    }
    #[test]
    fn salted_commitment_detects_tampering() -> Result<()> {
        let doc = json!({"name":"Ví dụ","items":[1,2],"complete":false});
        let proof = commit_report(&doc)?;
        assert!(verify_report(&doc, &proof)?);
        assert!(!verify_report(
            &json!({"name":"Ví dụ","items":[1,3],"complete":false}),
            &proof
        )?);
        assert_ne!(proof.digest, commit_report(&doc)?.digest);
        Ok(())
    }
}
