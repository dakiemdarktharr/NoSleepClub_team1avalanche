# Kết quả kiểm thử bản cục bộ

Ngày 01–02/11/2026, trên máy Windows của phiên phát triển. Đây là kết quả cho bản thử nghiệm một người dùng, không phải chứng nhận production hoặc đánh giá độ chính xác toàn quốc.

## Mở rộng rủi ro, Evolution và Scout, ngày 02/11/2026

- `cargo test --workspace --locked --offline`: **116 test đạt**. `cargo fmt --all -- --check`, Clippy với `-D warnings` và kiểm tra cú pháp JavaScript: đạt. Các test gồm cutoff và ngày Việt Nam, thiếu nguồn/định danh, thu hồi đánh giá, snapshot bất biến, nguồn bị loại, giới hạn và phân trang cohort, lease/quota/outbox, vòng feedback–evaluate–promote–rollback và cách ly quy tắc sau rollback.
- Kiểm thử HTTP với 1.001 hồ sơ giả: xếp toàn nhóm trước khi chia trang; hồ sơ đủ điều kiện ở vị trí 999 vẫn lên đầu. Nhóm giới hạn 1.000 có cờ báo phần chưa xét; snapshot lưu cả nhóm, API từ chối yêu cầu vượt giới hạn. Không gọi model, ngân sách không đổi.
- Playwright trên dữ liệu giả tách riêng, 205 hồ sơ: mở chín màn hình; tìm hồ sơ ngoài trang đầu; theo dõi/bỏ theo dõi; so sánh; thêm và rà soát nguồn; định danh và đánh giá đủ bốn chiều; lưu snapshot, loại nguồn và đối chiếu bản cũ. Đã kiểm tra phản hồi, đề xuất, áp dụng và khôi phục cấu hình; browser Back, phân trang, giữ bản nháp, ngày quá khứ/tương lai, xuất và xác minh báo cáo, từ chối bản sửa nội dung và điều kiện thiếu registry Fuji.
- Trình duyệt màn hình rộng và 390 px: sửa tràn ngang thanh tìm kiếm, kiểm tra menu/MIRA đóng mở và focus. Mô phỏng MediaRecorder hoàn tất đồng bộ để kiểm tra Escape: dừng track, hủy bản ghi, không gửi request phiên âm. Đây là kiểm tra điều khiển micro bằng mock; **chưa kiểm tra micro thật hoặc hội thoại AI thật trong đợt này**.
- Mẫu đo cục bộ 10 lượt mỗi endpoint trên bộ dữ liệu giả: `/api/revision` trả 79 byte, p50 10 ms, cao nhất 40 ms; `/api/state?view=risk` khoảng 235 KB, p50 61 ms, cao nhất 73 ms. Poll đã chuyển sang revision để giảm dữ liệu tải định kỳ. Các số này chỉ mô tả mẫu Windows cục bộ, không phải SLO hoặc kiểm thử tải.
- Wikidata miễn phí đã chạy qua nút bật nguồn, nhận một trang 100 binding và nhập 96 QID khác nhau; cursor tăng lên 1, hàng đợi về 0, cả hai cluster báo đã kết nối. Kho cục bộ có 197 hồ sơ sau lượt này, gồm dữ liệu trước đó; không phải 197 pháp nhân đã xác minh. GLEIF tiếp tục có lịch riêng. Phân trang có thể bỏ sót/lặp khi nguồn thay đổi, không phải snapshot đầy đủ toàn quốc.
- Không gọi thêm API AI trả phí, không mua feed. Sổ ngân sách vẫn có trần $5, $4,75 giữ chỗ, $0,01 ước tính đã ghi, $0,24 khả dụng. Đây là sổ kiểm soát cục bộ, chưa đối soát hóa đơn nhà cung cấp.

Kiểm thử này không chứng minh mức rủi ro đúng, agent cải thiện chất lượng, độ phủ toàn quốc, khả năng chịu tải, failover/restore hay an toàn production. Các luồng đã kiểm tra nêu trên có giới hạn dữ liệu và môi trường; chưa có bộ nhãn độc lập hoặc audit bên ngoài. Fuji vẫn chưa triển khai do chưa có test AVAX.

## Kết quả trước đợt mở rộng trên

### Scout và hai cluster, ngày 02/11/2026

