# ADR 0001: Rust web/PWA làm giao diện ban đầu

**Trạng thái:** được chọn cho bản cục bộ trong phiên này; cần xác nhận build, thiết bị và đánh giá trước pilot. **Ngày:** 01/11/2026.

## Bối cảnh

Người dùng chọn web/PWA và muốn tối đa Rust, bao gồm giao diện. MIRA cần nhận hội thoại tiếng Việt và giao việc trong khi LANTERN trình bày hồ sơ, bằng chứng, ngày chốt và trạng thái tác vụ. Ưu tiên một bản chạy dễ kiểm tra, không giả dữ liệu hoặc quyền nguồn, và không bắt buộc một desktop wrapper để mở demo. Phạm vi hiện tại là máy cục bộ một người dùng, không phải dịch vụ nhiều tenant.

## Quyết định

Dùng Axum làm HTTP server và API; Maud render document/fragment trong Rust; SQLite lưu state; CSS, JS, icon, manifest và service worker được nhúng vào binary. Browser nhận HTML từ Rust. JavaScript mỏng chỉ dùng cho sự kiện, fetch, media, wallet và PWA: không dùng React, framework SPA, TypeScript hay npm runtime. Server đọc cấu hình NORA được chỉ định khi hợp lệ, giữ model/effort đã cấu hình, không gửi key xuống browser; âm thanh có cấu hình riêng. Ngân sách thử live 5 USD được quản lý như hạn mức đặt giữ cần đối soát, không phải hóa đơn nhà cung cấp.

Một source of truth là Rust store và snapshot được render lại, không duy trì state nghiệp vụ song song trong browser. Voice là push-to-talk, audio qua xác nhận người dùng và transcript có thể sửa; không tuyên bố full-duplex. Service worker chỉ cache asset tĩnh, không cache các route API hay hồ sơ riêng. `system_known` mô tả dữ liệu hệ thống đã lưu tại mốc, không chứng minh có thể tái dựng mọi dữ liệu công khai lúc đó.

Không đưa WASM compiler vào bản này: server-rendered Maud đã đáp ứng mục tiêu và tránh thêm pipeline/build/client-state trước khi đo nhu cầu. Đây là lựa chọn đơn giản hóa, **không phải kết quả benchmark rằng SSR nhanh hơn WASM**. Có thể đánh giá WASM sau nếu profiling chỉ ra logic client thực sự cần chạy cục bộ và lợi ích đủ lớn.

## Hệ quả và lựa chọn đã cân nhắc

| Phương án | Quyết định và đánh đổi |
|---|---|
| Axum + Maud + SQLite + JS mỏng | Một binary và ít thành phần, phần lớn logic được kiểm trong Rust; tương tác browser vẫn cần JS và mạng tới server cục bộ. SSR không tự bảo đảm accessibility hay độ trễ. SQLite phù hợp phạm vi local, cần chính sách backup và bảo vệ file. |
| SPA JS framework | Không chọn cho giai đoạn này: tăng toolchain và một lớp state cho mục tiêu Rust-first; có thể cân nhắc lại theo nhu cầu tương tác đã đo. |
| Rust/WASM client | Chưa chọn: thêm build target và tương tác async/client storage; cân nhắc khi cần compute offline thực sự và có phép đo. |
| Tauri/native wrapper | Tùy chọn về sau nếu cần phân phối desktop hoặc tích hợp OS sâu. Hiện là web/PWA, **không tuyên bố đã đóng gói desktop**. |
| Dịch vụ nhiều tenant, PostgreSQL/feed thương mại | Để sau: cần xác thực, phân quyền, phân tách dữ liệu, vận hành, quyền nguồn và đánh giá pháp lý; không mở server local như production service. |

Thiết kế nghiên cứu rộng hơn trong [architecture-workflows.md](../architecture-workflows.md) đề xuất worker tách riêng, PostgreSQL/object storage và các mode thời gian đầy đủ; đó là hướng nghiên cứu cho quy mô lớn, không mô tả những gì SQLite local đã cung cấp. Quyết định này ghi đè lựa chọn stack cho bản web/PWA cục bộ, không xóa các yêu cầu tương lai về workflow, provenance và quyền dữ liệu.

## Điều kiện thay đổi và kiểm chứng

Kiểm build/test Rust, UI smoke, bàn phím/screen reader, bảo mật loopback, trạng thái mất mạng và PWA trên thiết bị thực. Đo latency và chi phí; mục tiêu trong [lộ trình UI](../DEVELOPMENT_ROADMAP.md) là giả thuyết, không phải benchmark đã đạt. Neo Fuji chỉ được ghi là xác nhận sau receipt đúng mạng/contract/digest, với chữ ký bằng ví do người dùng điều khiển. Báo cáo hoặc digest không xác nhận sự thật của nguồn.

Xem xét ADR mới nếu yêu cầu offline có dữ liệu riêng, nhiều tenant, tích hợp OS sâu, nguồn được cấp phép, tải vượt khả năng SQLite/local, hoặc đo đạc chứng minh một mô hình render khác tốt hơn. Các thay đổi về scoring dự báo cần outcome/horizon, nhãn, calibration, temporal validation và review pháp lý riêng; không suy ra từ lựa chọn framework UI.
