# Đánh giá, chi phí và lộ trình triển khai

**Kế hoạch ngày 01/11/2026. Tất cả mục tiêu dưới đây là đề xuất cần đo, không phải kết quả đã đạt.** Kết quả benchmark trong [tổng quan học thuật](../research/academic-evidence.md) chỉ thuộc bài gốc; chưa có benchmark sản phẩm, mẫu khách hàng hay hợp đồng dữ liệu trong repo.

## 1. Sản phẩm nào sẽ được kiểm tra

MVP: voice/text → chọn đúng pháp nhân → điều tra một bộ nguồn được phép → dossier có nguồn, ngày và khoảng trống → so sánh một nhóm hẹp nếu đủ dữ liệu → kiểm chứng phiên bản trên Fuji. Phải có casual chat và cơ chế quay lại mục tiêu đang chạy. Chỉ số chính là **số tác vụ thẩm định hoàn tất đúng bằng chứng trong thời gian/ngân sách cho phép**, không phải số agent gọi hoặc độ dài báo cáo.

Ban đầu chọn khoảng 30–50 doanh nghiệp công bố tương đối đầy đủ và 5–10 hồ sơ tư nhân có quyền dữ liệu để thiết kế; đây là cỡ thử kỹ thuật đề xuất, không đại diện toàn quốc. Nếu chưa có nguồn thật đủ quyền, dùng corpus tổng hợp có truth ledger để thử pipeline, ghi rõ synthetic, và giữ cổng nguồn thật là blocker của pilot thực tế. Không trình diễn synthetic như một nguồn thật đã tích hợp.

Chọn khách hàng đầu tiên: chuyên viên thẩm định đối tác/đầu tư, không đại chúng tìm mã tăng mạnh. Phỏng vấn 5–8 người dùng mục tiêu, đo các câu hỏi thường gặp, thời gian đang tốn, bằng chứng họ chấp nhận, chi phí dữ liệu và tổn thất do sai. Đây là công việc tương lai, chưa có khách hàng hay kết quả interview.

## 2. Bộ đánh giá đề xuất

| Lớp | Bộ kiểm thử ban đầu | Chấm điều gì |
|---|---|---|
| Voice | Khoảng 300 utterances do 15–20 người nói đồng ý tham gia, có Bắc/Trung/Nam, headset/loa/ồn, câu có MST/tên/số/ngày | WER/CER, critical-slot exact match, câu cần hỏi lại, p50/p95 từng stage, stop-speech, false interruption |
| Hội thoại | Khoảng 60 kịch bản casual ↔ task, sửa tên/ngày, ngắt/hủy, trả lời câu hỏi đang chờ, 2 goal | Giữ đúng goal, không lấy casual claim làm evidence, không tự tạo task ngoài ý định |
| Định danh | Khoảng 200 cặp/nhóm gồm tên trùng, bỏ dấu, đổi tên, chi nhánh, ticker/issuer và alias lịch sử | Precision nối đúng, false merge và false split, coverage/hỏi lại, confidence intervals |
| Evidence QA | Khoảng 150–200 câu có đáp án chuyên gia, citation/page/date; có bảng scan, restatement, đơn vị và không đủ nguồn | Đúng entity/kỳ, retrieval recall, citation support, phép tính, abstention precision/coverage |
| Dossier | Khoảng 30 nhiệm vụ chấm mù bởi 2 chuyên viên, bất đồng được phân xử | Đầy đủ mục bắt buộc, nguồn có thẩm quyền, sự kiện không nhân đôi, cách nêu mâu thuẫn, thời gian sửa |
| Temporal | Khoảng 60 cases publication/effective/ingest khác nhau; corrections và model version | Không lọt thông tin tương lai; tách public-knowable, system-knew, retrospective |
| Security | Bộ tấn công từ tài liệu, OCR, tool description, source spoofing; cross-tenant và egress | Tool/action/secret leak, đổi goal/đích, bypass source rights, attack success kèm benign utility |
| Reliability | Fault injection tại fetch, parse, checkpoint, model timeout, chain send/receipt; restart và cancel | Không mất goal được nhận, không duplicate purchase/tx, không báo success sai, reconcile đúng |
| Chain | Một bộ test vector canonicalization và một journey Fuji test thật | Verify gốc, tamper fail, version/supersede, sai chain/contract, pending receipt |

