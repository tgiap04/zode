# Brainstorm — Tách nơi lưu cấu hình của zode khỏi Zed

**Ngày:** 2026-08-08 · **Lens:** CTO · **Trạng thái:** thiết kế đã chốt, một mục cần quyết lại

## Commission

Người dùng cài **cả Zed lẫn zode** thì hai app không được giẫm lên nhau. Hiện zode đọc/ghi
đúng những đường dẫn của Zed.

## Khảo sát — sự thật đo được

### Danh tính đã fork, đường dẫn thì chưa

`crates/release_channel/src/lib.rs` đã đổi hết: `display_name` → `"Zode"`, bundle id →
`io.github.tgiap04.zode`, `app_identifier` (Windows) → `"Zode-Editor-*"`. `script/bundle-mac:206`
ký `Contents/MacOS/zode`.

Nhưng `crates/paths/src/paths.rs` vẫn trả về `Zed` / `zed` ở **mọi nhánh nền tảng**.

### Cấu trúc thuận lợi: 4 hàm gốc

Khoảng 30 đường dẫn trong `paths.rs` đều `join` lên **bốn** hàm:

| Hàm | macOS | Đang chứa gì |
|---|---|---|
| `config_dir()` | `~/.config/zed` | `settings.json`, `keymap.json`, `tasks.json`, `debug.json`, `themes/`, `snippets/` |
| `data_dir()` | `~/Library/Application Support/Zed` | `db/`, `extensions/`, `languages/`, `logs/`, `copilot/`, `debug_adapters/`, `external_agents/` |
| `state_dir()` | `~/.local/state/Zed` | trạng thái phụ |
| `temp_dir()` | `~/Library/Caches/Zed` | cache |

Sửa bốn chỗ là toàn bộ phần còn lại đi theo. Đã có sẵn `set_custom_data_dir()` cho phép ghi đè,
nên cơ chế override không phải dựng mới.

### Va chạm nặng nhất không nằm ở thư mục — nằm ở single-instance

`crates/zed/src/zed/mac_only_instance.rs`:

- `address()` tính cổng TCP **thuần từ release channel + uid**, không dính định danh app.
  Dev = `43737 + uid`. Zed và zode ra **cùng một con số**.
- `instance_handshake()` trả `"Zed Editor Dev Instance Running"` — zode kế thừa **y nguyên**.

Nên hai app tranh cùng cổng với cùng mật khẩu. Mở Zed trước rồi bấm zode → `main.rs:353` in
`zed is already running` rồi `return`. zode không mở nổi.

**Hôm nay chưa cắn** vì `crates/zed/RELEASE_CHANNEL` = `dev`, mà `main.rs:330-334` bỏ qua kiểm tra
này trên kênh dev. Nó cắn ngay lần đầu dựng bản stable/preview.

### Xung đột settings là thật và bất đối xứng

`assets/settings/default.json` của zode có các khoá Zed không biết: `multi_project.*` (:150),
`hibernate_after_ms` (:168), `show_branch_name` (:516), `show_project_items` (:518), và mặc định
theme `"Dark 2026"` / `"Light 2026"` mà Zed không có → Zed rơi về fallback.

### `db/` dùng chung là chỗ rủi ro nhất trong nhóm thư mục

zode nhét `project_groups` vào `serialized_state` của workspace (`persistence.rs`, dùng bởi
`multi_workspace.rs:2107`). Hai fork cùng ghi một file SQLite lịch sử cửa sổ/workspace.

### Mặt `ZED_*` env var — rộng hơn nhiều so với vẻ ngoài

Đo được: **52 call site**, **15 crate**, **40 biến riêng biệt**.
Trong đó **22 là build-time** (`option_env!` / `env!` — `ZED_BUNDLE`, `ZED_COMMIT_SHA`,
`ZED_PKG_VERSION`, `ZED_BUILD_ID`…), tức phải sửa cả build script và CI. 30 là runtime.

Đáng chú ý: `ZED_TERM` được **bơm vào môi trường terminal tích hợp**
(`crates/terminal/src/terminal.rs:124`) — script của người dùng có thể đang bám vào nó.

## Quyết định đã chốt

