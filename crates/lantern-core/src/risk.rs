//! Evidence-linked, ordinal screening; not a prediction of default or investment advice.
use super::{Company, Evidence, Store, decode, lock, now, parse_time, required};
use anyhow::{Context, Result, ensure};
use chrono::{Duration, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const AREAS: [&str; 4] = ["legal", "tax", "financial", "operations"];
const VERSION: &str = "screening-v1";
/// Maximum explicit comparison cohort. This limit does not imply market-wide coverage.
pub const MAX_RISK_COHORT: usize = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessmentInput {
    pub company_id: String,
    pub evidence_id: String,
    pub dimension: String,
    /// 0 = no concern identified in this evidence, 1 = low, 2 = moderate, 3 = high concern.
    pub severity: u8,
    pub rationale: String,
    pub analyst: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityReviewInput {
    pub company_id: String,
    pub evidence_id: String,
    /// Explicit human decision; false withdraws confirmation.
    pub confirmed: bool,
    pub rationale: String,
    pub analyst: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskJournalEntry {
    pub id: String,
    pub company_id: String,
    pub kind: String,
    pub recorded_at: String,
    pub analyst: String,
    pub rationale: String,
    pub evidence: Option<Evidence>,
    pub evidence_review_id: Option<i64>,
    pub evidence_reviewed_at: Option<String>,
    pub dimension: Option<String>,
    pub severity: Option<u8>,
    pub confirmed: Option<bool>,
    pub target_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDimension {
    pub dimension: String,
    pub severity: Option<u8>,
    pub assessment_ids: Vec<String>,
    pub explanation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskResult {
    pub company_id: String,
    /// Current stored name at calculation time, not a historical name as of the cutoff.
    #[serde(default)]
    pub company_name: String,
    pub identity_confirmed: bool,
    pub identity_review_id: Option<String>,
    pub dimensions: Vec<RiskDimension>,
    pub covered_dimensions: usize,
    pub required_dimensions: usize,
    /// None means abstention; it is never a low-risk score.
    pub score: Option<u8>,
    pub rank: Option<usize>,
    pub reasons: Vec<String>,
    /// Complete immutable source and analyst provenance used in the result.
    pub provenance: Vec<RiskJournalEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskSnapshot {
    pub id: String,
    /// Previews have ephemeral IDs; only saved snapshots can be retrieved later.
    #[serde(default = "saved_by_default")]
    pub saved: bool,
    pub methodology: String,
    pub as_of: String,
    pub created_at: String,
    /// Comparable entries first in ascending severity; abstentions last without ranks.
    pub results: Vec<RiskResult>,
}
fn saved_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskSnapshotSummary {
    pub id: String,
    pub methodology: String,
    pub as_of: String,
    pub created_at: String,
    pub company_count: usize,
    pub scored_count: usize,
}

fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS risk_journal(id TEXT PRIMARY KEY, company_id TEXT NOT NULL REFERENCES companies(id), recorded_at TEXT NOT NULL, payload TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS risk_journal_company ON risk_journal(company_id,recorded_at);
CREATE TABLE IF NOT EXISTS risk_snapshots(id TEXT PRIMARY KEY, created_at TEXT NOT NULL, payload TEXT NOT NULL);")?;
    Ok(())
}
fn entry(conn: &Connection, id: &str) -> Result<RiskJournalEntry> {
    let json: String = conn
        .query_row("SELECT payload FROM risk_journal WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .context("risk journal entry not found")?;
    decode(json)
}
fn append(conn: &Connection, e: &RiskJournalEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO risk_journal(id,company_id,recorded_at,payload) VALUES(?1,?2,?3,?4)",
        params![e.id, e.company_id, e.recorded_at, serde_json::to_string(e)?],
    )?;
    Ok(())
}
/// Find the most recent subject review at the cutoff. Parse dates, not lexical ordering.
fn subject_review(
    conn: &Connection,
    evidence_id: &str,
    cutoff: chrono::DateTime<Utc>,
) -> Result<Option<(i64, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id,decision,reviewed_at FROM evidence_reviews WHERE evidence_id=?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([evidence_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut latest = None;
    for row in rows {
        let (id, decision, at) = row?;
        if parse_time(&at, false)? <= cutoff {
            latest = Some((id, decision, at));
        }
    }
    Ok(latest)
}
fn evidence_for_input(
    conn: &Connection,
    company: &str,
    evidence_id: &str,
    require_confirmed: bool,
) -> Result<(Evidence, i64, String)> {
    let payload: String = conn
        .query_row(
            "SELECT payload FROM evidence WHERE id=?1 AND company_id=?2",
            params![evidence_id, company],
            |r| r.get(0),
        )
        .context("evidence not found for company")?;
    let evidence: Evidence = decode(payload)?;
    let cutoff = Utc::now();
    ensure!(
        parse_time(&evidence.observed_at, true)? <= cutoff,
        "evidence observation is in the future"
    );
    if let Some(p) = &evidence.published_at {
        ensure!(
            parse_time(p, false)? <= cutoff,
            "publication is in the future"
        );
    }
    let (id, decision, at) =
        subject_review(conn, evidence_id, cutoff)?.context("explicit subject review required")?;
    ensure!(
        !require_confirmed || decision == "subject_confirmed",
        "evidence subject must be confirmed"
    );
    Ok((evidence, id, at))
}
fn base(
    company: String,
    kind: &str,
    analyst: String,
    rationale: String,
) -> Result<RiskJournalEntry> {
    required(&company, "company id")?;
    required(&analyst, "analyst")?;
    required(&rationale, "rationale")?;
    ensure!(
        analyst.len() <= 200 && rationale.len() <= 4000,
        "analyst or rationale too long"
    );
    Ok(RiskJournalEntry {
        id: Uuid::new_v4().to_string(),
        company_id: company,
        kind: kind.into(),
        recorded_at: now(),
        analyst,
        rationale,
        evidence: None,
        evidence_review_id: None,
        evidence_reviewed_at: None,
        dimension: None,
        severity: None,
        confirmed: None,
        target_id: None,
    })
}
impl Store {
    pub fn risk_assess(&self, input: RiskAssessmentInput) -> Result<RiskJournalEntry> {
        ensure!(
            AREAS.contains(&input.dimension.as_str()),
            "unknown risk dimension"
        );
        ensure!(input.severity <= 3, "severity must be between 0 and 3");
        let mut conn = lock(&self.conn)?;
        initialize(&conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (evidence, review_id, reviewed_at) =
            evidence_for_input(&tx, &input.company_id, &input.evidence_id, true)?;
        ensure!(
            evidence.dimension == input.dimension,
            "assessment dimension must match evidence dimension"
        );
        let mut e = base(
            input.company_id,
            "assessment",
            input.analyst,
            input.rationale,
        )?;
        e.evidence = Some(evidence);
        e.evidence_review_id = Some(review_id);
        e.evidence_reviewed_at = Some(reviewed_at);
        e.dimension = Some(input.dimension);
        e.severity = Some(input.severity);
        append(&tx, &e)?;
        tx.commit()?;
        Ok(e)
    }
    pub fn risk_review_identity(&self, input: IdentityReviewInput) -> Result<RiskJournalEntry> {
        let mut conn = lock(&self.conn)?;
        initialize(&conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (evidence, review_id, reviewed_at) =
            evidence_for_input(&tx, &input.company_id, &input.evidence_id, input.confirmed)?;
        let mut e = base(
            input.company_id,
            "identity_review",
            input.analyst,
            input.rationale,
        )?;
        e.evidence = Some(evidence);
        e.evidence_review_id = Some(review_id);
        e.evidence_reviewed_at = Some(reviewed_at);
        e.confirmed = Some(input.confirmed);
        append(&tx, &e)?;
        tx.commit()?;
        Ok(e)
    }
    pub fn risk_retract_assessment(
        &self,
        id: &str,
        analyst: &str,
        reason: &str,
    ) -> Result<RiskJournalEntry> {
        let mut conn = lock(&self.conn)?;
        initialize(&conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let target = entry(&tx, id)?;
        ensure!(
            target.kind == "assessment",
            "only assessments may be retracted; use identity review for identity"
        );
        let mut e = base(
            target.company_id,
            "retraction",
            analyst.into(),
            reason.into(),
        )?;
        e.target_id = Some(id.into());
        append(&tx, &e)?;
        tx.commit()?;
        Ok(e)
    }
    pub fn risk_journal(&self, company_id: &str) -> Result<Vec<RiskJournalEntry>> {
        let conn = lock(&self.conn)?;
        initialize(&conn)?;
        let mut stmt =
            conn.prepare("SELECT payload FROM risk_journal WHERE company_id=?1 ORDER BY rowid")?;
        let rows = stmt.query_map([company_id], |r| r.get::<_, String>(0))?;
        rows.map(|r| decode(r?)).collect()
    }
    pub fn risk_get_snapshot(&self, id: &str) -> Result<Option<RiskSnapshot>> {
        let conn = lock(&self.conn)?;
        initialize(&conn)?;
        let value: Option<String> = conn
            .query_row(
                "SELECT payload FROM risk_snapshots WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        value.map(decode).transpose()
    }
    /// Return metadata for at most 30 saved snapshots, newest first.
    pub fn risk_list_snapshots(&self, limit: u32) -> Result<Vec<RiskSnapshotSummary>> {
        let conn = lock(&self.conn)?;
        initialize(&conn)?;
        let mut stmt =
            conn.prepare("SELECT payload FROM risk_snapshots ORDER BY rowid DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit.clamp(1, 30)], |r| r.get::<_, String>(0))?;
        rows.map(|row| {
            let snapshot: RiskSnapshot = decode(row?)?;
            Ok(RiskSnapshotSummary {
                id: snapshot.id,
                methodology: snapshot.methodology,
                as_of: snapshot.as_of,
                created_at: snapshot.created_at,
                company_count: snapshot.results.len(),
                scored_count: snapshot
                    .results
                    .iter()
                    .filter(|r| r.score.is_some())
                    .count(),
            })
        })
        .collect()
    }
    /// Evaluate and rank an explicit cohort of up to `MAX_RISK_COHORT` companies without
    /// saving the result. The preview ID is ephemeral; paginate only after this calculation.
    pub fn risk_preview(&self, company_ids: &[String], as_of: &str) -> Result<RiskSnapshot> {
        self.calculate_risk(company_ids, as_of, false)
    }
    /// Evaluate and save the entire explicitly supplied comparison cohort.
    pub fn risk_snapshot(&self, company_ids: &[String], as_of: &str) -> Result<RiskSnapshot> {
        self.calculate_risk(company_ids, as_of, true)
    }
    fn calculate_risk(
        &self,
        company_ids: &[String],
        as_of: &str,
        persist: bool,
    ) -> Result<RiskSnapshot> {
        ensure!(
            !company_ids.is_empty() && company_ids.len() <= MAX_RISK_COHORT,
            "provide 1 to {MAX_RISK_COHORT} companies"
        );
        let unique: HashSet<&String> = company_ids.iter().collect();
        ensure!(unique.len() == company_ids.len(), "duplicate company id");
        let requested = parse_time(as_of, true)?;
        let current = Utc::now();
        // Date-only today means end-of-day, clamped to the actual instant of calculation.
        ensure!(
            requested <= current || (as_of.len() == 10 && parse_time(as_of, false)? <= current),
            "future cutoff is not allowed"
        );
        let cutoff = requested.min(current);
        let mut conn = lock(&self.conn)?;
        initialize(&conn)?;
        let tx = conn.transaction_with_behavior(if persist {
            TransactionBehavior::Immediate
        } else {
            TransactionBehavior::Deferred
        })?;
        let mut results = Vec::new();
        for company in company_ids {
            let company_json: Option<String> = tx
                .query_row(
                    "SELECT payload FROM companies WHERE id=?1",
                    [company],
                    |r| r.get(0),
                )
                .optional()?;
            let current_company: Company =
                decode(company_json.with_context(|| format!("company not found: {company}"))?)?;
            let mut stmt =
                tx.prepare("SELECT payload FROM risk_journal WHERE company_id=?1 ORDER BY rowid")?;
            let rows = stmt.query_map([company], |r| r.get::<_, String>(0))?;
            let mut events: Vec<RiskJournalEntry> = Vec::new();
            for row in rows {
                let event: RiskJournalEntry = decode(row?)?;
                if parse_time(&event.recorded_at, false)? <= cutoff {
                    events.push(event);
                }
            }
            drop(stmt);
            let mut retracted = HashSet::new();
            for e in &events {
                if e.kind == "retraction"
                    && let Some(id) = &e.target_id
                {
                    retracted.insert(id.clone());
                }
            }
            let valid = |e: &RiskJournalEntry| -> Result<bool> {
                let Some(source) = &e.evidence else {
                    return Ok(false);
                };
                let Some(published) = &source.published_at else {
                    return Ok(false);
                };
                let published = match parse_time(published, true) {
                    Ok(v) => v,
                    Err(_) => return Ok(false),
                };
                let observed = match parse_time(&source.observed_at, true) {
                    Ok(v) => v,
                    Err(_) => return Ok(false),
                };
                if published > cutoff
                    || observed > cutoff
                    || cutoff.signed_duration_since(published) > Duration::days(365)
                    || cutoff.signed_duration_since(observed) > Duration::days(365)
                {
                    return Ok(false);
                }
                let latest = subject_review(&tx, &source.id, cutoff)?;
                Ok(latest.is_some_and(|(id, decision, _)| {
                    decision == "subject_confirmed" && Some(id) == e.evidence_review_id
                }))
            };
            // A later invalid identity decision is still a withdrawal, never fall back to older confirmation.
            let identity = events.iter().rev().find(|e| e.kind == "identity_review");
            let identity_confirmed = if let Some(e) = identity {
                e.confirmed == Some(true) && valid(e)?
            } else {
                false
            };
            let mut provenance = Vec::new();
            if let Some(e) = identity {
                provenance.push(e.clone());
            }
            let mut dimensions = Vec::new();
            let mut max_by_area: HashMap<&str, u8> = HashMap::new();
            for area in AREAS {
                let mut used = Vec::new();
                let mut maximum = None;
                for e in &events {
                    if e.kind == "assessment"
                        && e.dimension.as_deref() == Some(area)
                        && !retracted.contains(&e.id)
                        && valid(e)?
                    {
                        let severity = e.severity.context("corrupt severity")?;
                        maximum = Some(maximum.map_or(severity, |m: u8| m.max(severity)));
                        used.push(e.id.clone());
                        provenance.push(e.clone());
                    }
                }
                if let Some(v) = maximum {
                    max_by_area.insert(area, v);
                }
                dimensions.push(RiskDimension {
                    dimension: area.into(),
                    severity: maximum,
                    assessment_ids: used,
                    explanation: if maximum.is_some() {
                        "Highest active analyst-assessed concern from eligible evidence.".into()
                    } else {
                        "No current eligible assessment; unknown, not zero.".into()
                    },
                });
            }
            let covered = max_by_area.len();
            let mut reasons = Vec::new();
            if !identity_confirmed {
                reasons.push("Identity lacks a current explicit confirmation supported by eligible reviewed evidence. Date-only publication today is eligible after the day ends in Vietnam.".into());
            }
            for d in &dimensions {
                if d.severity.is_none() {
                    reasons.push(format!(
                        "{}: missing, undated, stale, rejected, re-reviewed, or retracted assessment; a date-only publication today is eligible after the day ends in Vietnam",
                        d.dimension
                    ));
                }
            }
            let score = if identity_confirmed && covered == AREAS.len() {
                max_by_area.values().copied().max()
            } else {
                None
            };
            if let Some(s) = score {
                reasons.push(format!("Ordinal concern {s}/3 is the maximum across the four assessed areas; identity is a gate."));
            }
            results.push(RiskResult {
                company_id: company.clone(),
                company_name: current_company.name,
                identity_confirmed,
                identity_review_id: identity.map(|e| e.id.clone()),
                dimensions,
                covered_dimensions: covered,
                required_dimensions: AREAS.len(),
                score,
                rank: None,
                reasons,
                provenance,
            });
        }
        results.sort_by(|a, b| {
            match (a.score, b.score) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then_with(|| a.company_id.cmp(&b.company_id))
        });
        let mut last = None;
        let mut rank = 0;
        for (index, item) in results.iter_mut().enumerate() {
            if let Some(s) = item.score {
                if last != Some(s) {
                    rank = index + 1;
                    last = Some(s);
                }
                item.rank = Some(rank);
            }
        }
        let snapshot = RiskSnapshot {
            id: Uuid::new_v4().to_string(),
            saved: persist,
            methodology: VERSION.into(),
            as_of: cutoff.to_rfc3339(),
            created_at: now(),
            results,
        };
        if persist {
            tx.execute(
                "INSERT INTO risk_snapshots(id,created_at,payload) VALUES(?1,?2,?3)",
                params![
                    snapshot.id,
                    snapshot.created_at,
                    serde_json::to_string(&snapshot)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Company;

    fn company(id: &str) -> Company {
        Company {
            id: id.into(),
            name: format!("Company {id}"),
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
        }
    }
    fn add(store: &Store, company: &str, area: &str, suffix: &str, age: i64) -> Result<String> {
        let id = format!("{company}-{area}-{suffix}");
        let date = (Utc::now() - Duration::days(age)).to_rfc3339();
        store.save_evidence(&Evidence {
            id: id.clone(),
            company_id: company.into(),
            title: "Source".into(),
            url: "https://example.org/source".into(),
            publisher: "Publisher".into(),
            claim: "Observation".into(),
            dimension: area.into(),
            status: "unverified".into(),
            published_at: Some(date.clone()),
            observed_at: date,
        })?;
        Ok(id)
    }
    fn confirm(store: &Store, id: &str) -> Result<()> {
        store.review_evidence(id, "subject_confirmed")
    }
    fn assess(
        store: &Store,
        company: &str,
        id: &str,
        area: &str,
        severity: u8,
    ) -> Result<RiskJournalEntry> {
        store.risk_assess(RiskAssessmentInput {
            company_id: company.into(),
            evidence_id: id.into(),
            dimension: area.into(),
            severity,
            rationale: "Analyst checked source".into(),
            analyst: "analyst".into(),
        })
    }
    fn complete(store: &Store, company_id: &str, severity: u8) -> Result<Vec<String>> {
        store.save_company(&company(company_id))?;
        let mut ids = Vec::new();
        for area in AREAS {
            let id = add(store, company_id, area, "1", 10)?;
            confirm(store, &id)?;
            assess(store, company_id, &id, area, severity)?;
            ids.push(id);
        }
        store.risk_review_identity(IdentityReviewInput {
            company_id: company_id.into(),
            evidence_id: ids[0].clone(),
            confirmed: true,
            rationale: "Registration identifiers checked against source".into(),
            analyst: "analyst".into(),
        })?;
        Ok(ids)
    }
    fn current() -> String {
        Utc::now().to_rfc3339()
    }
    #[test]
    fn tied_complete_companies_rank_before_missing_and_snapshot_is_immutable() -> Result<()> {
        let s = Store::memory()?;
        let ids = complete(&s, "a", 2)?;
        complete(&s, "b", 2)?;
        complete(&s, "higher", 3)?;
        s.save_company(&company("missing"))?;
        let cohort = vec!["missing".into(), "higher".into(), "b".into(), "a".into()];
        let preview = s.risk_preview(&cohort, &current())?;
        assert!(!preview.saved);
        assert!(s.risk_get_snapshot(&preview.id)?.is_none());
        let snap = s.risk_snapshot(&cohort, &current())?;
        assert!(snap.saved);
        assert_eq!(s.risk_list_snapshots(30)?.len(), 1);
        assert_eq!(snap.results[0].company_name, "Company a");
        let mut renamed = company("a");
        renamed.name = "Changed name".into();
        s.save_company(&renamed)?;
        assert_eq!(
            s.risk_get_snapshot(&snap.id)?.unwrap().results[0].company_name,
            "Company a"
        );
        assert_eq!(
            s.risk_preview(&["a".into()], &current())?.results[0].company_name,
            "Changed name"
        );
        assert_eq!(
            snap.results
                .iter()
                .map(|x| (&x.company_id, x.score, x.rank))
                .collect::<Vec<_>>(),
            vec![
                (&"a".to_string(), Some(2), Some(1)),
                (&"b".to_string(), Some(2), Some(1)),
                (&"higher".to_string(), Some(3), Some(3)),
                (&"missing".to_string(), None, None)
            ]
        );
        s.review_evidence(&ids[0], "not_about_company")?;
        let after = s.risk_preview(&cohort, &current())?;
        let a = after.results.iter().find(|r| r.company_id == "a").unwrap();
        assert_eq!(a.score, None);
        assert!(!a.identity_confirmed);
        s.risk_review_identity(IdentityReviewInput {
            company_id: "a".into(),
            evidence_id: ids[0].clone(),
            confirmed: false,
            rationale: "Source was found to concern another subject".into(),
            analyst: "analyst".into(),
        })?;
        assert_eq!(
            s.risk_get_snapshot(&snap.id)?.unwrap().results[0].score,
            Some(2)
        );
        // A new subject confirmation does not automatically revive old assessments.
        confirm(&s, &ids[0])?;
        assert_eq!(
            s.risk_preview(&cohort, &current())?
                .results
                .iter()
                .find(|r| r.company_id == "a")
                .unwrap()
                .score,
            None
        );
        Ok(())
    }
    #[test]
    fn unreviewed_wrong_subject_stale_and_retracted_are_ineligible() -> Result<()> {
        let s = Store::memory()?;
        let ids = complete(&s, "a", 1)?;
        s.save_company(&company("other"))?;
        let unreviewed = add(&s, "a", "tax", "unreviewed", 10)?;
        assert!(assess(&s, "a", &unreviewed, "tax", 3).is_err());
        assert!(assess(&s, "other", &ids[1], "tax", 3).is_err());
        assert!(assess(&s, "a", &ids[1], "legal", 3).is_err());
        assert!(assess(&s, "a", &ids[1], "tax", 4).is_err());
        let stale = add(&s, "a", "tax", "stale", 400)?;
        confirm(&s, &stale)?;
        assess(&s, "a", &stale, "tax", 3)?;
        assert_eq!(
            s.risk_preview(&["a".into()], &current())?.results[0].score,
            Some(1)
        );
        let original = s
            .risk_journal("a")?
            .into_iter()
            .find(|e| {
                e.kind == "assessment"
                    && e.dimension.as_deref() == Some("tax")
                    && e.evidence.as_ref().unwrap().id == ids[1]
            })
            .unwrap();
        s.risk_retract_assessment(&original.id, "analyst", "withdrawn")?;
        let result = &s.risk_preview(&["a".into()], &current())?.results[0];
        assert_eq!(result.score, None);
        assert_eq!(result.covered_dimensions, 3);
        Ok(())
    }
    #[test]
    fn future_review_or_assessment_cannot_leak_into_historical_screen() -> Result<()> {
        let s = Store::memory()?;
        complete(&s, "a", 3)?;
        let past = (Utc::now() - Duration::minutes(5)).to_rfc3339();
        let result = &s.risk_preview(&["a".into()], &past)?.results[0];
        assert_eq!(result.score, None);
        assert_eq!(result.covered_dimensions, 0);
        assert!(!result.identity_confirmed);
        assert!(
            s.risk_preview(
                &["a".into()],
                &(Utc::now() + Duration::days(1)).to_rfc3339()
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn same_day_publication_can_be_assessed_but_cannot_score_early() -> Result<()> {
        let s = Store::memory()?;
        s.save_company(&company("a"))?;
        let evidence = Evidence {
            id: "today".into(),
            company_id: "a".into(),
            title: "Source".into(),
            url: "https://example.org/source".into(),
            publisher: "Publisher".into(),
            claim: "Observation".into(),
            dimension: "legal".into(),
            status: "unverified".into(),
            published_at: Some(
                (Utc::now() + Duration::hours(7))
                    .format("%Y-%m-%d")
                    .to_string(),
            ),
            observed_at: (Utc::now() - Duration::minutes(1)).to_rfc3339(),
        };
        s.save_evidence(&evidence)?;
        confirm(&s, "today")?;
        assess(&s, "a", "today", "legal", 2)?;
        let result = &s.risk_preview(&["a".into()], &current())?.results[0];
        assert_eq!(result.covered_dimensions, 0);
        assert_eq!(result.score, None);
        Ok(())
    }
    #[test]
    fn snapshot_history_is_bounded_and_excludes_previews() -> Result<()> {
        let s = Store::memory()?;
        s.save_company(&company("a"))?;
        let cohort = ["a".into()];
        let first = s.risk_snapshot(&cohort, &current())?;
        for _ in 0..30 {
            s.risk_snapshot(&cohort, &current())?;
        }
        let preview = s.risk_preview(&cohort, &current())?;
        let history = s.risk_list_snapshots(100)?;
        assert_eq!(history.len(), 30);
        assert!(
            !history
                .iter()
                .any(|item| item.id == first.id || item.id == preview.id)
        );
        assert_eq!(history[0].company_count, 1);
        assert_eq!(history[0].scored_count, 0);
        Ok(())
    }
    #[test]
    fn whole_large_cohort_is_ranked_before_pagination() -> Result<()> {
        let s = Store::memory()?;
        complete(&s, "a-high", 3)?;
        complete(&s, "z-low", 1)?;
        complete(&s, "zz-low", 1)?;
        let mut cohort = vec!["a-high".to_string()];
        for n in 0..205 {
            let id = format!("m-{n:03}");
            s.save_company(&company(&id))?;
            cohort.push(id);
        }
        cohort.extend(["z-low".into(), "zz-low".into()]);
        let preview = s.risk_preview(&cohort, &current())?;
        assert_eq!(preview.results.len(), 208);
        assert_eq!(
            (
                &*preview.results[0].company_id,
                preview.results[0].score,
                preview.results[0].rank
            ),
            ("z-low", Some(1), Some(1))
        );
        assert_eq!(
            (
                &*preview.results[1].company_id,
                preview.results[1].score,
                preview.results[1].rank
            ),
            ("zz-low", Some(1), Some(1))
        );
        assert_eq!(
            (
                &*preview.results[2].company_id,
                preview.results[2].score,
                preview.results[2].rank
            ),
            ("a-high", Some(3), Some(3))
        );
        assert!(
            preview.results[3..]
                .iter()
                .all(|r| r.score.is_none() && r.rank.is_none())
        );
        for n in cohort.len()..MAX_RISK_COHORT {
            let id = format!("extra-{n:04}");
            s.save_company(&company(&id))?;
            cohort.push(id);
        }
        assert_eq!(
            s.risk_preview(&cohort, &current())?.results.len(),
            MAX_RISK_COHORT
        );
        let too_many: Vec<String> = (0..=MAX_RISK_COHORT).map(|n| format!("id-{n}")).collect();
        assert!(s.risk_preview(&too_many, &current()).is_err());
        assert!(s.risk_snapshot(&too_many, &current()).is_err());
        Ok(())
    }
}