Tập gán nhãn phải tách khỏi tập sửa prompt và demo. Với voice, tách người nói giữa development và held-out; với doanh nghiệp, tách cả thời gian và nhóm pháp nhân liên quan khi cần. Nhãn tài chính có 2 người kiểm và adjudication; báo agreement trước adjudication. Không dùng LLM tự chấm tất cả rồi gọi là ground truth. Lưu quyền dữ liệu và protocol, không commit audio/hồ sơ thật nhạy cảm vào repo public.

## 3. Gates cần đăng ký trước thử nghiệm

Đề xuất các ngưỡng khởi điểm để nhóm và chuyên viên chốt lại sau feasibility spike; giữ nguyên khi chạy held-out. Các ngưỡng sau không lấy từ một bài báo để suy ra phù hợp Việt Nam.

| Gate | Mục tiêu đề xuất | Giới hạn diễn giải |
|---|---|---|
| Entity matching | Precision trên các kết nối hệ thống chấp nhận ≥99%; báo denominator, 95% CI và abstention coverage | Mẫu 200 vẫn không đủ chứng nhận sai số toàn quốc; đừng giảm coverage về 0 để đạt precision |
| Critical slots | ≥98% exact match trên nhóm được chấp nhận hoặc hỏi lại đúng trường | Đánh giá riêng tên, MST, số/ngày và dialect; không lấy WER thấp thay thế |
| Claim support | ≥95% claims thực chất có evidence hỗ trợ đúng entity/kỳ; 100% claims pháp lý bất lợi cần đường về nguồn và review | Tỷ lệ citation tồn tại không bằng citation support; thiếu source không xuất kết luận |
| Financial arithmetic | 100% đúng trong supported deterministic cases hoặc abstain khi thiếu input | Không bao hàm mọi PDF/OCR; chấm end-to-end extraction riêng |
| Temporal integrity | Không lỗi nhìn trước tương lai trong bộ gate đã đăng ký | Không bảo đảm tuyệt đối ngoài bộ thử; case unknown phải abstain |
| Security | Không known unauthorized critical effect/cross-tenant leak trong gate suite; đóng finding mức nghiêm trọng | Không gọi là chống injection tuyệt đối; báo red-team scope và residuals |
| UX latency | Warm first useful spoken reply p95 ≤3 giây trong network/device profile đã định | Là mục tiêu; model xhigh có thể làm fail. Tách câu xác nhận tiếp nhận khỏi kết quả có ích, không che độ chậm bằng lời đệm |
| Research latency | Dossier cached p95 ≤30 giây, uncached hẹp ≤5 phút với source plan rõ | Ngân sách thăm dò; nguồn chặn/scan lớn có thể làm partial. Không hứa mọi DN đạt |
| Human utility | Giảm median active review time ≥30% so baseline, không tăng lỗi trọng yếu | Pilot nhỏ chỉ là directional; task order counterbalanced, cùng source budget |

Nếu không đạt latency với cấu hình model người dùng chọn, đo và báo đúng, giảm scope hoặc điều chỉnh UX; không âm thầm đổi model. Không đưa tiến độ giả hay hidden reasoning vào voice.

## 4. Xếp hạng phải có bài kiểm tra riêng

V0 mặc định kiểm đúng **tín hiệu** với rubric do chuyên viên phê duyệt. Decision table tổng hợp giả có thể thử trong nghiên cứu; chỉ thử thứ tự ưu tiên áp dụng doanh nghiệp thật sau ý kiến pháp lý và chuyên viên theo use case cụ thể. Agreement với chuyên viên không phải accuracy vỡ nợ. Tạo labels cho từng sự kiện, severity/status, thời điểm còn hiệu lực, và `unrankable`. Kiểm theo nhóm niêm yết/tư nhân, ngành, quy mô, tỉnh và tuổi dữ liệu. Bài toán pháp lý, tín dụng và đầu tư có chi phí false positive khác nhau; không gộp một confusion matrix.

