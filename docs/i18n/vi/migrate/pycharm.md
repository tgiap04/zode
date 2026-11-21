---
title: How to Migrate from PyCharm to Zode
description: "Guide for migrating from PyCharm to Zode, including settings and keybindings."
---

# Chuyển từ PyCharm sang Zode

Hướng dẫn này nói về cách thiết lập Zode nếu bạn đến từ PyCharm, gồm phím tắt, cài đặt và những điểm khác biệt bạn sẽ gặp.

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

Nếu bạn đến từ PyCharm, cách nhanh nhất để thấy quen thuộc là dùng keymap JetBrains. Trong lúc làm quen ban đầu, bạn có thể chọn nó làm keymap cơ sở. Nếu đã bỏ qua bước đó, bạn có thể đổi bất cứ lúc nào:

1. Mở Settings bằng `Cmd+,` (macOS) hoặc `Ctrl+,` (Linux/Windows)
2. Tìm `Base Keymap`
3. Chọn `JetBrains`

Keymap này ánh xạ các phím tắt quen thuộc như `Shift Shift` cho Search Everywhere, `Cmd+O` cho Go to Class và `Cmd+Shift+A` cho Find Action.

## Thiết lập tuỳ chọn trình soạn thảo

Bạn có thể cấu hình hầu hết cài đặt trong Settings Editor ({#kb zode::OpenSettings}). Với cài đặt nâng cao, chạy `zode: open settings file` từ bảng lệnh (command palette) để sửa trực tiếp file cài đặt.

Các cài đặt người dùng PyCharm thường chỉnh đầu tiên:

| Cài đặt Zode            | Tác dụng                                                                       |
| ----------------------- | ------------------------------------------------------------------------------ |
| `format_on_save`        | Tự định dạng khi lưu. Đặt thành `"on"` để bật.                                 |
| `soft_wrap`             | Ngắt dòng dài. Tuỳ chọn: `"none"`, `"editor_width"`, `"preferred_line_length"` |
| `preferred_line_length` | Độ rộng cột để ngắt dòng và vẽ thước. Mặc định 80, PEP 8 khuyến nghị 79.       |
| `inlay_hints`           | Hiện tên tham số và gợi ý kiểu ngay trong dòng, giống gợi ý của PyCharm.       |
| `relative_line_numbers` | Hữu ích nếu bạn đến từ IdeaVim.                                                |

Zode cũng hỗ trợ cài đặt theo từng dự án. Tạo file `.zode/settings.json` ở thư mục gốc dự án để ghi đè cài đặt toàn cục cho dự án đó, tương tự cách bạn dùng thư mục `.idea` trong PyCharm.

> **Mẹo:** Nếu bạn tham gia một dự án có sẵn, hãy kiểm tra `format_on_save` trước khi commit lần đầu. Không thì bạn có thể vô tình định dạng lại cả một file trong khi chỉ định sửa một dòng.

## Mở hoặc tạo dự án

Sau khi thiết lập, nhấn `Cmd+Shift+O` (với keymap JetBrains) để mở một thư mục. Thư mục này trở thành workspace của bạn trong Zode. Khác với PyCharm, không có trình hướng dẫn cấu hình dự án, không có hộp thoại chọn interpreter, và không cần thiết lập cấu trúc dự án.

Để bắt đầu dự án mới, tạo một thư mục bằng terminal hoặc trình quản lý file, rồi mở nó trong Zode. Trình soạn thảo coi thư mục đó là gốc của dự án.

Bạn cũng có thể mở Zode từ terminal bên trong bất kỳ thư mục nào bằng:
`zode .`

Khi đã ở trong một dự án:

- Dùng `Cmd+Shift+O` hoặc `Cmd+E` để chuyển nhanh giữa các file (như "Recent Files" của PyCharm)
- Dùng `Cmd+Shift+A` hoặc `Shift Shift` để mở bảng lệnh (như "Search Everywhere" của PyCharm)
- Dùng `Cmd+O` để tìm symbol (như "Go to Symbol" của PyCharm)

Các buffer đang mở hiện thành tab ở phía trên. Project Panel hiện cây file và trạng thái Git. Bật/tắt bằng `Cmd+1` (giống tool window Project của PyCharm).

## Khác biệt về phím tắt

Nếu bạn đã chọn keymap JetBrains lúc làm quen ban đầu, hầu hết phím tắt sẽ thấy quen. Dưới đây là bảng tra nhanh để so sánh Zode với PyCharm.

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

### Phím tắt khác nhau (PyCharm → Zode)

| Hành động              | PyCharm     | Zode (keymap JetBrains)  |
| ---------------------- | ----------- | ------------------------ |
| File Structure         | `Cmd + F12` | `Cmd + F12` (outline)    |
| Navigate to Next Error | `F2`        | `F2`                     |
| Run                    | `Ctrl + R`  | `Ctrl + Alt + R` (tasks) |
| Debug                  | `Ctrl + D`  | `Alt + Shift + F9`       |
| Stop                   | `Cmd + F2`  | `Ctrl + F2`              |

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

### Không có indexing

Nếu từng dùng PyCharm trên dự án lớn, bạn biết cảnh chờ đợi: "Indexing..." có thể mất từ 30 giây đến vài phút tuỳ kích thước dự án và các dependency. PyCharm dựng một chỉ mục toàn diện cho cả codebase để phục vụ khả năng hiểu code, và lập lại chỉ mục khi dependency đổi hoặc khi bạn cài package mới.

Zode không lập chỉ mục. Bạn mở thư mục và làm việc ngay. Tìm file và điều hướng luôn nhanh bất kể dự án lớn cỡ nào, không phải chờ những lần lập chỉ mục.

Chỉ mục của PyCharm cung cấp các tính năng như tìm mọi chỗ dùng trong toàn codebase, hiểu cây thừa kế lớp, và phát hiện import thừa trên toàn dự án. Zode giao việc này cho language server, vốn có thể phân tích không sâu và không rộng bằng.

**Cách thích nghi:**

- Để tìm symbol trên toàn dự án, dùng `Cmd+O` / Go to Symbol (dựa vào language server của bạn)
- Để tìm file theo tên, dùng `Cmd+Shift+O` / Go to File
- Để tìm văn bản trong nhiều file, dùng `Cmd+Shift+F` — nhanh ngay cả với codebase lớn
- Để phân tích tĩnh sâu, cân nhắc chạy các công cụ như `mypy`, `pylint` hoặc `ruff check` từ terminal

### LSP so với khả năng hiểu ngôn ngữ riêng

PyCharm có engine phân tích ngôn ngữ riêng, xây dựng riêng cho Python. Engine này hiểu code của bạn rất sâu: suy ra kiểu khi không có annotation, theo dõi luồng dữ liệu, biết về model Django và route Flask, và cung cấp các refactoring chuyên biệt.

Zode dùng Language Server Protocol (LSP) để hiểu code. Với Python, Zode cung cấp sẵn nhiều language server:

- **basedpyright** (mặc định) — Kiểm tra kiểu và tự hoàn thành nhanh
- **Ruff** (mặc định) — Lint và định dạng
- **ty** — Language server đang lên của Astral, xây dựng để chạy nhanh
- **Pyright** — Trình kiểm tra kiểu của Microsoft
- **PyLSP** — Server dạng plugin, tích hợp nhiều công cụ

Trải nghiệm LSP cho Python rất tốt. basedpyright cho gợi ý hoàn thành, kiểm tra kiểu và điều hướng chính xác. Ruff định dạng và lint với hiệu năng xuất sắc.

Những chỗ bạn có thể thấy khác biệt:

- Khả năng hiểu riêng theo framework (Django ORM, route Flask) không có sẵn
- Một số refactoring phức tạp (extract method với phân tích phạm vi đúng) có thể kém tinh vi hơn
- Gợi ý auto-import phụ thuộc vào những gì language server biết về môi trường của bạn

**Cách thích nghi:**

- Dùng `Alt+Enter` để xem các code action khả dụng — danh sách khác nhau tuỳ language server
- Đảm bảo đã chọn virtual environment để language server phân giải được dependency của bạn
- Dùng Ruff để định dạng nhanh và nhất quán (mặc định đã bật)
- Để kiểm tra code kiểu "Inspect Code" của PyCharm, chạy `ruff check .` hoặc xem bảng Diagnostics (`Cmd+6`) — basedpyright và Ruff kết hợp bắt được nhiều lỗi giống nhau

### Virtual environment và interpreter

Trong PyCharm, bạn chọn Python interpreter qua giao diện, và PyCharm quản lý kết nối giữa dự án và interpreter đó. Nó hiện các package có sẵn, cho cài thêm, và nhớ mỗi dự án dùng môi trường nào.

Zode xử lý virtual environment qua hệ thống toolchain:

- Zode tự phát hiện virtual environment ở các vị trí thông dụng (`.venv`, `venv`, `.env`, `env`)
- Khi phát hiện một virtual environment, terminal tự kích hoạt nó
- Language server được tự cấu hình để dùng môi trường đã phát hiện
- Bạn có thể chọn toolchain thủ công nếu tự phát hiện chọn sai

**Cách thích nghi:**

- Tạo virtual environment bằng `python -m venv .venv` hoặc `uv sync`
- Mở thư mục trong Zode — nó sẽ tự phát hiện môi trường
- Nếu cần đổi môi trường, dùng bộ chọn toolchain
- Với môi trường conda, đảm bảo đã kích hoạt trong shell trước khi mở Zode

> **Mẹo:** Nếu basedpyright báo lỗi import với package bạn đã cài, kiểm tra xem Zode đã chọn đúng virtual environment chưa. Dùng bộ chọn toolchain để xác nhận hoặc đổi môi trường đang dùng.

### Không có mô hình dự án

PyCharm quản lý dự án qua thư mục `.idea` chứa file cấu hình XML, việc gán interpreter và các run configuration. Mô hình này cho phép PyCharm nhớ interpreter bạn chọn, quản lý dependency qua giao diện, và lưu lại các thiết lập chạy/gỡ lỗi phức tạp.

Zode không có mô hình dự án. Dự án là một thư mục. Không có trình hướng dẫn, không có màn hình chọn interpreter, không có cấu hình cấu trúc dự án.

Điều này nghĩa là:

- Run configuration không tồn tại. Bạn định nghĩa task hoặc dùng terminal. Các run config PyCharm hiện có trong `.idea/` sẽ không được đọc — bạn sẽ tạo lại những cái cần trong `tasks.json`.
- Quản lý interpreter nằm bên ngoài. Zode phát hiện môi trường nhưng không tạo chúng.
- Dependency được quản lý qua pip, uv, poetry hoặc conda — không qua trình soạn thảo.
- Không có bảng Python Console (REPL tương tác). Hãy dùng `python` hoặc `ipython` trong terminal.

**Cách thích nghi:**

- Tạo `.zode/settings.json` ở gốc dự án cho cài đặt riêng của dự án
- Định nghĩa các lệnh hay dùng trong `tasks.json` (mở qua bảng lệnh: `zode: open tasks`):

```json
[
  {
    "label": "run",
    "command": "python main.py"
  },
  {
    "label": "test",
    "command": "pytest"
  },
  {
    "label": "test current file",
    "command": "pytest $ZODE_FILE"
  }
]
```

- Dùng `Ctrl+Alt+R` để chạy task nhanh
- Dựa vào terminal (`Alt+F12`) cho mọi thứ task không phủ tới

### Không có tích hợp framework

Giá trị của PyCharm Professional cho phát triển web phần lớn đến từ tích hợp framework. Template Django được hiểu và điều hướng được. Route Flask được lập chỉ mục. Model SQLAlchemy được xử lý đặc biệt. Biến trong template tự hoàn thành.

Zode không có những thứ này. Language server nhìn code Python chỉ như code Python — nó không hiểu `@app.route` định nghĩa một endpoint hay một lớp model Django tạo ra bảng cơ sở dữ liệu.

**Cách thích nghi:**

- Dùng grep và tìm file thoải mái. `Cmd+Shift+F` với regex có thể tìm định nghĩa route, lớp model hoặc chỗ dùng template.
- Dựa vào "find references" (`Alt+F7`) của language server để điều hướng — nó chạy được, chỉ thiếu ngữ cảnh framework
- Cân nhắc dùng công cụ CLI riêng của framework (`python manage.py`, `flask routes`) từ terminal của Zode

> **Mẹo:** Với việc làm cơ sở dữ liệu, hãy dùng công cụ chuyên dụng như DataGrip, DBeaver hoặc TablePlus. Nhiều lập trình viên chuyển sang Zode vẫn giữ DataGrip riêng cho SQL.

### Tool window và dock

PyCharm sắp xếp các view phụ thành tool window đánh số (Project = 1, Python Console = 4, Terminal = Alt+F12, v.v.). Zode dùng khái niệm tương tự gọi là "dock":

| Tool window của PyCharm | Tương đương trong Zode | Phím tắt (keymap JetBrains) |
| ----------------------- | ---------------------- | --------------------------- |
| Project (1)             | Project Panel          | `Cmd + 1`                   |
| Git (9 or Cmd+0)        | Git Panel              | `Cmd + 0`                   |
| Terminal (Alt+F12)      | Terminal Panel         | `Alt + F12`                 |
| Structure (7)           | Outline Panel          | `Cmd + 7`                   |
| Problems (6)            | Diagnostics            | `Cmd + 6`                   |
| Debug (5)               | Debug Panel            | `Cmd + 5`                   |

Zode có ba vị trí dock: trái, dưới và phải. Có thể di chuyển bảng giữa các dock bằng cách kéo thả hoặc qua cài đặt.

### Gỡ lỗi

Cả PyCharm và Zode đều có gỡ lỗi tích hợp, nhưng trải nghiệm khác nhau:

- Zode dùng `debugpy` (cùng debug adapter mà VS Code dùng)
- Đặt breakpoint bằng `Ctrl+F8`
- Bắt đầu gỡ lỗi bằng `Alt+Shift+F9` hoặc nhấn `F4` và chọn mục tiêu gỡ lỗi
- Chạy từng bước bằng `F7` (step into), `F8` (step over), `Shift+F8` (step out)
- Tiếp tục thực thi bằng `F9`

Zode có thể tự phát hiện các điểm vào có thể gỡ lỗi. Nhấn `F4` để xem các tuỳ chọn, gồm:

- Script Python
- Module
- Test pytest

Để kiểm soát nhiều hơn, tạo file `.zode/debug.json`:

```json
[
  {
    "label": "Debug Current File",
    "adapter": "Debugpy",
    "program": "$ZODE_FILE",
    "request": "launch"
  },
  {
    "label": "Debug Flask App",
    "adapter": "Debugpy",
    "request": "launch",
    "module": "flask",
    "args": ["run", "--debug"],
    "env": {
      "FLASK_APP": "app.py"
    }
  }
]
```

### Chạy test

PyCharm có trình chạy test riêng với giao diện hiện trạng thái đạt/hỏng của từng test. Zode chạy test qua:

- **Biểu tượng ở gutter** — Bấm nút play cạnh hàm hoặc lớp test
- **Task** — Định nghĩa lệnh pytest hoặc unittest trong `tasks.json`
- **Terminal** — Chạy `pytest` trực tiếp

Output của test hiện trong bảng terminal. Với pytest, dùng `--tb=short` để traceback gọn hoặc `-v` để output chi tiết.

### Extension so với plugin

PyCharm có danh mục plugin lớn, phủ mọi thứ từ hỗ trợ thêm ngôn ngữ đến công cụ cơ sở dữ liệu và tích hợp triển khai.

Danh mục extension của Zode nhỏ hơn và tập trung hơn:

- Hỗ trợ ngôn ngữ và tô màu cú pháp
- Giao diện (theme)
- Context server

Một số tính năng cần plugin trong PyCharm thì có sẵn trong Zode:

- Cộng tác thời gian thực kèm trò chuyện thoại
- Hỗ trợ lập trình bằng AI
- Terminal tích hợp
- Trình chạy task
- Khả năng hiểu code dựa trên LSP
- Định dạng và lint bằng Ruff

### Những gì Zode không có

Để đặt kỳ vọng rõ ràng, đây là những thứ PyCharm có mà Zode không có:

- **Scientific Mode / tích hợp Jupyter** — Với notebook và quy trình khoa học dữ liệu, dùng JupyterLab hoặc VS Code với extension Jupyter cùng Zode để soạn Python
- **Công cụ cơ sở dữ liệu** — Dùng DataGrip, DBeaver hoặc TablePlus
- **Điều hướng template Django/Flask** — Dùng tìm file và grep
- **Trình quản lý package dạng giao diện** — Dùng pip, uv hoặc poetry từ terminal
- **Remote interpreter** — Zode có phát triển từ xa, nhưng hoạt động khác
- **Tích hợp profiler** — Dùng cProfile, py-spy hoặc công cụ tương tự bên ngoài

## Cộng tác trong Zode so với PyCharm

PyCharm cung cấp Code With Me như một plugin riêng cho cộng tác. Zode tích hợp cộng tác ngay trong trải nghiệm cốt lõi.

- Mở Collab Panel ở dock trái
- Tạo một kênh và [mời cộng tác viên](https://zodekit.com/docs/collaboration#inviting-a-collaborator) tham gia
- [Chia sẻ màn hình hoặc codebase của bạn](https://zodekit.com/docs/collaboration#share-a-project) trực tiếp

Khi đã kết nối, bạn thấy con trỏ, vùng chọn và chỉnh sửa của nhau theo thời gian thực. Có sẵn trò chuyện thoại. Không cần công cụ riêng hay đăng nhập bên thứ ba.

## Dùng AI trong Zode

Nếu bạn quen với trợ lý AI trong PyCharm (như GitHub Copilot hay JetBrains AI Assistant), Zode có khả năng tương tự và linh hoạt hơn.

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

Dưới đây là vài mẹo hữu ích:

**Format on Save:**

```json
"format_on_save": "on"
```

**Bật hỗ trợ direnv (hữu ích với dự án Python dùng direnv):**

```json
"load_direnv": "shell_hook"
```

**Tuỳ chỉnh việc phát hiện virtual environment** (cần sửa JSON thủ công):

```json
{
  "terminal": {
    "detect_venv": {
      "on": {
        "directories": [".venv", "venv", ".env", "env"],
        "activate_script": "default"
      }
    }
  }
}
```

**Cấu hình mức nghiêm ngặt khi kiểm tra kiểu của basedpyright:**

Nếu thấy basedpyright quá khắt khe hoặc quá lỏng, hãy cấu hình trong `pyrightconfig.json` của dự án:

```json
{
  "typeCheckingMode": "basic"
}
```

Các tuỳ chọn là `"off"`, `"basic"`, `"standard"` (mặc định), `"strict"` hoặc `"all"`.

## Bước tiếp theo

Giờ bạn đã thiết lập xong, đây là một số tài nguyên giúp bạn dùng Zode hiệu quả nhất:

- [Tất cả cài đặt](../reference/all-settings.md) — Tuỳ chỉnh cài đặt, giao diện (theme) và hành vi trình soạn thảo
- [Phím tắt](../key-bindings.md) — Học cách tuỳ chỉnh và mở rộng keymap
- [Task](../tasks.md) — Thiết lập lệnh build và chạy cho dự án
- [Python trong Zode](../languages/python.md) — Thiết lập và cấu hình riêng cho Python
