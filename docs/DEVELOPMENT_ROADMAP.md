# Lộ trình giao diện MIRA + LANTERN VN

Ngày 01/11/2026. Tài liệu này mô tả giao diện web/PWA Rust đã được xây trong phiên và các bước hoàn thiện. **Có mã không đồng nghĩa đã qua nghiệm thu, có nguồn dữ liệu thương mại hay sẵn sàng vận hành production.** Tham chiếu [ADR kiến trúc](adr/0001-rust-web-pwa.md), [thiết kế workflow nghiên cứu](architecture-workflows.md) và [kế hoạch đánh giá](evaluation-roadmap.md). Tài liệu workflow trước đây là thiết kế định hướng rộng; khi mô tả bản chạy hiện tại, ADR và mã Rust trong repo là căn cứ.

## Mục tiêu và ranh giới

Một người dùng cục bộ có thể trò chuyện bằng tiếng Việt, tìm đúng doanh nghiệp, giao điều tra có ngày chốt, xem tiến trình, đọc bằng chứng và tải hoặc kiểm chứng phiên bản báo cáo. Thông tin chưa xác minh được nêu rõ; thiếu dữ liệu không được trình bày là an toàn. Không có feed doanh nghiệp được cấp phép toàn quốc, mô hình dự báo được kiểm định, xếp hạng rủi ro quốc gia hay cam kết Fuji đã xác nhận sẵn.

Bản hiện tại là một tiến trình Rust phục vụ trên loopback: Axum nhận request và trả HTML Maud, SQLite lưu trạng thái cục bộ, tài nguyên giao diện được nhúng trong binary. JavaScript chỉ kết nối các API trình duyệt và gửi request; không có React, Node/npm runtime hay framework giao diện JS. Model và reasoning effort đọc từ cấu hình NORA được chỉ định hoặc cấu hình máy chủ hợp lệ; không phơi khóa trong giao diện, không tự thay model khi lỗi. ASR/TTS dùng cấu hình âm thanh riêng. Mức **5 USD là hạn mức thử live do người dùng đặt**, không phải báo giá provider hoặc bảo đảm tuyệt đối tổng hóa đơn; cần đối soát các khoản giữ chỗ và usage thực tế.

## Hành trình nghiệm thu

1. Mở ứng dụng trên địa chỉ loopback. Thanh điều hướng, ô tìm kiếm, ngày chốt và MIRA hoạt động bằng bàn phím; không có dữ liệu thì hiển thị hướng dẫn thay vì số liệu giả.
2. Chọn ngày chốt **dữ liệu hệ thống đã biết**, nhập tên hoặc MST. Việc tìm có thể tạo tác vụ nền; giao diện cho biết đã nhận, cập nhật kết quả và cho mở từng ứng viên. Người dùng tự xác nhận đúng pháp nhân, không tự gộp chỉ vì trùng tên.
3. Mở hồ sơ, kiểm tra thông tin và nguồn, giao câu hỏi điều tra; quan sát trạng thái, tóm tắt AI chưa được chuyên viên xác minh, bằng chứng và thời điểm ghi nhận. Đối chiếu từng nguồn với doanh nghiệp đã chọn; xác nhận chủ thể chỉ mở điều kiện báo cáo, không chứng nhận nội dung là đúng. Nguồn không sẵn hoặc thiếu bằng chứng vẫn được thể hiện là thiếu.
4. Chọn từ hai đến bốn hồ sơ đã có để so sánh các trường thực có; thêm/bỏ theo dõi. Không tự xếp thứ hạng hoặc nội suy các trường thiếu.
5. Nhắn câu xã giao hoặc yêu cầu tiếp theo trong MIRA. Với voice, đồng ý gửi audio đến nhà cung cấp được hiển thị, ghi ngắn, xem và sửa transcript trước khi gửi. Dừng phát tiếng không hủy tác vụ; hủy tác vụ là thao tác riêng.
6. Với báo cáo đủ điều kiện, mở kiểm chứng, xem digest và trạng thái; xuất JSON, kiểm bản xuất và thử bản bị sửa. Phép kiểm tính toàn vẹn không xác nhận nội dung đúng. Neo Fuji chỉ bắt đầu sau thao tác rõ ràng của người dùng: chuẩn bị, kết nối/chọn mạng, kiểm tra và xác nhận trong ví; chỉ báo receipt đã xác minh sau kiểm tra server.
7. Đóng/mở lại ứng dụng và đối chiếu trạng thái đã lưu, tác vụ chưa hoàn tất, thông điệp lỗi và ngân sách. Kiểm tra cả trường hợp AI, nguồn, mạng hoặc ví không khả dụng.

