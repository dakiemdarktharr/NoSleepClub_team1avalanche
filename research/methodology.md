# Phương pháp, công cụ và giới hạn

Ngày chốt nghiên cứu: **01/11/2026, Asia/Saigon**. Ưu tiên công bố 2025–2026 xuất hiện không muộn hơn ngày này; giữ công trình nền cũ khi cần. Đây là deep research có định hướng thiết kế, không phải systematic review đăng ký trước, kiểm toán dữ liệu toàn quốc hoặc ý kiến pháp lý. Không có số PRISMA screening/dedup đáng tin để công bố.

## Phân công và phương pháp chuyên môn

Áp dụng skill `agency-agents`, profile Research Synthesist cho truy nguồn, phân loại bằng chứng, mâu thuẫn và khoảng trống; Backend Architect cho contracts, reliability và lựa chọn hệ thống tối thiểu. Ba agent con `academic_evidence`, `terminal_data`, `legal_feasibility` được tạo với **model `gpt-6.1-terra`, reasoning `xhigh`**. Không tạo agent với model khác. Tổng hợp và biên tập tiếng Việt theo hướng dẫn local Turnitout Humanizer, giữ số liệu, giới hạn và liên kết; không chạy hoặc tuyên bố dùng engine Turnitout proprietary.

## Công cụ đã dùng và trạng thái

| Công cụ / nguồn | Đã làm | Giới hạn cần biết |
|---|---|---|
| Consensus | Gửi một yêu cầu `deep` bao quát tài chính/voice/RAG/temporal/risk/safety | Trả thông báo hết hạn mức miễn phí tháng; không có nghiên cứu hoàn tất, không retry sau thông báo |
| CoChat | Tìm nghiên cứu, fetch các bài được dựa vào; đối chiếu paper gốc | Có hit sai chủ đề, metadata peer-review không luôn đúng, một số full text bị cắt, một fetch không tìm thấy; ghi cụ thể trong evidence review |
| Zotero | Tìm plugin theo tên bằng Plugin Management | Không có kết quả plugin trong phiên; không truy cập hay đồng bộ Zotero account. RIS là deliverable tương thích import, không phải một thư viện đã sync |
| Web | Mở arXiv/ACL/publisher, luật, cổng dữ liệu và tài liệu chính thức | Một số trang timeout, scan không OCR, giá/quyền hợp đồng chưa biết; hạn chế cụ thể nằm ở từng phụ lục |
| Firecrawl | Đọc live trang Team1 khi web fetch thất bại | Chỉ dùng nội dung đề, không lấy danh sách người tham gia làm dữ liệu dự án |
| GitHub | Tìm NORA và tạo repo public, xuất tài liệu bằng connector | Không gửi bài dự thi, không liên hệ bên thứ ba, không mua dịch vụ |
| Project local | Đọc tài liệu NORA và Cargo workspace ở workspace NORA cục bộ | Không chạy benchmark lại hay đọc secrets; revision local chưa xác nhận |

Muốn dùng lại Consensus, hạn mức phải được khôi phục hoặc nâng qua [trang Consensus](https://consensus.app/continue?from=mcp&client=chatgpt&intent=upgrade) và người dùng yêu cầu thử lại. Không gắn nhãn bản báo cáo này là kết quả Consensus. Nếu người dùng có Zotero ở môi trường khác, nhập [papers.ris](papers.ris); đã bổ sung danh sách tác giả theo metadata nguồn gốc được kiểm, có DOI/URL và ghi chú cần giữ. Sau import vẫn nên kiểm cách Zotero nhận dạng ấn phẩm/preprint.

## Cách chọn nguồn

Tìm tiếng Anh cho academic/Bloomberg/voice và tiếng Việt cho doanh nghiệp/pháp lý; dùng nhiều cụm khác nhau, kiểm lại danh tính nguồn. Loại hit y tế hoặc không liên quan, nghiên cứu sau ngày chốt, citation chưa xác minh và tuyên bố không có bằng chứng. Peer-reviewed được đối chiếu ấn phẩm thay vì tin cờ database. Bài preprint vẫn có giá trị cho thiết kế thử nghiệm, với trọng số thấp hơn và không suy ra replication.

Nguồn luật/văn bản ban hành là bằng chứng về câu chữ, nhưng vẫn cần kiểm vòng đời sửa đổi tại ngày ra mắt. Trang sản phẩm Bloomberg/Fiin chỉ xác nhận mô tả của vendor, không phải kiểm chứng độc lập coverage hoặc quyền dùng. Benchmark tài chính SEC, dữ liệu Trung Quốc/Italy/Indonesia và nghiên cứu ASR không đại diện doanh nghiệp Việt Nam ngoài phạm vi mẫu. Kết quả cùng dataset không tính là nhiều kiểm chứng độc lập.

Không cố lấy paywalled text, vượt CAPTCHA/login, mua quota hay tạo datasets doanh nghiệp. Không lưu toàn văn paper hoặc dữ liệu bên thứ ba vào repo; dùng metadata, liên kết và diễn giải có giới hạn. Các nhận định “chưa tìm thấy/chưa xác minh” là giới hạn tìm kiếm, không khẳng định nguồn hoặc API không tồn tại.

## Phân biệt facts, observations, decisions và unknowns

- **Fact có nguồn:** hạn nộp công khai, văn bản pháp luật, cấu hình/metrics paper theo phương pháp được nêu.
- **Observation:** nội dung tài liệu NORA đã đọc, lỗi fetch/metadata và kết quả tool tại phiên; không khái quát thành tính chất vĩnh viễn của dịch vụ.
- **Decision đề xuất:** MIRA/LANTERN, cohort đầu, stack tối thiểu, audit registry, SLO và timeline; tất cả chưa được thực nghiệm sản phẩm xác nhận.
- **Unknown:** hợp đồng/giá/bulk access, mức bao phủ, outcome labels, WER live tiếng Việt, efficacy của risk ranking, phân loại pháp lý và nhu cầu trả tiền.

## Nguồn kiến trúc bổ sung

| Nguồn chính thức đã mở | Dùng cho nhận định |
|---|---|
| [LangGraph persistence](https://docs.langchain.com/oss/python/langgraph/persistence) và [functional API trong repo tài liệu chính chủ](https://github.com/langchain-ai/docs/blob/main/src/oss/langgraph/functional-api.mdx) | Checkpointer/store, replay và yêu cầu idempotency; không dùng memory saver làm persistence sau restart |
| [LiveKit turns](https://docs.livekit.io/agents/logic/turns/) | Phân biệt turn detection, endpointing, interruption và VAD; không chứng nhận tiếng Việt của dự án |
| [pgvector](https://github.com/pgvector/pgvector) | Vector retrieval kết hợp PostgreSQL full-text; thiết kế hybrid cần đo trên corpus thật |
| [Avalanche Primary Network](https://build.avax.network/docs/primary-network) | C-Chain hỗ trợ smart contract EVM; Fuji chain ID 43113 |
| [Team1 UniHack](https://team1vn.xyz/) | Deadline, hồ sơ nộp, quy mô nhóm và yêu cầu Avalanche |

Các phụ lục [học thuật](academic-evidence.md), [dữ liệu/Bloomberg](bloomberg-data.md) và [pháp lý](legal-feasibility.md) chứa liên kết trực tiếp gần từng nhận định. Không có bằng chứng nào trong tập này xác nhận sản phẩm đã đạt chất lượng terminal thương mại, dự báo được mọi doanh nghiệp hay tuân thủ pháp luật khi ra mắt.
