//! Opt-in, bounded source polling. No source is enabled by default.
//! The injected store must implement durable leases, fencing, host-wide quotas and idempotent
//! atomic commit; this crate cannot provide those guarantees without a correct store adapter.
//! Fetched bytes are transient and never serialized/logged here. No entity matching is performed.
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Duration, Utc};
use reqwest::{StatusCode, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration as StdDuration,
};
use url::{Host, Url};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourcePolicy {
    pub source_id: String,
    pub source_url: String,
    pub enabled: bool,
    /// Operator attestation, not a legal conclusion or a license grant.
    pub access_authorized: bool,
    pub robots_and_terms_checked: bool,
    pub allowed_hosts: Vec<String>,
    pub min_interval_seconds: u64,
    /// Persistent cross-worker request ceiling per origin, enforced by the store adapter.
    pub max_requests_per_day: u32,
    pub timeout_seconds: u64,
    pub max_bytes: usize,
    pub max_attempts: u32,
    /// Exact lower-case MIME types; no inferred MIME or executable content.
    pub allowed_content_types: Vec<String>,
    /// Whether source terms and privacy policy permit returning bytes for processing.
    pub process_content_authorized: bool,
}
impl Default for SourcePolicy {
    fn default() -> Self {
        Self {
            source_id: String::new(),
            source_url: String::new(),
            enabled: false,
            access_authorized: false,
            robots_and_terms_checked: false,
            allowed_hosts: Vec::new(),
            min_interval_seconds: 3600,
            max_requests_per_day: 100,
            timeout_seconds: 15,
            max_bytes: 1024 * 1024,
            max_attempts: 3,
            allowed_content_types: vec![
                "text/html".into(),
                "text/plain".into(),
                "application/json".into(),
                "application/pdf".into(),
            ],
            process_content_authorized: false,
        }
    }
}
impl SourcePolicy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.enabled && self.access_authorized && self.robots_and_terms_checked,
            "source is not explicitly authorized and enabled"
        );
        ensure!(
            !self.source_id.trim().is_empty() && self.source_id.len() <= 128,
            "invalid source id"
        );
        ensure!(
            (300..=2_592_000).contains(&self.min_interval_seconds),
            "source interval must be 300 seconds to 30 days"
        );
        ensure!(
            (1..=1000).contains(&self.max_requests_per_day),
            "daily origin cap must be 1..1000"
        );
        ensure!(
            (1..=60).contains(&self.timeout_seconds),
            "source timeout must be 1..60 seconds"
        );
        ensure!(
            (1..=5 * 1024 * 1024).contains(&self.max_bytes),
            "invalid source byte limit"
        );
        ensure!(
            (1..=5).contains(&self.max_attempts),
            "max attempts must be 1..5"
        );
        ensure!(
            !self.allowed_hosts.is_empty() && self.allowed_hosts.len() <= 16,
            "host allowlist required"
        );
        ensure!(
            !self.allowed_content_types.is_empty() && self.allowed_content_types.len() <= 16,
            "content-type allowlist required"
        );
        for host in &self.allowed_hosts {
            ensure!(
                !host.is_empty()
                    && host.len() <= 253
                    && host == &host.to_ascii_lowercase()
                    && !host.contains('*')
                    && !host.ends_with('.')
                    && host.parse::<IpAddr>().is_err(),
                "invalid host allowlist entry"
            );
        }
        check_url(self, &self.source_url)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lease {
    pub job_id: String,
    pub fencing_token: String,
    pub policy: SourcePolicy,
    pub attempts: u32,
    pub previous_etag: Option<String>,
    pub previous_last_modified: Option<String>,
    pub previous_hash: Option<String>,
    pub not_before: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    FirstSeen,
    Changed,
    Unchanged,
    Missing,
    Blocked,
    Retry,
    Exhausted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FetchRecord {
    pub source_id: String,
    pub job_id: String,
    pub observed_at: DateTime<Utc>,
    pub change: Change,
    pub status_code: Option<u16>,
    pub content_type: Option<String>,
    pub content_hash: Option<String>,
    pub previous_hash: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub next_due: Option<DateTime<Utc>>,
    pub attempts: u32,
    /// Categorical result only; no URL, content, query, credentials or PII.
    pub reason: String,
}
/// The store must fence stale workers and atomically commit record + schedule/checkpoint. It must
/// enforce per-host concurrency, minimum interval and daily caps across *all* workers/clusters.
/// Metadata records do not include content. For authorized content, commit must atomically record
/// the version and durable downstream work before advancing validators; raw retention needs its
/// own legal permission. No generated claim is authoritative or entity-linked.
#[allow(async_fn_in_trait)]
pub trait ScoutStore: Send + Sync {
    async fn claim_due(&self, now: DateTime<Utc>) -> Result<Option<Lease>>;
    async fn commit(
        &self,
        lease: &Lease,
        record: &FetchRecord,
        content: Option<&[u8]>,
    ) -> Result<()>;
}
/// Bytes are transient, are not Debug/Serialize, and may contain sensitive data. Callers must
/// obey the source policy before processing/storing, and must not print or send them to a model.
pub struct Observation {
    pub record: FetchRecord,
    pub content: Option<Vec<u8>>,
}

/// One scheduled iteration. Drive this independently of chat in an owned, cancellable supervisor.
/// A claim/commit error propagates; a correctly implemented store must leave its lease recoverable.
pub async fn poll_once<S: ScoutStore>(store: &S) -> Result<Option<Observation>> {
    let Some(lease) = store.claim_due(Utc::now()).await? else {
        return Ok(None);
    };
    let observation = fetch(&lease).await;
    store
        .commit(&lease, &observation.record, observation.content.as_deref())
        .await?;
    Ok(Some(observation))
}
/// Validate a public URL. Direct IP URLs, userinfo, non-HTTPS and non-443 ports are forbidden.
pub fn check_url(policy: &SourcePolicy, raw: &str) -> Result<Url> {
    ensure!(raw.len() <= 2048, "URL too long");
    let url = Url::parse(raw).context("invalid source URL")?;
    ensure!(url.fragment().is_none(), "URL fragments are not supported");
    ensure!(
        !url.query_pairs().any(|(k, _)| matches!(
            k.to_ascii_lowercase().as_str(),
            "token"
                | "access_token"
                | "api_key"
                | "apikey"
                | "secret"
                | "password"
                | "authorization"
        )),
        "credentials in URLs are forbidden"
    );
    ensure!(url.scheme() == "https", "only HTTPS sources are permitted");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "userinfo is forbidden"
    );
    ensure!(
        url.port_or_known_default() == Some(443),
        "only HTTPS port 443 is allowed"
    );
    let host = match url.host() {
        Some(Host::Domain(s)) => s,
        _ => bail!("IP literals are forbidden"),
    };
    ensure!(
        policy.allowed_hosts.iter().any(|h| h == host),
        "host is not explicitly allowed"
    );
    Ok(url)
}
/// Public address filter. This is deliberately conservative, including IPv4 mapped/transition
/// IPv6 and documentation ranges. DNS answers are validated before being pinned into reqwest.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => is_public_v4(v),
        IpAddr::V6(v) => {
            let s = v.segments();
            (s[0] & 0xe000) == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] == 0 || s[1] == 0x0db8 || s[1] == 2))
                && v.to_ipv4().is_none()
        }
    }
}
fn is_public_v4(v: Ipv4Addr) -> bool {
    let [a, b, c, _] = v.octets();
    !v.is_private()
        && !v.is_loopback()
        && !v.is_link_local()
        && !v.is_multicast()
        && !v.is_unspecified()
        && a != 0
        && a < 224
        && !(a == 100 && (64..=127).contains(&b))
        && !(a == 192 && b == 0 && (c == 0 || c == 2))
        && !(a == 192 && b == 88 && c == 99)
        && !(a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
        && !(a == 203 && b == 0 && c == 113)
}
fn next_date(now: DateTime<Utc>, seconds: u64) -> DateTime<Utc> {
    now.checked_add_signed(Duration::seconds(seconds.min(31_536_000) as i64))
        .unwrap_or(now)
}
fn record(
    lease: &Lease,
    change: Change,
    code: Option<u16>,
    reason: &str,
    due: Option<DateTime<Utc>>,
) -> FetchRecord {
    FetchRecord {
        source_id: lease.policy.source_id.clone(),
        job_id: lease.job_id.clone(),
        observed_at: Utc::now(),
        change,
        status_code: code,
        content_type: None,
        content_hash: None,
        previous_hash: lease.previous_hash.clone(),
        etag: None,
        last_modified: None,
        next_due: due,
        attempts: lease.attempts.saturating_add(1),
        reason: reason.into(),
    }
}
fn retry(lease: &Lease, code: Option<u16>, reason: &str, minimum_seconds: u64) -> Observation {
    let attempts = lease.attempts.saturating_add(1);
    let exhausted = attempts >= lease.policy.max_attempts;
    let backoff = 30_u64.saturating_mul(1_u64 << attempts.min(10));
    let jitter =
        u64::from(Sha256::digest(format!("{}:{}", lease.job_id, attempts).as_bytes())[0]) % 31;
    let delay = backoff
        .max(minimum_seconds)
        .max(lease.policy.min_interval_seconds)
        .saturating_add(jitter);
    Observation {
        record: record(
            lease,
            if exhausted {
                Change::Exhausted
            } else {
                Change::Retry
            },
            code,
            reason,
            if exhausted {
                None
            } else {
                Some(next_date(Utc::now(), delay))
            },
        ),
        content: None,
    }
}
fn retry_after(value: Option<&header::HeaderValue>) -> Option<u64> {
    let text = value?.to_str().ok()?.trim();
    if let Ok(s) = text.parse::<u64>() {
        return Some(s);
    }
    let t = DateTime::parse_from_rfc2822(text).ok()?.with_timezone(&Utc);
    Some((t - Utc::now()).num_seconds().saturating_add(1).max(0) as u64)
}
fn clean_validator(v: Option<&header::HeaderValue>) -> Option<String> {
    let s = v?.to_str().ok()?;
    if s.len() > 512 || s.chars().any(char::is_control) {
        None
    } else {
        Some(s.to_owned())
    }
}
/// Fetch a single leased source; returns only categorical errors, never remote exception text.
pub async fn fetch(lease: &Lease) -> Observation {
    if lease.policy.validate().is_err() {
        return Observation {
            record: record(lease, Change::Blocked, None, "policy_not_authorized", None),
            content: None,
        };
    }
    if lease.not_before > Utc::now() || lease.expires_at <= Utc::now() {
        return Observation {
            record: record(
                lease,
                Change::Blocked,
                None,
                "invalid_or_not_due_lease",
                None,
            ),
            content: None,
        };
    }
    if lease.attempts >= lease.policy.max_attempts {
        return Observation {
            record: record(lease, Change::Exhausted, None, "retry_limit", None),
            content: None,
        };
    }
    match tokio::time::timeout(
        StdDuration::from_secs(lease.policy.timeout_seconds),
        fetch_inner(lease, None),
    )
    .await
    {
        Ok(v) => v,
        Err(_) => retry(lease, None, "timeout", 0),
    }
}
/// `fixture` is only passed by unit tests: DNS/HTTP override is compiled out of production.
async fn fetch_inner(lease: &Lease, fixture: Option<SocketAddr>) -> Observation {
    let mut url = match check_url(&lease.policy, &lease.policy.source_url) {
        Ok(u) => u,
        Err(_) => {
            return Observation {
                record: record(lease, Change::Blocked, None, "invalid_url", None),
                content: None,
            };
        }
    };
    let mut conditional = true;
    for redirects in 0..=3 {
        let host = match url.host_str() {
            Some(h) => h.to_owned(),
            None => return retry(lease, None, "invalid_host", 0),
        };
        let addrs: Vec<SocketAddr> = if let Some(addr) = fixture {
            #[cfg(test)]
            {
                vec![addr]
            }
            #[cfg(not(test))]
            {
                let _ = addr;
                return Observation {
                    record: record(lease, Change::Blocked, None, "fixture_forbidden", None),
                    content: None,
                };
            }
        } else {
            match tokio::net::lookup_host((host.as_str(), 443)).await {
                Ok(a) => a.collect(),
                Err(_) => return retry(lease, None, "dns_failure", 0),
            }
        };
        if addrs.is_empty() || (fixture.is_none() && addrs.iter().any(|a| !is_public_ip(a.ip()))) {
            return Observation {
                record: record(lease, Change::Blocked, None, "unsafe_dns_destination", None),
                content: None,
            };
        }
        let client = match reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(&host, &addrs)
            .timeout(StdDuration::from_secs(lease.policy.timeout_seconds))
            .user_agent("LANTERN-Scout/0.1 (source-policy opt-in)")
            .build()
        {
            Ok(c) => c,
            Err(_) => return retry(lease, None, "client_failure", 0),
        };
        #[allow(unused_mut)]
        let mut request_url = url.clone();
        #[cfg(test)]
        if let Some(addr) = fixture {
            let _ = request_url.set_scheme("http");
            let _ = request_url.set_port(Some(addr.port()));
        }
        let mut request = client
            .get(request_url)
            .header(header::ACCEPT_ENCODING, "identity");
        if conditional {
            if let Some(etag) = lease.previous_etag.as_deref().filter(|s| s.len() <= 512) {
                request = request.header(header::IF_NONE_MATCH, etag);
            }
            if let Some(last) = lease
                .previous_last_modified
                .as_deref()
                .filter(|s| s.len() <= 512)
            {
                request = request.header(header::IF_MODIFIED_SINCE, last);
            }
        }
        let mut response = match request.send().await {
            Ok(r) => r,
            Err(_) => return retry(lease, None, "network_failure", 0),
        };
        let status = response.status();
        let code = Some(status.as_u16());
        if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
            if redirects == 3 {
                return Observation {
                    record: record(lease, Change::Blocked, code, "redirect_limit", None),
                    content: None,
                };
            }
            let Some(raw) = response
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
            else {
                return retry(lease, code, "invalid_redirect", 0);
            };
            let target = match url.join(raw) {
                Ok(u) => u,
                Err(_) => return retry(lease, code, "invalid_redirect", 0),
            };
            if check_url(&lease.policy, target.as_str()).is_err() {
                return Observation {
                    record: record(lease, Change::Blocked, code, "redirect_not_allowed", None),
                    content: None,
                };
            }
            url = target;
            conditional = false;
            continue;
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Observation {
                record: record(
                    lease,
                    Change::Blocked,
                    code,
                    "access_denied_reapproval_required",
                    None,
                ),
                content: None,
            };
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            let delay = match response.headers().get(header::RETRY_AFTER) {
                Some(value) => match retry_after(Some(value)) {
                    Some(delay) => delay,
                    None => {
                        return Observation {
                            record: record(
                                lease,
                                Change::Blocked,
                                code,
                                "invalid_retry_after",
                                None,
                            ),
                            content: None,
                        };
                    }
                },
                None => 0,
            };
            if delay > 31_536_000 {
                return Observation {
                    record: record(
                        lease,
                        Change::Blocked,
                        code,
                        "retry_after_exceeds_one_year",
                        None,
                    ),
                    content: None,
                };
            }
            return retry(lease, code, "rate_limited", delay);
        }
        if status == StatusCode::NOT_FOUND || status == StatusCode::GONE {
            return Observation {
                record: record(
                    lease,
                    Change::Missing,
                    code,
                    "source_missing_not_deleted",
                    Some(next_date(Utc::now(), lease.policy.min_interval_seconds)),
                ),
                content: None,
            };
        }
        let etag = clean_validator(response.headers().get(header::ETAG));
        let last = clean_validator(response.headers().get(header::LAST_MODIFIED));
        if status == StatusCode::NOT_MODIFIED {
            if lease.previous_hash.is_none()
                || !conditional
                || (lease.previous_etag.is_none() && lease.previous_last_modified.is_none())
            {
                return retry(lease, code, "unexpected_304", 0);
            }
            let mut r = record(
                lease,
                Change::Unchanged,
                code,
                "not_modified",
                Some(next_date(Utc::now(), lease.policy.min_interval_seconds)),
            );
            r.content_hash = lease.previous_hash.clone();
            r.etag = etag.or_else(|| lease.previous_etag.clone());
            r.last_modified = last.or_else(|| lease.previous_last_modified.clone());
            return Observation {
                record: r,
                content: None,
            };
        }
        if status.is_server_error() {
            return retry(lease, code, "source_server_error", 0);
        }
        if !status.is_success() {
            return Observation {
                record: record(
                    lease,
                    Change::Blocked,
                    code,
                    "source_rejected_request",
                    None,
                ),
                content: None,
            };
        }
        if status != StatusCode::OK {
            return Observation {
                record: record(
                    lease,
                    Change::Blocked,
                    code,
                    "unexpected_or_partial_response",
                    None,
                ),
                content: None,
            };
        }
        if response
            .headers()
            .get(header::CONTENT_ENCODING)
            .is_some_and(|v| v.as_bytes() != b"identity")
        {
            return Observation {
                record: record(
                    lease,
                    Change::Blocked,
                    code,
                    "encoded_content_not_allowed",
                    None,
                ),
                content: None,
            };
        }
        let mime = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::trim)
            .map(str::to_ascii_lowercase);
        if !mime
            .as_ref()
            .is_some_and(|m| lease.policy.allowed_content_types.iter().any(|x| x == m))
        {
            return Observation {
                record: record(
                    lease,
                    Change::Blocked,
                    code,
                    "content_type_not_allowed",
                    None,
                ),
                content: None,
            };
        }
        if response
            .content_length()
            .is_some_and(|n| n > lease.policy.max_bytes as u64)
        {
            return Observation {
                record: record(lease, Change::Blocked, code, "content_too_large", None),
                content: None,
            };
        }
        let mut bytes = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    if bytes.len().saturating_add(chunk.len()) > lease.policy.max_bytes {
                        return Observation {
                            record: record(lease, Change::Blocked, code, "content_too_large", None),
                            content: None,
                        };
                    }
                    bytes.extend_from_slice(&chunk)
                }
                Ok(None) => break,
                Err(_) => return retry(lease, code, "body_read_failure", 0),
            }
        }
        // Common challenge markers are not evidence and must not be bypassed or retried.
        if mime.as_deref() == Some("text/html") {
            let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
            if text.contains("g-recaptcha")
                || text.contains("h-captcha")
                || text.contains("cf-chl-")
            {
                return Observation {
                    record: record(
                        lease,
                        Change::Blocked,
                        code,
                        "human_verification_required",
                        None,
                    ),
                    content: None,
                };
            }
        }
        let digest = hex::encode(Sha256::digest(&bytes));
        let change = match lease.previous_hash.as_deref() {
            None => Change::FirstSeen,
            Some(prev) if prev == digest => Change::Unchanged,
            Some(_) => Change::Changed,
        };
        let mut r = record(
            lease,
            change,
            code,
            "success",
            Some(next_date(Utc::now(), lease.policy.min_interval_seconds)),
        );
        r.content_type = mime;
        r.content_hash = Some(digest);
        r.etag = etag;
        r.last_modified = last;
        return Observation {
            record: r,
            content: if lease.policy.process_content_authorized {
                Some(bytes)
            } else {
                None
            },
        };
    }
    retry(lease, None, "unexpected_state", 0)
}