## Ma trận màn hình và công việc còn lại

Trạng thái “có mã” chỉ nghĩa là có luồng hoặc màn hình tương ứng; chạy được và chất lượng thực tế phải được xác nhận trong build/test cuối. “Phụ thuộc” là điều kiện bên ngoài, không được tự mô phỏng thành thành công.

| Khu vực | Bản hiện tại và trạng thái cần thấy | Hoàn thiện / điều kiện |
|---|---|---|
| Shell, điều hướng | Maud render thanh lệnh, rail, vùng làm việc và dock MIRA; `Ctrl/Cmd+K` đưa focus tới tìm kiếm; responsive có menu/dock. | Kiểm keyboard, focus sau điều hướng và thu nhỏ; kiểm trên Chrome, Edge, mobile và mức zoom 200%. |
| Ngày chốt | Ngày hiện tại hiển thị dữ liệu đang có. Với ngày quá khứ, ẩn hồ sơ và tác vụ, không tạo nghiên cứu mới; hội thoại và danh mục nguồn vẫn là hiện tại. | Cần lưu phiên bản pháp nhân, bằng chứng và review theo thời gian, kiểm công bố/ghi nhận muộn, correction và múi giờ. Public-knowable/retrospective cần archive và metadata riêng trước khi mở. |
| Tìm kiếm / danh sách | Gửi truy vấn tên/MST thành tác vụ; hiển thị kết quả trong state, định danh chưa chắc chắn và lỗi. | Nguồn web phụ thuộc model/cấu hình và quyền truy cập; cần kiểm thử tên trùng, đổi tên, chi nhánh, alias, false merge, nguồn và quota. Không cam kết bao phủ toàn quốc. |
| Hồ sơ doanh nghiệp | Hiện nhận diện, trường có dữ liệu, nguồn, câu hỏi điều tra, bằng chứng, diễn tiến tác vụ và các chiều cần xem xét. Link nguồn mở riêng, không suy điểm rủi ro. | Gắn trang/đoạn, phiên bản nguồn và mâu thuẫn khi backend có; phân biệt thời gian sự kiện pháp lý với log workflow, tóm tắt AI với kết luận được chuyên viên rà. |
| So sánh | Chọn hồ sơ đã có, đối chiếu tên, MST, định danh, ngành và địa phương; ghi “chưa có” với trường thiếu. | Mở rộng sang kỳ, đơn vị, hợp nhất/riêng lẻ và nguồn tương đương sau khi có dữ liệu; không biến thiếu dữ liệu thành thứ tự an toàn. |
| Theo dõi | Thêm/bỏ theo dõi và hiển thị hồ sơ đã lưu. | Quy định chính sách làm mới và thông báo khi có nguồn/nhịp cập nhật thực; hiện không hứa giám sát tự động liên tục. |
| Tác vụ và kết quả | Tiến trình có trạng thái lưu trữ; hủy riêng với âm thanh; thông tin thay đổi được lấy lại từ server. | Kiểm restart, timeout, partial, hủy khi request đang chạy, độ trễ cập nhật và đối soát phí. Không biến “đã gửi” thành “đã hoàn thành”. |
| MIRA casual + voice | Ô chat, ghi âm sau đồng ý, transcript sửa trước gửi, TTS dịch vụ theo thao tác và giọng thiết bị có nhãn riêng. Giới hạn bản ghi và phát âm thanh tách khỏi hủy việc. | Đây là push-to-talk, chưa chứng minh full-duplex, streaming hay barge-in. Cần corpus người bản ngữ, consent, WER/critical slots, độ trễ và các ca chuyển ngữ cảnh. |
| Cam kết / kiểm chứng / Fuji | Tạo cam kết cục bộ cho báo cáo đủ điều kiện, xuất JSON và kiểm nội dung với commitment và digest đang mở; giao diện chuẩn bị transaction, kết nối ví, gửi và kiểm receipt theo từng bước. | Neo thật phụ thuộc registry đã triển khai, Fuji RPC, ví, test AVAX và receipt khớp. Hash cục bộ không phải neo; receipt không chứng nhận sự thật hay danh tính pháp nhân. Có hash đang chờ thì kiểm lại, không tự gửi lặp. |
| Nguồn | Danh mục nguồn và link tham khảo. | Danh mục không phải API/feed hợp đồng; rà quyền ingest, hiển thị, cache, export, gửi đến model và mức độ cập nhật trước pilot. |
| Cài đặt, riêng tư, ngân sách | Hiện trạng thái cấu hình không lộ key, lựa chọn voice, giới hạn/đặt giữ ngân sách và điều kiện cài PWA. | Làm rõ consent, thời gian lưu, xóa/export dữ liệu và đối soát phí; ẩn hoặc vô hiệu thao tác không hỗ trợ, báo lý do. Không đưa raw audio, secrets hoặc PII lên chain. |
| Lỗi, loading, empty | Có thông báo lỗi, trạng thái trống và nhãn dữ liệu thiếu. | Chuẩn hóa pending/skeleton, retry an toàn, tránh mất bản nháp, focus sau lỗi, sự kiện chạy nền và phân biệt lỗi xác thực với không có kết quả. Không retry tự động thao tác trả phí/giao dịch. |
| PWA / offline | Manifest, icon và service worker chỉ cache các asset tĩnh cho shell. Không cache API, hồ sơ hay hội thoại. | Kiểm cài đặt thực trên trình duyệt hỗ trợ, update cache và xóa cache. **Chưa có offline dossier, tìm kiếm hoặc đồng bộ offline**; nếu server tắt, UI không hứa truy cập dữ liệu. |
| Accessibility và responsive | Có semantic landmarks, label, focus-visible, reduced motion, responsive drawer và bàn phím cơ bản. | Rà WCAG với axe, screen reader và người dùng thật; kiểm dialog focus/ESC, trạng thái live, contrast, text scaling, touch target và mobile viewport. Chưa tuyên bố đạt chuẩn khi chưa audit. |

