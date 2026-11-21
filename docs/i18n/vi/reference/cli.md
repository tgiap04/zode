---
title: CLI Reference
description: "Reference for Zode's command-line interface (CLI), including opening files and directories, integrating with tools, and controlling Zode from scripts."
---

# Tham chiếu CLI

Dùng giao diện dòng lệnh (CLI) của Zode để mở file và thư mục, tích hợp với công cụ khác và điều khiển Zode từ script.

## Cài đặt

**macOS:** Chạy lệnh `cli: install` từ bảng lệnh (command palette) ({#kb command_palette::Toggle}) để cài CLI `zode` vào `/usr/local/bin/zode`.

**Linux:** CLI được script cài đặt đặt ở `~/.local/bin/zode`. Xem [Zode trên Linux](../linux.md).

**Windows:** CLI đi kèm Zode. CLI là `zode.exe` trong thư mục `bin` của thư mục cài đặt. Nó nằm trong PATH nếu bạn đã tick tác vụ "Add to PATH" của trình cài đặt; nếu không, dùng đường dẫn đầy đủ.

## Cách dùng

```sh
zode [OPTIONS] [PATHS]...
```

## Mở file và thư mục

Mở một file:

```sh
zode myfile.txt
```

Mở một thư mục làm workspace:

```sh
zode ~/projects/myproject
```

Mở nhiều file hoặc thư mục:

```sh
zode file1.txt file2.txt ~/projects/myproject
```

Mở file tại một dòng và cột cụ thể:

```sh
zode myfile.txt:42        # Open at line 42
zode myfile.txt:42:10     # Open at line 42, column 10
```

## Tuỳ chọn

### `-w`, `--wait`

Chờ đóng hết các file đã mở rồi CLI mới thoát. Khi mở một thư mục, chờ tới khi cửa sổ đóng.

Hữu ích để tích hợp Zode với các công cụ cần trình soạn thảo chặn lại cho tới khi sửa xong (ví dụ `git commit`):

```sh
export EDITOR="zode --wait"
git commit  # Opens Zode and waits for you to close the commit message file
```

### `-n`, `--new`

Mở đường dẫn trong cửa sổ workspace mới, kể cả khi chúng đã mở trong một cửa sổ có sẵn:

```sh
zode -n ~/projects/myproject
```

### `-a`, `--add`

Thêm đường dẫn vào workspace đang focus thay vì mở cửa sổ mới. Khi có nhiều cửa sổ workspace, file mở trong cửa sổ đang focus:

```sh
zode -a newfile.txt
```

### `-r`, `--reuse`

Dùng lại một cửa sổ có sẵn, thay workspace hiện tại của nó bằng các đường dẫn mới:

```sh
zode -r ~/projects/different-project
```

### `--diff <OLD_PATH> <NEW_PATH>`

Mở giao diện diff so sánh hai file. Có thể chỉ định nhiều lần:

```sh
zode --diff file1.txt file2.txt
zode --diff old.rs new.rs --diff old2.rs new2.rs
```

### `--foreground`

Chạy Zode ở foreground, giữ terminal gắn với tiến trình. Hữu ích khi gỡ lỗi:

```sh
zode --foreground
```

### `--user-data-dir <DIR>`

Dùng một thư mục tuỳ chọn cho toàn bộ dữ liệu người dùng (cơ sở dữ liệu, extension, log) thay cho vị trí mặc định:

```sh
zode --user-data-dir ~/.zode-custom
```

Vị trí mặc định:

- **macOS:** `~/Library/Application Support/Zode`
- **Linux:** `$XDG_DATA_HOME/zode` (thường là `~/.local/share/zode`)
- **Windows:** `%LOCALAPPDATA%\Zode`

### `-v`, `--version`

In phiên bản Zode rồi thoát:

```sh
zode --version
```

### `--uninstall`

Gỡ cài đặt Zode và xoá mọi file liên quan (chỉ macOS và Linux):

```sh
zode --uninstall
```

### `--zode <PATH>`

Chỉ định đường dẫn tuỳ chọn tới ứng dụng hoặc binary Zode:

```sh
zode --zode /path/to/Zode.app myfile.txt
```

## Đọc từ standard input

Đọc nội dung từ stdin bằng cách truyền `-` làm đường dẫn:

```sh
echo "Hello, World!" | zode -
cat myfile.txt | zode -
ps aux | zode -
```

Lệnh này tạo một file tạm chứa nội dung stdin và mở nó trong Zode.

## Xử lý URL

CLI có thể mở các URL `zode://`, `http://` và `https://`:

```sh
zode zode://settings
zode https://github.com/zed-industries/zed
```

## Dùng Zode làm trình soạn thảo mặc định

Đặt Zode làm trình soạn thảo mặc định cho Git và các công cụ khác:

```sh
export EDITOR="zode --wait"
export VISUAL="zode --wait"
```

Thêm các dòng này vào file cấu hình shell (ví dụ `~/.bashrc`, `~/.zshrc`).

## macOS: chuyển kênh phát hành

Trên macOS, bạn có thể khởi chạy một kênh phát hành cụ thể bằng cách truyền tên kênh làm đối số đầu tiên:

```sh
zode --stable myfile.txt
zode --preview myfile.txt
zode --nightly myfile.txt
```

## Tích hợp WSL (Windows)

Trên Windows, CLI hỗ trợ mở đường dẫn từ các bản phân phối WSL. Việc này tự động xảy ra khi khởi chạy Zode từ bên trong WSL.

## Mã thoát

| Mã  | Ý nghĩa                     |
| --- | --------------------------- |
| `0` | Thành công                  |
| `1` | Lỗi (chi tiết in ra stderr) |

Khi dùng `--wait`, mã thoát cho biết các file đã được lưu trước khi đóng hay chưa.
