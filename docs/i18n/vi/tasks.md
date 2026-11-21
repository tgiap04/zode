---
title: Tasks - Run Commands in Zode
description: Run and rerun shell commands from Zode with task definitions. Supports variables, templates, and language-specific tasks.
---

# Task

Zode hỗ trợ các cách chạy (và chạy lại) lệnh bằng [terminal](./terminal.md) tích hợp để xuất kết quả. Các lệnh này đọc được một phần giới hạn trạng thái của Zode (như đường dẫn của file đang sửa hoặc đoạn văn bản đang chọn).

```json [tasks]
[
  {
    "label": "Example task",
    "command": "for i in {1..5}; do echo \"Hello $i/5\"; sleep 1; done",
    //"args": [],
    // Env overrides for the command, will be appended to the terminal's environment from the settings.
    "env": { "foo": "bar" },
    // Current working directory to spawn the command into, defaults to current project root.
    //"cwd": "/path/to/working/directory",
    // Whether to use a new terminal tab or reuse the existing one to spawn the process, defaults to `false`.
    "use_new_terminal": false,
    // Whether to allow multiple instances of the same task to be run, or rather wait for the existing ones to finish, defaults to `false`.
    "allow_concurrent_runs": false,
    // What to do with the terminal pane and tab, after the command was started:
    // * `always` — always show the task's pane, and focus the corresponding tab in it (default)
    // * `no_focus` — always show the task's pane, add the task's tab in it, but don't focus it
    // * `never` — do not alter focus, but still add/reuse the task's tab in its pane
    "reveal": "always",
    // What to do with the terminal pane and tab, after the command has finished:
    // * `never` — Do nothing when the command finishes (default)
    // * `always` — always hide the terminal tab, hide the pane also if it was the last tab in it
    // * `on_success` — hide the terminal tab on task success only, otherwise behaves similar to `always`
    "hide": "never",
    // Which shell to use when running a task inside the terminal.
    // May take 3 values:
    // 1. (default) Use the system's default terminal configuration in /etc/passwd
    //      "shell": "system"
    // 2. A program:
    //      "shell": {
    //        "program": "sh"
    //      }
    // 3. A program with arguments:
    //     "shell": {
    //         "with_arguments": {
    //           "program": "/bin/bash",
    //           "args": ["--login"]
    //         }
    //     }
    "shell": "system",
    // Whether to show the task line in the output of the spawned task, defaults to `true`.
    "show_summary": true,
    // Whether to show the command line in the output of the spawned task, defaults to `true`.
    "show_command": true,
    // Which edited buffers to save before running the task:
    // * `all` — save all edited buffers
    // * `current` — save currently active buffer only
    // * `none` — don't save any buffers
    "save": "none"
    // Represents the tags for inline runnable indicators, or spawning multiple tasks at once.
    // "tags": []
  }
]
```

Có hai action điều khiển quy trình dùng task: `task: spawn` và `task: rerun`.
`task: spawn` mở một modal liệt kê mọi task có trong file hiện tại.
`task: rerun` chạy lại task được chạy gần nhất. Bạn cũng có thể chạy lại task từ modal task.

Theo mặc định, chạy lại task dùng lại cùng terminal (do mặc định `"use_new_terminal": false`) nhưng chờ task trước kết thúc rồi mới bắt đầu (do mặc định `"allow_concurrent_runs": false`).

Giữ `"use_new_terminal": false` và đặt `"allow_concurrent_runs": true` để cho phép huỷ task trước khi chạy lại.

## Template task

Task có thể được định nghĩa:

