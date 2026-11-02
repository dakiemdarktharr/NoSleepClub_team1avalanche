# Scout và lưu trữ MongoDB

Scout lấy dữ liệu từ nguồn được khai báo trước, chạy nền trong tiến trình server và không cần model AI. Bật `LANTERN_SCOUT_ENABLED=true` trước lần chạy đầu, sau đó dùng nút trên giao diện để bật hoặc dừng lấy nguồn mới. Dữ liệu đã trong hàng đợi vẫn được tiếp tục lưu. Khi server tắt thì Scout cũng dừng. Hàng đợi, checkpoint và lịch được lưu trong `data/scout.db`; một worker bị ngắt không được tiếp tục ghi bằng lease cũ. Mỗi thư mục dữ liệu chỉ được một tiến trình LANTERN sử dụng tại một thời điểm. Thay đổi cấu hình môi trường sau khi policy đã tồn tại không âm thầm thay đổi trạng thái bật/tắt đã lưu.

Nguồn ban đầu là [GLEIF LEI API](https://www.gleif.org/en/lei-data/gleif-api/) với [điều khoản dữ liệu LEI CC0](https://www.gleif.org/en/meta/lei-data-terms-of-use/). Bộ lọc địa chỉ pháp lý Việt Nam và một trang mới cập nhật giúp minh họa workflow; nó không bao phủ mọi doanh nghiệp Việt Nam, không bảo đảm thu đủ mọi thay đổi, và LEI không phải mã số thuế. Giá trị `registeredAs` chỉ là một định danh đăng ký do nguồn cung cấp, không tự xác nhận là MST. Ứng viên giữ trạng thái chưa xác minh, không tự ghép với hồ sơ khác theo tên, và không tự suy ra rủi ro. Không tìm thấy thông tin xấu không chứng minh doanh nghiệp an toàn.

Không lưu trang gốc từ Scout; lưu metadata quan sát và hàng đợi ứng viên được trích xuất. Scout chặn URL ngoài danh sách cho phép, giới hạn kích cỡ, thời gian, khoảng cách và số yêu cầu theo origin; tôn trọng phản hồi từ chối và không vượt CAPTCHA. Quyền, tần suất và chính sách riêng của nguồn mới phải được kiểm tra trước khi bổ sung.

## Hai cluster

MongoDB là lớp lưu trữ tùy chọn cho dữ liệu Scout và các ứng viên/claim do điều tra MIRA nhập; SQLite vẫn cung cấp các tính năng cục bộ như hội thoại, điều tra và giao diện. Hàng đợi đồng bộ không gửi transcript chat, câu hỏi, tóm tắt hay phần query của URL. Claim do model đề xuất vẫn chưa được xác minh độc lập; nội dung claim có thể chứa thông tin nhạy cảm do model sinh ra, vì vậy chỉ dùng nguồn và yêu cầu phù hợp với nơi lưu trữ. Chuẩn bị **hai database `lantern` chuyên dụng, rỗng** trên hai cluster; tạo user giới hạn quyền trên database tương ứng, chỉ cho phép IP cần thiết và dùng TLS. Không dùng các project hay database của ứng dụng khác.

1. Bật `LANTERN_MONGO_ENABLED=true` và `LANTERN_MONGO_INIT_EMPTY=true` trong `.env.local`. Chỉ dùng `INIT_EMPTY=true` khi khởi tạo database mới đã được xác nhận rỗng.
2. Đặt `LANTERN_MONGO_URI_A` và `LANTERN_MONGO_URI_B` trong biến môi trường hoặc một file riêng không commit được trỏ bởi `LANTERN_MONGO_SECRETS_FILE`. Khóa quyền đọc file theo tài khoản chạy server. Không ghi credential vào ví dụ, log hay issue.
3. Khởi động và kiểm tra `GET /api/storage/health`. Khi cả hai cluster đã sẵn sàng, đổi `LANTERN_MONGO_INIT_EMPTY=false` trước lần chạy tiếp theo. Giữ an toàn `data/mongo-routing-v1.json` và `data/mongo-bootstrap-complete-v1.json` cùng dữ liệu liên quan; việc mất hoặc thay các file này có thể làm dữ liệu không đọc được hoặc không thể xác định trạng thái khởi tạo.
4. `GET /api/storage/candidates` đọc tối đa một giới hạn từ mỗi cluster, giữ riêng các phiên bản và đánh dấu `complete=false` nếu có lỗi hay cắt bớt. Đây không phải truy vấn phân trang đầy đủ.

Ứng viên được phân tuyến xác định theo 256 bucket cố định. Việc này có mục tiêu phân bố số khóa tương đối đồng đều, không bảo đảm dung lượng bằng nhau. Không có transaction xuyên cluster, tự động rebalance, chuyển dữ liệu sang cluster khác hay backup; bổ sung cluster cần migration có kiểm thử. Nếu cluster không sẵn sàng, Scout giữ outbox để thử lại, không báo đã ghi đủ. Nếu kết nối ban đầu thất bại, server thử kết nối lại có giới hạn; thay đổi file cấu hình vẫn cần khởi động lại. Không tùy tiện xóa pin hoặc cấu hình lại owner khi đang có dữ liệu.

Các phiên bản Mongo là lịch sử bất biến, không phản ánh việc người dùng rà soát hoặc loại bỏ nguồn sau khi một phiên bản đã gửi. Cần đối chiếu trạng thái hiện tại trong SQLite; không lấy bản mirror làm bằng chứng đã xác minh hoặc đầu vào xếp hạng. Bản ghi đã gửi trước khi người dùng loại không tự được thu hồi.

Atlas M0 có giới hạn tài nguyên; dịch vụ có thể phát sinh phí nếu bật chức năng trả phí ngoài phạm vi này. Việc IP máy thay đổi có thể cần cập nhật allowlist. Không đặt 0.0.0.0/0. Cấu hình này chưa thay thế quản trị sao lưu, phục hồi, an ninh hoặc hệ thống giám sát production.

## Kiểm tra

`cargo test --workspace --locked` chạy kiểm thử không cần nguồn ngoài hoặc cơ sở dữ liệu thật. `scripts/Smoke-Lantern.ps1` kiểm thử HTTP cục bộ không gọi model. Các endpoint Mongo/Scout có thể được kiểm tra riêng mà không chi tiền API model. Khởi chạy lại không tự phát lại request AI đã bị gián đoạn. Các tính năng voice và nghiên cứu có thể phát sinh phí theo cấu hình nhà cung cấp; sổ giới hạn mặc định là $5 và không thay thế hóa đơn.
