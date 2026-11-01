# Đề UniHack và những gì kế thừa từ NORA

Ngày đọc: 01/11/2026, múi giờ Asia/Saigon. URL người dùng gửi có dấu phẩy cuối; đã dùng địa chỉ chuẩn [team1vn.xyz](https://team1vn.xyz/). Web fetch thông thường không mở được; Firecrawl trả nội dung live thành công. Dưới đây chỉ ghi yêu cầu liên quan, không sao chép danh sách người tham gia.

## Đề yêu cầu gì

Trang công khai cho phép ý tưởng riêng và mang dự án có sẵn, kể cả web2, miễn có một phần thực sự chạy trên Avalanche: L1 riêng, Fuji hoặc C-Chain mainnet. Nhóm tối đa ba người; không bắt buộc chọn một trong 20 đề gợi ý. Lịch hiển thị 02–30/11/2026 nhưng **hạn nộp là 23:59 thứ Bảy 21/11**, công bố finalist 23/11, chung kết 30/11. Thời gian còn lại để nộp tính từ ngày nghiên cứu là khoảng 20 ngày, không phải 30 ngày phát triển trọn vẹn.

Hồ sơ nộp gồm GitHub repo, slides mở xem và video demo 2–3 phút. Repo có thể public hoặc private với quyền cho giám khảo được chỉ định; người dùng đã chọn public. Link không mở được có thể bị loại. Live app và contract/explorer có ô riêng trong biểu mẫu; việc có ô optional không xóa yêu cầu sản phẩm chạy trên Avalanche. Không tự nộp biểu mẫu hay liên hệ ban tổ chức.

| Tiêu chí công khai | Điểm | Cách thiết kế đề xuất đáp ứng |
|---|---:|---|
| Technical Implementation | 25 | Workflow có trạng thái, dữ liệu nguồn thật có quyền, phép kiểm chứng và crash recovery |
| Avalanche Integration | 25 | Ghi commitment phiên bản, xác minh receipt, phát hiện báo cáo bị sửa và supersession trên Fuji |
| Creativity & Innovation | 15 | Voice → nghiên cứu doanh nghiệp → bằng chứng theo thời điểm, phân biệt thiếu dữ liệu |
| Impact & Use Case | 15 | Giảm công rà soát đối tác/đầu tư, đo với chuyên viên và tác vụ cụ thể |
| Presentation & Pitching | 10 | Một câu hỏi, một điều tra có mâu thuẫn, một kết quả kiểm chứng được |
| UI/UX & Demo | 10 | Giọng nói ngắn, transcript rõ, evidence drawer, trạng thái chờ và hủy |

Nguồn yêu cầu và điểm: [FAQ, lịch và Judging Criteria trên trang cuộc thi](https://team1vn.xyz/). Các ánh xạ ở cột cuối là đề xuất, không phải bảo đảm được chấm bao nhiêu điểm. Audit registry phù hợp yêu cầu công khai có smart contract, nhưng mức độ thuyết phục của tích hợp còn cần phản hồi mentor; chưa có xác nhận chấp nhận ý tưởng cụ thể.

## Đã đọc project `zalo_hacks` như thế nào

Project được tìm qua danh sách project của Codex tại workspace NORA cục bộ, đối chiếu repo công khai [dakiemdarktharr/NoSleepClub_zalo_hacks](https://github.com/dakiemdarktharr/NoSleepClub_zalo_hacks). Đã đọc README, Cargo workspace, production integration, ADR-0006, validation và phần liên quan trong PRD/evaluation/workflow strategy. Không đọc secrets, dữ liệu người dùng hay chạy lại ứng dụng/benchmark. Git không cho đọc revision của thư mục bên ngoài workspace; vì vậy không gắn một commit SHA chưa xác nhận với snapshot tài liệu local đã đọc.

Các tài liệu này có chênh lệch thời điểm: `architecture.md` và một số strategy vẫn giữ phương án cũ. README/production integration/ADR-0006/validation ngày 01/11 là mô tả cập nhật hơn. Không lấy mô tả Tauri/Python cũ để nói NORA hiện chạy Tauri/LangGraph.

| Khía cạnh | Điều tài liệu project ghi nhận | Bài học cho MIRA / LANTERN VN |
|---|---|---|
| Client | Rust native eframe/egui, tray Windows, PTT; Cargo workspace 11 crates | Có nền tương tác và quản lý tài nguyên để xem xét tái dùng sau kiểm giấy phép và API |
| Voice | Silero → Whisper large-v3-turbo Q5 → LLM API → VieNeu FP32; tuần tự residency ASR/TTS | Cascade là baseline; chưa có bằng chứng full-duplex hay chất lượng live-mic đạt chuẩn |
| Goal | Goal/checkpoint, follow-up journal, effect ledger, khôi phục sau gián đoạn | Điều tra dài tách khỏi lượt nói; câu trả lời bổ sung phải đi đúng goal |
| Quyền | Typed broker, read/create/cloud-disclosure tách riêng, exact approvals | Phân quyền dữ liệu theo tenant/nguồn/mục đích; công cụ không được tự cấp quyền |
| Kiểm chứng | Readback/receipt/reconciliation, thông báo từ lifecycle event | LLM không tự ghi “đã tìm/đã xác minh/đã lên chuỗi” nếu thiếu receipt |
| Ranh giới hiện tại | Chưa có terminal doanh nghiệp, feed VN, risk labels hay pipeline pháp nhân | Domain/data engine phải xây mới; không gọi phần này là đã triển khai |

Nguồn project: [README](https://github.com/dakiemdarktharr/NoSleepClub_zalo_hacks/blob/main/README.md), [production integration](https://github.com/dakiemdarktharr/NoSleepClub_zalo_hacks/blob/main/docs/production-integration.md), [ADR-0006](https://github.com/dakiemdarktharr/NoSleepClub_zalo_hacks/blob/main/docs/adr/0006-integrated-rust-runtime.md), [validation](https://github.com/dakiemdarktharr/NoSleepClub_zalo_hacks/blob/main/docs/validation.md). Các URL `main` có thể thay đổi sau ngày đọc.

README ghi 188 Rust tests và một hành trình synthetic WAV 1,68 giây: final text khoảng 16,2 giây từ launch, playback drain khoảng 25 giây. Đây là **kết quả project tự ghi**, không phải kiểm chứng độc lập trong phiên này; không phải latency hội thoại 1–3 giây, WER hoặc bằng chứng con người nghe được. Tương tự, đo RAM trên một máy và audio tổng hợp không chứng nhận mọi laptop. Không cần lặp lại build/test để thực hiện nghiên cứu tài liệu.

## Tái sử dụng có chọn lọc

Giữ các contract về goal, checkpoint, quyền, receipt và update policy. Với demo hackathon, ưu tiên một web terminal có mic/text để giám khảo mở dễ; client NORA là adapter Windows giai đoạn kế tiếp, tránh đóng gói lại cả desktop chỉ để hiện bảng công ty. Một domain API chung cho phép hai client dùng cùng định danh, bằng chứng và workflow.

Nếu định hướng Windows-first là yêu cầu mới sau này, có thể dùng vỏ NORA ngay, nhưng vẫn phải đo live voice và kiểm lại nguồn dữ liệu/giấy phép. Dự án mới không kế thừa tự động quyền phân phối model, preset voice hay dữ liệu của cuộc thi khác.