| # | Quyết định | Lý do |
|---|---|---|
| 1 | **Tách cả bốn thư mục gốc** → `zode` / `Zode` | Tách config mà để chung `db/` là sửa nửa vời — lịch sử workspace mới là chỗ hai fork ghi đè nhau |
| 2 | **Bắt đầu trắng**, không di trú | Không phép thuật ngầm. Người dùng tự chép lại nếu muốn |
| 3 | **Gộp định danh single-instance** | Cùng bản chất "tách danh tính khỏi Zed", và là mục duy nhất gây lỗi cứng |

Tên theo đúng quy ước sẵn có: `~/.config/zode`, `~/Library/Application Support/Zode`,
`~/.local/state/Zode`, `~/Library/Caches/Zode`, Windows `%APPDATA%\Zode`.

## Quyết định 4 — `ZED_*` env var: đọc kép, chỉ biến runtime

**Đã chốt sau khi thu hẹp phạm vi.** Một accessor đọc `ZODE_<X>` trước, rơi về `ZED_<X>`.
Chỉ áp cho **30 biến runtime**; giữ nguyên 22 chỗ build-time và toàn bộ `script/bundle-*` + CI.
Cộng thêm chứ không sửa đè → bề mặt conflict tối thiểu, và ai quen `ZED_*` vẫn dùng được.

Lý do thu hẹp (người dùng ban đầu chọn "đổi hết", đổi ý sau khi có số đo):

- 52 call site trên 15 crate, mỗi chỗ đổi là **một điểm conflict vĩnh viễn khi rebase upstream**.
  Nhánh này đã theo nguyên tắc ngược lại ở nơi khác: plan theme
  (`brainstorm-260807`) loại thẳng phương án sửa `crates/theme_importer` **chỉ vì** lý do đó.
- 22 chỗ là build-time, đổi thì kéo theo `script/bundle-*` và CI — công việc không liên quan gì
  tới chuyện "hai app giẫm chân nhau".
- Biến môi trường **không phải nơi lưu trữ**. Chúng chỉ va chạm khi người dùng tự `export`, mà
  người tự export thì cũng tự biết mình đang chỉnh app nào.

Ba đường:

| Đường | Nội dung | Đánh giá |
|---|---|---|
| A. Bỏ qua | Giữ `ZED_*` | Bề mặt conflict bằng 0. Không giải quyết gì |
| **B. Đọc kép tại một chỗ** ✅ | Accessor đọc `ZODE_<X>` trước, rơi về `ZED_<X>`; chỉ biến runtime | **Đã chọn** |
| C. Đổi hết | 52 chỗ + build script + CI | Loại. Chi phí rebase vĩnh viễn cho lợi ích giả định |

CLI binary tên `cli` (`crates/cli/Cargo.toml`) — đổi tên là chuyện nhỏ, độc lập, không vướng gì.

## Rủi ro cần canh

- **Mất thiết lập hiện tại của người dùng.** Họ đang có `~/.config/zed/settings.json` (theme dark,
  `Dark 2026`) và `~/.config/zed/themes/vscode-2026.json`. Chọn "bắt đầu trắng" nghĩa là lần chạy
  đầu sau thay đổi sẽ về mặc định. Theme vẫn còn vì đã bundle; nhưng `mode: dark` thì mất.
  → Nói trước, kèm đúng một câu lệnh chép tay.
- **`set_custom_data_dir` panic nếu gọi sau `config_dir`/`data_dir`.** Thứ tự khởi tạo phải giữ.
- **`.zed_server` trên máy remote** (`remote_server_dir_relative`) — đổi thì client cũ/server cũ
  lệch nhau. Cần quyết riêng, không nằm trong phạm vi này.
- **Bốn hàm này là code upstream Zed** → điểm conflict khi rebase. Nhưng chỉ bốn hàm, chấp nhận được.

## Đo thành công

- zode không đọc/ghi bất kỳ đường dẫn nào chứa `Zed`/`zed` (trừ `.zed_server` để ngoài phạm vi)
- Chạy Zed và zode **đồng thời** trên bản stable, cả hai cùng mở được
- Sửa `~/.config/zed/settings.json` không ảnh hưởng zode và ngược lại
- `./script/clippy` sạch; test toàn bộ `-p workspace`, `-p paths`, `-p zed` xanh

## Bước tiếp theo

Chuyển sang `tkm:create-plan`, sau khi chốt đường cho `ZED_*`.
