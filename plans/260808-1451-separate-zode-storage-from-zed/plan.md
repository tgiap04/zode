---
title: 'Tách nơi lưu cấu hình và dữ liệu của zode khỏi Zed'
description: >-
  Đổi bốn hàm gốc trong crates/paths sang zode/Zode, tách định danh
  single-instance trên macOS, bắc cầu ZODE_* → ZED_* tại một chỗ, và đổi tên
  lệnh CLI. Mục tiêu: cài song song Zed và zode mà hai app không giẫm lên nhau.
status: completed
priority: P1
effort: 1-2d
branch: feat/separate-zode-storage
tags:
  - paths
  - fork
  - config
  - platform
blockedBy: []
blocks: []
work_type: feature
spec_waived: >-
  SDD mode tắt trong project này (takumi.sddMode: off), theo tiền lệ hai plan
  260726-1531 và 260807-1300. Yêu cầu đã chốt trọn trong
  plans/reports/brainstorm-260808-separate-config-from-zed.md
created: 2026-08-08
---

# Tách nơi lưu cấu hình và dữ liệu của zode khỏi Zed

**Thiết kế đã sealed** tại [`plans/reports/brainstorm-260808-separate-config-from-zed.md`](../reports/brainstorm-260808-separate-config-from-zed.md).
Đọc file đó trước. **Không re-litigate** bốn quyết định đã chốt.

## Phạm vi

Đổi **4 hàm** trong `crates/paths`, **2 hằng** trong `mac_only_instance.rs`, thêm **1 cầu nối env**
ở đầu `main()`, và đổi **tên lệnh CLI**. Không đụng `release_channel` (đã fork xong), không đụng
`script/bundle-*`, không đụng CI, không đụng `.zed_server`.

## Các phase

| # | Phase | Trạng thái | Phụ thuộc | Sở hữu file |
|---|-------|-----------|-----------|-------------|
| 01 | [Bốn thư mục gốc](phase-01-path-roots.md) | ✅ done | — | `crates/paths/src/paths.rs` |
| 02 | [Định danh single-instance macOS](phase-02-macos-instance-identity.md) | ✅ done | — | `crates/zed/src/zed/mac_only_instance.rs` |
| 03 | [Cầu nối ZODE_* → ZED_*](phase-03-env-var-bridge.md) | ✅ done | — | `crates/zed_env_vars/`, `crates/zed/src/main.rs` |
| 04 | [Tên lệnh CLI](phase-04-cli-command-name.md) | ✅ done | — | `crates/cli/src/main.rs` |
| 05 | [Kiểm chứng song song](phase-05-parallel-verification.md) | ⚠ một phần | 01–04 | không sở hữu file |

**Phase 01–04 chạy song song được** — bốn nhóm file rời nhau, không cái nào cần output của cái nào.

## Sửa lại một chỗ biên bản nói chưa đủ chính xác

Biên bản viết như thể single-instance hỏng chung trên mọi nền tảng. Rà lại từng cái thì **ba nền
tảng cho ba câu trả lời khác nhau**:

| Nền tảng | Cơ chế | Tình trạng |
|---|---|---|
| **macOS** | Cổng TCP + chuỗi bắt tay, hằng số cứng (`mac_only_instance.rs:33-36, 74-78`) | **Hỏng thật** — trùng hệt Zed. Phase 02 xử |
| **Linux/FreeBSD** | Unix socket `data_dir().join("zed-{channel}.sock")` (`open_listener.rs:293`) | **Tự khỏi nhờ phase 01** — đổi `data_dir()` là socket tách theo |
| **Windows** | Mutex + named pipe dẫn từ `app_identifier()` (`windows_only_instance.rs:34, 67`) | **Vốn đã an toàn** — `app_identifier()` đã là `Zode-Editor-*` |

Nên phase 02 chỉ động tới macOS. Tên file socket Linux vẫn là `zed-*.sock` nhưng nằm trong thư mục
đã tách, nên không va chạm — đổi tên file là việc thẩm mỹ, để ngoài phạm vi.

## Một chỗ lệch khỏi biên bản, có lý do

