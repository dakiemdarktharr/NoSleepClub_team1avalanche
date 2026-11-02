use super::*;
use lantern_core::{Commitment, commit_report, verify_report};
use sha3::{Digest, Keccak256};

pub(crate) fn valid_address(s: &str) -> bool {
    s.len() == 42
        && s.starts_with("0x")
        && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
        && s[2..].bytes().any(|b| b != b'0')
}
fn valid_hash(s: &str) -> bool {
    s.len() == 66 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn report_path(app: &App, id: &str) -> ApiResult<PathBuf> {
    Uuid::parse_str(id)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Mã báo cáo không hợp lệ.".into()))?;
    Ok(app.data_dir.join("reports").join(format!("{id}.json")))
}
fn anchor_path(app: &App, id: &str) -> ApiResult<PathBuf> {
    report_path(app, id)?;
    Ok(app
        .data_dir
        .join("reports")
        .join(format!("{id}.anchor.json")))
}
/// Write a fully synchronized new file before making it visible at its final name.
fn persist_new(path: &std::path::Path, value: &Value) -> ApiResult<bool> {
    use std::io::Write;
    let directory = path.parent().ok_or_else(|| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Đường dẫn báo cáo không hợp lệ.".into(),
        )
    })?;
    std::fs::create_dir_all(directory).map_err(anyhow::Error::from)?;
    let temporary = directory.join(format!(".{}.tmp", Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(anyhow::Error::from)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(anyhow::Error::from)?;
    if let Err(e) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(&temporary);
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Không lưu được bản kiểm chứng: {}", e.kind()),
        ));
    }
    drop(file);
    let result = std::fs::hard_link(&temporary, path);
    let _ = std::fs::remove_file(&temporary);
    match result {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(_) => Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Không tạo được phiên bản báo cáo nguyên tử trên hệ thống tệp này.".into(),
        )),
    }
}
fn include_anchor(app: &App, id: &str, value: &mut Value) -> ApiResult<()> {
    let path = anchor_path(app, id)?;
    if !path.is_file() {
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(anyhow::Error::from)?;
    if bytes.len() > 64 * 1024 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Receipt đã lưu vượt giới hạn.".into(),
        ));
    }
    let saved: Value = serde_json::from_slice(&bytes).map_err(anyhow::Error::from)?;
    if saved["report_digest"] != value["commitment"]["digest"] {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Receipt đã lưu không khớp phiên bản báo cáo.".into(),
        ));
    }
    value["anchor"] = saved;
    value["anchoring_status"] = json!("receipt_confirmed");
    value["anchor_observation_warning"] = json!(
        "Receipt was observed at checked_at; it is not a finality proof and is not rechecked when reading this saved bundle."
    );
    Ok(())
}
fn payload(app: &App, id: &str) -> ApiResult<Value> {
    let job = app
        .store
        .get_investigation(id)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Không tìm thấy báo cáo.".into()))?;
    if !matches!(job.status.as_str(), "completed" | "partial") || job.summary.trim().is_empty() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Báo cáo chưa có kết quả đủ để xuất.".into(),
        ));
    }
    let company = job
        .company_id
        .as_deref()
        .map(|id| app.store.get_company(id))
        .transpose()?
        .flatten();
    if company.is_none() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Tác vụ chưa gắn với doanh nghiệp đã chọn.".into(),
        ));
    }
    let evidence: Vec<_> = app
        .store
        .evidence_for_investigation(id)?
        .into_iter()
        .filter(|item| item.status == "subject_confirmed")
        .collect();
    if evidence.is_empty() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Chưa có bằng chứng của tác vụ được người dùng xác nhận đúng chủ thể.".into(),
        ));
    }
    Ok(
        json!({"schema":"lantern.report.v1","report_id":id,"investigation":job,"company":company,"evidence":evidence,
        "temporal_mode":"system_known","methodology":"A user confirmed only that each source concerns the selected subject. Claims remain unverified; they are not legal conclusions or investment advice.",
        "issuer":"Local LANTERN workspace. Identity is not certified by a hash."}),
    )
}
fn bundle(app: &App, id: &str) -> ApiResult<Value> {
    let path = report_path(app, id)?;
    if path.is_file() {
        let bytes = std::fs::read(path).map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Không đọc được phiên bản đã lưu.".into(),
            )
        })?;
        if bytes.len() > 5 * 1024 * 1024 {
            return Err(ApiError(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Bản lưu vượt giới hạn.".into(),
            ));
        }
        let mut value: Value = serde_json::from_slice(&bytes).map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Bản lưu không hợp lệ.".into(),
            )
        })?;
        let commitment: Commitment =
            serde_json::from_value(value["commitment"].clone()).map_err(|_| {
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Cam kết lưu bị hỏng.".into(),
                )
            })?;
        if !verify_report(&value["report"], &commitment)? {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "Bản báo cáo lưu đã thay đổi, không khớp cam kết.".into(),
            ));
        }
        include_anchor(app, id, &mut value)?;
        return Ok(value);
    }
    let report = payload(app, id)?;
    let c = commit_report(&report)?;
    let value = json!({"report":report,"commitment":c,"created_at":now(),"anchoring_status":"local_only",
        "warning":"Local salted commitment only; not yet anchored or identity certified. Salt/pseudonyms do not guarantee anonymization."});
    if !persist_new(&path, &value)? {
        return bundle(app, id);
    }
    Ok(value)
}
pub(crate) async fn export(State(app): State<App>, Path(id): Path<String>) -> ApiResult<Response> {
    let value = bundle(&app, &id)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=lantern-report-{id}.json").as_str(),
            ),
        ],
        serde_json::to_vec_pretty(&value).map_err(anyhow::Error::from)?,
    )
        .into_response())
}
pub(crate) async fn commitment(
    State(app): State<App>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let value = bundle(&app, &id)?;
    Ok(Json(
        json!({"ok":true,"commitment":value["commitment"],"anchoring_status":value["anchoring_status"],
        "report_id":id,"message":if value["anchoring_status"] == "receipt_confirmed" { "Receipt đã được ghi nhận tại thời điểm kiểm tra; cần kiểm tra lại chain, không chứng nhận nội dung đúng." } else { "Đã lưu cam kết có salt cho phiên bản báo cáo. Chưa có giao dịch Avalanche; không chứng nhận dữ liệu đúng." }}),
    ))
}
#[derive(Deserialize)]
pub(crate) struct VerifyInput {
    report: Value,
    commitment: Commitment,
}
pub(crate) async fn verify(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<VerifyInput>,
) -> ApiResult<Json<Value>> {
    report_path(&app, &id)?;
    let valid = verify_report(&input.report, &input.commitment)?;
    Ok(Json(
        json!({"ok":true,"valid":valid,"message":if valid{"Nội dung khớp cam kết. Điều này không xác minh tính đúng của nguồn."}else{"Nội dung không khớp cam kết; có thay đổi hoặc sai phiên bản."}}),
    ))
}
async fn rpc(app: &App, method: &str, params: Value) -> ApiResult<Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(anyhow::Error::from)?;
    let mut response = client
        .post(&app.rpc)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::BAD_GATEWAY,
                "Chưa kết nối được RPC Fuji.".into(),
            )
        })?;
    if !response.status().is_success() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "RPC Fuji trả lỗi.".into(),
        ));
    }
    const MAX_RPC: usize = 1024 * 1024;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RPC as u64)
    {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "Phản hồi RPC vượt giới hạn.".into(),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        ApiError(
            StatusCode::BAD_GATEWAY,
            "Không đọc được phản hồi RPC.".into(),
        )
    })? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RPC {
            return Err(ApiError(
                StatusCode::BAD_GATEWAY,
                "Phản hồi RPC vượt giới hạn.".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(anyhow::Error::from)?;
    if value.get("error").is_some() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            "RPC chưa xử lý được yêu cầu.".into(),
        ));
    }
    Ok(value["result"].clone())
}
pub(crate) async fn status(State(app): State<App>) -> ApiResult<Json<Value>> {
    let chain_id = rpc(&app, "eth_chainId", json!([])).await?;
    if chain_id != "0xa869" {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "RPC không thuộc Fuji chain ID 43113.".into(),
        ));
    }
    Ok(Json(
        json!({"ok":true,"chain_id":43113,"registry_configured":app.registry.is_some(),"registry":app.registry,
        "message":"Đã xác nhận RPC Fuji; kết nối RPC không có nghĩa báo cáo đã được neo."}),
    ))
}
fn calldata_with_supersedes(digest: &str, supersedes: &str) -> ApiResult<String> {
    let digest = digest.strip_prefix("0x").unwrap_or(digest);
    let supersedes = supersedes.strip_prefix("0x").unwrap_or(supersedes);
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Digest không hợp lệ.".into(),
        ));
    }
    if supersedes.len() != 64 || !supersedes.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Digest phiên bản trước không hợp lệ.".into(),
        ));
    }
    let selector = Keccak256::digest(b"anchor(bytes32,bytes32)");
    Ok(format!(
        "0x{}{}{}",
        hex::encode(&selector[..4]),
        digest,
        supersedes
    ))
}
fn matching_anchor_event(
    receipt: &Value,
    registry: &str,
    issuer: &str,
    digest: &str,
    supersedes: &str,
) -> bool {
    let event_signature = format!(
        "0x{}",
        hex::encode(Keccak256::digest(b"Anchored(bytes32,address,bytes32)"))
    );
    let expected_digest = format!("0x{}", digest.trim_start_matches("0x"));
    let issuer_topic = format!("0x{}{}", "0".repeat(24), issuer.trim_start_matches("0x"));
    let supersedes_topic = format!("0x{}", supersedes.strip_prefix("0x").unwrap_or(supersedes));
    receipt["logs"].as_array().into_iter().flatten().any(|log| {
        let topics = log["topics"].as_array();
        log["removed"] != true
            && log["address"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case(registry))
            && topics.is_some_and(|t| {
                t.len() == 4
                    && t[0]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case(&event_signature))
                    && t[1]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case(&expected_digest))
                    && t[2]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case(&issuer_topic))
                    && t[3]
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case(&supersedes_topic))
            })
    })
}
/// Check internal RPC consistency. The configured contract and RPC remain trust assumptions.
fn validate_observation(
    receipt: &Value,
    tx: &Value,
    hash: &str,
    registry: &str,
    digest: &str,
) -> Option<(String, String, String, String)> {
    let eq =
        |v: &Value, expected: &str| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case(expected));
    let issuer = tx["from"].as_str().filter(|s| valid_address(s))?;
    let block_hash = receipt["blockHash"].as_str().filter(|s| valid_hash(s))?;
    let block_number = receipt["blockNumber"].as_str().filter(|s| {
        s.strip_prefix("0x")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_hexdigit()))
    })?;
    let input = tx["input"].as_str()?;
    if !input.is_ascii() || input.len() != 138 || !input.starts_with("0x") {
        return None;
    }
    let supersedes = &input[74..138];
    let expected = calldata_with_supersedes(digest, supersedes).ok()?;
    if receipt["status"] != "0x1"
        || !eq(&receipt["transactionHash"], hash)
        || !eq(&tx["hash"], hash)
        || !eq(&receipt["to"], registry)
        || !eq(&tx["to"], registry)
        || !eq(&receipt["from"], issuer)
        || !eq(&tx["blockHash"], block_hash)
        || !eq(&tx["blockNumber"], block_number)
        || !input.eq_ignore_ascii_case(&expected)
        || !matching_anchor_event(receipt, registry, issuer, digest, supersedes)
    {
        return None;
    }
    Some((
        issuer.to_string(),
        format!("0x{supersedes}"),
        block_hash.to_string(),
        block_number.to_string(),
    ))
}
#[derive(Deserialize, Default)]
pub(crate) struct TransactionQuery {
    supersedes: Option<String>,
}
pub(crate) async fn transaction(
    State(app): State<App>,
    Path(id): Path<String>,
    Query(query): Query<TransactionQuery>,
) -> ApiResult<Json<Value>> {
    let registry = app.registry.as_deref().ok_or_else(|| {
        ApiError(
            StatusCode::PRECONDITION_REQUIRED,
            "Chưa cấu hình contract registry đã triển khai trên Fuji. Cam kết hiện chỉ lưu local."
                .into(),
        )
    })?;
    let b = bundle(&app, &id)?;
    let chain_id = rpc(&app, "eth_chainId", json!([])).await?;
    if chain_id != "0xa869" {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Sai chain; chỉ hỗ trợ Fuji.".into(),
        ));
    }
    let code = rpc(&app, "eth_getCode", json!([registry, "latest"])).await?;
    if matches!(code.as_str(), None | Some("0x") | Some("0x0")) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Địa chỉ registry chưa có bytecode trên Fuji.".into(),
        ));
    }
    let digest = b["commitment"]["digest"]
        .as_str()
        .ok_or_else(|| ApiError(StatusCode::CONFLICT, "Thiếu digest.".into()))?;
    let zero = "0".repeat(64);
    let supersedes = query.supersedes.as_deref().unwrap_or(&zero);
    Ok(Json(
        json!({"ok":true,"chain_id":"0xa869","transaction":{"to":registry,"data":calldata_with_supersedes(digest, supersedes)?,"value":"0x0"},
        "message":"Giao dịch chưa được gửi. Kiểm tra contract, chain và phí testnet trong ví trước khi ký."}),
    ))
}
#[derive(Deserialize)]
pub(crate) struct AnchorInput {
    transaction_hash: String,
}
pub(crate) async fn anchor(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<AnchorInput>,
) -> ApiResult<Json<Value>> {
    if !valid_hash(&input.transaction_hash) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Transaction hash không hợp lệ.".into(),
        ));
    }
    let registry = app.registry.as_deref().ok_or_else(|| {
        ApiError(
            StatusCode::PRECONDITION_REQUIRED,
            "Chưa cấu hình registry.".into(),
        )
    })?;
    let b = bundle(&app, &id)?;
    if rpc(&app, "eth_chainId", json!([])).await? != "0xa869" {
        return Err(ApiError(StatusCode::CONFLICT, "Sai chain.".into()));
    }
    let receipt = rpc(
        &app,
        "eth_getTransactionReceipt",
        json!([input.transaction_hash]),
    )
    .await?;
    if receipt.is_null() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Transaction đang chờ; chưa đánh dấu đã neo.".into(),
        ));
    }
    let tx = rpc(
        &app,
        "eth_getTransactionByHash",
        json!([input.transaction_hash]),
    )
    .await?;
    let digest = b["commitment"]["digest"].as_str().unwrap_or("");
    let (issuer, supersedes, block_hash, block_number) =
        validate_observation(&receipt, &tx, &input.transaction_hash, registry, digest).ok_or_else(
            || {
                ApiError(
                    StatusCode::CONFLICT,
                    "Giao dịch, receipt hoặc sự kiện không khớp báo cáo và contract.".into(),
                )
            },
        )?;
    let block = rpc(&app, "eth_getBlockByNumber", json!([block_number, false])).await?;
    if !block["hash"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case(&block_hash))
        || !block["number"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(&block_number))
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Block chưa khớp chuỗi RPC hiện tại; hãy kiểm tra lại sau.".into(),
        ));
    }
    let anchor = json!({"chain_id":43113,"transaction_hash":input.transaction_hash,"contract":registry,
        "report_digest":digest,"issuer_address":issuer,"supersedes":supersedes,"block_number":block_number,"block_hash":block_hash,"checked_at":now(),
        "observation":"receipt_seen_on_rpc_canonical_block_not_finality_proof",
        "identity_status":"address_not_organization_certified"});
    if !persist_new(&anchor_path(&app, &id)?, &anchor)? {
        let existing = bundle(&app, &id)?;
        if existing["anchor"]["transaction_hash"] != anchor["transaction_hash"] {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "Phiên bản này đã có một receipt khác; không ghi đè lịch sử.".into(),
            ));
        }
    }
    Ok(Json(
        json!({"ok":true,"anchor":anchor,"message":"RPC Fuji trả receipt và sự kiện khớp block tại thời điểm kiểm tra, chưa chứng minh finality. Ví không chứng nhận danh tính tổ chức; blockchain không xác minh nguồn."}),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calldata_is_exactly_two_abi_words() {
        let data = calldata_with_supersedes(&"ab".repeat(32), &"0".repeat(64)).unwrap();
        assert_eq!(data.len(), 138);
        assert!(data.ends_with(&"0".repeat(64)));
        assert!(calldata_with_supersedes("bad", &"0".repeat(64)).is_err());
        assert!(calldata_with_supersedes(&"ab".repeat(32), "bad").is_err());
    }
    #[test]
    fn addresses_reject_zero_and_code_injection() {
        assert!(!valid_address("0x0000000000000000000000000000000000000000"));
        assert!(!valid_address("../../etc"));
        assert!(valid_address("0x1234567890abcdef1234567890abcdef12345678"));
    }
    #[test]
    fn successful_fallback_without_expected_event_does_not_prove_anchor() {
        let registry = "0x1234567890abcdef1234567890abcdef12345678";
        let issuer = "0x234567890abcdef1234567890abcdef123456789";
        let digest = "ab".repeat(32);
        let event = format!(
            "0x{}",
            hex::encode(Keccak256::digest(b"Anchored(bytes32,address,bytes32)"))
        );
        let mut receipt = json!({"logs":[{"address":registry,"topics":[event,format!("0x{digest}"),format!("0x{}{}","0".repeat(24),&issuer[2..]),format!("0x{}","0".repeat(64))]}]});
        assert!(matching_anchor_event(
            &receipt,
            registry,
            issuer,
            &digest,
            &"0".repeat(64)
        ));
        receipt["logs"][0]["topics"][1] = json!(format!("0x{}", "cd".repeat(32)));
        assert!(!matching_anchor_event(
            &receipt,
            registry,
            issuer,
            &digest,
            &"0".repeat(64)
        ));
        assert!(!matching_anchor_event(
            &json!({"logs":[]}),
            registry,
            issuer,
            &digest,
            &"0".repeat(64)
        ));
    }
    #[test]
    fn receipt_must_bind_transaction_issuer_block_and_superseded_digest() {
        let registry = "0x1234567890abcdef1234567890abcdef12345678";
        let issuer = "0x234567890abcdef1234567890abcdef123456789";
        let hash = format!("0x{}", "12".repeat(32));
        let block = format!("0x{}", "34".repeat(32));
        let digest = "ab".repeat(32);
        let prior = "cd".repeat(32);
        let signature = format!(
            "0x{}",
            hex::encode(Keccak256::digest(b"Anchored(bytes32,address,bytes32)"))
        );
        let tx = json!({"hash":hash, "from":issuer,"to":registry,"blockHash":block,"blockNumber":"0x20","input":calldata_with_supersedes(&digest, &prior).unwrap()});
        let receipt = json!({"transactionHash":hash,"from":issuer,"to":registry,"status":"0x1","blockHash":block,"blockNumber":"0x20","logs":[{"address":registry,"removed":false,"topics":[signature,format!("0x{digest}"),format!("0x{}{}","0".repeat(24),&issuer[2..]),format!("0x{prior}")]}]});
        let observed = validate_observation(&receipt, &tx, &hash, registry, &digest).unwrap();
        assert_eq!(observed.1, format!("0x{prior}"));
        for (field, value) in [
            ("transactionHash", json!(format!("0x{}", "77".repeat(32)))),
            ("blockHash", json!(format!("0x{}", "55".repeat(32)))),
            ("from", json!(registry)),
            ("status", json!("0x0")),
        ] {
            let mut bad = receipt.clone();
            bad[field] = value;
            assert!(
                validate_observation(&bad, &tx, &hash, registry, &digest).is_none(),
                "{field}"
            );
        }
        let mut removed = receipt.clone();
        removed["logs"][0]["removed"] = json!(true);
        assert!(validate_observation(&removed, &tx, &hash, registry, &digest).is_none());
        let mut wrong_prior = receipt.clone();
        wrong_prior["logs"][0]["topics"][3] = json!(format!("0x{}", "00".repeat(32)));
        assert!(validate_observation(&wrong_prior, &tx, &hash, registry, &digest).is_none());
    }
    #[test]
    fn local_report_bundle_is_stable_and_detects_tampering() {
        let data = std::env::temp_dir()
            .join("lantern-test-reports")
            .join(Uuid::new_v4().to_string());
        let store = Store::memory().unwrap();
        let app = App {
            evolution: crate::evolution::EvolutionStore::open(":memory:").unwrap(),
            store: store.clone(),
            connector: None,
            provider: json!({}),
            data_dir: data,
            allowed_hosts: Arc::new(vec!["localhost:3210".into()]),
            provider_slots: Arc::new(Semaphore::new(1)),
            registry: None,
            rpc: "https://api.avax-test.network/ext/bc/C/rpc".into(),
            scout: None,
            mongo: None,
            research_mirror: None,
        };
        let company = Company {
            id: Uuid::new_v4().to_string(),
            name: "Công ty kiểm thử".into(),
            tax_id: None,
            ticker: None,
            sector: None,
            location: None,
            website: None,
            source_url: Some("https://example.org/filing".into()),
            description: None,
            identity_status: "candidate_unverified".into(),
            watched: false,
            updated_at: now(),
        };
        store.save_company(&company).unwrap();
        let id = Uuid::new_v4().to_string();
        let job = Investigation {
            id: id.clone(),
            company_id: Some(company.id.clone()),
            query: "kiểm thử".into(),
            as_of: None,
            status: "queued".into(),
            stage: "queued_research".into(),
            summary: String::new(),
            limitations: vec![],
            created_at: now(),
            updated_at: now(),
        };
        store.create_investigation(&job).unwrap();
        store
            .transition_investigation(&id, "running", "collecting", "working")
            .unwrap();
        store
            .transition_investigation(&id, "verifying", "verifying", "checking")
            .unwrap();
        let e = lantern_core::Evidence {
            id: Uuid::new_v4().to_string(),
            company_id: company.id.clone(),
            title: "Synthetic source".into(),
            url: "https://example.org/filing".into(),
            publisher: "Test".into(),
            claim: "Synthetic claim".into(),
            dimension: "identity".into(),
            status: "unverified".into(),
            published_at: None,
            observed_at: now(),
        };
        let evidence_id = e.id.clone();
        store
            .finalize_import(&id, &[], &[e], "Kết quả kiểm thử", &[], "completed")
            .unwrap();
        assert!(payload(&app, &id).is_err(), "subject review is required");
        store
            .review_evidence(&evidence_id, "subject_confirmed")
            .unwrap();
        // A separate task on the same company must never contribute its evidence.
        let other_id = Uuid::new_v4().to_string();
        let other = Investigation {
            id: other_id.clone(),
            company_id: Some(company.id.clone()),
            query: "another".into(),
            as_of: None,
            status: "queued".into(),
            stage: "queued_research".into(),
            summary: String::new(),
            limitations: vec![],
            created_at: now(),
            updated_at: now(),
        };
        store.create_investigation(&other).unwrap();
        store
            .transition_investigation(&other_id, "running", "collecting", "working")
            .unwrap();
        store
            .transition_investigation(&other_id, "verifying", "verifying", "checking")
            .unwrap();
        let other_e = lantern_core::Evidence {
            id: Uuid::new_v4().to_string(),
            company_id: company.id,
            title: "Other job source".into(),
            url: "https://example.org/other".into(),
            publisher: "Test".into(),
            claim: "Other claim".into(),
            dimension: "identity".into(),
            status: "unverified".into(),
            published_at: None,
            observed_at: now(),
        };
        let other_e_id = other_e.id.clone();
        store
            .finalize_import(&other_id, &[], &[other_e], "Other report", &[], "completed")
            .unwrap();
        store
            .review_evidence(&other_e_id, "subject_confirmed")
            .unwrap();
        let first = bundle(&app, &id).unwrap();
        assert_eq!(first["report"]["evidence"].as_array().unwrap().len(), 1);
        assert_eq!(first["report"]["evidence"][0]["id"], evidence_id);
        let second = bundle(&app, &id).unwrap();
        assert_eq!(first["anchoring_status"], "local_only");
        assert_eq!(first["commitment"], second["commitment"]);
        let commitment: Commitment = serde_json::from_value(first["commitment"].clone()).unwrap();
        assert!(verify_report(&first["report"], &commitment).unwrap());
        let mut changed = first["report"].clone();
        changed["methodology"] = json!("tampered");
        assert!(!verify_report(&changed, &commitment).unwrap());
    }
}