## Nguồn trạng thái và hợp đồng giao diện

`GET /` trả document; `GET /ui` trả fragment Maud gồm nội dung và MIRA; `GET /api/state` trả snapshot. `companies`, `evidence`, `investigations`, `events`, `messages`, `sources` và `config` do Rust server/store quyết định; browser không tự tạo thông tin doanh nghiệp hay kết quả nghiên cứu. Route tham số `view`, `selected`, `ids`, `q`, `as_of` điều khiển màn hình và snapshot; ngày được gắn rõ `system_known`.

Các thao tác gửi qua các route Rust: `/api/search`, `/api/investigations`, `/api/chat`, `/api/watchlist`, `/api/jobs/{id}/cancel`, `/api/evidence/{id}/review`, `/api/transcribe`, `/api/speech` và nhóm `/api/reports/{id}/{export,commitment,verify,transaction,anchor}`. Liên kết ứng viên và bằng chứng được lưu theo từng tác vụ; dữ liệu cũ thiếu liên kết không tự suy đoán để đưa vào báo cáo. Các thao tác lỗi hiển thị lỗi từ server; không tự đổi model hay tự retry giao dịch. Trình duyệt không giữ API key hay private key. SQLite giữ các record theo chính sách của bản cục bộ; browser giữ bản nháp trong phiên và asset tĩnh, transaction hash chờ receipt được giữ trong session để người dùng thử kiểm tra lại. Không giả định storage là bất biến mãi hoặc phù hợp nhiều tenant.

Theo dõi nguồn duy nhất cho UI: [lantern-ui](../crates/lantern-ui/src/lib.rs) render, [server](../crates/lantern-server/src/main.rs) định tuyến và ghép snapshot, [core](../crates/lantern-core/src/lib.rs) quản lý state; adapter nhà cung cấp nằm trong [connectors](../crates/lantern-connectors/src/lib.rs). Khi thay schema, cập nhật producer, renderer và test cùng lúc. Không trộn dữ liệu quan sát được với placeholder demo.