/// A GLEIF LEI candidate only: `registered_as` is the value at the named registration
/// authority, not an independently verified Vietnamese MST. LEI coverage is not universal.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GleifCandidate {
    pub lei: String,
    pub legal_name: String,
    pub registered_as: Option<String>,
    pub registration_authority: Option<String>,
    pub lei_status: Option<String>,
    pub legal_entity_status: Option<String>,
    pub country: String,
    pub source_url: String,
    pub last_updated: Option<String>,
}
/// Parse a bounded GLEIF JSON:API response, selecting only legal addresses in Vietnam.
/// Pure parsing only: no lookup, no persistence, no matching/merging and no risk conclusions.
/// Data dictionary: https://www.gleif.org/lei-data/access-and-use-lei-data/gleif-data-dictionary/2025-11-18_gleif-data-dictionary_v1.2_final.pdf
pub fn parse_gleif_vn_json(bytes: &[u8]) -> Result<Vec<GleifCandidate>> {
    ensure!(
        bytes.len() <= 5 * 1024 * 1024,
        "GLEIF response exceeds size limit"
    );
    let root: serde_json::Value = serde_json::from_slice(bytes).context("invalid GLEIF JSON")?;
    let data = root.get("data").context("missing GLEIF data")?;
    let records: Vec<&serde_json::Value> = match data {
        serde_json::Value::Array(items) => items.iter().collect(),
        serde_json::Value::Object(_) => vec![data],
        _ => bail!("invalid GLEIF data"),
    };
    ensure!(records.len() <= 1000, "too many GLEIF records");
    let mut result = Vec::new();
    for record in records {
        let attrs = record
            .get("attributes")
            .context("missing GLEIF attributes")?;
        let entity = attrs.get("entity").context("missing GLEIF entity")?;
        if entity
            .pointer("/legalAddress/country")
            .and_then(|x| x.as_str())
            != Some("VN")
        {
            continue;
        }
        let lei = record
            .get("id")
            .and_then(|x| x.as_str())
            .context("missing LEI")?;
        ensure!(
            lei.len() == 20
                && lei
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()),
            "invalid LEI format"
        );
        if let Some(other) = attrs.get("lei").and_then(|x| x.as_str()) {
            ensure!(other == lei, "LEI field mismatch");
        }
        let name = entity
            .pointer("/legalName/name")
            .and_then(|x| x.as_str())
            .context("missing legal name")?
            .trim();
        ensure!(
            !name.is_empty() && name.chars().count() <= 500,
            "invalid legal name"
        );
        let optional = |value: Option<&serde_json::Value>| -> Option<String> {
            value
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty() && s.chars().count() <= 500)
                .map(str::to_owned)
        };
        let updated = optional(attrs.pointer("/registration/lastUpdateDate"))
            .filter(|s| DateTime::parse_from_rfc3339(s).is_ok());
        result.push(GleifCandidate {
            lei: lei.into(),
            legal_name: name.into(),
            registered_as: optional(entity.get("registeredAs")),
            registration_authority: optional(entity.pointer("/registeredAt/id")),
            lei_status: optional(attrs.pointer("/registration/status")),
            legal_entity_status: optional(entity.get("status")),
            country: "VN".into(),
            source_url: format!("https://api.gleif.org/api/v1/lei-records/{lei}"),
            last_updated: updated,
        });
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    fn lease() -> Lease {
        Lease {
            job_id: "job".into(),
            fencing_token: "token".into(),
            policy: SourcePolicy {
                source_id: "sample".into(),
                source_url: "https://source.example/path".into(),
                enabled: true,
                access_authorized: true,
                robots_and_terms_checked: true,
                allowed_hosts: vec!["source.example".into()],
                process_content_authorized: true,
                ..SourcePolicy::default()
            },
            attempts: 0,
            previous_etag: None,
            previous_last_modified: None,
            previous_hash: None,
            not_before: Utc::now() - Duration::seconds(1),
            expires_at: Utc::now() + Duration::minutes(5),
        }
    }
    async fn mock(response: &'static [u8]) -> (SocketAddr, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let n = socket.read(&mut request).await.unwrap();
            request.truncate(n);
            socket.write_all(response).await.unwrap();
            request
        });
        (addr, task)
    }
    #[test]
    fn default_denies_and_url_checks_are_exact() {
        assert!(SourcePolicy::default().validate().is_err());
        let p = lease().policy;
        assert!(check_url(&p, "https://source.example/path").is_ok());
        for url in [
            "http://source.example/x",
            "https://source.example.evil/x",
            "https://u:p@source.example/x",
            "https://127.0.0.1/",
            "https://source.example:8443/x",
        ] {
            assert!(check_url(&p, url).is_err(), "{url}");
        }
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
        for ip in [
            "127.0.0.1",
            "169.254.169.254",
            "10.0.0.1",
            "100.64.0.1",
            "192.0.2.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
        ] {
            assert!(!is_public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
    #[test]
    fn gleif_candidates_are_filtered_and_not_merged_or_mislabeled() {
        let document = serde_json::json!({"data":[
            {"id":"11111111111111111111","links":{"self":"https://attacker.example/"},"attributes":{"lei":"11111111111111111111","entity":{"legalName":{"name":"Cong ty A"},"legalAddress":{"country":"VN"},"registeredAs":"REG-123","registeredAt":{"id":"RA000000"},"status":"ACTIVE"},"registration":{"status":"LAPSED","lastUpdateDate":"2026-01-02T00:00:00Z"}}},
            {"id":"22222222222222222222","attributes":{"entity":{"legalName":{"name":"Cong ty A"},"legalAddress":{"country":"VN"},"status":"INACTIVE"},"registration":{"status":"ISSUED"}}},
            {"id":"33333333333333333333","attributes":{"entity":{"legalName":{"name":"Other"},"legalAddress":{"country":"US"}},"registration":{}}}
        ]});
        let parsed = parse_gleif_vn_json(&serde_json::to_vec(&document).unwrap()).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].registered_as.as_deref(), Some("REG-123"));
        assert_eq!(parsed[0].lei_status.as_deref(), Some("LAPSED"));
        assert_eq!(parsed[0].legal_entity_status.as_deref(), Some("ACTIVE"));
        assert_eq!(
            parsed[0].registration_authority.as_deref(),
            Some("RA000000")
        );
        assert_eq!(
            parsed[0].source_url,
            "https://api.gleif.org/api/v1/lei-records/11111111111111111111"
        );
        assert_eq!(parsed[1].registered_as, None);
        assert_ne!(parsed[0].lei, parsed[1].lei);
        let mut bad = document;
        bad["data"][0]["id"] = serde_json::json!("invalid");
        assert!(parse_gleif_vn_json(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    #[tokio::test]
    async fn poll_once_claims_and_commits_without_a_network_request_when_disabled() {
        struct MockStore {
            lease: std::sync::Mutex<Option<Lease>>,
            records: std::sync::Mutex<Vec<FetchRecord>>,
        }
        impl ScoutStore for MockStore {
            async fn claim_due(&self, _: DateTime<Utc>) -> Result<Option<Lease>> {
                Ok(self.lease.lock().unwrap().take())
            }
            async fn commit(
                &self,
                _: &Lease,
                record: &FetchRecord,
                content: Option<&[u8]>,
            ) -> Result<()> {
                assert!(content.is_none());
                self.records.lock().unwrap().push(record.clone());
                Ok(())
            }
        }
        let mut disabled = lease();
        disabled.policy.enabled = false;
        let store = MockStore {
            lease: std::sync::Mutex::new(Some(disabled)),
            records: std::sync::Mutex::new(Vec::new()),
        };
        let observation = poll_once(&store).await.unwrap().unwrap();
        assert_eq!(observation.record.change, Change::Blocked);
        assert!(poll_once(&store).await.unwrap().is_none());
        assert_eq!(store.records.lock().unwrap().len(), 1);
    }
    #[tokio::test]
    async fn source_change_hash_and_transient_bytes() {
        let (addr,handle)=mock(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nhello").await;
        let item = fetch_inner(&lease(), Some(addr)).await;
        let _ = handle.await.unwrap();
        assert_eq!(item.record.change, Change::FirstSeen);
        assert_eq!(item.content.as_deref(), Some(&b"hello"[..]));
        assert_eq!(
            item.record.content_hash,
            Some(hex::encode(Sha256::digest(b"hello")))
        );
        assert_eq!(item.record.etag.as_deref(), Some("\"v1\""));
    }
    #[tokio::test]
    async fn conditional_304_preserves_version() {
        let (addr, handle) = mock(b"HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n").await;
        let mut l = lease();
        l.previous_etag = Some("\"v1\"".into());
        l.previous_hash = Some("oldhash".into());
        let item = fetch_inner(&l, Some(addr)).await;
        let req = String::from_utf8_lossy(&handle.await.unwrap()).to_ascii_lowercase();
        assert!(req.contains("if-none-match: \"v1\""));
        assert_eq!(item.record.change, Change::Unchanged);
        assert_eq!(item.record.content_hash.as_deref(), Some("oldhash"));
        assert!(item.content.is_none());
    }
    #[tokio::test]
    async fn blocked_redirect_rate_limit_and_size_are_not_retried_unbounded() {
        let (addr,handle)=mock(b"HTTP/1.1 302 Found\r\nLocation: https://127.0.0.1/admin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        let item = fetch_inner(&lease(), Some(addr)).await;
        let _ = handle.await.unwrap();
        assert_eq!(item.record.change, Change::Blocked);
        assert_eq!(item.record.reason, "redirect_not_allowed");
        let (addr,handle)=mock(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 3600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        let before = Utc::now();
        let item = fetch_inner(&lease(), Some(addr)).await;
        let _ = handle.await.unwrap();
        assert_eq!(item.record.change, Change::Retry);
        assert!(item.record.next_due.unwrap() >= before + Duration::seconds(3599));
        let (addr,handle)=mock(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 9999999\r\nConnection: close\r\n\r\n").await;
        let item = fetch_inner(&lease(), Some(addr)).await;
        let _ = handle.await.unwrap();
        assert_eq!(item.record.reason, "content_too_large");
    }
    #[tokio::test]
    async fn forbidden_and_partial_content_require_operator_review() {
        let (addr, handle) =
            mock(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        let item = fetch_inner(&lease(), Some(addr)).await;
        let _ = handle.await.unwrap();
        assert_eq!(item.record.change, Change::Blocked);
        assert_eq!(item.record.reason, "access_denied_reapproval_required");
        assert!(item.record.next_due.is_none());
        let (addr,handle)=mock(b"HTTP/1.1 206 Partial Content\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await;
        let item = fetch_inner(&lease(), Some(addr)).await;
        let _ = handle.await.unwrap();
        assert_eq!(item.record.reason, "unexpected_or_partial_response");
        assert!(item.content.is_none());
    }
    #[tokio::test]
    async fn challenge_page_never_becomes_evidence() {
        let body = b"<div class=\"g-recaptcha\"></div>";
        let response=format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),std::str::from_utf8(body).unwrap()).into_bytes();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            socket.write_all(&response).await.unwrap();
        });
        let item = fetch_inner(&lease(), Some(addr)).await;
        task.await.unwrap();
        assert_eq!(item.record.change, Change::Blocked);
        assert_eq!(item.record.reason, "human_verification_required");
        assert!(item.content.is_none());
    }
    #[tokio::test]
    async fn disabled_lease_and_no_content_permission_fail_closed() {
        let mut l = lease();
        l.policy.enabled = false;
        assert_eq!(fetch(&l).await.record.change, Change::Blocked);
        let (addr,handle)=mock(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").await;
        l.policy.enabled = true;
        l.policy.process_content_authorized = false;
        let item = fetch_inner(&l, Some(addr)).await;
        let _ = handle.await.unwrap();
        assert!(item.content.is_none());
        assert!(item.record.content_hash.is_some());
    }
}