Khi có dữ liệu V1: cố định prediction time, horizon (ví dụ 12 tháng), outcome và quyền sử dụng. Các quan sát chưa theo dõi đủ không là negative; xử lý censoring và competing events. Tránh dùng EBIT cùng kỳ để dự đoán label định nghĩa trực tiếp bằng EBIT mà gọi là dự báo tương lai. Giữ negative sampling/population rates đúng khi đo calibration.

So baseline constant/base rate, rules, logistic, gradient boosting; graph features/GNN chỉ là ablation thêm. Chia thời gian train/validation/test, purge/embargo theo horizon và độ trễ công bố, kiểm nhóm công ty/mẫu lặp. Báo AUROC **và** PR-AUC/base rate, precision/recall ở workload thực, Brier/calibration curves, uncertainty, coverage/abstention và confidence intervals. Không chọn mô hình bằng AUROC cao nhất duy nhất, không dùng accuracy trong dữ liệu hiếm để hứa success. GNN hoặc LLM giải thích không tự chứng minh quan hệ nhân quả.

Historical financial facts có thể bị LLM biết từ pretraining; mô phỏng as-of chỉ chặn retrieval chưa đủ. Khi cần chứng minh năng lực dự báo, dùng holdout thời gian ngoài training mà có thể xác định, synthetic/controlled extraction tasks và model/data cutoffs được ghi; nếu không xác định được, không gọi phép thử là backtest sạch tuyệt đối.

## 5. Chi phí: tính theo workload

Không có báo giá data feed hoặc API pricing được xác nhận cho hợp đồng dự án; do đó không đưa một mức thuê Bloomberg/Fiin hoặc USD/phút như giá đã biết. Biến số lớn thường là quyền dữ liệu, rà soát và vận hành, không chỉ token. Đây là mô hình ngân sách để thay bằng giá thật:

`cost/goal = STT_minutes × rate_STT + TTS_chars_or_minutes × rate_TTS + input_tokens × rate_in + output_tokens × rate_out + retrieval/OCR + report_purchase + storage/egress + human_review`.

Không cộng cả tokens cache và uncached cùng giá; có model reasoning tokens thì tính theo usage thực. Theo dõi failed/retry cost, warm/wasted audio và nguồn có minimum contract. Đặt budget reservation/cap trước gọi; không tự mua dữ liệu khi vượt quyền hoặc quota.

Ví dụ **giả định workload, không phải ước lượng giá**: 100 dossiers/ngày, mỗi dossier 15 tài liệu × 2 MB → khoảng 3 GB raw/ngày, 90 GB/30 ngày nếu không dedup/retention. Hai phút thoại/goal → 200 phút thoại/ngày. Nếu mỗi goal dùng 20.000 input và 2.000 output tokens, tổng là 2 triệu input và 200.000 output/ngày, trước retries. Thay các hệ số bằng usage đo thật, retention có quyền và hợp đồng; embeddings, OCR images và bản version có thể tăng dung lượng.

Chiến lược scale: danh mục/lookup có quyền + incremental refresh, watchlist refresh theo mức quan tâm, hồ sơ sâu on-demand, dedup theo document hash/event và cache theo entity + as-of + source version + rights + policy. Cache không được lẫn tenant hoặc dùng lại bản hết quyền. Không chạy nhiều LLM agent điều tra toàn bộ danh mục mỗi ngày. Nếu không có bulk licence, không tuyên bố đã index toàn quốc.

## 6. Lịch làm tới hạn nộp