Biên bản chốt "đọc kép tại **một accessor**". Làm đúng chữ đó nghĩa là sửa **30 call site** để
chúng gọi accessor mới — tức 30 điểm conflict khi rebase, đúng thứ mà quyết định này sinh ra để
tránh.

Phase 03 giữ **nguyên ngữ nghĩa** nhưng đổi cơ chế: bắc cầu **một lần ở đầu `main()`** — quét
`ZODE_<X>` trong môi trường, gán sang `ZED_<X>`. Mọi chỗ đọc hiện có không phải sửa một dòng nào.
Chi tiết và cái giá phải trả (`set_var` là `unsafe` ở edition 2024) nằm trong
[phase 03](phase-03-env-var-bridge.md).

## Phụ thuộc chéo giữa các plan

Rà ba plan đang mở. `260726-1531-remove-auth-cloud-hard-fork` có nhắc `ZED_STATELESS` (dùng làm cờ
khi kiểm tra khởi động, phase 09/11) và `ZED_RPC_URL` (nằm trong danh sách xoá).

→ **Không có phụ thuộc ngữ nghĩa.** Cầu nối phase 03 là cộng thêm, `ZED_STATELESS` vẫn chạy y
nguyên. `blockedBy`/`blocks` để trống.

**Một rủi ro merge cần canh:** plan đó xoá 53 crate gồm auto-update và crash reporting. `paths.rs`
có `crashes_dir()` và `crashes_retired_dir()`. Nếu plan kia chạy trước, hai hàm đó có thể biến mất
— phase 01 phải xử lý cái còn tồn tại lúc thi công, không bám cứng danh sách.

## Rủi ro

| Rủi ro | Mức | Đối phó |
|---|---|---|
| **Người dùng mất thiết lập hiện có.** `~/.config/zed/settings.json` (`mode: dark`) và `~/.config/zed/themes/vscode-2026.json` **không tự chuyển**. Đây là hệ quả cố ý của quyết định "bắt đầu trắng", không phải lỗi | **Cao** | Phase 05 đưa đúng một câu lệnh chép tay và bắt buộc nói với người dùng **trước** khi họ chạy bản mới |
| `paths.rs` và `mac_only_instance.rs` là code upstream Zed → điểm conflict vĩnh viễn khi rebase | Trung bình | Chấp nhận. Bốn hàm + hai hằng, ghi vào changelog để nhớ |
| `set_custom_data_dir()` **panic** nếu gọi sau `config_dir()`/`data_dir()` (`paths.rs:69-71`) | Trung bình | Phase 01 không đổi thứ tự khởi tạo; phase 05 có bước chạy với `--user-data-dir` |
| Cầu nối env chạy **sau** khi một `LazyLock` nào đó đã đọc env → cầu vô tác dụng, âm thầm | Trung bình | Phase 03 đặt cầu là **câu lệnh đầu tiên** của `main()`, kèm test khoá thứ tự |
| `ZED_TERM` bơm vào terminal tích hợp (`terminal.rs:124`) — script người dùng có thể bám vào | Thấp | Không đổi. Cầu nối chỉ cộng thêm |
| Lỗi cổng trùng **không lộ trên kênh dev** (`main.rs:330-334` bỏ qua kiểm tra) | Trung bình | Phase 02 test thẳng hàm `address()`/`instance_handshake()`, không qua đường khởi động |

## Định nghĩa hoàn thành

- `./script/clippy` sạch (**không** dùng `cargo clippy`); `cargo test -p paths -p workspace -p zode` xanh
- Không đường dẫn nào zode đọc/ghi còn chứa `Zed`/`zed` — trừ `.zed_server`/`.zed_wsl_server` (ngoài phạm vi)
- Sửa `~/.config/zed/settings.json` **không** ảnh hưởng zode, và ngược lại
- Trên bản **stable**: mở Zed rồi mở zode → **cả hai cùng chạy**, không cái nào in `zed is already running`
- `ZODE_STATELESS=1` có tác dụng; `ZED_STATELESS=1` vẫn có tác dụng; đặt cả hai thì `ZODE_` thắng
- `zode --help` không còn tự xưng là Zed

