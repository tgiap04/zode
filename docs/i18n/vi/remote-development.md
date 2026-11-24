---
title: Remote Development in Zode - SSH Workflows
description: Use remote development in Zode to edit code over SSH with local UI performance, remote terminals, language servers, and tasks.
---

# Phát triển từ xa

Phát triển từ xa cho phép bạn sửa code trên máy chủ từ xa trong khi chạy Zode ở máy mình. Giao diện luôn mượt vì chạy trên máy bạn, còn language server, task và terminal chạy trên máy chủ.

Để làm việc hằng ngày, hãy dùng phát triển từ xa cùng [Task](./tasks.md),
[Terminal](./terminal.md) và [Trình gỡ lỗi (debugger)](./debugger.md).

## Tổng quan

Phát triển từ xa cần hai máy tính: máy cục bộ chạy giao diện Zode và máy chủ từ xa chạy một server Zode headless. Hai bên giao tiếp qua SSH, nên bạn cần SSH được từ máy cục bộ vào máy chủ từ xa để dùng tính năng này.

![Sơ đồ kiến trúc của phát triển từ xa trong Zode](https://zed.dev/img/remote-development/diagram.png)

Trên máy cục bộ, Zode chạy giao diện, dùng Tree-sitter để phân tích và tô màu cú pháp, đồng thời lưu các thay đổi chưa lưu và các dự án gần đây. Mã nguồn, language server, task và terminal đều chạy trên máy chủ từ xa.

> **Lưu ý:** Phiên bản đầu của phát triển từ xa gửi lưu lượng qua máy chủ của Zode. Từ Zode v0.157, chế độ đó không còn dùng được.

## Thiết lập

1. Tải và cài [Zode](https://github.com/tgiap04/zode/releases) mới nhất.
1. Dùng {#kb projects::OpenRemote} để mở hộp thoại "Remote Projects".
1. Bấm "Connect New Server" và nhập lệnh bạn dùng để SSH vào máy chủ. Xem [Tuỳ chọn SSH được hỗ trợ](#supported-ssh-options) để biết các tuỳ chọn có thể truyền.
1. Máy cục bộ sẽ thử kết nối tới máy chủ từ xa bằng binary `ssh` trong PATH của bạn. Nếu kết nối thành công, Zode sẽ tải server về máy chủ từ xa và khởi động nó.
1. Khi server Zode đã chạy, bạn sẽ được nhắc chọn một đường dẫn để mở trên máy chủ từ xa.
   > **Lưu ý:** Hiện Zode chưa xử lý tốt việc mở các thư mục rất lớn (ví dụ `/` hay `~` có thể có >100.000 file). Chúng tôi đang cải thiện, nhưng tạm thời khuyên bạn chỉ mở các dự án cụ thể, hoặc các thư mục con của mono-repo rất lớn.

Với trường hợp đơn giản không cần tham số SSH, bạn có thể chạy `zode ssh://[<user>@]<host>[:<port>]/<path>` để mở thẳng một thư mục/file từ xa. Nếu muốn tạo liên kết nhảy thẳng vào một dự án SSH, dùng liên kết có dạng: `zode://ssh/[<user>@]<host>[:<port>]/<path>`.

## Nền tảng được hỗ trợ

Máy từ xa phải chạy được server của Zode. Các nền tảng sau nên hoạt động, tuy chúng tôi chưa kiểm tra hết mọi bản phân phối Linux:

- macOS Catalina trở lên (Intel hoặc Apple Silicon)
- Linux (x86_64 hoặc arm64, chưa hỗ trợ nền tảng 32-bit)
- Windows chưa được hỗ trợ làm máy chủ từ xa, nhưng Windows dùng được làm máy cục bộ để kết nối tới các máy chủ từ xa.

## Cấu hình

Danh sách máy chủ từ xa được lưu trong file cài đặt của bạn {#kb zode::OpenSettings}. Bạn có thể sửa danh sách này bằng hộp thoại Remote Projects {#kb projects::OpenRemote}, giúp an toàn hơn - ví dụ nó kiểm tra kết nối thiết lập được trước khi ghi vào file cài đặt.

```json [settings]
{
  "ssh_connections": [
    {
      "host": "192.168.1.10",
      "projects": [{ "paths": ["~/code/zed/zed"] }]
    }
  ]
}
```

Zode gọi `ssh` trong PATH của bạn, nên nó kế thừa mọi cấu hình bạn có trong `~/.ssh/config` cho host đó. Dù vậy, nếu cần ghi đè bất cứ gì, bạn có thể cấu hình thêm các tuỳ chọn sau cho từng kết nối:

```json [settings]
{
  "ssh_connections": [
    {
      "host": "192.168.1.10",
      "projects": [{ "paths": ["~/code/zed/zed"] }],
      // any argument to pass to the ssh master process
      "args": ["-i", "~/.ssh/work_id_file"],
      "port": 22, // defaults to 22
      // defaults to your username on your local machine
      "username": "me"
    }
  ]
}
```

Mỗi kết nối còn có hai tuỳ chọn riêng của Zode là `upload_binary_over_ssh` và `nickname`:

```json [settings]
{
  "ssh_connections": [
    {
      "host": "192.168.1.10",
      "projects": [{ "paths": ["~/code/zed/zed"] }],
      // by default Zode will download the server binary from the internet on the remote.
      // When this is true, it'll be downloaded to your laptop and uploaded over SSH.
      // This is useful when your remote server has restricted internet access.
      "upload_binary_over_ssh": true,
      // Shown in the Zode UI to help distinguish multiple hosts.
      "nickname": "lil-linux"
    }
  ]
}
```

Nếu bạn dùng dòng lệnh để mở kết nối tới một host bằng `zode ssh://192.168.1.10/~/.vimrc`, các tuỳ chọn bổ sung được đọc từ file cài đặt bằng cách tìm kết nối đầu tiên khớp host/username/port của URL trên dòng lệnh.

Ngoài ra, dù bạn có thể truyền mật khẩu trên dòng lệnh `zode ssh://user:password@host/~`, chúng tôi không hỗ trợ ghi mật khẩu vào file cài đặt. Nếu thường xuyên kết nối tới cùng một host, bạn nên cấu hình xác thực bằng khoá.

## Phát triển từ xa trên Windows (SSH)

Zode trên Windows hỗ trợ SSH từ xa và sẽ nhắc nhập thông tin xác thực khi cần.

Nếu gặp sự cố xác thực, hãy kiểm tra SSH key agent của bạn đang chạy (ví dụ ssh-agent hoặc agent của Git client) và ssh.exe nằm trong PATH.

### Xử lý sự cố SSH trên Windows

Khi được nhắc nhập thông tin xác thực, hãy dùng hộp thoại askpass đồ hoạ. Nếu nó không hiện, kiểm tra xung đột với credential manager và xem terminal của bạn có đang chặn hộp thoại đồ hoạ không.

## Hỗ trợ WSL

Zode hỗ trợ mở thư mục bên trong WSL ngay trên Windows.

### Mở thư mục cục bộ trong WSL

Để mở một thư mục cục bộ bên trong WSL, dùng action `projects: open in wsl` và chọn thư mục muốn mở. Bạn sẽ thấy danh sách các bản phân phối WSL có sẵn để mở thư mục trong đó.

### Mở thư mục đã nằm trong WSL

Để mở thư mục vốn đã nằm bên trong WSL, dùng action `projects: open wsl` và chọn bản phân phối WSL. Bản phân phối sẽ được thêm vào cửa sổ `Remote Projects`, nơi bạn mở được thư mục.

## Cái gì chạy ở đâu

Trong cửa sổ WSL, SSH hoặc dev container, công cụ của dự án chạy trên máy chứa dự án, không phải máy có màn hình bạn đang nhìn. Zode cần server từ xa của nó chạy ở đó để làm được vậy.

| Tính năng                                                                                             | Chạy ở đâu                                                                                                           |
| ----------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| CLI của agent (Claude Code, Codex, Copilot, opencode)                                                 | Trên host, trong thư mục dự án. Phiên, lịch sử và việc tiếp tục phiên được đọc từ thư mục agent của host.            |
| [Lịch sử phiên](./agent-sessions.md#remote-projects)                                                  | Liệt kê, tiếp tục, tách nhánh và xoá các phiên lưu trên host.                                                        |
| [Điều khiển từ xa](./remote-control.md)                                                               | Phiên của mọi host từ xa đang mở hiện trong History của thiết bị; app thiết bị bản mới đánh dấu host cho từng phiên. |
| Công cụ container và cluster (`docker`, `podman`, `kubectl`, `docker compose`), `gh`, `glab` và `git` | Bản tìm thấy trong `PATH` của host. Trong WSL, chương trình Windows dưới `/mnt/<letter>` không được dùng.            |
| Đồng bộ `.env` và vault                                                                               | Đọc và ghi qua file của host. Xem [file .env trong cửa sổ từ xa](#env-files-in-remote-windows).                      |
| Driver cơ sở dữ liệu                                                                                  | Được tải, cài và khởi động trên host. Xem [Driver cơ sở dữ liệu trên host](#database-drivers-on-a-host).             |
| [Đồ thị commit](./git.md#commit-graph)                                                                | Commit và chi tiết commit được đọc từ repository của host.                                                           |
| [Mức dùng của agent](./agent-usage.md#remote-windows)                                                 | Mức dùng của tài khoản đang đăng nhập trên host.                                                                     |

Nếu không kết nối được tới host, tính năng sẽ báo như vậy; nó không quay về máy cục bộ của bạn.

### File .env trong cửa sổ từ xa

Đồng bộ file `.env` trong cửa sổ từ xa đọc và ghi nó trên host. Trong cửa sổ WSL, dự án nằm trên ổ Windows (`/mnt/c`, `/mnt/d`, v.v.) không được đồng bộ: các file đó thuộc về máy Windows, không thuộc distro, nên hãy chuyển dự án vào hệ thống file riêng của distro để đồng bộ các file `.env`.

### Driver cơ sở dữ liệu trên host

Driver của bảng cơ sở dữ liệu được tính theo từng host. Lần đầu bạn kết nối tới một engine trong cửa sổ từ xa, driver được tải và cài trên host đó, và mọi cửa sổ trên host đó dùng lại nó. Driver cài trên máy cục bộ không giúp được dự án từ xa, và ngược lại.

### "Update the remote server…"

Zode thay server từ xa trên host khi phiên bản khác nhau, nhưng một cửa sổ vẫn có thể đang kết nối tới server cũ hơn, chưa có một tính năng nào đó. Tính năng như vậy sẽ báo thay vì không làm gì, ví dụ "Update the remote server to use database drivers here" hoặc "Update the remote server to sync this file", còn lịch sử phiên ghi "Remote server is out of date — reconnect, then refresh". Kết nối lại cửa sổ sẽ khiến Zode cài server khớp phiên bản (xem [Khởi tạo server từ xa](#initializing-the-remote-server)).

### "The remote server is older than this version of Zode"

Tải lại thư mục dự án (nút ↻ hoặc **Reload** trong Project Panel, xem [Project Panel](./visual-customization.md#project-panel)) yêu cầu server từ xa quét lại thư mục trên host. Server cũ không làm được, và trả lời "The remote server is older than this version of Zode. Reconnect to update it, then reload again." Hãy kết nối lại cửa sổ rồi tải lại.

### Đồ thị git trong cửa sổ từ xa

Phần Graph của Git Panel, phần commit của nó và tab Git Graph hoạt động trong cửa sổ từ xa; xem [Đồ thị commit](./git.md#commit-graph) để biết các trạng thái phần này có thể hiện. Nếu cửa sổ kết nối tới server cũ chưa có tính năng này, đồ thị ghi "The remote server is older than this version of Zode. Reconnect to update it to show the git graph." Hãy kết nối lại cửa sổ để cài server khớp phiên bản. Đồ thị tải lỗi ("Could not load the graph: ...") sẽ không tự thử lại: hãy tải lại bảng hoặc mở lại tab Git Graph.

## Chuyển tiếp cổng

Nếu muốn kết nối tới các cổng trên máy chủ từ xa từ máy cục bộ, bạn có thể cấu hình chuyển tiếp cổng trong file cài đặt. Đặc biệt hữu ích khi phát triển website, để bạn xem trang trên trình duyệt trong lúc làm việc.

```json [settings]
{
  "ssh_connections": [
    {
      "host": "192.168.1.10",
      "port_forwards": [{ "local_port": 8080, "remote_port": 80 }]
    }
  ]
}
```

Cấu hình này khiến request từ máy cục bộ tới `localhost:8080` được chuyển tới cổng 80 của máy từ xa. Bên dưới, nó dùng tham số `-L` của ssh.

Mặc định các cổng này gắn vào localhost, nên máy khác trong cùng mạng với máy phát triển của bạn không truy cập được. Bạn có thể đặt local_host để gắn vào giao diện khác, ví dụ 0.0.0.0 sẽ gắn vào mọi giao diện cục bộ.

```json [settings]
{
  "ssh_connections": [
    {
      "host": "192.168.1.10",
      "port_forwards": [
        {
          "local_port": 8080,
          "remote_port": 80,
          "local_host": "0.0.0.0"
        }
      ]
    }
  ]
}
```

Các cổng này cũng mặc định dùng giao diện `localhost` trên host từ xa. Nếu cần đổi, bạn cũng có thể đặt remote host:

```json [settings]
{
  "ssh_connections": [
    {
      "host": "192.168.1.10",
      "port_forwards": [
        {
          "local_port": 8080,
          "remote_port": 80,
          "remote_host": "docker-host"
        }
      ]
    }
  ]
}
```

## Cài đặt Zode

Khi mở một dự án từ xa có ba nơi chứa cài đặt liên quan:

- Cài đặt Zode cục bộ (trong `~/.zode/settings.json` trên macOS hoặc `~/.config/zode/settings.json` trên Linux) trên máy cục bộ.
- Cài đặt Zode của server (cùng vị trí) trên máy chủ từ xa.
- Cài đặt dự án (trong `.zode/settings.json` hoặc `.editorconfig` của dự án)

Cả Zode cục bộ và Zode server đều đọc cài đặt dự án, nhưng chúng không biết file cài đặt chính của bên kia.

Nên dùng file cài đặt nào tuỳ loại cài đặt bạn muốn đặt:

- Cài đặt dự án dùng cho những thứ ảnh hưởng tới dự án: thụt lề, formatter / language server nào được dùng, v.v.
- Cài đặt server dùng cho những thứ ảnh hưởng tới server: đường dẫn tới language server, cài đặt proxy, v.v.
- Cài đặt cục bộ dùng cho những thứ ảnh hưởng tới giao diện: cỡ chữ, v.v.

Ngoài ra, mọi extension bạn cài ở máy cục bộ sẽ được truyền sang máy chủ từ xa. Nhờ vậy language server, v.v. chạy đúng.

## Cấu hình proxy

Server từ xa sẽ không dùng cấu hình proxy của máy cục bộ vì hai bên có thể theo chính sách mạng khác nhau. Nếu máy chủ từ xa cần proxy để ra internet, bạn phải cấu hình ngay trên máy chủ từ xa.

Trong hầu hết trường hợp, máy chủ từ xa đã có sẵn biến môi trường proxy. Zode sẽ tự dùng chúng khi tải language server, giao tiếp với mô hình LLM, v.v.

Nếu cần, bạn có thể đặt các biến môi trường này trong cấu hình shell của server (ví dụ `~/.bashrc`):

```bash
export http_proxy="http://proxy.example.com:8080"
export https_proxy="http://proxy.example.com:8080"
export no_proxy="localhost,127.0.0.1"
```

Hoặc bạn có thể cấu hình proxy trong `~/.config/zode/settings.json` (Linux) hay `~/.zode/settings.json` (macOS) của máy từ xa:

```json
{
  "proxy": "http://proxy.example.com:8080"
}
```

Xem [tài liệu proxy](./reference/all-settings.md#network-proxy) để biết các loại proxy được hỗ trợ và tuỳ chọn cấu hình thêm.

## Khởi tạo server từ xa

Khi bạn đã cung cấp các tuỳ chọn SSH, Zode gọi `ssh` trên máy cục bộ để tạo một kết nối ControlMaster với các tuỳ chọn bạn đưa.

Mọi lời nhắc mà SSH cần sẽ hiện trên giao diện, nên bạn có thể xác minh host key, nhập mật khẩu khoá, v.v.

Khi kết nối master đã thiết lập, Zode kiểm tra xem binary của server từ xa có trong `~/.zode_server` trên máy từ xa không, và phiên bản của nó có khớp phiên bản Zode bạn đang dùng không.

Nếu chưa có hoặc phiên bản không khớp, Zode sẽ thử tải bản mới nhất. Mặc định nó tải trực tiếp từ [GitHub releases](https://github.com/tgiap04/zode/releases) của Zode, nhưng nếu bạn đặt: `{"upload_binary_over_ssh":true}` trong cài đặt cho server đó, nó sẽ tải binary về máy cục bộ rồi tải lên máy chủ từ xa.

Nếu muốn tự duy trì binary của server, bạn có thể làm vậy. Bạn có thể tải bản dựng sẵn của chúng tôi từ [GitHub](https://github.com/tgiap04/zode/releases), hoặc [tự build](https://zodekit.com/docs/development) bằng `cargo build -p remote_server --release`. Nếu làm vậy, bạn phải tải nó lên `~/.zode_server/zode-remote-server-{RELEASE_CHANNEL}-{VERSION}` trên server, ví dụ `~/.zode_server/zode-remote-server-stable-0.217.3+stable.105.80433cb239e868271457ac376673a5f75bc4adb1`. Phiên bản phải khớp chính xác với phiên bản Zode bạn đang dùng.

## Duy trì kết nối SSH

Khi server đã được khởi tạo, Zode sẽ tạo các kết nối SSH mới (dùng lại ControlMaster có sẵn) để chạy server phát triển từ xa.

Mỗi kết nối cố chạy server phát triển ở chế độ proxy. Chế độ này sẽ khởi động daemon nếu nó chưa chạy, và kết nối lại nếu nó đang chạy. Nhờ vậy khi kết nối rớt rồi được khởi động lại, bạn có thể tiếp tục làm việc không bị gián đoạn.

Trường hợp kết nối lại thất bại, daemon sẽ không được dùng lại. Dù vậy, các thay đổi chưa lưu mặc định được giữ cục bộ, nên bạn không mất công việc. Bạn luôn có thể kết nối lại dự án vào lúc khác và Zode sẽ khôi phục các thay đổi chưa lưu.

Nếu gặp sự cố kết nối, bạn có thể xem thêm thông tin trong log của Zode `cmd-shift-p Open Log`. Nếu thấy điều gì bất thường, hãy tạo [GitHub issue](https://github.com/tgiap04/zode/issues/new/choose).

## Tuỳ chọn SSH được hỗ trợ

Bên dưới, Zode gọi binary `ssh` để kết nối tới máy chủ từ xa. Chúng tôi tạo một SSH control master cho mỗi dự án, rồi dùng nó để ghép kênh các kết nối SSH cho chính giao thức Zode, mọi terminal bạn mở và task bạn chạy. Chúng tôi đọc cài đặt từ file cấu hình SSH của bạn, nhưng nếu muốn chỉ định thêm tuỳ chọn cho SSH control master, bạn có thể cấu hình Zode để đặt chúng.

Khi gõ trong hộp thoại "Connect New Server", bạn có thể dùng dấu nháy kiểu bash để truyền các tuỳ chọn chứa dấu cách. Sau khi tạo server, nó sẽ được thêm vào mảng `"ssh_connections": []` trong file cài đặt. Bạn có thể sửa trực tiếp file cài đặt để đổi các kết nối SSH.

Các tuỳ chọn được hỗ trợ:

- `-p` / `-l` - tương đương việc truyền cổng và tên người dùng trong chuỗi host.
- `-L` / `-R` để chuyển tiếp cổng
- `-i` - để dùng một file khoá cụ thể
- `-o` - để đặt tuỳ chọn tuỳ ý
- `-J` / `-w` - để proxy kết nối SSH
- `-F` để chỉ định một `ssh_config`
- Và cả... `-4`, `-6`, `-A`, `-B`, `-C`, `-D`, `-I`, `-K`, `-P`, `-X`, `-Y`, `-a`, `-b`, `-c`, `-i`, `-k`, `-l`, `-m`, `-o`, `-p`, `-w`, `-x`, `-y`

Lưu ý chúng tôi cố ý không cho phép một số tuỳ chọn (ví dụ `-t` hay `-T`) vì Zode sẽ tự đặt cho bạn.

## Hạn chế đã biết

- Bạn không thể mở file từ Terminal từ xa bằng cách gõ lệnh `zode`.

## Phản hồi

Hãy gửi phản hồi về phát triển từ xa bằng cách mở issue trong [kho mã Zode](https://github.com/tgiap04/zode/issues).

## Xem thêm

- [Chạy và kiểm thử](./running-testing.md): Chạy task, lệnh terminal và
  phiên gỡ lỗi khi bạn làm việc từ xa.
- [Cấu hình Zode](./configuring-zode.md): Quản lý cài đặt dùng chung và theo dự án,
  gồm `.zode/settings.json`.
- [Phát triển từ xa](https://zodekit.com/docs/remote-development): Tổng quan
  và cập nhật phát hành.
