---
title: How to Migrate from RustRover to Zode
description: "Guide for migrating from RustRover to Zode, including settings and keybindings."
---

# Chuyển từ RustRover sang Zode

Hướng dẫn này nói về phím tắt, cài đặt và những khác biệt bạn sẽ gặp khi là lập trình viên Rust chuyển từ RustRover.

## Cài đặt Zode

Zode có trên macOS, Windows và Linux.

Với macOS và Windows, tải bộ cài (`Zode-<arch>.dmg` hoặc `Zode-<arch>.exe`) từ [trang releases](https://github.com/tgiap04/zode/releases).

Với đa số người dùng Linux, cách dễ nhất để cài Zode là dùng script cài đặt của chúng tôi:

```sh
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | sh
```

Sau khi cài, bạn có thể mở Zode từ thư mục Applications (macOS), menu Start (Windows), hoặc trực tiếp từ terminal bằng:
`zode .`
Lệnh này mở thư mục hiện tại trong Zode.

## Thiết lập keymap JetBrains

Nếu bạn đến từ RustRover, cách nhanh nhất để thấy quen thuộc là dùng keymap JetBrains. Trong lúc làm quen ban đầu, bạn có thể chọn nó làm keymap cơ sở. Nếu đã bỏ qua bước đó, bạn có thể đổi bất cứ lúc nào:

1. Mở Settings bằng `Cmd+,` (macOS) hoặc `Ctrl+,` (Linux/Windows)
2. Tìm `Base Keymap`
3. Chọn `JetBrains`

Keymap này ánh xạ các phím tắt quen thuộc như `Shift Shift` cho Search Everywhere, `Cmd+O` cho Go to Class và `Cmd+Shift+A` cho Find Action.

## Thiết lập tuỳ chọn trình soạn thảo

Bạn có thể cấu hình hầu hết cài đặt trong Settings Editor ({#kb zode::OpenSettings}). Với cài đặt nâng cao, chạy `zode: open settings file` từ bảng lệnh (command palette) để sửa trực tiếp file cài đặt.

Các cài đặt người dùng RustRover thường chỉnh đầu tiên:

| Cài đặt Zode            | Tác dụng                                                                       |
| ----------------------- | ------------------------------------------------------------------------------ |
| `format_on_save`        | Tự định dạng khi lưu. Đặt thành `"on"` để bật (mặc định dùng rustfmt).         |
| `soft_wrap`             | Ngắt dòng dài. Tuỳ chọn: `"none"`, `"editor_width"`, `"preferred_line_length"` |
| `preferred_line_length` | Độ rộng cột để ngắt dòng và vẽ thước. Quy ước của Rust là 100.                 |
| `inlay_hints`           | Hiện gợi ý kiểu, tên tham số và gợi ý chuỗi gọi ngay trong dòng.               |
| `relative_line_numbers` | Hữu ích nếu bạn đến từ IdeaVim.                                                |

Zode cũng hỗ trợ cài đặt theo từng dự án. Tạo file `.zode/settings.json` ở thư mục gốc dự án để ghi đè cài đặt toàn cục cho dự án đó.

> **Mẹo:** Nếu bạn tham gia một dự án có sẵn, hãy kiểm tra `format_on_save` trước khi commit lần đầu. Không thì bạn có thể vô tình định dạng lại cả một file trong khi chỉ định sửa một dòng.

## Mở hoặc tạo dự án

Sau khi thiết lập, nhấn `Cmd+Shift+O` (với keymap JetBrains) để mở một thư mục. Thư mục này trở thành workspace của bạn trong Zode.

Để bắt đầu dự án mới, dùng Cargo từ terminal:

```sh
cargo new my_project
cd my_project
zode .
```

Hoặc với một thư viện:

```sh
cargo new --lib my_library
```

Bạn cũng có thể mở Zode từ terminal bên trong bất kỳ dự án Cargo nào có sẵn bằng:
`zode .`

Khi đã ở trong một dự án:

- Dùng `Cmd+Shift+O` hoặc `Cmd+E` để chuyển nhanh giữa các file (như "Recent Files" của RustRover)
- Dùng `Cmd+Shift+A` hoặc `Shift Shift` để mở bảng lệnh (như "Search Everywhere" của RustRover)
- Dùng `Cmd+O` để tìm symbol (như "Go to Symbol" của RustRover)

Các buffer đang mở hiện thành tab ở phía trên. Project Panel hiện cây file và trạng thái Git. Bật/tắt bằng `Cmd+1` (giống tool window Project của RustRover).

## Khác biệt về phím tắt

Nếu bạn đã chọn keymap JetBrains lúc làm quen ban đầu, hầu hết phím tắt sẽ thấy quen. Dưới đây là bảng tra nhanh để so sánh Zode với RustRover.

### Phím tắt dùng chung phổ biến

| Hành động                     | Phím tắt                |
| ----------------------------- | ----------------------- |
| Search Everywhere             | `Shift Shift`           |
| Find Action / Command Palette | `Cmd + Shift + A`       |
| Go to File                    | `Cmd + Shift + O`       |
| Go to Symbol                  | `Cmd + O`               |
| Recent Files                  | `Cmd + E`               |
| Go to Definition              | `Cmd + B`               |
| Find Usages                   | `Alt + F7`              |
| Rename Symbol                 | `Shift + F6`            |
| Reformat Code                 | `Cmd + Alt + L`         |
| Toggle Project Panel          | `Cmd + 1`               |
| Toggle Terminal               | `Alt + F12`             |
| Duplicate Line                | `Cmd + D`               |
| Delete Line                   | `Cmd + Backspace`       |
| Move Line Up/Down             | `Shift + Alt + Up/Down` |
| Expand/Shrink Selection       | `Alt + Up/Down`         |
| Comment Line                  | `Cmd + /`               |
| Go Back / Forward             | `Cmd + [` / `Cmd + ]`   |
| Toggle Breakpoint             | `Ctrl + F8`             |

### Phím tắt khác nhau (RustRover → Zode)

| Hành động              | RustRover   | Zode (keymap JetBrains)  |
| ---------------------- | ----------- | ------------------------ |
| File Structure         | `Cmd + F12` | `Cmd + F12` (outline)    |
| Navigate to Next Error | `F2`        | `F2`                     |
| Run                    | `Ctrl + R`  | `Ctrl + Alt + R` (tasks) |
| Debug                  | `Ctrl + D`  | `Alt + Shift + F9`       |
| Stop                   | `Cmd + F2`  | `Ctrl + F2`              |
| Expand Macro           | `Alt+Enter` | `Cmd + Shift + M`        |

### Riêng của Zode

| Hành động         | Phím tắt                   | Ghi chú                       |
| ----------------- | -------------------------- | ----------------------------- |
| Toggle Right Dock | `Cmd + R`                  | Bảng agent, thông báo         |
| Split Panes       | `Cmd + K`, then arrow keys | Tạo khung chia theo mọi hướng |

### Cách tuỳ chỉnh phím tắt

- Mở bảng lệnh (`Cmd+Shift+A` hoặc `Shift Shift`)
- Chạy `zode: open keymap`

Lệnh này mở danh sách mọi phím gán sẵn. Bạn có thể ghi đè từng phím tắt hoặc bỏ các xung đột.

Zode cũng hỗ trợ chuỗi phím (phím tắt nhiều phím).

## Khác biệt về giao diện người dùng

### Engine phân tích khác nhau

RustRover dùng engine phân tích code độc quyền của riêng nó để hiểu Rust. Zode dùng rust-analyzer qua Language Server Protocol (LSP).

Điều này có nghĩa với bạn:

- **Tự hoàn thành, go-to-definition, find usages, suy luận kiểu** — Đều có trong Zode qua rust-analyzer
- **Mở rộng macro** — Có ở cả hai (dùng `Cmd+Shift+M` trong Zode)
- **Inlay hint** — Cả hai đều hỗ trợ gợi ý kiểu, gợi ý tham số và gợi ý chuỗi gọi

Những chỗ bạn có thể thấy khác biệt:

- Một số refactoring có trong RustRover có thể không có tương đương ở rust-analyzer
- Các inspection riêng của RustRover (ngoài Clippy) sẽ không có trong Zode
- rust-analyzer được cấu hình bằng JSON trong Zode, không qua giao diện đồ hoạ

**Cách thích nghi:**

- Dùng `Alt+Enter` để xem các code action khả dụng — rust-analyzer cung cấp khá nhiều
- Cấu hình rust-analyzer trong `.zode/settings.json` cho nhu cầu riêng của dự án
- Chạy `cargo clippy` để lint (nó tích hợp với chẩn đoán của rust-analyzer)

### Cấu hình dự án

Cả hai trình soạn thảo đều lưu cấu hình theo dự án trong một thư mục ẩn. RustRover dùng `.idea` (với file XML), Zode dùng `.zode` (với file JSON).

**Run configuration không chuyển được.** RustRover lưu cấu hình chạy/gỡ lỗi trong `.idea`. Chúng không có đường di chuyển tự động. Bạn sẽ tạo lại chúng thành [task](../tasks.md) của Zode trong `.zode/tasks.json` và cấu hình gỡ lỗi trong `.zode/debug.json`.

**Không có tool window Cargo.** RustRover cung cấp cây trực quan gồm các thành viên workspace, target, feature và dependency. Zode không có thứ này. Bạn làm việc trực tiếp với `Cargo.toml` và CLI của Cargo.

**Quản lý toolchain nằm bên ngoài.** RustRover cho chọn và chuyển toolchain trong giao diện cài đặt. Trong Zode, bạn quản lý toolchain qua `rustup`.

**Cấu hình là tuỳ chọn.** RustRover tự sinh `.idea` khi bạn mở dự án. Zode không sinh gì cả. Bạn tự tạo `.zode/settings.json`, `tasks.json` và `debug.json` khi cần.

**Cách thích nghi:**

- Tạo `.zode/settings.json` ở gốc dự án cho cài đặt riêng của dự án
- Định nghĩa các lệnh hay dùng trong `tasks.json` (mở qua bảng lệnh: `zode: open tasks`):

```json
[
  {
    "label": "cargo run",
    "command": "cargo run"
  },
  {
    "label": "cargo build",
    "command": "cargo build"
  },
  {
    "label": "cargo test",
    "command": "cargo test"
  },
  {
    "label": "cargo clippy",
    "command": "cargo clippy"
  },
  {
    "label": "cargo run --release",
    "command": "cargo run --release"
  }
]
```

- Dùng `Ctrl+Alt+R` để chạy task nhanh
- Dựa vào terminal (`Alt+F12`) cho mọi thứ task không phủ tới

### Không có giao diện tích hợp Cargo

Tool window Cargo của RustRover cho phép truy cập trực quan vào target, dependency và các lệnh Cargo thông dụng của dự án. Bạn có thể chạy build, test và benchmark chỉ bằng một cú bấm.

Zode không có giao diện đồ hoạ cho Cargo. Bạn làm việc với Cargo qua:

- **Terminal** — Chạy trực tiếp bất kỳ lệnh Cargo nào
- **Task** — Định nghĩa lối tắt cho các lệnh hay dùng
- **Biểu tượng ở gutter** — Chạy test và binary bằng biểu tượng bấm được

**Cách thích nghi:**

- Làm quen với các lệnh CLI của Cargo: `cargo build`, `cargo run`, `cargo test`, `cargo clippy`, `cargo doc`
- Dùng task cho các lệnh bạn chạy thường xuyên
- Để quản lý dependency, sửa trực tiếp `Cargo.toml` (rust-analyzer gợi ý hoàn thành tên crate và phiên bản)

### Tool window và dock

RustRover sắp xếp các view phụ thành tool window đánh số (Project = 1, Cargo = Alt+1, Terminal = Alt+F12, v.v.). Zode dùng khái niệm tương tự gọi là "dock":

| Tool window của RustRover | Tương đương trong Zode | Phím tắt (keymap JetBrains) |
| ------------------------- | ---------------------- | --------------------------- |
| Project (1)               | Project Panel          | `Cmd + 1`                   |
| Git (9 or Cmd+0)          | Git Panel              | `Cmd + 0`                   |
| Terminal (Alt+F12)        | Terminal Panel         | `Alt + F12`                 |
| Structure (7)             | Outline Panel          | `Cmd + 7`                   |
| Problems (6)              | Diagnostics            | `Cmd + 6`                   |
| Debug (5)                 | Debug Panel            | `Cmd + 5`                   |

Zode có ba vị trí dock: trái, dưới và phải. Có thể di chuyển bảng giữa các dock bằng cách kéo thả hoặc qua cài đặt.

Lưu ý Zode không có tool window Cargo riêng. Dùng terminal hoặc định nghĩa task cho các lệnh Cargo thông dụng của bạn.

### Gỡ lỗi

Cả RustRover và Zode đều có gỡ lỗi tích hợp cho Rust, nhưng dùng backend khác nhau:

- RustRover dùng tích hợp debugger riêng của nó
- Zode dùng **CodeLLDB** (cùng debug adapter phổ biến trong VS Code)

Để gỡ lỗi code Rust trong Zode:

- Đặt breakpoint bằng `Ctrl+F8`
- Bắt đầu gỡ lỗi bằng `Alt+Shift+F9` hoặc nhấn `F4` và chọn mục tiêu gỡ lỗi
- Chạy từng bước bằng `F7` (step into), `F8` (step over), `Shift+F8` (step out)
- Tiếp tục thực thi bằng `F9`

Zode có thể tự phát hiện các target gỡ lỗi được trong dự án Cargo của bạn. Nhấn `F4` để xem các tuỳ chọn.

Để kiểm soát nhiều hơn, tạo file `.zode/debug.json`:

```json
[
  {
    "label": "Debug Binary",
    "adapter": "CodeLLDB",
    "request": "launch",
    "program": "${workspaceFolder}/target/debug/my_project"
  },
  {
    "label": "Debug Tests",
    "adapter": "CodeLLDB",
    "request": "launch",
    "cargo": {
      "args": ["test", "--no-run"],
      "filter": {
        "kind": "test"
      }
    }
  },
  {
    "label": "Debug with Arguments",
    "adapter": "CodeLLDB",
    "request": "launch",
    "program": "${workspaceFolder}/target/debug/my_project",
    "args": ["--config", "dev.toml"]
  }
]
```

### Chạy test

RustRover có trình chạy test riêng với giao diện hiện trạng thái đạt/hỏng của từng test. Zode chạy test qua:

- **Biểu tượng ở gutter** — Bấm nút play cạnh hàm `#[test]` hoặc module test
- **Task** — Định nghĩa lệnh `cargo test` trong `tasks.json`
- **Terminal** — Chạy `cargo test` trực tiếp

Output của test hiện trong bảng terminal. Để có output chi tiết hơn, dùng:

- `cargo test -- --nocapture` để xem output của println!
- `cargo test -- --test-threads=1` để chạy test tuần tự
- `cargo test specific_test_name` để chạy một test duy nhất

### Extension so với plugin

RustRover có đầy đủ danh mục plugin của JetBrains.

Danh mục extension của Zode nhỏ hơn và tập trung hơn:

- Hỗ trợ ngôn ngữ và tô màu cú pháp
- Giao diện (theme)
- Context server

Một số tính năng có thể cần plugin ở các trình soạn thảo khác thì có sẵn trong Zode:

- Cộng tác thời gian thực kèm trò chuyện thoại
- Hỗ trợ lập trình bằng AI
- Terminal tích hợp
- Trình chạy task
- Tích hợp rust-analyzer
- Định dạng bằng rustfmt

### Những gì Zode không có

Đây là những thứ RustRover có mà Zode không có:

- **Tích hợp profiler** — Dùng `cargo flamegraph`, `perf` hoặc công cụ profiling bên ngoài
- **Công cụ cơ sở dữ liệu** — Dùng DataGrip, DBeaver hoặc TablePlus
- **HTTP Client** — Dùng các công cụ như `curl`, `httpie` hoặc Postman
- **Hiển thị độ phủ test** — Dùng `cargo tarpaulin` hoặc `cargo llvm-cov` bên ngoài

## Lưu ý về giấy phép và telemetry

Về giấy phép và telemetry:

- **Zode là mã nguồn mở** (trình soạn thảo theo giấy phép MIT, dịch vụ cộng tác theo AGPL)
- **Telemetry là tuỳ chọn** và có thể tắt lúc làm quen ban đầu hoặc trong cài đặt

## Cộng tác trong Zode so với RustRover

RustRover cung cấp Code With Me như một tính năng riêng cho cộng tác. Zode tích hợp cộng tác ngay trong trải nghiệm cốt lõi.

- Mở Collab Panel ở dock trái
- Tạo một kênh và [mời cộng tác viên](https://zodekit.com/docs/collaboration#inviting-a-collaborator) tham gia
- [Chia sẻ màn hình hoặc codebase của bạn](https://zodekit.com/docs/collaboration#share-a-project) trực tiếp

Khi đã kết nối, bạn thấy con trỏ, vùng chọn và chỉnh sửa của nhau theo thời gian thực. Có sẵn trò chuyện thoại. Không cần công cụ riêng hay đăng nhập bên thứ ba.

## Dùng AI trong Zode

Zode có sẵn các tính năng AI. Nếu bạn từng dùng JetBrains AI Assistant, đây là cách thiết lập.

### Cấu hình GitHub Copilot

1. Mở Settings bằng `Cmd+,` (macOS) hoặc `Ctrl+,` (Linux/Windows)
2. Vào **AI → Edit Predictions**
3. Bấm **Configure** cạnh "Configure Providers"
4. Dưới **GitHub Copilot**, bấm **Sign in to GitHub**

Đăng nhập xong, cứ gõ là được. Zode sẽ đưa gợi ý ngay trong dòng để bạn chấp nhận.

### Các lựa chọn AI khác

Để dùng các mô hình AI khác trong Zode, bạn có nhiều lựa chọn:

- Zode không có mô hình lưu trữ sẵn hay gói đăng ký: tính năng AI dùng key hoặc agent của riêng bạn.
- Dùng [API key](https://zodekit.com/docs/ai/llm-providers.html) của riêng bạn, không cần xác thực
- Dùng [agent bên ngoài như Claude Agent](https://zodekit.com/docs/ai/external-agents.html)

## Cấu hình nâng cao và mẹo tăng năng suất

Zode cung cấp các cài đặt nâng cao cho người dùng muốn tinh chỉnh môi trường của mình.

Dưới đây là vài mẹo hữu ích cho lập trình viên Rust:

**Format on Save (mặc định dùng rustfmt):**

```json
"format_on_save": "on"
```

**Cấu hình inlay hint cho Rust:**

```json
{
  "inlay_hints": {
    "enabled": true,
    "show_type_hints": true,
    "show_parameter_hints": true,
    "show_other_hints": true
  }
}
```

**Cấu hình rust-analyzer** (cần sửa JSON thủ công):

```json
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "checkOnSave": {
          "command": "clippy"
        },
        "cargo": {
          "allFeatures": true
        },
        "procMacro": {
          "enable": true
        }
      }
    }
  }
}
```

**Dùng thư mục target riêng cho rust-analyzer (build nhanh hơn):**

```json
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "rust-analyzer.cargo.targetDir": true
      }
    }
  }
}
```

Cài đặt này bảo rust-analyzer dùng `target/rust-analyzer` thay cho `target`, để phân tích của IDE không xung đột với các lệnh `cargo build` bạn chạy tay.

**Bật hỗ trợ direnv (hữu ích với dự án Rust dùng direnv):**

```json
"load_direnv": "shell_hook"
```

**Cấu hình linked project cho workspace:**

Nếu bạn làm việc với nhiều dự án Cargo không nằm trong một workspace, bạn có thể cho rust-analyzer biết về chúng:

```json
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "linkedProjects": ["./project-a/Cargo.toml", "./project-b/Cargo.toml"]
      }
    }
  }
}
```

## Bước tiếp theo

Giờ bạn đã thiết lập xong, đây là một số tài nguyên giúp bạn dùng Zode hiệu quả nhất:

- [Tất cả cài đặt](../reference/all-settings.md) — Tuỳ chỉnh cài đặt, giao diện (theme) và hành vi trình soạn thảo
- [Phím tắt](../key-bindings.md) — Học cách tuỳ chỉnh và mở rộng keymap
- [Task](../tasks.md) — Thiết lập lệnh build và chạy cho dự án
- [Rust trong Zode](../languages/rust.md) — Thiết lập và cấu hình riêng cho Rust
