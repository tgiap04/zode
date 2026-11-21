---
title: How to Migrate from WebStorm to Zode
description: "Guide for migrating from WebStorm to Zode, including settings and keybindings."
---

# Chuyển từ WebStorm sang Zode

Hướng dẫn này nói cách thiết lập Zode nếu bạn đến từ WebStorm, gồm phím tắt, cài đặt và những khác biệt bạn nên chờ đợi khi là lập trình viên JavaScript/TypeScript.

## Cài Zode

Zode có trên macOS, Windows và Linux.

Với macOS và Windows, tải bộ cài (`Zode-<arch>.dmg` hoặc `Zode-<arch>.exe`) từ [trang releases](https://github.com/tgiap04/zode/releases).

Với hầu hết người dùng Linux, cách dễ nhất để cài Zode là dùng script cài đặt của chúng tôi:

```sh
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | sh
```

Sau khi cài, bạn có thể mở Zode từ thư mục Applications (macOS), menu Start (Windows), hoặc trực tiếp từ terminal bằng:
`zode .`
Lệnh này mở thư mục hiện tại trong Zode.

## Thiết lập keymap JetBrains

Nếu bạn đến từ WebStorm, cách nhanh nhất để thấy quen thuộc là dùng keymap JetBrains. Khi làm quen ban đầu, bạn có thể chọn nó làm keymap cơ sở. Nếu bỏ lỡ bước đó, bạn có thể đổi bất cứ lúc nào:

1. Mở Cài đặt bằng {#kb zode::OpenSettings}
2. Tìm `Base Keymap`
3. Chọn `JetBrains`

Việc này ánh xạ các phím tắt quen thuộc như {#kb:jetbrains project_symbols::Toggle} cho Go to Class và {#kb:jetbrains command_palette::Toggle} cho Find Action.

## Thiết lập tùy chọn trình soạn thảo

Bạn có thể cấu hình hầu hết cài đặt trong trình chỉnh sửa Cài đặt ({#kb zode::OpenSettings}). Với cài đặt nâng cao, chạy `zode: open settings file` từ bảng lệnh (Command Palette) để sửa trực tiếp file cài đặt.

Những cài đặt người dùng WebStorm thường cấu hình đầu tiên:

| Cài đặt Zode            | Tác dụng                                                                       |
| ----------------------- | ------------------------------------------------------------------------------ |
| `format_on_save`        | Tự động định dạng khi lưu. Đặt `"on"` để bật.                                  |
| `soft_wrap`             | Ngắt dòng dài. Tùy chọn: `"none"`, `"editor_width"`, `"preferred_line_length"` |
| `preferred_line_length` | Độ rộng cột để ngắt dòng và vẽ đường kẻ. Mặc định là 80.                       |
| `inlay_hints`           | Hiện tên tham số và gợi ý kiểu ngay trong dòng, giống gợi ý của WebStorm.      |
| `relative_line_numbers` | Hữu ích nếu bạn đến từ IdeaVim.                                                |

Zode cũng hỗ trợ cài đặt theo từng dự án. Tạo file `.zode/settings.json` ở thư mục gốc dự án để ghi đè cài đặt toàn cục cho dự án đó, tương tự cách bạn dùng thư mục `.idea` trong WebStorm.

> **Mẹo:** Nếu bạn tham gia một dự án có sẵn, hãy kiểm tra `format_on_save` trước khi commit lần đầu. Nếu không, bạn có thể vô tình định dạng lại cả file trong khi chỉ định sửa một dòng.

## Mở hoặc tạo dự án

Sau khi thiết lập, dùng {#kb:jetbrains file_finder::Toggle} để mở một thư mục. Thư mục này trở thành workspace của bạn trong Zode. Khác WebStorm, không có trình hướng dẫn cấu hình dự án, không có hộp thoại chọn framework, và không cần thiết lập cấu trúc dự án.

Để bắt đầu dự án mới, tạo một thư mục bằng terminal hoặc trình quản lý file, rồi mở nó trong Zode. Trình soạn thảo sẽ coi thư mục đó là gốc của dự án. Với dự án mới, thường bạn chạy `npm init`, `pnpm create` hoặc công cụ CLI của framework trước, rồi mở thư mục kết quả trong Zode.

Bạn cũng có thể mở Zode từ terminal bên trong bất kỳ thư mục nào bằng:
`zode .`

Khi đã ở trong dự án:

- Dùng {#kb:jetbrains file_finder::Toggle} để nhảy nhanh giữa các file (như "Recent Files" của WebStorm)
- Dùng {#kb:jetbrains command_palette::Toggle} để mở bảng lệnh (Command Palette) (như "Search Everywhere" của WebStorm)
- Dùng {#kb:jetbrains project_symbols::Toggle} để tìm symbol (như "Go to Symbol" của WebStorm)

Các buffer đang mở hiện thành tab ở phía trên. Bảng Project hiện cây file và trạng thái Git. Bật/tắt nó bằng {#kb:jetbrains project_panel::ToggleFocus} (giống cửa sổ công cụ Project của WebStorm).

## Khác biệt về phím tắt

Nếu bạn đã chọn keymap JetBrains khi làm quen ban đầu, hầu hết phím tắt sẽ thấy quen. Dưới đây là bảng tham chiếu nhanh các thao tác thường dùng và phím tắt tương ứng khi keymap JetBrains đang bật.

### Phím tắt thường dùng

| Thao tác             | Phím tắt Zode                                   |
| -------------------- | ----------------------------------------------- |
| Command Palette      | {#kb:jetbrains command_palette::Toggle}         |
| Go to File           | {#kb:jetbrains file_finder::Toggle}             |
| Go to Symbol         | {#kb:jetbrains project_symbols::Toggle}         |
| File Outline         | {#kb:jetbrains outline::Toggle}                 |
| Go to Definition     | {#kb:jetbrains editor::GoToDefinition}          |
| Find Usages          | {#kb:jetbrains editor::FindAllReferences}       |
| Rename Symbol        | {#kb:jetbrains editor::Rename}                  |
| Reformat Code        | {#kb:jetbrains editor::Format}                  |
| Bật/tắt bảng Project | {#kb:jetbrains project_panel::ToggleFocus}      |
| Bật/tắt Terminal     | {#kb:jetbrains terminal_panel::Toggle}          |
| Nhân đôi dòng        | {#kb:jetbrains editor::DuplicateSelection}      |
| Xóa dòng             | {#kb:jetbrains editor::DeleteLine}              |
| Chuyển dòng lên      | {#kb:jetbrains editor::MoveLineUp}              |
| Chuyển dòng xuống    | {#kb:jetbrains editor::MoveLineDown}            |
| Mở rộng vùng chọn    | {#kb:jetbrains editor::SelectLargerSyntaxNode}  |
| Thu hẹp vùng chọn    | {#kb:jetbrains editor::SelectSmallerSyntaxNode} |
| Comment dòng         | {#kb:jetbrains editor::ToggleComments}          |
| Quay lại             | {#kb:jetbrains pane::GoBack}                    |
| Đi tiếp              | {#kb:jetbrains pane::GoForward}                 |
| Bật/tắt breakpoint   | {#kb:jetbrains editor::ToggleBreakpoint}        |
| Tới lỗi kế tiếp      | {#kb:jetbrains editor::GoToDiagnostic}          |

### Chỉ có ở Zode

| Thao tác             | Phím tắt                         | Ghi chú                                            |
| -------------------- | -------------------------------- | -------------------------------------------------- |
| Bật/tắt dock phải    | {#kb workspace::ToggleRightDock} | Bảng Agent, thông báo                              |
| Tách khung sang phải | {#kb pane::SplitRight}           | Dùng các phím mũi tên khác để tách theo hướng khác |

### Cách tùy biến phím tắt

- Mở bảng lệnh (Command Palette) ({#kb:jetbrains command_palette::Toggle})
- Chạy `zode: open keymap`

Lệnh này mở danh sách mọi phím tắt có sẵn. Bạn có thể ghi đè từng phím tắt hoặc gỡ xung đột.

Zode cũng hỗ trợ chuỗi phím (phím tắt nhiều phím).

## Khác biệt về giao diện người dùng

### Không lập chỉ mục

Nếu bạn từng dùng WebStorm trên dự án lớn, bạn biết cảm giác chờ đợi. Mở dự án có nhiều thư viện phụ thuộc có thể phải ngồi nhìn "Indexing..." từ 30 giây đến vài phút. WebStorm lập chỉ mục toàn bộ mã nguồn và `node_modules` để vận hành khả năng hiểu mã, và lập chỉ mục lại khi thư viện phụ thuộc thay đổi.

Zode không lập chỉ mục. Bạn mở thư mục và bắt đầu viết mã ngay—không thanh tiến trình, không biểu ngữ "Indexing paused". Tìm file và điều hướng luôn nhanh bất kể dự án lớn cỡ nào hay có bao nhiêu thư viện phụ thuộc trong `node_modules`.

Chỉ mục của WebStorm cho phép các tính năng như tìm mọi chỗ dùng trong toàn bộ mã nguồn, theo dõi cây import, và cảnh báo export không dùng trên toàn dự án. Zode dựa vào language server cho việc phân tích này, nên có thể không bao phủ rộng bằng.

**Cách thích nghi:**

- Tìm symbol trên toàn dự án bằng {#kb:jetbrains project_symbols::Toggle} (do TypeScript language server đảm nhiệm)
- Tìm file theo tên bằng {#kb:jetbrains file_finder::Toggle}
- Dùng {#kb pane::DeploySearch} để tìm văn bản—vẫn nhanh ngay cả trong monorepo lớn
- Chạy `tsc --noEmit` hoặc `eslint .` từ terminal khi cần phân tích sâu hơn trên toàn dự án

### LSP so với khả năng hiểu mã gốc

WebStorm có engine phân tích JavaScript và TypeScript riêng do JetBrains xây dựng. Engine này hiểu mã của bạn rất sâu: phân giải kiểu, theo dõi luồng dữ liệu, biết các mẫu riêng của từng framework, và cung cấp các thao tác refactor chuyên biệt.

Zode dùng Language Server Protocol (LSP) để hiểu mã. Với JavaScript và TypeScript, Zode hỗ trợ:

- **vtsls** (mặc định) — TypeScript language server nhanh, hiệu năng xuất sắc
- **typescript-language-server** — Bản triển khai TypeScript LSP chuẩn
- **ESLint** — Tích hợp lint
- **Prettier** — Định dạng mã (tích hợp sẵn)

Trải nghiệm TypeScript LSP được hỗ trợ tốt. Bạn có gợi ý chính xác, kiểm tra kiểu, đi tới định nghĩa và tìm tham chiếu. Trải nghiệm tương đương VS Code, vốn dùng cùng các dịch vụ TypeScript bên dưới.

Những chỗ bạn có thể thấy khác biệt:

- Khả năng hiểu riêng của framework (template Angular, Vue SFC) có thể ít được tích hợp hơn
- Một số thao tác refactor phức tạp (tách component kèm import đúng) có thể kém tinh vi hơn
- Gợi ý auto-import phụ thuộc vào những gì language server biết về dự án của bạn

**Cách thích nghi:**

- Dùng {#kb:jetbrains editor::ToggleCodeActions} để xem các code action có sẵn—danh sách sẽ khác nhau tùy language server
- Đảm bảo `tsconfig.json` được cấu hình đúng để language server hiểu cấu trúc dự án của bạn
- Dùng Prettier để định dạng nhất quán (mặc định đã bật cho JS/TS)
- Để kiểm tra mã giống "Inspect Code" của WebStorm, xem bảng Diagnostics ({#kb:jetbrains diagnostics::Deploy})—ESLint và TypeScript cùng nhau bắt được nhiều lỗi giống nhau

### Không có mô hình dự án

WebStorm quản lý dự án qua các thư mục `.idea` chứa file cấu hình XML, nhận diện framework và cấu hình chạy. Mô hình này cho phép WebStorm nhớ cài đặt dự án, quản lý script npm qua giao diện, và lưu thiết lập chạy/gỡ lỗi.

Zode theo cách khác: dự án chỉ là một thư mục. Không có trình hướng dẫn thiết lập, không có hộp thoại nhận diện framework, không có cấu trúc dự án để cấu hình.

Trong thực tế điều này nghĩa là:

- Không có khái niệm cấu hình chạy. Hãy định nghĩa các lệnh dùng lại trong `tasks.json`. Lưu ý các cấu hình `.idea/` hiện có của bạn sẽ không chuyển sang—bạn sẽ thiết lập lại những cái cần.
- Script npm nằm ở terminal. Chạy trực tiếp `npm run dev`, `pnpm build` hoặc `yarn test`—không có bảng npm riêng.
- Không nhận diện framework. Zode đối xử với React, Angular, Vue và JS/TS thuần như nhau.

**Cách thích nghi:**

- Tạo `.zode/settings.json` ở thư mục gốc dự án cho cài đặt riêng của dự án
- Định nghĩa các lệnh thường dùng trong `tasks.json` (mở qua bảng lệnh (Command Palette): `zode: open tasks`):

```json
[
  {
    "label": "dev",
    "command": "npm run dev"
  },
  {
    "label": "build",
    "command": "npm run build"
  },
  {
    "label": "test",
    "command": "npm test"
  },
  {
    "label": "test current file",
    "command": "npm test -- $ZODE_FILE"
  }
]
```

- Dùng {#kb:jetbrains task::Spawn} để chạy task nhanh
- Dựa vào terminal ({#kb:jetbrains terminal_panel::Toggle}) cho mọi thứ task không phủ tới

### Không tích hợp framework

Giá trị của WebStorm cho phát triển web phần lớn đến từ tích hợp framework. Component React được xử lý đặc biệt. Angular có công cụ riêng. Single-file component của Vue được hiểu đầy đủ. Cửa sổ công cụ npm hiện mọi script của bạn.

Zode không có thứ nào trong số này sẵn. TypeScript language server coi mã của bạn là TypeScript—nó không hiểu một hàm là component React hay một file là service Angular.

**Cách thích nghi:**

- Dùng grep và tìm file thoải mái. {#kb pane::DeploySearch} với regex có thể tìm định nghĩa component, cấu hình route hay endpoint API.
- Dựa vào "find references" ({#kb:jetbrains editor::FindAllReferences}) của language server để điều hướng—nó chạy được, chỉ là không có ngữ cảnh framework
- Cân nhắc dùng công cụ CLI riêng của framework (`ng`, `next`, `vite`) từ terminal của Zode
- Với React, cú pháp JSX/TSX và kiểu TypeScript vẫn cho khả năng hiểu mã tốt

> **Mẹo:** Với dự án có cấu hình phức tạp, hãy để sẵn tài liệu của framework bên cạnh. Tốc độ của Zode đi kèm việc ít cầm tay chỉ việc hơn cho các tính năng riêng của framework.

### Cửa sổ công cụ so với dock

WebStorm sắp xếp các khung nhìn phụ vào các cửa sổ công cụ đánh số. Zode dùng khái niệm tương tự gọi là "dock":

| Cửa sổ công cụ WebStorm | Tương đương trong Zode | Phím tắt Zode                              |
| ----------------------- | ---------------------- | ------------------------------------------ |
| Project                 | Bảng Project           | {#kb:jetbrains project_panel::ToggleFocus} |
| Git                     | Bảng Git               | {#kb:jetbrains git_panel::ToggleFocus}     |
| Terminal                | Bảng Terminal          | {#kb:jetbrains terminal_panel::Toggle}     |
| Structure               | Bảng Outline           | {#kb:jetbrains outline_panel::ToggleFocus} |
| Problems                | Diagnostics            | {#kb:jetbrains diagnostics::Deploy}        |
| Debug                   | Bảng Debug             | {#kb:jetbrains debug_panel::ToggleFocus}   |

Zode có ba vị trí dock: trái, dưới và phải. Có thể di chuyển bảng giữa các dock bằng cách kéo thả hoặc qua cài đặt.

Lưu ý Zode không có cửa sổ công cụ npm riêng. Hãy dùng terminal hoặc định nghĩa task cho các script npm thường dùng.

### Gỡ lỗi

Cả WebStorm và Zode đều có gỡ lỗi tích hợp cho JavaScript và TypeScript:

- Zode dùng `vscode-js-debug` (cùng debug adapter mà VS Code dùng)
- Đặt breakpoint bằng {#kb:jetbrains editor::ToggleBreakpoint}
- Bắt đầu gỡ lỗi bằng {#kb:jetbrains debugger::Start}
- Duyệt từng bước bằng {#kb:jetbrains debugger::StepInto} (step into), {#kb:jetbrains debugger::StepOver} (step over), {#kb:jetbrains debugger::StepOut} (step out)
- Tiếp tục chạy bằng {#kb:jetbrains debugger::Continue}

Zode có thể gỡ lỗi:

- Ứng dụng và script Node.js
- JavaScript trên Chrome/trình duyệt
- Jest, Mocha, Vitest và các framework test khác
- Next.js (cả phía server lẫn client)

Để kiểm soát nhiều hơn, tạo file `.zode/debug.json`:

```json
[
  {
    "label": "Debug Current File",
    "adapter": "JavaScript",
    "program": "$ZODE_FILE",
    "request": "launch"
  },
  {
    "label": "Debug Node Server",
    "adapter": "JavaScript",
    "request": "launch",
    "program": "${workspaceFolder}/src/server.js"
  },
  {
    "label": "Attach to Chrome",
    "adapter": "JavaScript",
    "request": "attach",
    "port": 9222
  }
]
```

Zode cũng nhận diện cấu hình `.vscode/launch.json`, nên các thiết lập gỡ lỗi VS Code hiện có thường dùng được ngay.

### Chạy test

WebStorm có trình chạy test riêng với giao diện hiển thị trạng thái đạt/hỏng của từng test. Zode chạy test qua:

- **Biểu tượng ở gutter** — Bấm nút play cạnh hàm test hoặc khối describe
- **Task** — Định nghĩa lệnh test trong `tasks.json`
- **Terminal** — Chạy trực tiếp `npm test`, `jest`, `vitest`, v.v.

Zode hỗ trợ tự phát hiện các framework test phổ biến:

- Jest
- Mocha
- Vitest
- Jasmine
- Bun test
- Node.js test runner

Đầu ra test hiện trong bảng terminal. Với Jest, dùng `--verbose` để có đầu ra chi tiết hoặc `--watch` để test liên tục trong lúc phát triển.

### Extension so với plugin

WebStorm có danh mục plugin phủ việc hỗ trợ thêm ngôn ngữ, giao diện (theme) và tích hợp công cụ.

Danh mục extension của Zode nhỏ hơn và tập trung hơn:

- Hỗ trợ ngôn ngữ và tô màu cú pháp
- Giao diện (theme)
- Context server

Một số tính năng cần plugin trong WebStorm được tích hợp sẵn trong Zode:

- Cộng tác thời gian thực kèm chat thoại
- Hỗ trợ lập trình bằng AI
- Terminal tích hợp
- Trình chạy task
- Khả năng hiểu mã dựa trên LSP
- Định dạng bằng Prettier
- Tích hợp ESLint

### Những gì Zode không có

Để đặt kỳ vọng rõ ràng, đây là những gì WebStorm có mà Zode không có:

- **Cửa sổ công cụ npm** — Dùng terminal hoặc task thay thế
- **HTTP Client** — Dùng công cụ như Postman, Insomnia hoặc curl
- **Công cụ cơ sở dữ liệu** — Dùng DataGrip, DBeaver hoặc TablePlus
- **Công cụ riêng của framework** (Angular schematics, refactor React) — Dùng công cụ CLI
- **Trình soạn thảo package.json trực quan** — Sửa trực tiếp file
- **REST client tích hợp** — Dùng công cụ bên ngoài hoặc extension
- **Tích hợp profiler** — Dùng Chrome DevTools hoặc công cụ profiling của Node.js

## Cộng tác trong Zode so với WebStorm

WebStorm cung cấp Code With Me như một tính năng riêng để cộng tác. Zode tích hợp cộng tác ngay trong trải nghiệm cốt lõi.

- Mở bảng Collab ở dock bên trái
- Tạo kênh và [mời người cộng tác](https://zodekit.com/docs/collaboration#inviting-a-collaborator) tham gia
- [Chia sẻ màn hình hoặc mã nguồn](https://zodekit.com/docs/collaboration#share-a-project) trực tiếp

Khi đã kết nối, bạn sẽ thấy con trỏ, vùng chọn và chỉnh sửa của nhau theo thời gian thực. Có sẵn chat thoại. Không cần công cụ riêng hay đăng nhập bên thứ ba.

## Dùng AI trong Zode

Nếu bạn quen với trợ lý AI trong WebStorm (như GitHub Copilot, JetBrains AI Assistant hay Junie), Zode có khả năng tương tự và linh hoạt hơn.

### Cấu hình GitHub Copilot

1. Mở Cài đặt bằng {#kb zode::OpenSettings}
2. Vào **AI → Edit Predictions**
3. Bấm **Configure** cạnh "Configure Providers"
4. Ở mục **GitHub Copilot**, bấm **Sign in to GitHub**

Khi đã đăng nhập, cứ bắt đầu gõ. Zode sẽ đưa gợi ý ngay trong dòng để bạn chấp nhận.

### Các lựa chọn AI khác

Để dùng các mô hình AI khác trong Zode, bạn có nhiều lựa chọn:

- Zode không có mô hình lưu trữ sẵn hay gói đăng ký: tính năng AI dùng key hoặc agent của riêng bạn.
- Dùng [khóa API](https://zodekit.com/docs/ai/llm-providers.html) của riêng bạn, không cần xác thực
- Dùng [agent bên ngoài như Claude Agent](https://zodekit.com/docs/ai/external-agents.html)

## Cấu hình nâng cao và mẹo tăng năng suất

Zode cung cấp các cài đặt nâng cao cho người dùng thành thạo muốn tinh chỉnh môi trường.

Dưới đây là vài mẹo hữu ích cho lập trình viên JavaScript/TypeScript:

**Định dạng khi lưu:**

```json
"format_on_save": "on"
```

**Cấu hình Prettier làm trình định dạng mặc định** (cần sửa JSON thủ công):

```json
{
  "formatter": {
    "external": {
      "command": "prettier",
      "arguments": ["--stdin-filepath", "{buffer_path}"]
    }
  }
}
```

**Bật code action của ESLint** (cần sửa JSON thủ công):

```json
{
  "lsp": {
    "eslint": {
      "settings": {
        "codeActionOnSave": {
          "rules": ["import/order"]
        }
      }
    }
  }
}
```

**Cấu hình gợi ý chế độ strict của TypeScript:**

Trong `tsconfig.json`, bật chế độ strict để kiểm tra kiểu tốt hơn:

```json
{
  "compilerOptions": {
    "strict": true,
    "noUncheckedIndexedAccess": true
  }
}
```

**Bật hỗ trợ direnv (hữu ích cho dự án dùng direnv để quản lý biến môi trường):**

```json
"load_direnv": "shell_hook"
```

## Bước tiếp theo

Giờ bạn đã thiết lập xong, đây là một số tài nguyên giúp bạn dùng Zode hiệu quả nhất:

- [Tất cả cài đặt](../reference/all-settings.md) — Tùy chỉnh cài đặt, giao diện (theme) và hành vi trình soạn thảo
- [Phím tắt](../key-bindings.md) — Học cách tùy biến và mở rộng keymap của bạn
- [Task](../tasks.md) — Thiết lập lệnh build và chạy cho dự án
- [JavaScript trong Zode](../languages/javascript.md) — Thiết lập và cấu hình riêng cho JavaScript
- [TypeScript trong Zode](../languages/typescript.md) — Thiết lập và cấu hình riêng cho TypeScript
