# NoSleepClub_team1avalanche

**MIRA** là trợ lý thoại tiếng Việt vừa trò chuyện, vừa nhận mục tiêu nghiên cứu dài. **LANTERN VN** là terminal tra cứu và thẩm định doanh nghiệp theo bằng chứng mà MIRA sử dụng.

**Trạng thái:** web/PWA cục bộ viết chủ yếu bằng Rust, có Scout hai nguồn miễn phí, sàng lọc rủi ro theo bằng chứng và vòng cải thiện cách tìm nguồn của MIRA. Đây là bản thử nghiệm; chưa có mô hình dự báo rủi ro đã kiểm định hoặc smart contract Fuji đã triển khai. Tên sản phẩm là tên làm việc, chưa kiểm tra nhãn hiệu.

## Chạy ứng dụng

Backend, workflow, SQLite và giao diện HTML dùng Rust/Axum/Maud; JavaScript mỏng kết nối tính năng trình duyệt. Không cần Node.js để chạy. Yêu cầu build: Rust 1.89+, Cargo và C compiler cho SQLite. Trên Windows:

```powershell
Copy-Item .env.example .env.local
# Sửa .env.local trên máy: chỉ định LANTERN_NORA_CONFIG_PATH hoặc LANTERN_* riêng.
./scripts/Start-Lantern.ps1 -Build
```

