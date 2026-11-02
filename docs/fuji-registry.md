# Registry trên Fuji

Trạng thái: contract đã được kiểm thử trên EVM cục bộ; chưa có bằng chứng triển khai Fuji hay receipt Fuji. Không sử dụng ví có tiền thật, không nhập khóa vào LANTERN và không đưa dữ liệu doanh nghiệp, salt, tên, MST, âm thanh hoặc điểm số lên chain.

## Kiểm thử cục bộ

Các lệnh sau cài công cụ vào thư mục build `target/contract-test`, không cài toàn cục. Chúng chạy mạng EVM nội bộ, không gọi faucet hay Fuji:

```powershell
npm install --prefix target/contract-test --no-save --ignore-scripts --no-audit --no-fund solc@0.8.24 ganache@7.9.2 ethers@6.15.0
New-Item -ItemType Directory -Force -Path target/contract-test/tmp | Out-Null
$env:TEMP = (Resolve-Path target/contract-test/tmp).Path
$env:TMP = $env:TEMP
node scripts/test-report-registry.cjs
```

Kiểm thử gồm hai issuer cùng digest, ngăn trùng trong một namespace, ngăn tham chiếu bản của issuer khác, supersession một nhánh và digest bằng 0. Đây không phải kiểm toán bảo mật độc lập.

Ngoài lựa chọn ví trình duyệt và Remix bên dưới, Windows có thể tạo ví Fuji dùng một lần bằng `node scripts/create-fuji-test-wallet.cjs` sau khi cài các dependency ở trên. Khóa được bảo vệ bằng DPAPI CurrentUser tại `target/contract-test`, nằm ngoài Git. Không dùng ví này cho tài sản thật. `node scripts/deploy-fuji-registry.cjs --max-gas-avax 0.02` chỉ chạy thử; thêm `--broadcast` chỉ sau khi có test AVAX và kiểm tra lại chain, ví và trần phí. Khi không chắc kết quả gửi, dùng hash đã in để kiểm tra trước khi thử lại.

## Ví testnet riêng đã chuẩn bị trong phiên này

Địa chỉ công khai: `0x8D3b3444Da6b6b4051a9B182A4E57C8Cb9729E7B`. Khóa của ví dùng một lần này được bảo vệ bằng Windows DPAPI CurrentUser trong `target/contract-test/fuji-test-wallet.dpapi`; file địa chỉ công khai cùng thư mục. Đây là dữ liệu cục bộ, không đưa thư mục `target` hay khóa vào repo và không gửi tài sản thật tới địa chỉ này. Nếu xóa thư mục hoặc đổi tài khoản Windows, ví này có thể không dùng tiếp được.

Khi chính người dùng đã cấp test AVAX cho địa chỉ trên, có thể chạy từ cùng tài khoản Windows, cùng các dependency ở mục trước:

```powershell
# Đọc chain, số dư và ước tính; không ký hay gửi.
node scripts/deploy-fuji-registry.cjs --max-gas-avax 0.02
# Chỉ chạy sau khi kiểm tra kết quả dry run và muốn deploy Fuji.
node scripts/deploy-fuji-registry.cjs --max-gas-avax 0.02 --broadcast
```

Script khóa RPC và chain ID Fuji, có trần tuyệt đối 0,05 test AVAX, kiểm số dư và phí tối đa, chỉ mở khóa trong bộ nhớ khi có `--broadcast`, in hash trước khi gửi và đối chiếu bytecode sau receipt. Nếu gửi bị gián đoạn, kiểm hash trong explorer trước khi thử lại; không tự phát lại. Khi kiểm tra ngày 02/11/2026, số dư ví là 0 test AVAX và faucet thay thế không tải danh sách mạng trong môi trường thử. Chưa gửi giao dịch.

## Chuẩn bị và triển khai testnet

1. Tạo **ví mới chỉ dành cho thử nghiệm**, không tái sử dụng ví hiện có hoặc chuyển tài sản thật. Chọn [Fuji C-Chain](https://build.avax.network/docs/primary-network), chain ID `43113` (`0xa869`), RPC `https://api.avax-test.network/ext/bc/C/rpc`. Kiểm lại tên mạng và chain ID trong ví trước mỗi giao dịch.
2. Nhận test AVAX cho địa chỉ C-Chain mới qua [faucet của Avalanche](https://build.avax.network/console/primary-network/faucet). Avalanche mô tả phương án [faucet thay thế](https://build.avax.network/academy/avalanche-l1/avalanche-fundamentals/04-creating-an-l1/02a-claim-testnet-tokens). Điều kiện tài khoản, CAPTCHA, giới hạn và số dư faucet có thể thay đổi; người dùng trực tiếp đáp ứng điều kiện, không vượt qua cơ chế bảo vệ.
3. Mở [Remix IDE](https://remix.ethereum.org/), tạo file `ReportVersionRegistry.sol` chứa chính xác mã tại `contracts/ReportVersionRegistry.sol`, biên dịch bằng Solidity `0.8.24` với optimizer tắt. Trong **Deploy & Run Transactions**, chọn môi trường ví trình duyệt, kiểm tra ví đang ở Fuji 43113 và có test AVAX, chọn `ReportVersionRegistry`, rồi xem và xác nhận giao dịch triển khai trong ví. Không nhập seed phrase hoặc private key vào Remix, LANTERN hay dòng lệnh.
4. Ghi lại địa chỉ contract, transaction hash và block; mở [Fuji explorer](https://explorer-test.avax.network/c-chain), kiểm tra receipt, địa chỉ và bytecode. Đối chiếu source đã biên dịch với bytecode đã deploy theo [hướng dẫn verify contract của Avalanche](https://build.avax.network/docs/primary-network/verify-contract/explorer). Một contract bất kỳ cũng có thể phát event giống registry, vì thế phải xác minh contract được cấu hình; RPC được chọn là một nguồn tin cần tin cậy.
5. Cấu hình `LANTERN_REGISTRY_ADDRESS` bằng địa chỉ registry đã kiểm tra, `LANTERN_FUJI_RPC_URL` bằng RPC Fuji, rồi khởi động lại server. Dùng báo cáo giả đủ điều kiện, mở **Kiểm chứng báo cáo → Chuẩn bị neo Fuji**, kiểm tra contract, chain, calldata và phí testnet trong ví trước khi ký. Sau đó kiểm tra hash bằng nút receipt và lưu link explorer. Chỉ khẳng định đã neo sau khi có giao dịch, receipt và event khớp, kèm thời điểm kiểm tra.

`anchor(bytes32,bytes32)` giữ cách gọi cũ: tham số thứ hai là 0 cho bản đầu. Muốn tạo giao dịch bản sửa qua API, thêm query `?supersedes=0x<64 ký tự hex của digest trước>` vào `POST /api/reports/{id}/transaction` và ký bằng **cùng ví** từng neo digest trước. UI hiện tại tạo bản đầu với giá trị 0; chức năng chọn bản trước chưa có trong UI. Chỉ một bản kế tiếp được nhận cho mỗi digest và issuer; issuer khác không thể chặn hoặc thay thế bản của ví này chỉ bằng cách sao chép digest.

Receipt do RPC trả và block canonical chỉ phản ánh quan sát tại lúc kiểm tra, không phải bằng chứng finality hay danh tính pháp nhân; bản lưu không tự kiểm lại chain. Salt ngẫu nhiên giảm đoán trực tiếp nhưng không bảo đảm ẩn danh. Việc supersede không xóa event cũ. Không dùng digest làm bằng chứng dữ liệu đúng, đầy đủ, hợp pháp hay doanh nghiệp an toàn.
