# Self-improving agents và theo dõi doanh nghiệp liên tục

**Ngày nghiên cứu: 02/11/2026.** Mục tiêu là xác định cách MIRA/LANTERN học từ quá trình làm việc và duy trì kho bằng chứng về doanh nghiệp Việt Nam. Đây là phương án nghiên cứu; chưa triển khai crawler, MongoDB hay vòng tự cải thiện. Bản ứng dụng hiện dùng Rust và SQLite, có tác vụ điều tra theo yêu cầu và rà soát chủ thể; không có kho lịch sử đầy đủ hoặc xếp hạng toàn quốc đã kiểm định.

## Kết luận

Nên xây hai vòng riêng. **LANTERN Scout** theo dõi nguồn, phát hiện thay đổi và cập nhật bằng chứng. **Evolution Lab** phân tích lỗi, đề xuất thay đổi cách tìm, trích xuất hoặc ghi nhớ, rồi so sánh phiên bản mới với phiên bản cũ trên bộ kiểm thử độc lập. Thu thập thêm tài liệu là cập nhật dữ liệu; chỉ gọi là tự cải thiện khi cách agent xử lý nhiệm vụ sau đó thực sự thay đổi và được đo tốt hơn.

Ưu tiên cải thiện bộ nhớ và prompt trước; chưa cần huấn luyện lại model hoặc để agent tự sửa production. Với dữ liệu doanh nghiệp, một kết quả sai pháp nhân hoặc sai thời điểm có thể gây hại hơn lợi ích của việc trả lời nhanh.

## Các nghiên cứu liên quan

Tìm kiếm có chủ đích bằng CoChat, đối chiếu ngày, tóm tắt, nội dung sẵn có và thông tin hội nghị với nguồn gốc đến ngày 02/11/2026. Đây không phải tổng quan hệ thống hoặc danh sách đầy đủ mọi bài mới. Một số bản CoChat bị cắt hoặc chỉ trả tóm tắt; khi có, bản HTML và thông tin hội nghị gốc được dùng để kiểm tra thêm. Những kết quả trên benchmark khác không chứng minh hiệu quả cho dữ liệu doanh nghiệp Việt Nam.