- `cargo test --workspace --locked --offline`: 86 test đạt. `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: đạt trên môi trường Windows phát triển.
- Đã kiểm tra API GLEIF trực tiếp bằng bộ lọc địa chỉ pháp lý Việt Nam, sắp theo ngày cập nhật; Scout nhập một trang 100 hồ sơ LEI và hoàn tất hàng đợi. Đây là mẫu nguồn giới hạn, không phải độ phủ toàn quốc hay xác nhận MST.
- Hai Atlas M0 trong hai project riêng cùng báo health sẵn sàng. API đọc lại đủ 100 bản ghi trên cả hai cluster; theo bảng định tuyến cố định, 50 khóa thuộc A và 50 thuộc B trong mẫu này. Kết quả này không bảo đảm dung lượng cân bằng với tập khác, không phải thử nghiệm failover hay khôi phục backup.
- Sau khi bổ sung outbox điều tra và khởi động lại, hai bản ghi nghiên cứu hiện có được đồng bộ; pending về 0. Đây là xác nhận tích hợp nhỏ, không phải kiểm thử tải hay xác minh nội dung claim.
- Smoke HTTP cục bộ qua các màn hình, asset, chặn request khác origin và kiểm soát ngân sách; không gọi AI trả phí. Đã mở giao diện bằng Playwright ở màn hình rộng và hẹp, tìm trong kho cục bộ mà không thay đổi ngân sách, và không thấy lỗi JavaScript ở console.
- Registry biên dịch và chạy kiểm thử trên EVM cục bộ; dry run tới Fuji xác nhận chain ID 43113 nhưng ví thử nghiệm có 0 AVAX, nên không ký hay gửi giao dịch. Chưa có contract hoặc receipt Fuji đã triển khai.

### Bản ứng dụng ban đầu, ngày 01/11/2026

- `cargo test --workspace --locked --offline`: 38 test đạt; gồm ranh giới host/origin, hủy và nhập liệu nguyên tử, ngân sách, nguồn theo từng tác vụ, ngày quá khứ, review chủ thể, sửa báo cáo và kiểm hash.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: đạt. Script smoke kiểm tám màn hình, asset/PWA, header, request khác origin, WAV không hợp lệ và điều kiện xuất báo cáo; không gọi dịch vụ trả phí.
- Chat thực dùng cấu hình NORA đã chọn; tra cứu FPT có trả một ứng viên và một lượt điều tra trả hai nguồn gợi ý. Một URL trong số đó trả 404 khi công cụ đọc nguồn thử truy cập; URL còn lại có nội dung được công cụ tìm kiếm lập chỉ mục nhưng tài liệu PDF đầy đủ vượt giới hạn đọc của công cụ. Các kết quả vẫn chưa được chuyên viên xác minh; nguồn được rà đúng chủ thể trước khi vào báo cáo. Một lượt thử trước đó gắn sai nguồn do thiếu ngữ cảnh pháp nhân đã được cách ly, sửa nguyên nhân và bổ sung kiểm thử; không dùng kết quả đó làm bằng chứng.
- Một file TTS tiếng Việt tổng hợp dài 2,448 giây đã được chuyển về WAV mono và phiên âm thành đúng câu thử; phiên âm cũng đã thử lại thành công với `gpt-transcribe`. Đây là mẫu tổng hợp duy nhất, không chứng minh chất lượng micro, giọng địa phương, thời gian phản hồi hay khả năng hội thoại toàn song công.
- Đo tham khảo build release với dữ liệu cục bộ nhỏ, 40 request warm `/ui?view=overview` qua loopback: p50 4,19 ms; p95 27,38 ms. Không gồm network, web search, model hoặc thao tác trình duyệt. Đây không phải benchmark tải nhiều người dùng.
- Chưa có hợp đồng Fuji đã triển khai, giao dịch thật, feed trả phí, dữ liệu bao phủ toàn quốc, xếp hạng đã kiểm định, thử nghiệm với người dùng hoặc audit accessibility và bảo mật độc lập.

Hạn mức thử nghiệm là 5 USD. Sau kiểm thử, sổ cục bộ còn 0,24 USD khả dụng, phần còn lại gồm khoản giữ và mức dự phòng ước tính. Đây không phải hóa đơn thực; cần đối chiếu nhà cung cấp trước khi giải phóng khoản giữ hoặc kết luận chi phí.

## Bảo trì voice

Mặc định phiên âm dùng `gpt-transcribe`. TTS đang dùng `gpt-4o-mini-tts` qua Speech API và cần được thay bằng workflow Realtime trước ngày 06/01/2027 theo [lịch ngừng model của OpenAI](https://developers.openai.com/api/docs/deprecations). Không tự đổi model chat hoặc reasoning effort trong cấu hình NORA. Các model âm thanh có thể cấu hình riêng, nhưng một tên model khác chỉ dùng được nếu tương thích endpoint tương ứng.