## Kết quả thực thi (2026-08-08)

Phase 01–04 xong. Phase 05 xong phần đo được; một mục **không** kiểm được, nói rõ bên dưới.

### Bốn chỗ chệch khỏi bản vẽ, đều có lý do

1. **Bản vẽ nói "bốn gốc", thi công thấy năm — và bản vẽ đã tự sửa trước khi cắt.**
   `logs_dir()` trên macOS đi thẳng `~/Library/Logs/Zed`, cộng hai tên file `Zed.log`.
   Tổng cộng 16 chuỗi trong `paths.rs`, không phải 12.

2. **Cổng: dịch *xuống*, không dịch lên — và phải chặn biên `uid`.**
   Bản vẽ nói "dịch khối cổng ra khỏi dải của Zed" mà không nói hướng. Tính ra thì Zed chiếm
   `[43737, 65534]` — **trọn dải trên**, vì phần bù `uid` của nó chạy tới đỉnh không gian cổng.
   Nên không có chỗ phía trên. Zode dùng base `39737` và **chặn `uid % 3000`**; nếu để `uid` chạy
   tự do như Zed thì dải vẫn giao dù base khác. Cái giá: hai người dùng có `uid` lệch nhau đúng
   bội số 3000 sẽ trùng cổng — nằm ngoài dải thật của cả macOS (501) lẫn Linux (1000), và khi xảy
   ra thì rơi vào đúng nhánh dự phòng Zed vẫn luôn có.

3. **`instance_handshake()` và `address()` phải tách hàm thuần mới test được.**
   Cả hai đọc `RELEASE_CHANNEL` toàn tiến trình. Bản test đầu tiên của `handshake` chép lại chuỗi
   thay vì gọi hàm — tức **không có răng**: trả hàm về bản cũ nó vẫn xanh. Đã tách `handshake_for`
   và `port_for` nhận `channel` làm tham số.

4. **Cầu env lộ ra một lỗi do chính việc đổi tên cờ CLI gây ra.**
   Đổi `--zed` thành `--zode` làm nhánh Flatpak (`cli/src/main.rs:1024`) mất khả năng nhận diện —
   nó so chuỗi `arg == "--zed"`, nên với `--zode` sẽ tự chèn thêm một đường dẫn app đè lên cái
   người dùng đưa. Đã sửa nhận cả hai. `--zed` giữ làm alias cho script cũ.

### Đã kiểm chứng chạy thật

| Kiểm | Kết quả |
|---|---|
| Ba thư mục `Zode` được tạo | `~/.config/zode`, `~/Library/Application Support/Zode` (db, extensions, languages, debug_adapters, hang_traces), `~/Library/Logs/Zode/Zode.log` |
| `~/.config/zed` không bị đụng | mtime giữ nguyên qua mọi lần chạy |
| Cầu env, 4 tổ hợp | không đặt → `dev`; chỉ `ZED_` → `nightly`; chỉ `ZODE_` → `nightly`; cả hai → `ZODE_` thắng |
| Cổng thật khi chạy kênh stable | **40438**, thấp hơn đáy dải Zed 3299 cổng |
| Bắt tay đọc qua socket thật | `Zode Stable Instance Running` |
| Đếm call site `env::var("ZED_` | **30** — không call site nào bị sửa, đúng thiết kế |

Dùng `ZODE_RELEASE_CHANNEL` để kiểm cầu env là có chủ ý: `RELEASE_CHANNEL_NAME` chính là
`LazyLock` dễ bị đọc trước cầu nhất, nên nó đổi được chứng minh luôn thứ tự khởi tạo đúng.

### Không kiểm được

**Mở Zed và zode cùng lúc.** Máy này không cài `/Applications/Zed.app`. Cổng và chuỗi bắt tay đã
đo trực tiếp và đều nằm ngoài dải Zed, nhưng bản thân cảnh "hai app cùng chạy" thì **chưa ai
chứng kiến**.

**Nhánh Flatpak** (`cli/src/main.rs:1024`) là `target_os = "linux"`, không được biên dịch trên
macOS — sửa ở đó chưa qua trình biên dịch lần nào.