- trong file `tasks.json` toàn cục; các task này dùng được trong mọi dự án Zode bạn làm việc. File này thường nằm ở `~/.config/zode/tasks.json`. Bạn sửa chúng bằng action `zode: open tasks`.
- trong file `.zode/tasks.json` riêng của worktree (cục bộ); các task này chỉ dùng được khi làm việc với dự án có worktree đó. Bạn sửa task riêng của worktree bằng action `zode: open project tasks`.
- ngay lập tức bằng [task oneshot](#oneshot-tasks). Các task này gắn với dự án và không được giữ lại giữa các phiên.
- bởi extension ngôn ngữ.

## Biến

Task của Zode hoạt động y như shell của bạn; nghĩa là bạn có thể tham chiếu biến môi trường bằng cú pháp `$VAR_NAME` kiểu sh. Một vài biến môi trường bổ sung được đặt sẵn cho tiện.
Các biến này cho phép bạn lấy thông tin từ trình soạn thảo hiện tại và dùng trong task. Các biến có sẵn:

- `ZODE_COLUMN`: cột của dòng hiện tại
- `ZODE_ROW`: số dòng hiện tại
- `ZODE_FILE`: đường dẫn tuyệt đối của file đang mở (ví dụ `/Users/my-user/path/to/project/src/main.rs`)
- `ZODE_FILENAME`: tên file của file đang mở (ví dụ `main.rs`)
- `ZODE_DIRNAME`: đường dẫn tuyệt đối của file đang mở, bỏ tên file (ví dụ `/Users/my-user/path/to/project/src`)
- `ZODE_RELATIVE_FILE`: đường dẫn của file đang mở, tương đối so với `ZODE_WORKTREE_ROOT` (ví dụ `src/main.rs`)
- `ZODE_RELATIVE_DIR`: đường dẫn thư mục của file đang mở, tương đối so với `ZODE_WORKTREE_ROOT` (ví dụ `src`)
- `ZODE_STEM`: phần tên (tên file không có phần mở rộng) của file đang mở (ví dụ `main`)
- `ZODE_SYMBOL`: symbol đang chọn; khớp với symbol cuối cùng hiện trong breadcrumb symbol (ví dụ `mod tests > fn test_task_contexts`)
- `ZODE_SELECTED_TEXT`: văn bản đang chọn
- `ZODE_LANGUAGE`: ngôn ngữ của buffer đang mở (ví dụ `Rust`, `Python`, `Shell Script`)
- `ZODE_WORKTREE_ROOT`: đường dẫn tuyệt đối tới gốc của worktree hiện tại. (ví dụ `/Users/my-user/path/to/project`)
- `ZODE_CUSTOM_RUST_PACKAGE`: (riêng cho Rust) tên package cha của file nguồn $ZODE_FILE.

Để dùng biến trong task, thêm dấu đô la (`$`) phía trước:

```json [tasks]
{
  "label": "echo current file's path",
  "command": "echo $ZODE_FILE"
}
```

Bạn cũng có thể dùng cú pháp dài, cho phép chỉ định giá trị mặc định nếu biến không có: `${ZODE_FILE:default_value}`

Các biến môi trường này cũng dùng được trong các trường `cwd`, `args` và `label` của task.

### Đặt dấu nháy cho biến

Khi làm việc với đường dẫn có dấu cách hoặc ký tự đặc biệt khác, hãy đảm bảo các biến được escape đúng.

Ví dụ, thay vì cách này (sẽ lỗi nếu đường dẫn có dấu cách):

```json [tasks]
{
  "label": "stat current file",
  "command": "stat $ZODE_FILE"
}
```

Hãy viết như sau:

```json [tasks]
{
  "label": "stat current file",
  "command": "stat",
  "args": ["$ZODE_FILE"]
}
```

Hoặc ghi rõ dấu nháy được escape như sau:

```json [tasks]
{
  "label": "stat current file",
  "command": "stat \"$ZODE_FILE\""
}
```

### Lọc task theo biến

Định nghĩa task có biến mà tại thời điểm xác định danh sách task biến đó không tồn tại sẽ bị lọc bỏ.
Ví dụ, task sau chỉ hiện trong modal spawn khi có đoạn văn bản được chọn:

```json [tasks]
{
  "label": "selected text",
  "command": "echo \"$ZODE_SELECTED_TEXT\""
}
```

Đặt giá trị mặc định cho các biến như vậy để các task đó luôn được hiển thị:

```json [tasks]
{
  "label": "selected text with default",
  "command": "echo \"${ZODE_SELECTED_TEXT:no text selected}\""
}
```

## Task oneshot

Cũng modal task mở bằng `task: spawn` hỗ trợ chạy lệnh kiểu bash tuỳ ý: gõ một lệnh vào ô văn bản của modal, rồi dùng `opt-enter` để chạy nó.

Modal task giữ các lệnh tạm này trong suốt phiên, `task: rerun` cũng sẽ chạy lại các task như vậy nếu chúng là task được chạy gần nhất.

Bạn cũng có thể chỉnh task đang chọn trong modal (`tab` là phím tắt mặc định). Làm vậy sẽ đưa lệnh của nó vào một ô nhập để sửa rồi chạy như task oneshot.

### Task tạm thời

Bạn có thể dùng phím bổ trợ `cmd` khi chạy task qua modal; task chạy theo cách này sẽ không được tăng số lần dùng (do đó chúng sẽ không được chạy lại bằng `task: rerun` và không có thứ hạng cao trong modal task).
Mục đích của task tạm thời là giữ mạch làm việc khi dùng `task: rerun` liên tục.

### Kiểm soát thêm việc chạy lại task

Theo mặc định, task ghi biến của nó vào một ngữ cảnh một lần, và "task đã phân giải" này luôn được chạy lại.

Có thể điều khiển việc này bằng đối số `"reevaluate_context"` của task: đặt thành `true` sẽ buộc task được đánh giá lại trước mỗi lần chạy.

```json [keymap]
{
  "context": "Workspace",
  "bindings": {
    "alt-t": ["task::Rerun", { "reevaluate_context": true }]
  }
}
```

## Phím tắt tuỳ chỉnh cho task

Bạn có thể định nghĩa phím tắt riêng cho task bằng một đối số bổ sung cho `task::Spawn`. Nếu muốn gán task `echo current file's path` ở trên cho `alt-g`, bạn thêm đoạn sau vào file [`keymap.json`](./key-bindings.md):

```json [keymap]
{
  "context": "Workspace",
  "bindings": {
    "alt-g": ["task::Spawn", { "task_name": "echo current file's path" }]
  }
}
```

Lưu ý các task này cũng có thể chỉ định một 'target' để điều khiển nơi task được chạy hiện ra.
Điều này hữu ích để mở một ứng dụng terminal mà bạn muốn dùng ở vùng giữa:

```json [tasks]
// In tasks.json
{
  "label": "start lazygit",
  "command": "lazygit -p $ZODE_WORKTREE_ROOT"
}
```

```json [keymap]
// In keymap.json
{
  "context": "Workspace",
  "bindings": {
    "alt-g": [
      "task::Spawn",
      { "task_name": "start lazygit", "reveal_target": "center" }
    ]
  }
}
```

## Định dạng task của VS Code

Khi nhập task VS Code từ `.vscode/tasks.json`, bạn có thể bỏ trường `label`. Zode tự tạo nhãn dựa trên loại task:

- **Task npm**: `npm: <script>` (ví dụ `npm: start`)
- **Task gulp**: `gulp: <task>` (ví dụ `gulp: build`)
- **Task shell**: Dùng thẳng chuỗi `command` (ví dụ `echo hello`), hoặc `shell` nếu command rỗng
- **Task không có type**: `Untitled Task`

File task mẫu với nhãn tự tạo:

```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "type": "npm",
      "script": "start"
    },
    {
      "type": "shell",
      "command": "cargo build --release"
    }
  ]
}
```

Các task này hiện trong bộ chọn task là "npm: start" và "cargo build --release". Bạn có thể ghi đè nhãn tự tạo bằng cách cung cấp trường `label` tường minh.

## Gán tag runnable cho template task

Zode hỗ trợ ghi đè action mặc định của các chỉ báo runnable nội tuyến qua file `tasks.json` cục bộ của workspace và toàn cục theo thứ tự ưu tiên sau:

1. `tasks.json` của workspace
2. `tasks.json` toàn cục
3. Gán tag do ngôn ngữ cung cấp (mặc định).

Để gắn tag cho một task, thêm tên tag runnable vào trường `tags` của template task:

```json [tasks]
{
  "label": "echo current file's path",
  "command": "echo $ZODE_FILE",
  "tags": ["rust-test"]
}
```

Làm vậy, bạn có thể đổi task nào được hiển thị trong chỉ báo runnable.

## Phím tắt chạy task gắn với runnable

Khi có định nghĩa task gắn với runnable, bạn có thể chạy nhanh bằng [Code Actions](https://zodekit.com/docs/configuring-languages?#code-actions), kích hoạt qua lệnh `editor: Toggle Code Actions` hoặc phím tắt `cmd-.`/`ctrl-.`. Task của bạn sẽ đứng đầu trong danh sách thả xuống. Task chạy ngay nếu dòng đó không có Code Action nào khác.

## Chạy script Bash

Bạn có thể chạy script bash trực tiếp từ Zode. Khi mở file `.sh` hoặc `.bash`, Zode tự nhận script là runnable và đưa nó vào bộ chọn task.

Để chạy một script bash:

1. Mở bảng lệnh (command palette) bằng {#kb command_palette::Toggle}
2. Tìm "task" và chọn **task: spawn**
3. Chọn script từ danh sách

Script bash được gắn tag `bash-script`, cho phép bạn lọc hoặc tham chiếu chúng trong cấu hình task.

Nếu cần truyền đối số hoặc tuỳ chỉnh môi trường chạy, hãy thêm cấu hình task vào `.zode/tasks.json`:

```json
[
  {
    "label": "run my-script.sh with args",
    "command": "./my-script.sh",
    "args": ["--verbose", "--output=results.txt"],
    "tags": ["bash-script"]
  }
]
```

## Khởi tạo shell

Khi Zode chạy một task, nó mở lệnh trong login shell. Điều này đảm bảo các file khởi tạo của shell (`.bash_profile`, `.zshrc`, v.v.) được nạp trước khi task thực thi.

Hành vi này cho task truy cập cùng biến môi trường, alias và các thay đổi PATH mà bạn đã cấu hình trong profile shell. Nếu một task không tìm thấy lệnh mà chạy được trong terminal của bạn, hãy kiểm tra các file cấu hình shell đã thiết lập đúng chưa.

Để ghi đè shell dùng cho task, cấu hình cài đặt `terminal.shell`:

```json
{
  "terminal": {
    "shell": {
      "program": "/bin/zsh"
    }
  }
}
```

Xem [cấu hình Terminal](./terminal.md) để biết đầy đủ các tuỳ chọn shell.
