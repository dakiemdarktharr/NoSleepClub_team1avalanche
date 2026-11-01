# NoSleepClub_team1avalanche

**MIRA** là trợ lý thoại tiếng Việt vừa trò chuyện, vừa nhận mục tiêu nghiên cứu dài. **LANTERN VN** là terminal tra cứu và thẩm định doanh nghiệp theo bằng chứng mà MIRA sử dụng.

**Trạng thái ngày 01/11/2026: nghiên cứu và thiết kế; chưa có code sản phẩm, mô hình rủi ro đã kiểm định, nguồn dữ liệu được mua hay smart contract đã triển khai.** Tên sản phẩm là tên làm việc, chưa kiểm tra nhãn hiệu.

## Đọc báo cáo

- [Báo cáo và phương án đề xuất](REPORT.md): quyết định sản phẩm, dữ liệu, pháp lý, khả thi và thứ tự triển khai.
- [Kiến trúc và workflow](docs/architecture-workflows.md): casual chat, goal bền vững, tìm doanh nghiệp, nghiên cứu, as-of, so sánh và Avalanche.
- [Đánh giá, chi phí và lịch thực hiện](docs/evaluation-roadmap.md): tiêu chí nghiệm thu, thử nghiệm, ngân sách giả định và mốc 21/11.
- [Đọc đề và đối chiếu NORA](docs/brief-nora-review.md): yêu cầu UniHack và phần có thể kế thừa từ `zalo_hacks`.
- [Nghiên cứu học thuật](research/academic-evidence.md), [Bloomberg và nguồn dữ liệu](research/bloomberg-data.md), [pháp lý và khả thi](research/legal-feasibility.md).
- [Phương pháp, giới hạn công cụ và nguồn kiến trúc](research/methodology.md).
- [Thư mục tài liệu RIS cho Zotero](research/papers.ris): nhập bằng File → Import; xem ghi chú metadata trong từng bản ghi. Chưa đồng bộ vào tài khoản Zotero.

## Phương án ưu tiên

Một nền tảng bằng chứng có định danh MST, nguồn, thời điểm và mức độ thiếu dữ liệu; trợ lý thoại gọi các công cụ có kiểu dữ liệu và giữ tiến độ. Khởi đầu bằng hồ sơ doanh nghiệp cùng danh sách tín hiệu cần kiểm tra. Xếp hạng rủi ro chỉ mở sau khi xác định tiêu chí, tập so sánh, quyền dữ liệu, khả năng kiểm định và phạm vi pháp lý.

Không tìm thấy tin xấu **không** có nghĩa là ít rủi ro. Bằng chứng công khai không đủ để bảo đảm xếp hạng mọi doanh nghiệp Việt Nam. Risk ranking cũng không tự tạo ngoại lệ khỏi quy định về tư vấn chứng khoán, xếp hạng tín nhiệm hay thông tin tín dụng.

## Đề thi và phạm vi Avalanche

[Team1 Vietnam UniHack](https://team1vn.xyz/) chốt nộp **23:59 ngày 21/11/2026, giờ Việt Nam**, demo cuối ngày 30/11. Hướng đề xuất là một registry kiểm chứng phiên bản báo cáo trên **Avalanche Fuji**, dữ liệu chi tiết giữ ngoài chuỗi; hash chứng minh tính toàn vẹn của phiên bản, không chứng minh doanh nghiệp an toàn. Chưa triển khai hoặc gửi bài dự thi trong phiên nghiên cứu này.

## Nguồn gốc nghiên cứu

Đã đọc tài liệu và cấu trúc workspace của [NoSleepClub_zalo_hacks / NORA](https://github.com/dakiemdarktharr/NoSleepClub_zalo_hacks). Ba agent chuyên môn áp dụng `agency-agents`, dùng đúng **gpt-6.1-terra / xhigh** theo yêu cầu. CoChat được dùng để tìm và đọc bài; nguồn chính thức dùng để đối chiếu. Consensus đã được gọi nhưng hết hạn mức; Zotero không có plugin khả dụng trong phiên. Không coi hai dịch vụ này là đã hoàn thành nghiên cứu hay đồng bộ tài liệu.

Repo chỉ chứa tài liệu gốc, liên kết và metadata nghiên cứu. Không chứa dữ liệu doanh nghiệp thu thập hàng loạt, PDF có bản quyền, giọng nói cá nhân, model weights hoặc credentials. Việc repo public không cấp quyền sử dụng dữ liệu hay thương hiệu của bên thứ ba.