Mốc nội bộ 21/11 theo [Team1 UniHack](https://team1vn.xyz/); [Builder Hub](https://build.avax.network/events/596c0b2d-63aa-4cf0-a0ad-a7e34ddbfdc0) hiện ghi mốc và nơi nộp khác, cần xác nhận với ban tổ chức. Chia việc dưới đây cho nhóm tối đa ba người; tên vai trò là công việc, không yêu cầu thêm nhân sự.

| Thời gian, giờ Việt Nam | Việc ưu tiên | Kết quả review được / điều kiện qua |
|---|---|---|
| 02–04/11 | Chốt use case, mẫu hồ sơ và 2–3 nguồn có quyền; hỏi mentor độ phù hợp contract audit; spike voice | Bộ truy vấn, truth ledger nhỏ, quyền nguồn hoặc blocker rõ; ghi latency/slots trên máy thật |
| 05–08/11 | Entity IDs, ingestion/parse/citation, terminal dossier text, workflow checkpoint | Một lệnh text ra dossier; thiếu dữ liệu không bị đổi thành an toàn; restart tiếp tục đúng |
| 09–12/11 | Voice/casual router, sửa goal, streaming status; so sánh cohort; rules synthetic hoặc thứ tự trên DN thật sau review pháp lý và chuyên viên | End-to-end mic → dossier, ngắt/hủy/hỏi lại đúng; evidence drawer dùng được |
| 13–15/11 | Fuji registry, receipt/proof/tamper/supersede; quyền/egress/injection cases | Một phần on-chain chạy thật và chứng minh được; không đưa PII lên chain |
| 16–18/11 | Held-out eval, 2 người rà dossier, lỗi/scoring gaps, UI latency | Kết quả với denominators và failure cases; chưa đủ nguồn thì hạ claim/coverage |
| 19–20/11 | Freeze scope, chỉnh báo cáo/README, slides, video 2–3 phút, mở thử link incognito | Bộ nộp mọi link mở được, demo có fail case và limitation, không bịa thành tích |
| 21/11 trước 23:59 | Nộp sớm theo form, đọc lại submission receipt | Người dùng/team thực sự submit; repo hiện tại chưa phải bài đã nộp |
| 22–30/11 nếu vào final | Sửa lỗi cần thiết, rehearsal, mentor feedback | Không đổi trọng tâm hoặc hứa coverage chưa có |

Ba nhóm việc: (A) data/entity/evidence; (B) workflow/voice/terminal; (C) Fuji/evaluation/product. Giữ code ownership rõ nhưng review chéo; tránh cả ba cùng dựng framework agents mà chưa có một nguồn dữ liệu hợp lệ.

## 7. Demo 2–3 phút đề xuất

0:00–0:20: casual chat ngắn rồi giao điều tra có ngày chốt. 0:20–0:45: tên trùng → MIRA hỏi đúng MST, goal xuất hiện. 0:45–1:20: đọc checklist, mở nguồn và một mâu thuẫn/correction có ngày. 1:20–1:45: so sánh hai hồ sơ đủ dữ liệu, hồ sơ thứ ba thiếu nên `unrankable`. 1:45–2:15: kiểm commitment thật trên Fuji, sửa tài liệu làm verify fail. 2:15–2:45: hạn chế dữ liệu, không tư vấn mua/bán, hướng pilot. Có thể dùng corpus tổng hợp có ghi nhãn rõ để chứng minh temporal logic và security; phần nguồn thật chỉ trình diễn quyền/coverage thực sự có.

## 8. Điều kiện môi trường khi bắt đầu code

Không cài hoặc triển khai dịch vụ trong giai đoạn nghiên cứu. Trước khi dùng Node, xử lý cảnh báo môi trường về Node.js 24.18.0 và nâng ít nhất 24.22.1 theo kiểm tra bảo mật của phiên. Nếu chọn Vercel cho web UI, **rất nên cài Vercel CLI bằng `npm i -g vercel`** để agent dùng `vercel env pull`, `vercel deploy`, `vercel logs`; CLI chưa có trong phiên. Chỉ provision integrations sau khi chốt scope, quyền và ngân sách; backend worker/voice có thể chọn host riêng phù hợp vòng đời. Không có lý do phải chuyển toàn dự án sang Vercel để thực hiện nghiên cứu này.
