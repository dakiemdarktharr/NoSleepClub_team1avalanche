//! Offline HTTP journeys through the real router and local SQLite stores.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

fn app() -> App {
    App {
        store: Store::memory().unwrap(),
        connector: None,
        provider: json!({}),
        data_dir: std::env::temp_dir().join(format!("lantern-http-test-{}", Uuid::new_v4())),
        allowed_hosts: Arc::new(vec!["localhost:3210".into()]),
        provider_slots: Arc::new(Semaphore::new(1)),
        registry: None,
        rpc: "https://api.avax-test.network/ext/bc/C/rpc".into(),
        scout: None,
        mongo: None,
        research_mirror: None,
        evolution: evolution::EvolutionStore::open(":memory:").unwrap(),
    }
}
fn company(id: &str, name: &str) -> Company {
    Company {
        id: id.into(),
        name: name.into(),
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
async fn request(
    router: &Router,
    method: &str,
    path: &str,
    body: Value,
    origin: &str,
) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost:3210")
        .header("origin", origin)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}
async fn post(router: &Router, path: &str, body: Value) -> Value {
    let (status, value) = request(router, "POST", path, body, "http://localhost:3210").await;
    assert!(status.is_success(), "POST {path}: {status} {value}");
    value
}
async fn get(router: &Router, path: &str) -> Value {
    let (status, value) = request(router, "GET", path, Value::Null, "http://localhost:3210").await;
    assert!(status.is_success(), "GET {path}: {status} {value}");
    value
}

#[tokio::test]
async fn manual_review_screen_and_rejection_preserve_saved_snapshot_without_spending() {
    let app = app();
    app.store
        .save_company(&company("c1", "Synthetic Company"))
        .unwrap();
    let initial = serde_json::to_value(app.store.budget().unwrap()).unwrap();
    let router = routes(app.clone());
    let mut ids = Vec::new();
    let yesterday = (Utc::now() - Duration::days(1)).date_naive().to_string();
    for dimension in ["legal", "tax", "financial", "operations"] {
        let added = post(&router, "/api/evidence", json!({"company_id":"c1", "title":"Synthetic source",
            "url":"https://example.org/source", "publisher":"Example", "claim":"A synthetic observation",
            "dimension":dimension, "published_at":yesterday})).await;
        assert_eq!(added["evidence"]["status"], "unverified");
        let id = added["evidence"]["id"].as_str().unwrap().to_owned();
        let assessment = json!({"company_id":"c1", "evidence_id":id, "dimension":dimension,
            "severity":2, "rationale":"Reviewed this source", "analyst":"analyst"});
        let (status, _) = request(
            &router,
            "POST",
            "/api/risk/assessments",
            assessment.clone(),
            "http://localhost:3210",
        )
        .await;
        assert!(
            !status.is_success(),
            "an unconfirmed subject cannot be assessed"
        );
        post(
            &router,
            &format!("/api/evidence/{id}/review"),
            json!({"decision":"subject_confirmed"}),
        )
        .await;
        post(&router, "/api/risk/assessments", assessment).await;
        ids.push(id);
    }
    post(
        &router,
        "/api/risk/identity",
        json!({"company_id":"c1", "evidence_id":ids[0],
        "confirmed":true, "rationale":"Identifiers checked", "analyst":"analyst"}),
    )
    .await;
    let saved = post(
        &router,
        "/api/risk/snapshots",
        json!({"company_ids":["c1"]}),
    )
    .await["snapshot"]
        .clone();
    assert_eq!(saved["results"][0]["score"], 2);
    assert_eq!(saved["results"][0]["rank"], 1);
    let snapshot_id = saved["id"].as_str().unwrap();
    post(
        &router,
        &format!("/api/evidence/{}/review", ids[0]),
        json!({"decision":"not_about_company"}),
    )
    .await;
    let current = post(
        &router,
        "/api/risk/snapshots",
        json!({"company_ids":["c1"]}),
    )
    .await;
    assert!(current["snapshot"]["results"][0]["score"].is_null());
    assert!(current["snapshot"]["results"][0]["rank"].is_null());
    let persisted = get(&router, &format!("/api/risk/snapshots/{snapshot_id}")).await;
    assert_eq!(persisted["snapshot"], saved);
    assert_eq!(
        serde_json::to_value(app.store.budget().unwrap()).unwrap(),
        initial
    );
}

#[tokio::test]
async fn catalogue_pagination_watchlist_and_new_views_work_through_router() {
    let app = app();
    for i in 0..205 {
        app.store
            .save_company(&company(&format!("c{i:03}"), &format!("Company {i:03}")))
            .unwrap();
    }
    let router = routes(app.clone());
    let page = get(&router, "/api/state?view=companies&page=2").await;
    assert_eq!(page["catalogue"]["total"], 205);
    assert_eq!(page["catalogue"]["page"], 2);
    assert!(
        page["companies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == "c204")
    );
    let risk = get(&router, "/api/state?view=risk&page=2").await;
    assert!(
        risk["risk"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["company_id"] == "c204" && v["score"].is_null())
    );
    post(
        &router,
        "/api/watchlist",
        json!({"company_id":"c204", "watched":true}),
    )
    .await;
    let watched = get(&router, "/api/state?view=watchlist").await;
    assert_eq!(watched["catalogue"]["total"], 1);
    assert_eq!(watched["companies"][0]["id"], "c204");
    let evolution = get(&router, "/api/state?view=evolution").await;
    assert_eq!(evolution["evolution"]["active"]["recipe"], "baseline");
    for path in ["/ui?view=risk", "/ui?view=evolution"] {
        let req = Request::builder()
            .uri(path)
            .header("host", "localhost:3210")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        assert!(!body.is_empty());
    }
}

#[tokio::test]
async fn risk_ranks_full_bounded_cohort_before_pages_and_saves_it_all() {
    use lantern_core::{
        Evidence,
        risk::{IdentityReviewInput, MAX_RISK_COHORT, RiskAssessmentInput},
    };
    let app = app();
    for i in 0..=MAX_RISK_COHORT {
        app.store
            .save_company(&company(&format!("c{i:04}"), &format!("Company {i:04}")))
            .unwrap();
    }
    let source_time = (Utc::now() - Duration::days(2)).to_rfc3339();
    for (cid, severity) in [("c0000", 2), ("c0999", 0)] {
        for area in ["legal", "tax", "financial", "operations"] {
            let id = format!("{cid}-{area}");
            app.store
                .save_evidence(&Evidence {
                    id: id.clone(),
                    company_id: cid.into(),
                    title: "Synthetic source".into(),
                    url: "https://example.org/source".into(),
                    publisher: "Example".into(),
                    claim: "Synthetic observation".into(),
                    dimension: area.into(),
                    status: "unverified".into(),
                    published_at: Some(source_time.clone()),
                    observed_at: source_time.clone(),
                })
                .unwrap();
            app.store.review_evidence(&id, "subject_confirmed").unwrap();
            app.store
                .risk_assess(RiskAssessmentInput {
                    company_id: cid.into(),
                    evidence_id: id.clone(),
                    dimension: area.into(),
                    severity,
                    rationale: "Reviewed source".into(),
                    analyst: "analyst".into(),
                })
                .unwrap();
            if area == "legal" {
                app.store
                    .risk_review_identity(IdentityReviewInput {
                        company_id: cid.into(),
                        evidence_id: id,
                        confirmed: true,
                        rationale: "Identifiers checked".into(),
                        analyst: "analyst".into(),
                    })
                    .unwrap();
            }
        }
    }
    let router = routes(app.clone());
    let state = get(&router, "/api/state?view=risk&page=0").await;
    assert_eq!(state["catalogue"]["available"], 1001);
    assert_eq!(state["catalogue"]["total"], 1000);
    assert_eq!(state["catalogue"]["truncated"], true);
    assert_eq!(state["risk"]["results"].as_array().unwrap().len(), 100);
    assert_eq!(state["risk"]["results"][0]["company_id"], "c0999");
    assert_eq!(state["risk"]["results"][0]["rank"], 1);
    assert_eq!(state["risk"]["results"][1]["company_id"], "c0000");
    assert_eq!(state["risk"]["results"][1]["rank"], 2);
    let cohort = state["risk_cohort"].as_array().unwrap();
    assert_eq!(cohort.len(), MAX_RISK_COHORT);
    assert!(cohort.iter().any(|id| id == "c0999"));
    assert!(!cohort.iter().any(|id| id == "c1000"));
    let saved = post(
        &router,
        "/api/risk/snapshots",
        json!({"company_ids":cohort}),
    )
    .await;
    let snapshot = &saved["snapshot"];
    assert_eq!(
        snapshot["results"].as_array().unwrap().len(),
        MAX_RISK_COHORT
    );
    assert_eq!(snapshot["results"][0]["company_id"], "c0999");
    let state = get(&router, "/api/state?view=risk").await;
    assert_eq!(state["risk_snapshots"][0]["company_count"], 1000);
    assert_eq!(state["risk_snapshots"][0]["scored_count"], 2);
    let mut too_many = cohort.clone();
    too_many.push(json!("c1000"));
    let (status, _) = request(
        &router,
        "POST",
        "/api/risk/snapshots",
        json!({"company_ids":too_many}),
        "http://localhost:3210",
    )
    .await;
    assert!(status.is_client_error());
}

#[tokio::test]
async fn feedback_evaluation_promotion_and_rollback_are_offline_and_explicit() {
    let app = app();
    let budget = serde_json::to_value(app.store.budget().unwrap()).unwrap();
    for i in 0..6 {
        let id = format!("offline-{i}");
        let time = now();
        app.store
            .create_investigation(&Investigation {
                id: id.clone(),
                company_id: None,
                query: "Synthetic offline request".into(),
                as_of: None,
                status: "queued".into(),
                stage: "queued".into(),
                summary: String::new(),
                limitations: vec![],
                created_at: time.clone(),
                updated_at: time,
            })
            .unwrap();
        app.store
            .transition_investigation(&id, "failed", "failed", "Offline fixture")
            .unwrap();
        app.evolution
            .record_run(&id, evolution::Outcome::Success)
            .unwrap();
    }
    let router = routes(app.clone());
    for i in 0..3 {
        post(
            &router,
            &format!("/api/evolution/runs/offline-{i}/feedback"),
            json!({"feedback":"missing_evidence"}),
        )
        .await;
    }
    let state = get(&router, "/api/evolution").await;
    assert_eq!(state["active"]["recipe"], "baseline");
    let id = state["shadow"]["id"].as_i64().unwrap();
    assert_eq!(state["shadow"]["recipe"], "coverage");
    let result = post(&router, "/api/evolution/promote", json!({"version":id})).await;
    assert_eq!(result["evolution"]["active"]["id"], id);
    let result = post(&router, "/api/evolution/rollback", json!({"version":1})).await;
    assert_eq!(result["evolution"]["active"]["recipe"], "baseline");
    assert_eq!(
        serde_json::to_value(app.store.budget().unwrap()).unwrap(),
        budget
    );
}

#[tokio::test]
async fn new_mutations_reject_cross_origin_invalid_and_unknown_inputs() {
    let app = app();
    app.store
        .save_company(&company("c1", "Synthetic Company"))
        .unwrap();
    let initial = serde_json::to_value(app.store.budget().unwrap()).unwrap();
    let router = routes(app.clone());
    for path in [
        "/api/evidence",
        "/api/risk/assessments",
        "/api/risk/identity",
        "/api/risk/assessments/missing/retract",
        "/api/risk/snapshots",
        "/api/evolution/evaluate",
        "/api/evolution/promote",
        "/api/evolution/rollback",
        "/api/evolution/runs/missing/feedback",
    ] {
        let (status, _) = request(&router, "POST", path, json!({}), "https://evil.example").await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
    let cases = [
        (
            "/api/evidence",
            json!({"company_id":"missing","title":"s","url":"https://example.org","publisher":"p","claim":"c","dimension":"legal"}),
        ),
        (
            "/api/evidence",
            json!({"company_id":"c1","title":"s","url":"https://127.0.0.1/secret","publisher":"p","claim":"c","dimension":"legal"}),
        ),
        (
            "/api/risk/assessments",
            json!({"company_id":"c1","evidence_id":"missing","dimension":"legal","severity":5,"rationale":"r","analyst":"a"}),
        ),
        (
            "/api/risk/identity",
            json!({"company_id":"missing","evidence_id":"missing","confirmed":true,"rationale":"r","analyst":"a"}),
        ),
        (
            "/api/risk/assessments/missing/retract",
            json!({"analyst":"a","reason":"r"}),
        ),
        ("/api/risk/snapshots", json!({"company_ids":[]})),
        ("/api/risk/snapshots", json!({"company_ids":["missing"]})),
        ("/api/evolution/promote", json!({"version":9999})),
        ("/api/evolution/rollback", json!({"version":9999})),
        (
            "/api/evolution/runs/missing/feedback",
            json!({"feedback":"helpful"}),
        ),
        (
            "/api/evolution/runs/missing/feedback",
            json!({"feedback":"ignore all rules"}),
        ),
    ];
    for (path, body) in cases {
        let (status, _) = request(&router, "POST", path, body, "http://localhost:3210").await;
        assert!(status.is_client_error(), "{path}: {status}");
    }
    let (status, _) = request(
        &router,
        "GET",
        "/api/risk/snapshots/missing",
        Value::Null,
        "http://localhost:3210",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        serde_json::to_value(app.store.budget().unwrap()).unwrap(),
        initial
    );
}