Mở [http://127.0.0.1:3210](http://127.0.0.1:3210) nếu trình duyệt không tự mở. Linux/macOS có thể chạy `cargo run --release -p lantern-server --locked`. Bản hiện tại chỉ bind loopback cho một người dùng; không đưa port trực tiếp lên Internet. PWA có thể cài khi trình duyệt hỗ trợ, nhưng vẫn cần server cục bộ cho dữ liệu và AI.

Giữ API key trong file riêng ngoài GitHub. Khi chọn cấu hình NORA, model và mức suy luận lấy từ file đã chọn; không tự đổi model. Nghiên cứu dùng trần output riêng mặc định 8.192 tokens, cấu hình được qua `LANTERN_RESEARCH_MAX_OUTPUT_TOKENS`. Ứng dụng chỉ mở mic khi người dùng yêu cầu. Nếu Scout được bật, nó gọi nguồn công khai theo lịch ngay cả khi không có cuộc trò chuyện.

## Các khả năng hiện có

- Tìm doanh nghiệp trong kho cục bộ không gọi AI; nút “Tìm bằng AI” tạo tác vụ web riêng có giới hạn ngân sách và lưu ứng viên **chưa xác minh**. Chọn đúng pháp nhân trước khi điều tra. Claim chỉ được nhập khi URL khớp metadata nguồn và nội dung claim nêu tên/MST ứng viên. Các điều kiện này chưa chứng minh chủ thể hay nội dung đúng. Người dùng cần đối chiếu nguồn và xác nhận đúng chủ thể; chỉ các nguồn đã được xác nhận và thuộc đúng tác vụ mới có thể vào báo cáo.
- Chat, điều tra nền, bằng chứng, watchlist và tiến trình được lưu trong SQLite. Tác vụ bị ngắt cần đối soát, không tự gửi lại request trả phí; hủy và nhập kết quả được bảo vệ bằng giao dịch.
- Ghi âm theo lựa chọn, gửi audio tới nhà cung cấp được cấu hình để phiên âm, sửa transcript trước khi gửi và đọc bằng TTS dịch vụ hoặc giọng thiết bị khi hỗ trợ. Server chỉ nhận WAV PCM16 mono tối đa 30 giây. Đây là push-to-talk, chưa phải hội thoại streaming toàn song công.
- Sàng lọc bốn nhóm pháp lý, thuế, tài chính và hoạt động theo đánh giá có nguồn của người rà soát, kèm điều kiện định danh. Xếp mức quan ngại 0–3 từ thấp đến cao trên toàn nhóm tối đa 1.000 hồ sơ trước khi phân trang; hồ sơ thiếu, cũ hoặc bị loại nguồn không được xếp hạng. Có nhật ký thu hồi và phiên bản bất biến. Đây không phải xác suất vỡ nợ hoặc khuyến nghị đầu tư; [phương pháp](docs/RISK_METHODOLOGY.md).
- Vòng cải thiện MIRA ghi kết quả tìm nguồn và phản hồi, đề xuất cấu hình từ tập quy tắc cố định, kiểm thử cục bộ, cho phép áp dụng và khôi phục. Khôi phục sẽ giữ lại đề xuất cũ đến khi có thêm lỗi mới. Cấu hình đang dùng được gắn vào từng lần nghiên cứu; không tự sửa code, ngân sách, nguồn hoặc tiêu chí rủi ro. Kiểm thử quy tắc chưa chứng minh model chính xác hơn; [cơ chế và giới hạn](docs/EVOLUTION.md).
- Xuất/kiểm chứng phiên bản báo cáo và chuẩn bị giao dịch Fuji. Muốn neo thật cần registry đã triển khai và ví; hash/receipt không chứng minh nội dung đúng hoặc danh tính tổ chức. Xem [hướng dẫn registry Fuji](docs/fuji-registry.md). Ngày quá khứ chưa có snapshot đầy đủ: hồ sơ và tác vụ được ẩn, không tạo điều tra mới cho ngày đó.
- Ngân sách cục bộ mặc định $5 với khoản đặt giữ trước request; giữ chỗ không phải hóa đơn. Có thể đổi giới hạn và đối chiếu từng khoản bằng màn Cài đặt.
- Scout chạy khi server hoạt động, với lịch, hạn mức, checkpoint và outbox bền vững riêng cho từng nguồn. GLEIF quét phân trang các LEI có địa chỉ pháp lý tại Việt Nam; Wikidata bổ sung mục doanh nghiệp, website và ticker cộng đồng. Cả hai dùng dữ liệu CC0, chưa xác minh MST và không tự gộp pháp nhân theo tên. Wikidata cần `LANTERN_WIKIDATA_CONTACT` là URL dự án hoặc liên hệ người vận hành rồi bật trong màn Tổng quan; [phạm vi nguồn miễn phí](docs/FREE_SOURCES.md).
- Có thể bật hai cluster MongoDB riêng và định tuyến theo 256 bucket cố định. Dữ liệu Scout cùng ứng viên và claim từ điều tra MIRA được ghi idempotent vào cluster sở hữu; SQLite vẫn là kho thao tác và trạng thái rà soát cục bộ. Đọc từ hai cluster phải báo rõ kết quả thiếu hoặc bị giới hạn. Cấu hình, khôi phục và giới hạn có trong [hướng dẫn vận hành](docs/SCOUT_MONGODB.md).

Chưa có feed được cấp phép phủ toàn quốc, giám sát watchlist tự động, mô hình dự báo được kiểm định, xác thực nhiều người dùng hay chứng nhận production. Xem [kế hoạch phát triển tiếp theo](docs/NEXT_DEVELOPMENT_PLAN.md), [roadmap từng màn hình](docs/DEVELOPMENT_ROADMAP.md) và [ADR Rust web/PWA](docs/adr/0001-rust-web-pwa.md).

TTS mặc định cần được chuyển sang workflow mới trước **06/01/2027** theo [lịch ngừng model của OpenAI](https://developers.openai.com/api/docs/deprecations); xem [kết quả kiểm thử và giới hạn](docs/VALIDATION.md).

Kiểm thử: `cargo test --workspace --locked`; khi server đang chạy, `./scripts/Smoke-Lantern.ps1` kiểm tra route và bảo vệ cục bộ mà không gọi AI. Kiểm thử live dùng API có thể phát sinh phí. Nếu chọn Vercel cho các phần phù hợp sau này, nên cài CLI bằng `npm i -g vercel` để dùng `vercel env pull`, `vercel deploy` và `vercel logs`; ứng dụng cục bộ không phụ thuộc CLI đó.

## Đọc báo cáo

- [Báo cáo và phương án đề xuất](REPORT.md): quyết định sản phẩm, dữ liệu, pháp lý, khả thi và thứ tự triển khai.
- [Kiến trúc và workflow](docs/architecture-workflows.md): casual chat, goal bền vững, tìm doanh nghiệp, nghiên cứu, as-of, so sánh và Avalanche.
- [Đánh giá, chi phí và lịch thực hiện](docs/evaluation-roadmap.md): tiêu chí nghiệm thu, thử nghiệm, ngân sách giả định và mốc 21/11.
- [Đọc đề và đối chiếu NORA](docs/brief-nora-review.md): yêu cầu UniHack và phần có thể kế thừa từ `zalo_hacks`.
- [Nghiên cứu học thuật](research/academic-evidence.md), [Bloomberg và nguồn dữ liệu](research/bloomberg-data.md), [pháp lý và khả thi](research/legal-feasibility.md).
- [Self-improvement và agent theo dõi liên tục](research/self-improvement-and-continuous-monitoring.md), cùng [tài liệu RIS](research/self-improving-agents-2026.ris).
- [Phương pháp, giới hạn công cụ và nguồn kiến trúc](research/methodology.md).
- [Thư mục tài liệu RIS cho Zotero](research/papers.ris): nhập bằng File → Import; xem ghi chú metadata trong từng bản ghi. Chưa đồng bộ vào tài khoản Zotero.

## Phương án ưu tiên

Một nền tảng bằng chứng có định danh MST, nguồn, thời điểm và mức độ thiếu dữ liệu; trợ lý thoại gọi các công cụ có kiểu dữ liệu và giữ tiến độ. Sàng lọc hiện tại dùng tiêu chí công khai và đánh giá của người rà soát. Muốn dự báo rủi ro cần tập outcome và nhãn độc lập, kiểm định theo thời gian/ngành và rà phạm vi pháp lý.

Không tìm thấy tin xấu **không** có nghĩa là ít rủi ro. Bằng chứng công khai không đủ để bảo đảm xếp hạng mọi doanh nghiệp Việt Nam. Risk ranking cũng không tự tạo ngoại lệ khỏi quy định về tư vấn chứng khoán, xếp hạng tín nhiệm hay thông tin tín dụng.

## Đề thi và phạm vi Avalanche

[Team1 Vietnam UniHack](https://team1vn.xyz/) ghi hạn nộp **23:59 ngày 21/11/2026**, trong khi [Avalanche Builder Hub](https://build.avax.network/events/596c0b2d-63aa-4cf0-a0ad-a7e34ddbfdc0) ghi cổng đóng **05:59 ngày 25/11/2026**, cùng giờ Việt Nam; thông tin nơi nộp cũng khác nhau. Dùng 21/11 làm mốc nội bộ và xác nhận lại với ban tổ chức. Hướng đề xuất là registry kiểm chứng phiên bản báo cáo trên **Avalanche Fuji**, dữ liệu chi tiết giữ ngoài chuỗi; hash chứng minh tính toàn vẹn, không chứng minh doanh nghiệp an toàn. Chưa triển khai contract hoặc gửi bài dự thi.

## Nguồn gốc nghiên cứu

Đã đọc tài liệu và cấu trúc workspace của [NoSleepClub_zalo_hacks / NORA](https://github.com/dakiemdarktharr/NoSleepClub_zalo_hacks). Ba agent chuyên môn áp dụng `agency-agents`, dùng đúng **gpt-6.1-terra / xhigh** theo yêu cầu. CoChat được dùng để tìm và đọc bài; nguồn chính thức dùng để đối chiếu. Consensus đã được gọi nhưng hết hạn mức. Plugin Zotero đã được kiểm tra nhưng chưa truy cập được profile/API cục bộ; các file RIS chưa được đồng bộ vào thư viện.

Repo chứa mã nguồn, tài liệu gốc, liên kết và metadata nghiên cứu. Không chứa dữ liệu doanh nghiệp thu thập hàng loạt, PDF có bản quyền, giọng nói cá nhân, model weights hoặc credentials. Mã/tài liệu gốc theo [MIT](LICENSE); xem [thông báo bên thứ ba](THIRD_PARTY_NOTICES.md). Việc repo public không cấp quyền sử dụng dữ liệu hay thương hiệu của bên thứ ba.
