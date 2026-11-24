---
title: How to Migrate from IntelliJ IDEA to Zode
description: "Guide for migrating from IntelliJ IDEA to Zode, including settings and keybindings."
---

# Chuyển từ IntelliJ IDEA sang Zode

Hướng dẫn này nói cách thiết lập Zode nếu bạn đến từ IntelliJ IDEA, gồm phím tắt, cài đặt và những khác biệt bạn nên chờ đợi.

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

Nếu bạn đến từ IntelliJ, cách nhanh nhất để thấy quen thuộc là dùng keymap JetBrains. Trong lúc làm quen ban đầu, bạn có thể chọn nó làm keymap cơ sở. Nếu đã bỏ qua bước đó, bạn có thể đổi bất cứ lúc nào:

1. Mở Settings bằng `Cmd+,` (macOS) hoặc `Ctrl+,` (Linux/Windows)
2. Tìm `Base Keymap`
3. Chọn `JetBrains`

Keymap này ánh xạ các phím tắt quen thuộc như `Shift Shift` cho Search Everywhere, `Cmd+O` cho Go to Class và `Cmd+Shift+A` cho Find Action.

## Thiết lập tuỳ chọn trình soạn thảo

Bạn có thể cấu hình hầu hết cài đặt trong Settings Editor ({#kb zode::OpenSettings}). Với cài đặt nâng cao, chạy `zode: open settings file` từ bảng lệnh (command palette) để sửa trực tiếp file cài đặt.

Những cài đặt người dùng IntelliJ thường cấu hình trước:

| Cài đặt Zode            | Tác dụng                                                                       |
| ----------------------- | ------------------------------------------------------------------------------ |
| `format_on_save`        | Tự động định dạng khi lưu. Đặt `"on"` để bật.                                  |
| `soft_wrap`             | Ngắt dòng dài. Tuỳ chọn: `"none"`, `"editor_width"`, `"preferred_line_length"` |
| `preferred_line_length` | Độ rộng cột để ngắt dòng và vẽ thước. Mặc định là 80.                          |
| `inlay_hints`           | Hiện tên tham số và gợi ý kiểu ngay trong dòng, như gợi ý của IntelliJ.        |
| `relative_line_numbers` | Hữu ích nếu bạn đến từ IdeaVim.                                                |

Zode cũng hỗ trợ cài đặt theo từng dự án. Tạo file `.zode/settings.json` ở thư mục gốc dự án để ghi đè cài đặt toàn cục cho dự án đó, tương tự cách bạn dùng thư mục `.idea` trong IntelliJ.

> **Mẹo:** Nếu bạn tham gia một dự án có sẵn, hãy kiểm tra `format_on_save` trước lần commit đầu tiên. Nếu không, bạn có thể vô tình định dạng lại cả file trong khi chỉ định sửa một dòng.

## Mở hoặc tạo dự án

Sau khi thiết lập, nhấn `Cmd+Shift+O` (với keymap JetBrains) để mở một thư mục. Nó trở thành workspace của bạn trong Zode. Khác với IntelliJ, không có trình hướng dẫn cấu hình dự án, không có file `.iml` và không cần thiết lập SDK.

Để bắt đầu dự án mới, tạo một thư mục bằng terminal hoặc trình quản lý file, rồi mở nó trong Zode. Trình soạn thảo sẽ coi thư mục đó là gốc của dự án.

Bạn cũng có thể mở Zode từ terminal bên trong bất kỳ thư mục nào bằng:
`zode .`

Khi đã ở trong dự án:

- Dùng `Cmd+Shift+O` hoặc `Cmd+E` để nhảy nhanh giữa các file (như "Recent Files" của IntelliJ)
- Dùng `Cmd+Shift+A` hoặc `Shift Shift` để mở bảng lệnh (như "Search Everywhere" của IntelliJ)
- Dùng `Cmd+O` để tìm symbol (như "Go to Class" của IntelliJ)

Các buffer đang mở hiện thành tab dọc phía trên. Bảng dự án (Project Panel) cho thấy cây file và trạng thái Git. Bật/tắt nó bằng `Cmd+1` (giống tool window Project của IntelliJ).

## Khác biệt về phím tắt

Nếu bạn đã chọn keymap JetBrains lúc làm quen ban đầu, hầu hết phím tắt sẽ đã thấy quen. Dưới đây là bảng tra nhanh so sánh Zode với IntelliJ.

### Phím tắt chung (Zode với keymap JetBrains ↔ IntelliJ)

| Thao tác                      | Phím tắt                |
| ----------------------------- | ----------------------- |
| Search Everywhere             | `Shift Shift`           |
| Find Action / Command Palette | `Cmd + Shift + A`       |
| Go to File                    | `Cmd + Shift + O`       |
| Go to Symbol / Class          | `Cmd + O`               |
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

### Phím tắt khác nhau (IntelliJ → Zode)

| Thao tác               | IntelliJ    | Zode (keymap JetBrains)  |
| ---------------------- | ----------- | ------------------------ |
| File Structure         | `Cmd + F12` | `Cmd + F12` (outline)    |
| Navigate to Next Error | `F2`        | `F2`                     |
| Run                    | `Ctrl + R`  | `Ctrl + Alt + R` (tasks) |
| Debug                  | `Ctrl + D`  | `Alt + Shift + F9`       |
| Stop                   | `Cmd + F2`  | `Ctrl + F2`              |

### Chỉ có ở Zode

| Thao tác          | Phím tắt                   | Ghi chú                       |
| ----------------- | -------------------------- | ----------------------------- |
| Toggle Right Dock | `Cmd + R`                  | Bảng agent, thông báo         |
| Split Panes       | `Cmd + K`, then arrow keys | Tạo khung chia theo mọi hướng |

### Cách tuỳ chỉnh phím tắt

- Mở bảng lệnh (`Cmd+Shift+A` hoặc `Shift Shift`)
- Chạy `zode: open keymap`

Lệnh này mở danh sách mọi phím gán sẵn. Bạn có thể ghi đè từng phím tắt hoặc gỡ xung đột.

Zode cũng hỗ trợ chuỗi phím (phím tắt nhiều phím).

## Khác biệt về giao diện người dùng

### Không lập chỉ mục

Nếu từng dùng IntelliJ trên dự án lớn, bạn biết cảnh chờ: "Indexing..." có thể mất từ 30 giây đến 15 phút tuỳ kích thước dự án. IntelliJ xây một chỉ mục toàn diện cho toàn bộ codebase để chạy tính năng hiểu code, và nó lập chỉ mục lại khi dependency thay đổi hoặc sau khi build.

Zode không lập chỉ mục. Bạn mở thư mục và làm việc ngay. Tìm file và điều hướng chạy tức thì bất kể dự án lớn cỡ nào.

Chỉ mục của IntelliJ cung cấp các tính năng như tìm mọi chỗ dùng trên toàn codebase, hiểu cây phân cấp class và phát hiện code chết. Zode giao việc này cho các language server, vốn có thể không phân tích sâu bằng.

**Cách thích nghi:**

- Để tìm symbol trên toàn dự án, dùng `Cmd+O` / Go to Symbol (dựa vào language server của bạn)
- Để tìm file theo tên, dùng `Cmd+Shift+O` / Go to File
- Để tìm văn bản trên nhiều file, dùng `Cmd+Shift+F`—nhanh ngay cả với codebase lớn
- Nếu cần phân tích tĩnh sâu cho code JVM, hãy cân nhắc chạy inspection của IntelliJ như một bước riêng hoặc dùng công cụ độc lập như Checkstyle, PMD hay SpotBugs

### LSP so với hiểu ngôn ngữ nguyên bản

IntelliJ có engine phân tích ngôn ngữ riêng, xây từ đầu cho từng ngôn ngữ được hỗ trợ. Với Java, Kotlin và các ngôn ngữ JVM khác, engine này hiểu code của bạn rất kỹ: nó phân giải kiểu, theo dõi luồng dữ liệu, biết về annotation của framework và có hàng chục thao tác refactor chuyên biệt.

Zode dùng Language Server Protocol (LSP) để hiểu code. Mỗi ngôn ngữ có server riêng: `jdtls` cho Java, `rust-analyzer` cho Rust, v.v.

Với một số ngôn ngữ, trải nghiệm LSP rất tốt. TypeScript, Rust và Go có language server trưởng thành, cho gợi ý hoàn thành, chẩn đoán và refactor nhanh và chính xác. Với các ngôn ngữ JVM, khoảng cách có thể rõ hơn. Language server Java dựa trên Eclipse làm được nhiều việc, nhưng không sâu bằng IntelliJ ở những thứ như:

- Xử lý annotation của Spring và Jakarta EE
- Refactor phức tạp (extract interface, pull members up, đổi chữ ký hàm cùng mọi nơi gọi)
- Inspection nhận biết framework
- Tự động tối ưu import với quy tắc sắp xếp tuỳ chỉnh

**Cách thích nghi:**

- Dùng `Alt+Enter` để xem các code action có sẵn—danh sách sẽ khác nhau tuỳ language server
- Với Java, đảm bảo `jdtls` được cấu hình đúng với đường dẫn JDK của bạn trong cài đặt

### Không có mô hình dự án

IntelliJ quản lý dự án qua các thư mục `.idea` chứa file cấu hình XML, định nghĩa module `.iml`, gán SDK và run configuration. Mô hình này cho phép IntelliJ hiểu dự án nhiều module, tự quản lý dependency và lưu các thiết lập chạy/gỡ lỗi phức tạp.

Zode không có mô hình dự án. Một dự án là một thư mục. Không có wizard, không có màn hình chọn SDK, không có cấu hình module.

Điều này có nghĩa là:

- Lệnh build làm thủ công. Zode không nhận diện dự án Maven hay Gradle.
- Run configuration không tồn tại. Bạn định nghĩa task hoặc dùng terminal.
- Quản lý SDK nằm bên ngoài. Language server của bạn dùng JDK nào có trong PATH.
- Không có ranh giới module. Zode thấy thư mục, không thấy cấu trúc dự án.

**Cách thích nghi:**

- Tạo `.zode/settings.json` ở thư mục gốc dự án cho cài đặt riêng của dự án
- Định nghĩa các lệnh thường dùng trong `tasks.json` (mở qua bảng lệnh: `zode: open tasks`):

```json
[
  {
    "label": "build",
    "command": "./gradlew build"
  },
  {
    "label": "run",
    "command": "./gradlew bootRun"
  },
  {
    "label": "test current file",
    "command": "./gradlew test --tests $ZODE_STEM"
  }
]
```

- Dùng `Ctrl+Alt+R` để chạy task nhanh
- Dựa vào terminal (`Alt+F12`) cho mọi thứ task không phủ tới
- Với dự án nhiều module, bạn có thể mở mỗi module thành một cửa sổ Zode riêng, hoặc mở thư mục gốc và điều hướng qua bộ tìm file

### Không tích hợp framework

Giá trị của IntelliJ cho phát triển Java doanh nghiệp phần lớn đến từ tích hợp framework. Spring bean được hiểu và điều hướng được. JPA entity được xử lý đặc biệt. Endpoint được lập chỉ mục và tìm kiếm được. Annotation Jakarta EE làm đổi cách IDE phân tích code của bạn.

Zode không có những thứ này. Language server nhìn code Java như code Java, nên nó không hiểu `@Autowired` mang ý nghĩa đặc biệt hay class này là một REST controller.

Tương tự với các stack khác: không tích hợp Rails, không nhận biết Django, không có công cụ riêng cho Angular/React ngoài những gì language server TypeScript cung cấp.

**Cách thích nghi:**

- Dùng grep và tìm file thoải mái. `Cmd+Shift+F` kèm regex có thể tìm định nghĩa endpoint, tên bean hay chỗ dùng annotation.
- Dựa vào "find references" (`Alt+F7`) của language server để điều hướng—nó chạy được, chỉ là không có ngữ cảnh framework
- Với Spring Boot, giữ các endpoint Actuator hoặc một công cụ riêng để hiểu cách bean được nối với nhau
- Cân nhắc dùng các công cụ CLI riêng của framework (Spring CLI, Rails generator) từ terminal của Zode

> **Mẹo:** Với việc về cơ sở dữ liệu, hãy dùng công cụ chuyên dụng như DataGrip, DBeaver hay TablePlus. Nhiều lập trình viên chuyển sang Zode vẫn giữ DataGrip riêng cho SQL—nó tích hợp tốt với giấy phép JetBrains sẵn có của bạn.

Nếu công việc hằng ngày phụ thuộc nhiều vào điều hướng và refactor nhận biết framework, bạn sẽ thấy khoảng trống. Zode phù hợp nhất khi bạn thoải mái điều hướng code bằng tìm kiếm thay vì công cụ chuyên biệt, hoặc khi ngôn ngữ của bạn có LSP mạnh phủ phần lớn nhu cầu.

### Tool window và dock

IntelliJ sắp xếp các khung nhìn phụ thành tool window đánh số (Project = 1, Git = 9, Terminal = Alt+F12, v.v.). Zode dùng khái niệm tương tự gọi là "dock":

| Tool window của IntelliJ | Tương đương trong Zode | Phím tắt (keymap JetBrains) |
| ------------------------ | ---------------------- | --------------------------- |
| Project (1)              | Project Panel          | `Cmd + 1`                   |
| Git (9 or Cmd+0)         | Git Panel              | `Cmd + 0`                   |
| Terminal (Alt+F12)       | Terminal Panel         | `Alt + F12`                 |
| Structure (7)            | Outline Panel          | `Cmd + 7`                   |
| Problems (6)             | Diagnostics            | `Cmd + 6`                   |
| Debug (5)                | Debug Panel            | `Cmd + 5`                   |

Zode có ba vị trí dock: trái, dưới và phải. Có thể di chuyển các bảng giữa các dock bằng cách kéo hoặc qua cài đặt.

> **Mẹo:** IntelliJ có cài đặt "Override IDE shortcuts" để các phím tắt terminal như `Ctrl+Left/Right` hoạt động bình thường. Trong Zode, phím tắt của terminal tách riêng—hãy kiểm tra keymap nếu phím tắt quen thuộc không chạy trong bảng terminal.

### Gỡ lỗi

Cả IntelliJ và Zode đều có gỡ lỗi tích hợp, nhưng trải nghiệm khác nhau:

- Trình gỡ lỗi (debugger) của Zode dùng Debug Adapter Protocol (DAP), hỗ trợ nhiều ngôn ngữ
- Đặt breakpoint bằng `Ctrl+F8`
- Bắt đầu gỡ lỗi bằng `Alt+Shift+F9`
- Chạy từng bước với `F7` (step into), `F8` (step over), `Shift+F8` (step out)
- Tiếp tục chạy bằng `F9`

Bảng Debug (`Cmd+5`) hiển thị biến, call stack và breakpoint—tương tự tool window Debug của IntelliJ.

### Extension và plugin

IntelliJ có danh mục plugin lớn, phủ từ hỗ trợ ngôn ngữ đến công cụ cơ sở dữ liệu và tích hợp triển khai.

Danh mục extension của Zode nhỏ hơn và tập trung hơn:

- Hỗ trợ ngôn ngữ và tô sáng cú pháp
- Giao diện (theme)
- Context server

Nhiều tính năng cần plugin ở các trình soạn thảo khác đã có sẵn trong Zode:

- Cộng tác thời gian thực kèm chat thoại
- Hỗ trợ viết code bằng AI
- Terminal tích hợp
- Trình chạy task
- Hiểu code dựa trên LSP

Bạn sẽ không tìm được sản phẩm thay thế một-đổi-một cho mọi plugin của IntelliJ, đặc biệt là công cụ riêng của framework, client cơ sở dữ liệu hay tích hợp application server. Với những quy trình đó, bạn có thể phải dùng công cụ bên ngoài song song với Zode.

## Cộng tác trong Zode so với IntelliJ

IntelliJ có Code With Me, một plugin riêng để cộng tác. Zode tích hợp cộng tác ngay trong trải nghiệm cốt lõi.

- Mở bảng Collab ở dock bên trái
- Tạo một channel và [mời cộng tác viên](https://zodekit.com/docs/collaboration#inviting-a-collaborator) tham gia
- [Chia sẻ màn hình hoặc codebase của bạn](https://zodekit.com/docs/collaboration#share-a-project) trực tiếp

Khi đã kết nối, bạn sẽ thấy con trỏ, vùng chọn và chỉnh sửa của nhau theo thời gian thực. Có sẵn chat thoại. Không cần công cụ riêng hay đăng nhập bên thứ ba.

## Dùng AI trong Zode

Nếu bạn quen với trợ lý AI trong IntelliJ (như GitHub Copilot hay JetBrains AI), Zode có khả năng tương tự và linh hoạt hơn.

### Cấu hình GitHub Copilot

1. Mở Settings bằng `Cmd+,` (macOS) hoặc `Ctrl+,` (Linux/Windows)
2. Vào **AI → Edit Predictions**
3. Bấm **Configure** cạnh "Configure Providers"
4. Dưới **GitHub Copilot**, bấm **Sign in to GitHub**

Sau khi đăng nhập, bạn chỉ cần bắt đầu gõ. Zode sẽ gợi ý ngay trong dòng để bạn chấp nhận.

### Các lựa chọn AI khác

Để dùng các mô hình AI khác trong Zode, bạn có nhiều lựa chọn:

- Zode không có mô hình lưu trữ sẵn hay gói đăng ký: tính năng AI dùng key hoặc agent của riêng bạn.
- Dùng [API key](https://zodekit.com/docs/ai/llm-providers.html) của riêng bạn, không cần xác thực
- Dùng [agent bên ngoài như Claude Agent](https://zodekit.com/docs/ai/external-agents.html)

## Cấu hình nâng cao và mẹo tăng năng suất

Zode có các cài đặt nâng cao cho người dùng chuyên sâu muốn tinh chỉnh môi trường của mình.

Một vài mẹo hữu ích:

**Định dạng khi lưu:**

```json
"format_on_save": "on"
```

**Bật hỗ trợ direnv:**

```json
"load_direnv": "shell_hook"
```

**Cấu hình language server** (cần sửa JSON thủ công): Với phát triển Java, bạn có thể muốn cấu hình language server Java trong cài đặt:

```json
{
  "lsp": {
    "jdtls": {
      "settings": {
        "java_home": "/path/to/jdk"
      }
    }
  }
}
```

## Bước tiếp theo

Giờ bạn đã thiết lập xong, đây là một số tài nguyên giúp bạn dùng Zode hiệu quả hơn:

- [Tất cả cài đặt](../reference/all-settings.md) — Tuỳ chỉnh cài đặt, giao diện (theme) và hành vi của trình soạn thảo
- [Phím tắt](../key-bindings.md) — Học cách tuỳ chỉnh và mở rộng keymap
- [Task](../tasks.md) — Thiết lập lệnh build và run cho dự án
- [Ngôn ngữ](../languages.md) — Hướng dẫn thiết lập theo ngôn ngữ, gồm Java và Kotlin
