# Nguồn tham khảo và thành phần bên thứ ba

- [NoSleepClub_zalo_hacks / NORA](https://github.com/dakiemdarktharr/NoSleepClub_zalo_hacks) — ý tưởng goal/checkpoint, tách quyền, receipt và cách đọc cấu hình NORA là nền tham khảo cho dự án; mã nguồn NORA được công bố theo MIT, Copyright (c) 2026 NoSleepClub contributors. Không sao chép model weights, audio, credentials hoặc dữ liệu riêng.
- Các Rust crates được pin tại `Cargo.lock`; mỗi crate có giấy phép riêng. `rusqlite` sử dụng SQLite thông qua `libsqlite3-sys`; SQLite được phát hành theo chính sách public domain của dự án SQLite. Bản Windows cục bộ có thể liên kết với SQLite từ MSYS2 nếu compiler trong môi trường đó không build được bản bundled.
- OpenAI APIs là dịch vụ bên ngoài theo điều khoản và giá của nhà cung cấp, không phải phần mã hoặc dữ liệu nằm trong repo. Audio model và quyền dữ liệu phải được người vận hành kiểm tra theo phiên bản hiện hành.
- `ReportVersionRegistry.sol` là mã gốc của dự án, không sao chép bytecode hoặc dữ liệu của Bloomberg. Avalanche và Bloomberg là thương hiệu của chủ sở hữu tương ứng.

Danh sách này là notice khởi đầu, không phải SBOM hoặc báo cáo xác nhận đầy đủ quyền phân phối binary, dữ liệu và model. Phải làm inventory/notice export cho build release chính xác trước khi phát hành rộng.
