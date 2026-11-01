# Kết quả kiểm thử bản cục bộ

Ngày 01/11/2026, trên máy Windows của phiên phát triển. Đây là kết quả cho bản thử nghiệm một người dùng, không phải chứng nhận production hoặc đánh giá độ chính xác toàn quốc.

- `cargo test --workspace --locked --offline`: 38 test đạt; gồm ranh giới host/origin, hủy và nhập liệu nguyên tử, ngân sách, nguồn theo từng tác vụ, ngày quá khứ, review chủ thể, sửa báo cáo và kiểm hash.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: đạt. Script smoke kiểm tám màn hình, asset/PWA, header, request khác origin, WAV không hợp lệ và điều kiện xuất báo cáo; không gọi dịch vụ trả phí.
- Chat thực dùng cấu hình NORA đã chọn; tra cứu FPT có trả một ứng viên và một lượt điều tra trả hai nguồn gợi ý. Một URL trong số đó trả 404 khi công cụ đọc nguồn thử truy cập; URL còn lại có nội dung được công cụ tìm kiếm lập chỉ mục nhưng tài liệu PDF đầy đủ vượt giới hạn đọc của công cụ. Các kết quả vẫn chưa được chuyên viên xác minh; nguồn được rà đúng chủ thể trước khi vào báo cáo. Một lượt thử trước đó gắn sai nguồn do thiếu ngữ cảnh pháp nhân đã được cách ly, sửa nguyên nhân và bổ sung kiểm thử; không dùng kết quả đó làm bằng chứng.
- Một file TTS tiếng Việt tổng hợp dài 2,448 giây đã được chuyển về WAV mono và phiên âm thành đúng câu thử; phiên âm cũng đã thử lại thành công với `gpt-transcribe`. Đây là mẫu tổng hợp duy nhất, không chứng minh chất lượng micro, giọng địa phương, thời gian phản hồi hay khả năng hội thoại toàn song công.
- Đo tham khảo build release với dữ liệu cục bộ nhỏ, 40 request warm `/ui?view=overview` qua loopback: p50 4,19 ms; p95 27,38 ms. Không gồm network, web search, model hoặc thao tác trình duyệt. Đây không phải benchmark tải nhiều người dùng.
- Chưa có hợp đồng Fuji đã triển khai, giao dịch thật, feed trả phí, dữ liệu bao phủ toàn quốc, xếp hạng đã kiểm định, thử nghiệm với người dùng hoặc audit accessibility và bảo mật độc lập.

Hạn mức thử nghiệm là 5 USD. Sau kiểm thử, sổ cục bộ còn 0,24 USD khả dụng, phần còn lại gồm khoản giữ và mức dự phòng ước tính. Đây không phải hóa đơn thực; cần đối chiếu nhà cung cấp trước khi giải phóng khoản giữ hoặc kết luận chi phí.

## Bảo trì voice

Mặc định phiên âm dùng `gpt-transcribe`. TTS đang dùng `gpt-4o-mini-tts` qua Speech API và cần được thay bằng workflow Realtime trước ngày 06/01/2027 theo [lịch ngừng model của OpenAI](https://developers.openai.com/api/docs/deprecations). Không tự đổi model chat hoặc reasoning effort trong cấu hình NORA. Các model âm thanh có thể cấu hình riêng, nhưng một tên model khác chỉ dùng được nếu tương thích endpoint tương ứng.
