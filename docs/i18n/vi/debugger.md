---
title: Debugger - Zode
description: Debug code in Zode with the Debug Adapter Protocol (DAP). Breakpoints, stepping, variable inspection across multiple languages.
---

# Trình gỡ lỗi (Debugger)

Zode dùng [Debug Adapter Protocol (DAP)](https://microsoft.github.io/debug-adapter-protocol/) để cung cấp chức năng gỡ lỗi cho nhiều ngôn ngữ lập trình.
DAP là giao thức chuẩn hoá, quy định cách trình gỡ lỗi, trình soạn thảo và IDE giao tiếp với nhau.
Nhờ đó Zode hỗ trợ được nhiều trình gỡ lỗi mà không phải tự cài đặt logic gỡ lỗi riêng cho từng ngôn ngữ.
Zode cài đặt phía client của giao thức, còn các _debug adapter_ cài đặt phía server.

Giao thức này cho phép đặt breakpoint, chạy từng bước qua code, xem biến,
và nhiều việc khác, theo cách nhất quán giữa các ngôn ngữ lập trình và môi trường chạy.

## Ngôn ngữ được hỗ trợ

Để gỡ lỗi code viết bằng một ngôn ngữ nào đó, Zode cần tìm được debug adapter cho ngôn ngữ đó. Một số debug adapter có sẵn trong Zode, không cần thiết lập thêm, số khác do [extension ngôn ngữ](./extensions/debugger-extensions.md) cung cấp. Hiện các ngôn ngữ sau có debug adapter:

<!-- keep this sorted -->

- [C](./languages/c.md#debugging) (tích hợp sẵn)
- [C++](./languages/cpp.md#debugging) (tích hợp sẵn)
- [Go](./languages/go.md#debugging) (tích hợp sẵn)
- [Java](./languages/java.md#debugging) (do extension cung cấp)
- [JavaScript](./languages/javascript.md#debugging) (tích hợp sẵn)
- [PHP](./languages/php.md#debugging) (tích hợp sẵn)
- [Python](./languages/python.md#debugging) (tích hợp sẵn)
- [Ruby](./languages/ruby.md#debugging) (do extension cung cấp)
- [Rust](./languages/rust.md#debugging) (tích hợp sẵn)
- [Swift](./languages/swift.md#debugging) (do extension cung cấp)
- [TypeScript](./languages/typescript.md#debugging) (tích hợp sẵn)

> Nếu ngôn ngữ của bạn chưa có trong danh sách, bạn có thể đóng góp bằng cách thêm debug adapter cho nó. Xem tài liệu [extension cho trình gỡ lỗi](./extensions/debugger-extensions.md) để biết thêm.

Theo các link trên để xem thông tin và ví dụ riêng cho từng ngôn ngữ và adapter, hoặc đọc tiếp phần các tính năng gỡ lỗi chung của Zode áp dụng cho mọi adapter.

## Bắt đầu

Với phần lớn ngôn ngữ, cách nhanh nhất để bắt đầu là chạy {#action debugger::Start} ({#kb debugger::Start}). Lệnh này mở _new process modal_, hiển thị danh sách debug task cấu hình sẵn theo ngữ cảnh của dự án hiện tại. Debug task được tạo từ test, điểm vào (như hàm `main`) và các nguồn khác — xem tài liệu của ngôn ngữ bạn dùng để biết đầy đủ những gì được hỗ trợ.

Bạn cũng có thể mở modal này bằng cách bấm nút "dấu cộng" ở góc trên bên phải của bảng debug.

Với các ngôn ngữ không có debug task cấu hình sẵn (gồm C, C++ và một số ngôn ngữ do extension hỗ trợ), bạn có thể định nghĩa cấu hình gỡ lỗi trong file `.zode/debug.json` ở thư mục gốc dự án. File này là một mảng các object cấu hình:

```json [debug]
[
  {
    "adapter": "CodeLLDB",
    "label": "First configuration"
    // ...
  },
  {
    "adapter": "Debugpy",
    "label": "Second configuration"
    // ...
  }
]
```

Xem tài liệu của ngôn ngữ bạn dùng để lấy cấu hình mẫu cho các trường hợp thường gặp. Khi đã thêm cấu hình vào `.zode/debug.json`, chúng sẽ hiện trong danh sách ở new process modal.

Zode cũng nạp cấu hình gỡ lỗi từ `.vscode/launch.json`, và hiển thị chúng trong new process modal nếu `.zode/debug.json` không có cấu hình nào.

#### Cấu hình gỡ lỗi toàn cục

Nếu bạn chạy cùng các profile khởi chạy ở nhiều dự án, có thể lưu chúng một lần trong cấu hình người dùng. Gọi {#action zode::OpenDebugTasks} từ bảng lệnh (command palette) để mở file `debug.json` toàn cục; Zode tạo nó cạnh `settings.json` của người dùng và giữ đồng bộ với giao diện debugger. File nằm ở:

- **macOS:** `~/.config/zode/debug.json`
- **Linux/BSD:** `$XDG_CONFIG_HOME/zode/debug.json` (nếu không có thì dùng `~/.config/zode/debug.json`)
- **Windows:** `%APPDATA%\Zode\debug.json`

Điền vào file này cùng loại mảng object bạn sẽ đặt trong `.zode/debug.json`. Mọi kịch bản định nghĩa ở đây được gộp vào mọi workspace, nên các preset khởi chạy ưa thích của bạn tự hiện trong hộp thoại "New Debug Session".

### Khởi chạy và đính kèm

Debugger của Zode có hai cách gỡ lỗi chương trình: _khởi chạy_ một phiên bản mới của chương trình, hoặc _đính kèm_ (attach) vào một tiến trình đang chạy.
Chọn cách nào tuỳ vào việc bạn muốn đạt được gì.

Khi khởi chạy phiên bản mới, Zode (và debug adapter bên dưới) thường lấy được thông tin gỡ lỗi tốt hơn so với đính kèm vào tiến trình có sẵn, vì nó kiểm soát vòng đời của cả chương trình.
Chạy unit test hoặc bản debug build của ứng dụng là trường hợp phù hợp để khởi chạy.

So với khởi chạy, đính kèm vào tiến trình có sẵn có vẻ kém hơn, nhưng thực ra không phải vậy; có những lúc bạn không thể khởi động lại chương trình, chẳng hạn vì lỗi không tái hiện được ngoài môi trường production hoặc do hoàn cảnh nào đó khác.

## Cấu hình

Zode yêu cầu các trường `adapter` và `label` cho mọi debug task. Ngoài ra, Zode dùng trường `build` để chạy các bước chuẩn bị cần thiết trước khi debugger khởi động [(xem bên dưới)](#build-tasks), và nhận trường `tcp_connection` để kết nối tới một tiến trình có sẵn.

Mọi trường khác do debug adapter cung cấp và có thể chứa [biến của task](./tasks.md#variables). Hầu hết adapter hỗ trợ `request`, `program` và `cwd`:

```json [debug]
[
  {
    // The label for the debug configuration and used to identify the debug session inside the debug panel & new process modal
    "label": "Example Start debugger config",
    // The debug adapter that Zode should use to debug the program
    "adapter": "Example adapter name",
    // Request:
    //  - launch: Zode will launch the program if specified, or show a debug terminal with the right configuration
    //  - attach: Zode will attach to a running program to debug it, or when the process_id is not specified, will show a process picker (only supported for node currently)
    "request": "launch",
    // The program to debug. This field supports path resolution with ~ or . symbols.
    "program": "path_to_program",
    // cwd: defaults to the current working directory of your project ($ZODE_WORKTREE_ROOT)
    "cwd": "$ZODE_WORKTREE_ROOT"
  }
]
```

Xem tài liệu của debug adapter để biết thêm về các trường nó hỗ trợ.

### Build task

Zode cho phép nhúng một task của Zode vào trường `build`, chạy trước khi debugger khởi động. Hữu ích để chuẩn bị môi trường hoặc chạy các bước thiết lập cần thiết trước khi debugger khởi động.

```json [debug]
[
  {
    "label": "Build Binary",
    "adapter": "CodeLLDB",
    "program": "path_to_program",
    "request": "launch",
    "build": {
      "command": "make",
      "args": ["build", "-j8"]
    }
  }
]
```

Build task cũng có thể tham chiếu các task có sẵn bằng label chưa thay thế biến:

```json [debug]
[
  {
    "label": "Build Binary",
    "adapter": "CodeLLDB",
    "program": "path_to_program",
    "request": "launch",
    "build": "my build task" // Or "my build task for $ZODE_FILE"
  }
]
```

### Tự động tạo kịch bản

Từ một task của Zode, Zode có thể tự tạo kịch bản cho bạn. Tính năng tự tạo kịch bản cũng là nền của việc tạo kịch bản từ gutter.
Hiện tính năng tự tạo kịch bản hỗ trợ Rust, Go, Python, JavaScript và TypeScript.

## Breakpoint

Để đặt breakpoint, chỉ cần bấm cạnh số dòng trong gutter của trình soạn thảo.
Có thể tinh chỉnh breakpoint theo nhu cầu; để xem thêm tuỳ chọn của một breakpoint, bấm chuột phải vào biểu tượng breakpoint trong gutter và chọn tuỳ chọn mong muốn.
Hiện bạn có thể:

- Thêm log vào breakpoint, sẽ in một thông điệp log mỗi khi breakpoint đó bị chạm tới.
- Biến breakpoint thành có điều kiện, chỉ dừng tại breakpoint khi điều kiện thoả. Cú pháp điều kiện tuỳ theo adapter.
- Thêm số lần chạm (hit count) cho breakpoint, chỉ dừng tại breakpoint sau khi nó bị chạm tới một số lần nhất định.
- Tắt một breakpoint, để nó không bị chạm tới nhưng vẫn hiện trong gutter.

Một số debug adapter (ví dụ CodeLLDB và JavaScript) còn _xác minh_ breakpoint của bạn có thể bị chạm tới hay không; breakpoint không thể chạm tới được hiển thị nổi bật hơn trong giao diện.

Mọi breakpoint đang bật của một dự án cũng được liệt kê ở mục "Breakpoints" trong giao diện phiên gỡ lỗi. Từ mục "Breakpoints" bạn cũng quản lý được các exception breakpoint.
Debug adapter sẽ dừng mỗi khi xảy ra một exception thuộc loại đã chọn. Loại exception nào được hỗ trợ tuỳ vào debug adapter.

## Làm việc với các khung chia

Khi gỡ lỗi với nhiều khung chia đang mở, Zode hiển thị dòng debug đang hoạt động trong một khung và giữ nguyên bố cục ở các khung còn lại. Nếu cùng một file mở ở nhiều khung, debugger chọn khung mà file đó đang là tab hoạt động—nó không đổi tab ở các khung mà file đang không hoạt động.

Khi debugger đã chọn một khung, nó tiếp tục dùng khung đó cho các breakpoint sau trong phiên. Nếu bạn kéo tab chứa dòng debug đang hoạt động sang một khung chia khác, debugger theo dõi việc di chuyển và dùng khung mới.

Nhờ vậy debugger không làm gián đoạn công việc của bạn khi chạy từng bước qua code ở nhiều file khác nhau.

## Cài đặt

Các cài đặt của debugger được gom dưới khoá `debugger` trong `settings.json`:

- `dock`: Vị trí của bảng debug trong giao diện.
- `stepping_granularity`: Độ chi tiết của mỗi bước.
- `save_breakpoints`: Có dùng lại breakpoint giữa các phiên Zode hay không.
- `button`: Có hiện nút debug trên thanh trạng thái hay không.
- `timeout`: Thời gian chờ tính bằng mili giây trước khi báo lỗi timeout khi kết nối tới debug adapter TCP.
- `log_dap_communications`: Có ghi log các thông điệp giữa debug adapter đang chạy và Zode hay không.
- `format_dap_log_messages`: Có định dạng các thông điệp DAP khi ghi vào logger của debug adapter hay không.

### Vị trí bảng (Dock)

- Mô tả: Vị trí của bảng debug trong giao diện.
- Mặc định: `bottom`
- Cài đặt: debugger.dock

**Tuỳ chọn**

1. `left` - Bảng debug được gắn vào bên trái giao diện.
2. `right` - Bảng debug được gắn vào bên phải giao diện.
3. `bottom` - Bảng debug được gắn vào cạnh dưới giao diện.

```json [settings]
"debugger": {
  "dock": "bottom"
},
```

### Độ chi tiết của bước

- Mô tả: Độ chi tiết của bước mà debugger dùng
- Mặc định: `line`
- Cài đặt: `debugger.stepping_granularity`

**Tuỳ chọn**

1. Statement - Mỗi bước cho chương trình chạy tới khi câu lệnh hiện tại thực thi xong.
   Thế nào là một câu lệnh do adapter quyết định và có thể coi tương đương một dòng.
   Ví dụ `for(int i = 0; i < 10; i++)` có thể coi là có 3 câu lệnh `int i = 0`, `i < 10` và `i++`.

```json [settings]
{
  "debugger": {
    "stepping_granularity": "statement"
  }
}
```

2. Line - Mỗi bước cho chương trình chạy tới khi dòng nguồn hiện tại thực thi xong.

```json [settings]
{
  "debugger": {
    "stepping_granularity": "line"
  }
}
```

3. Instruction - Mỗi bước cho phép một lệnh máy thực thi (ví dụ một lệnh x86).

```json [settings]
{
  "debugger": {
    "stepping_granularity": "instruction"
  }
}
```

### Lưu breakpoint

- Mô tả: Có lưu breakpoint giữa các phiên Zode hay không.
- Mặc định: `true`
- Cài đặt: `debugger.save_breakpoints`

**Tuỳ chọn**

Giá trị `boolean`

```json [settings]
{
  "debugger": {
    "save_breakpoints": true
  }
}
```

### Nút

- Mô tả: Có hiện nút trên thanh công cụ debugger hay không.
- Mặc định: `true`
- Cài đặt: `debugger.button`

**Tuỳ chọn**

Giá trị `boolean`

```json [settings]
{
  "debugger": {
    "button": true
  }
}
```

### Timeout

- Mô tả: Thời gian chờ tính bằng mili giây trước khi báo lỗi timeout khi kết nối tới debug adapter TCP.
- Mặc định: `2000`
- Cài đặt: `debugger.timeout`

**Tuỳ chọn**

Giá trị `integer`

```json [settings]
{
  "debugger": {
    "timeout": 3000
  }
}
```

### Giá trị inline

- Mô tả: Có bật inlay hint của trình soạn thảo để hiện giá trị biến ngay trong code khi gỡ lỗi hay không.
- Mặc định: `true`
- Cài đặt: `inlay_hints.show_value_hints`

**Tuỳ chọn**

```json [settings]
{
  "inlay_hints": {
    "show_value_hints": false
  }
}
```

Cũng có thể bật/tắt inline value hint từ menu Editor Controls trên thanh công cụ của trình soạn thảo.

### Ghi log giao tiếp DAP

- Mô tả: Có ghi log các thông điệp giữa debug adapter đang chạy và Zode hay không. (Dùng cho phát triển DAP)
- Mặc định: false
- Cài đặt: debugger.log_dap_communications

**Tuỳ chọn**

Giá trị `boolean`

```json [settings]
{
  "debugger": {
    "log_dap_communications": true
  }
}
```

### Định dạng log thông điệp DAP

- Mô tả: Có định dạng các thông điệp DAP khi ghi vào logger của debug adapter hay không. (Dùng cho phát triển DAP)
- Mặc định: false
- Cài đặt: debugger.format_dap_log_messages

**Tuỳ chọn**

Giá trị `boolean`

```json [settings]
{
  "debugger": {
    "format_dap_log_messages": true
  }
}
```

### Tuỳ chỉnh debug adapter

- Mô tả: Đường dẫn chương trình và đối số tuỳ chọn để ghi đè cách Zode khởi chạy một debug adapter cụ thể.
- Mặc định: tuỳ theo adapter
- Cài đặt: `dap.$ADAPTER.binary` và `dap.$ADAPTER.args`

Bạn có thể truyền `binary`, `args`, hoặc cả hai. `binary` phải là đường dẫn tới một _debug adapter_ (như `lldb-dap`) chứ không phải một _debugger_ (như chính `lldb`). Cài đặt `args` ghi đè mọi đối số mà Zode vốn sẽ truyền cho adapter.

```json [settings]
{
  "dap": {
    "CodeLLDB": {
      "binary": "/Users/name/bin/lldb-dap",
      "args": ["--wait-for-debugger"]
    }
  }
}
```

## Giao diện (Theme)

Debugger hỗ trợ các tuỳ chọn theme sau:

- `debugger.accent`: Màu dùng để nhấn breakpoint và các ký hiệu liên quan breakpoint
- `editor.debugger_active_line.background`: Màu nền của dòng debug đang hoạt động

## Xử lý sự cố

Nếu gặp vấn đề với debugger, hãy [mở một GitHub issue](https://github.com/tgiap04/zode/issues/new?template=10_bug_report.yml), cung cấp càng nhiều ngữ cảnh càng tốt. Cũng có vài tính năng giúp bạn thu thập thêm thông tin về vấn đề:

- Khi có một phiên đang chạy trong bảng debug, bạn có thể chạy action {#action dev::CopyDebugAdapterArguments} để chép vào clipboard một đoạn JSON mô tả cách Zode khởi tạo phiên. Đặc biệt hữu ích khi phiên không khởi động được, và là ngữ cảnh tốt để đính kèm nếu bạn mở GitHub issue.
- Bạn cũng có thể dùng action {#action dev::OpenDebugAdapterLogs} để xem vết toàn bộ giao tiếp của Zode với debug adapter trong các phiên gỡ lỗi gần nhất.