## Thứ tự hoàn thiện

1. **Ổn định lát cắt cục bộ:** build sạch, lint/test, smoke toàn bộ route, test SQLite và lifecycle; sửa mọi lỗi làm mất tác vụ, sai entity hoặc lộ key. Ghi hạn chế bản chạy sau khi đo, không dựa vào danh sách tính năng.
2. **Trải nghiệm bằng chứng:** kiểm tìm kiếm, ứng viên trùng, ngày chốt, nguồn và hồ sơ/so sánh; bổ sung trạng thái loading, failure và các chiều chưa có dữ liệu. Cần dataset tổng hợp có truth ledger trước khi dùng dữ liệu thật.
3. **Voice và MIRA:** kiểm quyền mic, file WAV, dừng, bản chép, giao việc, TTS, cấu hình NORA và ngân sách; đo tiếng Việt trên thiết bị thật. Cải thiện từng điều kiện đã đo trước khi xem xét full-duplex.
4. **Kiểm chứng:** test canonicalization, tamper, digest khác, contract/chain sai, chờ receipt và khôi phục hash. Chạy Fuji thật chỉ sau khi có contract và ví do người dùng cấu hình; lưu bằng chứng transaction/receipt, không mô tả local commitment thành neo.
5. **Pilot giới hạn:** kiểm quyền nguồn, chuyên viên rà các kết luận và lỗi adverse, kiểm an ninh/cài đặt/PWA. Giữ scope doanh nghiệp và nguồn có quyền; chuẩn bị bộ nộp theo [lịch và phép đo](evaluation-roadmap.md).
6. **Sau pilot:** external deployment, phân quyền nhiều người/tenant, feed hợp đồng, retention và giám sát chỉ sau thiết kế, kiểm thử và phê duyệt vận hành riêng. Predictive risk scoring là dự án có outcome/horizon/nhãn, calibration, temporal holdout và đánh giá pháp lý riêng, không phải bước bật một công tắc giao diện.

## Ngân sách hiệu năng và cửa phát hành

Các mức sau là **mục tiêu đề xuất, chưa phải số đo hiện tại**. Ghi thiết bị, trình duyệt, mạng, model, cache và cỡ dữ liệu khi đo; báo p50/p95 cùng lỗi và timeout. Khi chưa có dữ liệu không đưa “0 ms” hay “đã đạt”.

| Phép đo | Mục tiêu đề xuất | Kiểm tra |
|---|---|---|
| Điều hướng UI cục bộ | p95 dưới 500 ms cho fragment từ store nhỏ | Đo riêng server/render/network; không gộp thời gian nghiên cứu mạng. |
| Voice | Phản hồi giọng có ích đầu tiên p95 ≤3 giây khi warm | Đo STT/model/TTS riêng; ngưỡng chỉ mang tính thử nghiệm, cấu hình xhigh có thể không đạt. |
| Dossier | Cache p95 ≤30 giây; tác vụ hẹp chưa cache ≤5 phút | Nêu nguồn bị chặn, tài liệu lớn và partial; không hứa cho mọi doanh nghiệp. |
| Nghiệp vụ | Không lỗi sai entity/kỳ hoặc mất tác vụ trong bộ gate đăng ký | Mỗi phép thử báo mẫu số; không suy ra tỷ lệ toàn quốc. |

Trước demo, chạy test Rust unit/integration; UI smoke các màn hình, keyboard và mobile; luồng lỗi API, thiếu key, thiếu ngân sách, nguồn rỗng, restart/hủy; WAV 30 giây/5 MB và consent; JSON gốc/sửa/digest khác; wallet từ chối, chain sai, pending và receipt khớp. Không dùng live call nếu chưa có hạn mức và sự cho phép phù hợp. Báo kết quả thật, thiết bị và lỗi còn lại. Trước pilot thật, thêm review dữ liệu/pháp lý, threat model, quyền nguồn, backup/khôi phục, accessibility audit và thẩm định chuyên viên; không dùng kết quả demo như chứng nhận production.