| Nghiên cứu | Bằng chứng và điều có thể áp dụng |
|---|---|
| [GEPA](https://proceedings.iclr.cc/paper_files/paper/2026/hash/0e9e708b6f48e14fd0ac29e167413f76-Abstract-Conference.html), ICLR 2026 | Phản ánh lỗi trong trace để tạo prompt mới rồi kiểm thử. Trên sáu tác vụ, bài báo cáo trung bình hơn GRPO 6 điểm phần trăm. Đây không phải mức tăng có thể hứa cho LANTERN; số rollout thấp hơn cũng không tự chứng minh hóa đơn thấp hơn. |
| [ReasoningBank](https://arxiv.org/abs/2509.25140), ICLR 2026 | Rút kinh nghiệm từ cả thành công và thất bại để truy xuất cho tác vụ sau mà không phải cập nhật weights. Có thể dùng cho kinh nghiệm xử lý nguồn, nhưng một nhận xét do model tự sinh không tự trở thành sự thật về doanh nghiệp. |
| [Darwin Gödel Machine](https://openreview.net/pdf?id=pUpzQZTvGY), ICLR 2026 | Tạo các phiên bản agent sửa code và kiểm tra bằng benchmark lập trình, trong sandbox có giám sát. Kết quả là bằng chứng trong thiết lập coding cụ thể; điểm trong vòng chọn không nên được mô tả như toàn bộ là đánh giá độc lập. |
| [AIDE²](https://arxiv.org/abs/2609.26457), preprint 22/09/2026 | Trong một lượt tự cải thiện tám ngày, nhóm tác giả báo cáo bảy thay đổi liên tiếp và kiểm tra chuyển giao trên bốn benchmark giữ riêng. Đây là thí nghiệm nghiên cứu AI, chưa cho thấy hiệu quả tương đương trên hồ sơ pháp lý Việt Nam. |
| [Reflective Prompt Optimization of a Deep-Research Agent](https://arxiv.org/abs/2610.12601), preprint 08/10/2026 | Trên 500 câu hỏi giữ riêng theo chủ đề, điểm tổng hợp về trích dẫn tăng từ 0,702 lên 0,781; 147 câu vẫn giảm. Điểm là thước đo của bài, không phải xác suất sự thật hay độ chính xác doanh nghiệp. Mẫu validation chỉ có 24 câu nên việc chọn phiên bản có thể nhiễu. |
| [Are Self-Evolving Agents Continual Learners?](https://arxiv.org/abs/2610.25858), preprint 19/10/2026 | Thử ba phương pháp trên bốn luồng tác vụ thay đổi; sửa bộ nhớ/prompt chung có thể làm quên kiến thức cũ, còn truy xuất theo độ giống có thể chọn nhầm kinh nghiệm. Cần giới hạn bộ nhớ theo nguồn, tác vụ, pháp nhân, thời điểm và phiên bản. |
| [Batching the Reflective Optimization Loop](https://arxiv.org/abs/2610.36774), preprint/workshop 27/10/2026 | Thử đánh giá nhiều đề xuất song song trên hai tác vụ; một số thiết lập nhanh hơn về thời gian chạy. Các thí nghiệm dùng hàng nghìn lượt gọi metric; không thể coi là miễn phí hoặc phù hợp ngay với ngân sách thử 5 USD. |
| [Search-Time Contamination](https://arxiv.org/abs/2606.05241), preprint 03/06/2026 | Agent tìm web có thể lấy nhầm đáp án benchmark công khai và làm điểm đánh giá cao giả. Thiết lập bài báo tập trung benchmark lâm sàng; bài học cho LANTERN là giữ bộ đề và đáp án kiểm tra riêng, không cho optimizer tìm lại đáp án. |
| [Auditing the Product, Not the Recursion](https://arxiv.org/abs/2610.27610), preprint 2026 | Khảo sát 114 nghiên cứu; trong 90 nghiên cứu tự nhận RSI, chỉ một phần nhỏ kiểm tra lặp lại toàn bộ vòng nhiều lần. Một phiên bản cuối tốt hơn chưa đủ chứng minh chính cơ chế tự cải thiện ổn định. |

File [RIS](self-improving-agents-2026.ris) chứa metadata để nhập Zotero. Plugin Zotero đã được kiểm tra bằng lệnh trạng thái: không tìm thấy profile và local API bị từ chối kết nối trong môi trường này. Chưa tìm trong thư viện hoặc đồng bộ record vào tài khoản Zotero.

## LANTERN Scout: agent theo dõi nguồn

Scout là một worker Rust chạy nền độc lập với cửa sổ chat. Muốn chạy 24/7 khi máy người dùng tắt, cần một máy chủ hoặc dịch vụ luôn hoạt động, cùng cấu hình, backup và ngân sách vận hành riêng. MIRA chỉ điều khiển và giải thích trạng thái; Scout không tự mua feed hoặc mở rộng ngân sách.

**Luồng:**

`Danh mục nguồn được phép → lịch/queue → lấy thay đổi → lưu phiên bản nguồn → nhận diện doanh nghiệp → trích xuất claims → rà soát → MongoDB → tín hiệu/ranking đủ điều kiện → cam kết Avalanche`

1. **Danh mục nguồn.** Mỗi nguồn có chủ sở hữu, quyền truy cập và tái sử dụng, địa chỉ được phép, định dạng, chu kỳ, hạn mức, chính sách xóa/sửa và người chịu trách nhiệm. Ưu tiên feed/API được phép, RSS hoặc sitemap; chỉ kiểm tra HTML theo điều kiện của từng nguồn. Lịch ví dụ: nhóm theo dõi hằng ngày và nhóm ít ưu tiên hằng tuần, nếu nguồn cho phép. Không gọi đó là thời gian thực hoặc SLA.
2. **Thu thập tăng dần.** Scheduler kiểm tra thay đổi bằng cơ chế nguồn hỗ trợ như ETag hoặc `Last-Modified`; chỉ tải và phân tích lại khi cần. Giới hạn request đồng thời, thời gian, kích thước, chi phí; tôn trọng `Retry-After`, dừng khi bị chặn và không vượt CAPTCHA. Lưu checkpoint, retry có giới hạn và hàng đợi lỗi; đối soát quét bù sau gián đoạn. Không giả định mỗi sự kiện chỉ đến một lần.
3. **Lưu và đối chiếu.** Lưu URL, nguồn, fingerprint/version, quyền, thời điểm công bố, thời điểm hệ thống ghi nhận và lần kiểm tra gần nhất. Bản gốc lớn chỉ lưu ở object storage nếu quyền cho phép; MongoDB giữ metadata và liên kết. Chuẩn hóa MST dưới dạng chuỗi, đối chiếu ID và tên; tên giống nhau không đủ để gộp. Tài liệu bị sửa hoặc rút cần trạng thái và chính sách xử lý/xóa phù hợp, không giữ vô thời hạn một cách mặc định.
4. **Tách dữ kiện khỏi kết luận.** Mỗi claim có đoạn/nguồn, pháp nhân ứng viên, loại sự kiện, thời điểm, trạng thái rà soát và phiên bản bộ trích xuất. Xác nhận nguồn nói về đúng chủ thể không đồng nghĩa xác nhận toàn bộ claim là đúng. Kết quả bất lợi và đối tượng mơ hồ cần kiểm tra trước khi ảnh hưởng điểm. Khi nguồn không hoạt động, báo **chưa kiểm tra được**, không kết luận không có rủi ro.
5. **Chống lệnh độc trong tài liệu.** Văn bản web/PDF là dữ liệu không tin cậy, không được đổi chỉ dẫn, quyền, quota, ví hay tập kiểm thử. Fetcher chỉ truy cập đích được phép, kiểm tra redirect/IP và chặn địa chỉ nội bộ; parser có giới hạn dung lượng và thời gian.

Nguồn khởi đầu có thể gồm [thông tin đăng ký doanh nghiệp](https://dangkykinhdoanh.gov.vn/vn/Pages/Staticpage.aspx?key=dvc), công bố của doanh nghiệp/HNX, thông tin đấu thầu và thông báo cơ quan có thẩm quyền trong phạm vi cho phép. [Điều khoản Cổng đăng ký doanh nghiệp](https://dangkykinhdoanh.gov.vn/vn/tin-tuc/603/1763/dieu-khoan-va-dieu-kien-su-dungcong-thong-tin-dang-ky-doanh-nghiep-quoc-gia.aspx) nêu hạn chế kinh doanh thông tin chưa được chấp thuận; việc xem công khai không tự cấp quyền crawl hàng loạt, lưu cache hay bán lại. [HNX InfoGate](https://hnx.vn/vi-vn/dich-vu-cctt/huong-dan-yeu-cau-ky-thuat-YCKT.html) là dịch vụ dữ liệu cần làm rõ hợp đồng, không mặc định là feed miễn phí.

## MongoDB, lịch sử và Avalanche

Các cluster độc lập cần một bộ định tuyến Rust: map `company_id` sang bucket ổn định và map bucket sang cluster. Theo dõi dung lượng/tải thực tế rồi di chuyển bucket có đối soát; hash không bảo đảm dữ liệu cân bằng tuyệt đối. Tìm kiếm tổng hợp phải nêu thiếu dữ liệu khi cluster không khả dụng. Ghi xuyên cluster cần outbox, idempotency và đối soát, không giả định có transaction ACID toàn cục. Nếu cần một cơ sở dữ liệu logic tự chia dữ liệu, cân nhắc Atlas sharding; đây là topology khác với nhiều cluster độc lập. Chưa provision hay chuyển dữ liệu hiện có.

Lưu riêng dữ liệu doanh nghiệp, phiên bản nguồn, claims, công việc và ký ức học tập. Ngày xảy ra, ngày công bố và ngày hệ thống biết là các trường khác nhau; nếu thiếu archive thì không hứa dựng lại lịch sử tại một ngày cũ. Bằng chứng không có quyền lưu phải theo chính sách nguồn; MongoDB không tự cấp quyền đó.

Sau khi một báo cáo đủ điều kiện được phát hành, có thể ký và ghi commitment lên Fuji để người nhận kiểm tra phiên bản và receipt. Chuỗi không xác minh nội dung; không đưa tài liệu, PII hoặc salt lên chain. Repo hiện mới có mã registry và kiểm tra cục bộ, chưa chứng minh contract đã triển khai hay giao dịch đã chạy.

## Evolution Lab và kiểm thử

1. Ghi trace có kiểm soát: câu hỏi, phiên bản cấu hình, nguồn, trạng thái lỗi, chi phí và kết quả được rà soát. Tách kinh nghiệm vận hành khỏi kho bằng chứng doanh nghiệp; giới hạn thời gian sống và phạm vi mỗi memory.
2. Gom lỗi thành giả thuyết: ví dụ nhầm pháp nhân, thiếu nguồn, sai kỳ hoặc trích dẫn không hỗ trợ claim. Tạo candidate thay đổi prompt, truy vấn, parser hoặc quy tắc tìm; không cho trang web ghi trực tiếp vào memory có quyền chỉ dẫn.
3. Kiểm thử trong sandbox bằng replay và bộ đề tách theo doanh nghiệp, thời gian và loại nguồn. So với baseline nhiều lần; bộ đáp án giữ riêng khỏi optimizer. Đo đúng pháp nhân, độ hỗ trợ claim bởi nguồn, lỗi thời gian, false positive, coverage/abstention, chi phí và latency. Có ca injection, nguồn lỗi và sửa/thu hồi thông tin.
4. Chạy shadow, sau đó rollout nhỏ nếu các ngưỡng được định trước vượt baseline mà không làm xấu các nhóm quan trọng; giữ version và rollback. Thay code production, quyền nguồn, ngân sách, evaluator, ví hoặc quy tắc kết luận bất lợi cần bước phê duyệt phù hợp. Không để agent tự sửa ngưỡng chấm của chính mình.

Các ngưỡng cụ thể phải dựa trên baseline và mẫu gán nhãn, không đặt số đẹp trước khi đo. Bộ chuẩn cần lấy mẫu độc lập, có chuyên gia rà và phân giải bất đồng; chỉ kiểm lại các claim agent tự tìm thấy sẽ bỏ sót false negatives.

## Xếp hạng và thứ tự triển khai

Trước hết cần xác định **rủi ro gì, trong khoảng thời gian nào và đối với nhóm doanh nghiệp nào**. Một tín hiệu thuế, kiện tụng, thanh khoản hay ngừng hoạt động không phải cùng một outcome. Theo từng nguồn, đo độ phủ và độ mới riêng; doanh nghiệp thiếu dữ liệu không tự trở thành ít rủi ro. Khi chưa có outcome và nhãn đủ tốt, hiển thị tín hiệu cùng bằng chứng thay vì điểm dự đoán. Chỉ xếp hạng nhóm đủ điều kiện sau kiểm định theo thời gian/ngành và rà phạm vi pháp lý; không hứa bao phủ mọi doanh nghiệp Việt Nam từ dữ liệu web công khai.

Thứ tự đề xuất: **(1)** chốt nguồn được phép và tạo bộ đánh giá có nhãn; **(2)** tạo Scout cho một vài nguồn, có queue, lịch và theo dõi gián đoạn; **(3)** đưa dữ liệu đã đối soát sang MongoDB và kiểm migration; **(4)** xây baseline tín hiệu, sau đó thử Evolution Lab ở chế độ shadow; **(5)** mở rộng cluster, ranking và kiểm chứng Fuji sau khi đáp ứng điều kiện dữ liệu, quyền và ngân sách. Các lượt thử model trước đây có hạn mức 5 USD; vận hành nền cần ngân sách riêng, không được suy ra là đã được cấp.
