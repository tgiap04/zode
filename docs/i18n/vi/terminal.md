---
title: Built-in Terminal - Zode
description: Zode's integrated terminal with multiple instances, custom shells, and deep editor integration.
---

# Terminal

Zode có sẵn trình giả lập terminal, hỗ trợ nhiều terminal cùng lúc, shell tuỳ chỉnh và tích hợp sâu với trình soạn thảo.

## Mở terminal

| Thao tác                | macOS        | Linux/Windows |
| ----------------------- | ------------ | ------------- |
| Bật/tắt bảng terminal   | `` Ctrl+` `` | `` Ctrl+` ``  |
| Mở terminal mới         | `Ctrl+~`     | `Ctrl+~`      |
| Mở terminal ở vùng giữa | Bảng lệnh    | Bảng lệnh     |

Bạn cũng có thể mở terminal từ bảng lệnh (command palette) bằng `terminal panel: toggle` hoặc `workspace: new terminal`.

### Bảng terminal và terminal ở vùng giữa

Terminal có thể mở ở hai nơi:

- **Bảng terminal** — Gắn ở dưới (mặc định), trái hoặc phải của workspace. Bật/tắt bằng `` Ctrl+` ``.
- **Khung giữa** — Mở như một tab thường cạnh các file. Dùng `workspace: new center terminal` từ bảng lệnh.

## Làm việc với nhiều terminal

Tạo thêm terminal bằng `Cmd+N` (macOS) hoặc `Ctrl+N` (Linux/Windows) khi đang focus ở bảng terminal. Mỗi terminal hiện thành một tab trong bảng.

Chia terminal theo chiều ngang bằng `Cmd+D` (macOS) hoặc `Ctrl+Shift+5` (Linux/Windows).

## Cấu hình shell

Theo mặc định, Zode dùng shell mặc định của hệ thống (từ `/etc/passwd` trên các hệ Unix). Để dùng shell khác:

```json [settings]
{
  "terminal": {
    "shell": {
      "program": "/bin/zsh"
    }
  }
}
```

Để truyền đối số cho shell:

```json [settings]
{
  "terminal": {
    "shell": {
      "with_arguments": {
        "program": "/bin/bash",
        "args": ["--login"]
      }
    }
  }
}
```

## Thư mục làm việc

Điều khiển nơi các terminal mới bắt đầu:

| Giá trị                                       | Hành vi                                                                                                 |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `"current_file_directory"`                    | Dùng thư mục của file hiện tại, nếu không có thì dùng thư mục dự án, rồi dự án đầu tiên trong workspace |
| `"current_project_directory"`                 | Dùng thư mục dự án của file hiện tại (mặc định)                                                         |
| `"first_project_directory"`                   | Dùng dự án đầu tiên trong workspace của bạn                                                             |
| `"always_home"`                               | Luôn bắt đầu ở thư mục home của bạn                                                                     |
| `{ "always": { "directory": "~/projects" } }` | Luôn bắt đầu ở một thư mục cụ thể                                                                       |

```json [settings]
{
  "terminal": {
    "working_directory": "first_project_directory"
  }
}
```

## Biến môi trường

Thêm biến môi trường cho mọi phiên terminal:

```json [settings]
{
  "terminal": {
    "env": {
      "EDITOR": "zode --wait",
      "MY_VAR": "value"
    }
  }
}
```

> **Mẹo:** Dùng `:` để ngăn cách nhiều giá trị trong một biến: `"PATH": "/custom/path:$PATH"`

### Nhận diện môi trường ảo Python

Zode có thể tự động kích hoạt môi trường ảo Python khi mở terminal. Theo mặc định, nó tìm các thư mục `.env`, `env`, `.venv` và `venv`:

```json [settings]
{
  "terminal": {
    "detect_venv": {
      "on": {
        "directories": [".venv", "venv"],
        "activate_script": "default"
      }
    }
  }
}
```

Tuỳ chọn `activate_script` hỗ trợ `"default"`, `"csh"`, `"fish"` và `"nushell"`.

Để tắt việc nhận diện môi trường ảo:

```json [settings]
{
  "terminal": {
    "detect_venv": "off"
  }
}
```

## Phông chữ và giao diện

Terminal có thể dùng phông chữ khác với trình soạn thảo:

```json [settings]
{
  "terminal": {
    "font_family": "JetBrains Mono",
    "font_size": 14,
    "font_features": {
      "calt": false
    },
    "line_height": "comfortable"
  }
}
```

Các tuỳ chọn chiều cao dòng:

- `"comfortable"` — tỷ lệ 1.618, dễ đọc (mặc định)
- `"standard"` — tỷ lệ 1.3, phù hợp hơn cho ứng dụng TUI có ký tự vẽ khung
- `{ "custom": 1.5 }` — Tỷ lệ tuỳ chọn

### Con trỏ

Cấu hình giao diện con trỏ:

```json [settings]
{
  "terminal": {
    "cursor_shape": "bar",
    "blinking": "on"
  }
}
```

Hình dạng con trỏ: `"block"`, `"bar"`, `"underline"`, `"hollow"`

Tuỳ chọn nhấp nháy: `"off"`, `"terminal_controlled"` (mặc định), `"on"`

### Độ tương phản tối thiểu

Zode điều chỉnh màu terminal để giữ độ dễ đọc. Giá trị mặc định `45` đảm bảo chữ luôn nhìn rõ. Đặt `0` để tắt việc chỉnh tương phản và dùng đúng màu của giao diện (theme):

```json [settings]
{
  "terminal": {
    "minimum_contrast": 0
  }
}
```

## Cuộn

Duyệt lịch sử terminal bằng các phím tắt sau:

| Thao tác             | macOS                            | Linux/Windows    |
| -------------------- | -------------------------------- | ---------------- |
| Cuộn lên một trang   | `Shift+PageUp` hoặc `Cmd+Up`     | `Shift+PageUp`   |
| Cuộn xuống một trang | `Shift+PageDown` hoặc `Cmd+Down` | `Shift+PageDown` |
| Cuộn lên một dòng    | `Shift+Up`                       | `Shift+Up`       |
| Cuộn xuống một dòng  | `Shift+Down`                     | `Shift+Down`     |
| Cuộn lên đầu         | `Shift+Home` hoặc `Cmd+Home`     | `Shift+Home`     |
| Cuộn xuống cuối      | `Shift+End` hoặc `Cmd+End`       | `Shift+End`      |

Khi bạn đang cuộn lên trong lịch sử, một nút tròn nhỏ có mũi tên xuống hiện ở góc dưới bên phải terminal. Bấm vào để nhảy xuống cuối và focus terminal; tooltip của nó hiển thị phím tắt `terminal::ScrollToBottom` hiện tại của bạn.

Chỉnh tốc độ cuộn bằng:

```json [settings]
{
  "terminal": {
    "scroll_multiplier": 3.0
  }
}
```

## Sao chép và dán

| Thao tác | macOS   | Linux/Windows  |
| -------- | ------- | -------------- |
| Sao chép | `Cmd+C` | `Ctrl+Shift+C` |
| Dán      | `Cmd+V` | `Ctrl+Shift+V` |

### Sao chép khi chọn

Tự động sao chép văn bản đã chọn vào clipboard:

```json [settings]
{
  "terminal": {
    "copy_on_select": true
  }
}
```

### Giữ vùng chọn sau khi sao chép

Theo mặc định, văn bản vẫn được chọn sau khi sao chép. Để bỏ vùng chọn:

```json [settings]
{
  "terminal": {
    "keep_selection_on_copy": false
  }
}
```

## Tìm kiếm

Tìm nội dung terminal bằng `Cmd+F` (macOS) hoặc `Ctrl+Shift+F` (Linux/Windows). Thao tác này mở cùng thanh tìm kiếm dùng trong trình soạn thảo.

## Chế độ Vi

Bật/tắt điều hướng kiểu vi trong terminal bằng `Ctrl+Shift+Space`. Nhờ đó bạn điều hướng và chọn văn bản bằng phím tắt vi.

## Xoá terminal

Xoá màn hình terminal:

- macOS: `Cmd+K`
- Linux/Windows: `Ctrl+Shift+L`

## Option làm Meta (macOS)

Với người dùng Emacs hoặc ứng dụng dùng tổ hợp phím Meta, hãy bật Option as Meta:

```json [settings]
{
  "terminal": {
    "option_as_meta": true
  }
}
```

Cài đặt này hiểu phím Option là Meta, để các chuỗi như `Alt+X` hoạt động đúng.

## Chế độ cuộn thay thế

Khi bật, sự kiện cuộn chuột được chuyển thành phím mũi tên trong các ứng dụng như `vim` hoặc `less`:

```json [settings]
{
  "terminal": {
    "alternate_scroll": "on"
  }
}
```

## Siêu liên kết đường dẫn

Zode nhận diện đường dẫn file trong đầu ra của terminal và biến chúng thành liên kết bấm được. `Cmd+Click` (macOS) hoặc `Ctrl+Click` (Linux/Windows) mở file trong Zode, nhảy tới số dòng nếu phát hiện được.

Các định dạng thường gặp được nhận diện:

- `src/main.rs:42` — Mở tại dòng 42
- `src/main.rs:42:10` — Mở tại dòng 42, cột 10
- `File "script.py", line 10` — Traceback của Python

## Cấu hình bảng

### Vị trí dock

```json [settings]
{
  "terminal": {
    "dock": "bottom"
  }
}
```

Tuỳ chọn: `"bottom"` (mặc định), `"left"`, `"right"`

### Kích thước mặc định

```json [settings]
{
  "terminal": {
    "default_width": 640,
    "default_height": 320
  }
}
```

Khi terminal được gắn ở dưới, `default_height` đặt chiều cao của cả dock dưới, không chỉ của terminal — dock giữ một chiều cao chung cho mọi bảng gắn ở đó. Chuyển sang bảng debug không làm đổi cạnh; kéo khi bảng debug đang hoạt động cũng đổi cỡ terminal.

### Nút terminal

Ẩn nút terminal trên thanh trạng thái:

```json [settings]
{
  "terminal": {
    "button": false
  }
}
```

### Thanh công cụ

Hiện tiêu đề terminal trong thanh công cụ breadcrumb:

```json [settings]
{
  "terminal": {
    "toolbar": {
      "breadcrumbs": true
    }
  }
}
```

Shell của bạn có thể đặt tiêu đề bằng chuỗi escape `\e]2;Title\007`.

## Tích hợp với task

Terminal tích hợp với [hệ thống task](./tasks.md) của Zode. Khi bạn chạy một task, nó thực thi trong terminal. Chạy lại task cuối từ terminal bằng:

- macOS: `Cmd+Alt+R`
- Linux/Windows: `Ctrl+Shift+R` hoặc `Alt+T`

## Gửi văn bản và phím bấm

Để tuỳ chỉnh phím tắt nâng cao, bạn có thể gửi văn bản thô hoặc phím bấm tới terminal:

```json [keymap]
{
  "context": "Terminal",
  "bindings": {
    "alt-left": ["terminal::SendText", "\u001bb"],
    "ctrl-c": ["terminal::SendKeystroke", "ctrl-c"]
  }
}
```

## Tất cả cài đặt terminal

Danh sách đầy đủ cài đặt terminal xem ở [mục Terminal trong Tất cả cài đặt](./reference/all-settings.md#terminal).

## Tiếp theo

- [Task](./tasks.md) — Chạy lệnh và script từ Zode
- [REPL](./repl.md) — Thực thi code tương tác
- [Tham chiếu CLI](./reference/cli.md) — Giao diện dòng lệnh để mở file trong Zode
