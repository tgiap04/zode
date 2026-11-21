---
title: Zode on Linux
description: "The installation script on the download page is the fastest way to install Zode:"
---

# Zode trên Linux

## Cài đặt tiêu chuẩn

Script cài đặt là cách nhanh nhất để cài Zode:

```sh
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | sh
```

Chúng tôi cũng có bản build nightly của Zode. Bạn cài nó bằng:

```sh
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | ZODE_CHANNEL=nightly sh
```

Để cài một phiên bản cụ thể, đặt `ZODE_VERSION`, ví dụ `ZODE_VERSION=0.1.6`. Bản preview không được phát hành. Các tên biến cũ hơn `ZODE_VERSION` và `ZODE_CHANNEL` vẫn được chấp nhận.

Zode cài bằng script chạy tốt nhất trên hệ thống:

- có GPU tương thích Vulkan (ví dụ Linux trên MacBook dòng M)
- có glibc cài toàn hệ thống (NixOS và Alpine mặc định thì không), phiên bản >= 2.35 —
  Ubuntu 22.04 trở lên, Debian 12 trở lên — trên cả x86_64 và aarch64

Bản build của chúng tôi vẫn chạy trên NixOS và Alpine nếu bạn cài lớp tương thích glibc. Trên NixOS bạn có thể thử [nix-ld](https://github.com/Mic92/nix-ld), còn trên Alpine là [gcompat](https://wiki.alpinelinux.org/wiki/Running_glibc_programs).

Bạn cần build từ mã nguồn cho:

- kiến trúc không phải Intel 64-bit hoặc ARM 64-bit (ví dụ máy 32-bit hoặc RISC-V)
- Redhat Enterprise Linux 8.x, Rocky Linux 8, AlmaLinux 8, Amazon Linux 2 trên mọi kiến trúc
- Redhat Enterprise Linux 9.x, Rocky Linux 9.3, AlmaLinux 8, Amazon Linux 2023 trên aarch64 (x86_x64 thì được)

## Các cách khác để cài Zode trên Linux

Zode là mã nguồn mở, và [bạn có thể cài từ mã nguồn](./development/linux.md).

### Cài qua trình quản lý gói

Chưa có bản phân phối Linux hay trình quản lý gói nào đóng gói Zode. Gói tên `zed` hoặc `zed-editor` cài Zed gốc, không phải Zode.

### Tải thủ công

Nếu muốn, bạn cài Zode bằng cách tải file .tar.gz dựng sẵn của chúng tôi. Đây là cùng artifact mà script cài đặt dùng, nhưng bạn tuỳ chỉnh được vị trí cài đặt bằng cách sửa các hướng dẫn bên dưới:

Tải file `.tar.gz` từ [trang releases](https://github.com/tgiap04/zode/releases):

- [zode-linux-x86_64.tar.gz](https://github.com/tgiap04/zode/releases/latest/download/zode-linux-x86_64.tar.gz)
- [zode-linux-aarch64.tar.gz](https://github.com/tgiap04/zode/releases/latest/download/zode-linux-aarch64.tar.gz)

Rồi đảm bảo binary `zode` trong tarball nằm trong path của bạn. Cách dễ nhất là giải nén tarball và tạo symlink:

```sh
mkdir -p ~/.local/bin
# extract zode to ~/.local/zode.app/
tar -xvf <path/to/download>.tar.gz -C ~/.local
# link the zode binary to ~/.local/bin (or another directory in your $PATH)
ln -sf ~/.local/zode.app/bin/zode ~/.local/bin/zode
```

Nếu muốn tích hợp với môi trường desktop tương thích XDG, bạn cũng cần cài file `.desktop`:

```sh
install -D ~/.local/zode.app/share/applications/io.github.tgiap04.zode.desktop -t ~/.local/share/applications
sed -i "s|Icon=zode|Icon=$HOME/.local/zode.app/share/icons/hicolor/512x512/apps/zode.png|g" ~/.local/share/applications/io.github.tgiap04.zode.desktop
sed -i "s|Exec=zode|Exec=$HOME/.local/zode.app/bin/zode|g" ~/.local/share/applications/io.github.tgiap04.zode.desktop
```

### Nâng cấp từ Zode 0.1.5

Zode 0.1.5 trên Linux cài vào `~/.local/zed.app` (`~/.local/zed-nightly.app` ở kênh nightly) và liên kết `~/.local/bin/zed`. Từ 0.1.6 bố cục là `~/.local/zode.app` (`~/.local/zode-nightly.app`) và `~/.local/bin/zode`, nên trình cập nhật trong ứng dụng của 0.1.5 không áp dụng được bản cập nhật 0.1.6. Hãy cài lại một lần bằng tay với script cài đặt ở trên. Sau đó, cập nhật trong ứng dụng chạy như trước.

Các script không bao giờ xoá thư mục cũ. `~/.local/zed.app` và `~/.local/bin/zed` cũng là nơi Zed gốc dùng, nên hãy kiểm tra trước khi xoá: bản cài Zode 0.1.5 có `~/.local/zed.app/share/applications/io.github.tgiap04.zode.desktop` (`~/.local/zed-nightly.app/share/applications/io.github.tgiap04.zode.nightly.desktop` ở kênh nightly), còn Zed gốc thì không. Chỉ xoá chúng nếu file đó tồn tại.

Đừng chạy `--uninstall` từ binary 0.1.5 còn sót (`zed --uninstall`, `~/.local/bin/zed --uninstall` hoặc `~/.local/zed.app/bin/zed --uninstall`). Zode 0.1.5 nhúng script gỡ cài đặt cũ, script này xoá `~/.local/share/zed`, `~/.config/zed` và `~/.zode_server`: các thư mục mà Zed gốc lưu dữ liệu riêng. Hãy xoá thư mục ứng dụng cũ bằng tay sau khi kiểm tra ở trên, và chỉ dùng `zode --uninstall` khi bạn đã ở 0.1.6 trở lên.

## Gỡ cài đặt Zode

### Gỡ cài đặt tiêu chuẩn

Nếu Zode được cài bằng script cài đặt mặc định, bạn gỡ nó bằng cách truyền cờ `--uninstall` cho lệnh shell `zode`

```sh
zode --uninstall
```

Nếu không có lỗi, shell sẽ hỏi bạn muốn giữ hay xoá các tuỳ chọn của mình. Sau khi chọn, bạn sẽ thấy thông báo Zode đã được gỡ cài đặt thành công.

Trong trường hợp không tìm thấy lệnh shell `zode` trong PATH, bạn có thể thử một trong các lệnh sau

```sh
$HOME/.local/bin/zode --uninstall
```

hoặc

```sh
$HOME/.local/zode.app/bin/zode --uninstall
```

Trường hợp đầu có thể thất bại nếu symlink giữa `$HOME/.local/bin/zode` và `$HOME/.local/zode.app/bin/zode` chưa được tạo đúng. Nhưng trường hợp thứ hai nên chạy được miễn là Zode được cài ở vị trí mặc định.

Nếu Zode được cài ở vị trí khác, bạn phải gọi binary `zode` nằm trong thư mục cài đặt đó và truyền cờ `--uninstall` cho nó theo cùng định dạng như các lệnh trước.

### Trình quản lý gói

Nếu Zode được cài bằng trình quản lý gói, hãy xem tài liệu của trình quản lý gói đó về cách gỡ một gói.

## Xử lý sự cố

Linux chạy trên rất nhiều hệ thống được cấu hình theo nhiều cách khác nhau. Chúng tôi chủ yếu test Zode trên Ubuntu nguyên bản, vì đây là bản phân phối phổ biến nhất mà người dùng của chúng tôi dùng, dù vậy chúng tôi vẫn kỳ vọng nó chạy được trên nhiều loại máy.

### Zode không khởi động được

Nếu bạn thấy lỗi như "/lib64/libc.so.6: version 'GLIBC_2.29' not found" nghĩa là phiên bản glibc của bản phân phối quá cũ. Bạn có thể nâng cấp hệ thống, hoặc [cài Zode từ mã nguồn](./development/linux.md).

### Sự cố đồ hoạ

#### Zode không mở được cửa sổ

Zode cần GPU để chạy hiệu quả. Bên dưới, chúng tôi dùng [Vulkan](https://www.vulkan.org/) để giao tiếp với GPU của bạn. Nếu bạn gặp vấn đề về hiệu năng, hoặc Zode không tải được, có thể Vulkan là thủ phạm.

Nếu bạn thấy thông báo `Zode failed to open a window: NoSupportedDeviceFound`, nghĩa là Vulkan không tìm thấy GPU tương thích. Bạn có thể thử chạy [vkcube](https://github.com/krh/vkcube) (thường có trong gói `vulkaninfo` hoặc `vulkan-tools` trên nhiều bản phân phối) để dò xem vấn đề đến từ đâu, như sau:

```
vkcube
```

> **_Lưu ý_**: Hãy thử chạy ở cả chế độ X11 và wayland bằng `vkcube -m [x11|wayland]`. Một số phiên bản `vkcube` dùng `vkcube` để chạy trong X11 và `vkcube-wayland` để chạy trong wayland.

Lệnh này sẽ in ra một dòng mô tả thiết lập đồ hoạ hiện tại và hiện một khối lập phương xoay. Nếu không được, bạn nên sửa được bằng cách cài driver GPU tương thích Vulkan, tuy nhiên có trường hợp chưa có hỗ trợ Vulkan.

Bạn biết Zode đang dùng card đồ hoạ nào bằng cách xem log của Zode (`~/.local/share/zode/logs/Zode.log`) và tìm `Using GPU: ...`.

Nếu bạn thấy lỗi như `ERROR_INITIALIZATION_FAILED` hoặc `GPU Crashed` hoặc `ERROR_SURFACE_LOST_KHR` thì có thể né bằng cách cài driver khác cho GPU, hoặc chọn GPU khác để chạy. (Xem [#14225](https://github.com/zed-industries/zed/issues/14225))

Trên một số hệ thống, file `/etc/prime-discrete` dùng để ép dùng GPU rời qua [PRIME](https://wiki.archlinux.org/title/PRIME). Tuỳ chi tiết thiết lập của bạn, có thể bạn cần đổi nội dung file này thành "on" (ép dùng đồ hoạ rời) hoặc "off" (ép dùng đồ hoạ tích hợp).

Trên hệ thống khác, bạn có thể đặt biến môi trường `DRI_PRIME=1` khi chạy Zode để ép dùng GPU rời.

Nếu bạn dùng GPU AMD, có thể gặp lỗi 'Broken Pipe'. Hãy thử dùng driver RADV hoặc Mesa. (Xem [#13880](https://github.com/zed-industries/zed/issues/13880))

Nếu bạn dùng `amdvlk`, driver đồ hoạ AMD mã nguồn mở mặc định, có thể Zode liên tục không khởi chạy được. Đây là lỗi đã biết với một số người dùng, ví dụ trên Omarchy (xem issue [#28851](https://github.com/zed-industries/zed/issues/28851)). Để sửa, bạn cần dùng driver khác. Chúng tôi khuyên gỡ các gói `amdvlk` và `lib32-amdvlk` rồi cài `vulkan-radeon` thay vào (xem issue [#14141](https://github.com/zed-industries/zed/issues/14141)).

Để biết thêm, [hướng dẫn Vulkan của Arch](https://wiki.archlinux.org/title/Vulkan) có một số bước hay áp dụng tốt cho hầu hết bản phân phối.

#### Ép Zode dùng một GPU cụ thể

Có vài cách để ép Zode dùng một GPU cụ thể:

##### Cách A

Bạn dùng biến môi trường `ZODE_DEVICE_ID={device_id}` để chỉ định device ID của GPU bạn muốn Zode dùng.

Bạn lấy device ID của GPU bằng cách chạy `lspci -nn | grep VGA`, lệnh này in mỗi GPU trên một dòng như:

```
08:00.0 VGA compatible controller [0300]: NVIDIA Corporation GA104 [GeForce RTX 3070] [10de:2484] (rev a1)
```

trong đó device ID ở đây là `2484`. Giá trị này ở dạng thập lục phân, nên để ép Zode dùng GPU này bạn đặt biến môi trường như sau:

```
ZODE_DEVICE_ID=0x2484 zode
```

Nhớ export biến nếu bạn chọn định nghĩa nó toàn cục trong `.bashrc` hoặc tương tự.

##### Cách B

Nếu bạn dùng Mesa, bạn chạy `MESA_VK_DEVICE_SELECT=list zode --foreground` để lấy danh sách GPU có sẵn rồi export `MESA_VK_DEVICE_SELECT=xxxx:yyyy` để chọn một thiết bị cụ thể. Ngoài ra, bạn có thể lùi về xwayland bằng cách export thêm `WAYLAND_DISPLAY=""`.

##### Cách C

Dùng [vkdevicechooser](https://github.com/jiriks74/vkdevicechooser).

#### Báo cáo sự cố đồ hoạ

Nếu Vulkan đã cấu hình đúng mà Zode vẫn không chạy được, vui lòng [tạo một issue](https://github.com/tgiap04/zode/issues) kèm thật nhiều thông tin.

Khi báo trên GitHub các lỗi Zode không khởi động do lỗi khởi tạo đồ hoạ, có thể không chạy được lệnh `zode: copy system specs into clipboard` như chúng tôi hướng dẫn trong mẫu issue. Chúng tôi cung cấp cách khác để thu thập thông số hệ thống riêng cho tình huống này.

Truyền cờ `--system-specs` cho Zode như

```sh
zode --system-specs
```

sẽ in thông số hệ thống ra terminal như vậy. Chúng tôi rất khuyên chép nguyên văn output vào issue trên GitHub, vì nó dùng định dạng markdown để output dễ đọc.

Ngoài ra, rất hữu ích nếu bạn đính kèm nội dung log của Zode khi báo các lỗi như vậy. Log thường nằm ở `~/.local/share/zode/logs/Zode.log`. Quy trình khuyến nghị để tạo một file log hữu ích như sau:

```sh
truncate -s 0 ~/.local/share/zode/logs/Zode.log # Clear the log file
ZODE_LOG=wgpu=info zode .
cat ~/.local/share/zode/logs/Zode.log
# copy the output
```

Hoặc, nếu đã cài Zode cli, bạn có thể làm

```sh
ZODE_LOG=wgpu=info zode --foreground .
# copy the output
```

Khi dán log vào issue GitHub, cũng rất nên dùng mẫu sau:

> **_Lưu ý_**: Khoảng trắng trong mẫu rất quan trọng, và sẽ gây lỗi định dạng nếu không được giữ nguyên.

````
<details><summary>Zode Log</summary>

```
{zode log contents}
```

</details>
````

Nhờ vậy log được thu gọn mặc định, giúp đọc issue dễ hơn.

### Tôi không mở được file nào

### Bấm liên kết không có tác dụng

Các tính năng này do XDG desktop portal cung cấp, cụ thể:

- `org.freedesktop.portal.FileChooser`
- `org.freedesktop.portal.OpenURI`

Một số window manager, như `Hyprland`, mặc định không cung cấp bộ chọn file. Xem [danh sách này](https://wiki.archlinux.org/title/XDG_Desktop_Portal#List_of_backends_and_interfaces) làm điểm khởi đầu để tìm phương án thay thế.

### Zode không nhớ API key của tôi

### Zode không nhớ đăng nhập của tôi

Các tính năng này cũng cần XDG desktop portal, cụ thể:

- `org.freedesktop.portal.Secret` hoặc
- `org.freedesktop.Secrets`

Zode cần một nơi lưu bí mật an toàn như cookie đăng nhập Zode hay API key OpenAI của bạn, và chúng tôi dùng keychain do hệ thống cung cấp để làm việc này. Ví dụ các gói cung cấp keychain là `gnome-keyring`, `KWallet` và `keepassxc`, cùng nhiều gói khác.

### Không khởi động được inotify

Zode dựa vào inotify để theo dõi thay đổi trong hệ thống file. Nếu không khởi động được inotify, Zode sẽ không chạy ổn định.

Nếu bạn thấy "too many open files", trước hết hãy thử `sysctl fs.inotify`.

- Bạn nên thấy max_user_instances là 128 trở lên (đổi giới hạn bằng `sudo sysctl fs.inotify.max_user_instances=1024`). Zode chỉ cần 1 instance inotify.
- Bạn nên thấy `max_user_watches` là 8000 trở lên (đổi giới hạn bằng `sudo sysctl fs.inotify.max_user_watches=64000`). Zode cần một watch cho mỗi thư mục trong mọi dự án đang mở + một cho mỗi repository git + vài cái nữa cho cài đặt, giao diện (theme), keymap, extension.

Cũng có thể bạn đang hết file descriptor. Bạn kiểm tra giới hạn bằng `ulimit` và cập nhật bằng cách sửa `/etc/security/limits.conf`.

### Không có âm thanh hoặc sai thiết bị đầu ra

Nếu bạn không nghe thấy âm thanh trong Zode hoặc âm thanh đi sai thiết bị, có thể do hệ thống âm thanh không khớp nhau. Zode dựa vào ALSA, trong khi hệ thống của bạn có thể dùng PipeWire hoặc PulseAudio. Để khắc phục, bạn cần cấu hình ALSA để đưa âm thanh qua PipeWire/PulseAudio.

Nếu hệ thống của bạn dùng PipeWire:

1. **Cài plugin PipeWire ALSA**

   Trên hệ thống nền Debian, chạy:

   ```bash
   sudo apt install pipewire-alsa
   ```

2. **Cấu hình ALSA dùng PipeWire**

   Thêm cấu hình sau vào file cài đặt ALSA. Bạn dùng `~/.asoundrc` (cấp người dùng) hoặc `/etc/asound.conf` (toàn hệ thống) đều được:

   ```bash
   pcm.!default {
       type pipewire
   }

   ctl.!default {
       type pipewire
   }
   ```

3. **Khởi động lại hệ thống**

### Ép hệ số tỉ lệ X11

Trên hệ thống X11, Zode tự phát hiện hệ số tỉ lệ phù hợp cho màn hình DPI cao. Hệ số tỉ lệ được xác định theo thứ tự ưu tiên sau:

1. Biến môi trường `GPUI_X11_SCALE_FACTOR` (nếu được đặt)
2. `Xft.dpi` từ cơ sở dữ liệu X resources (xrdb)
3. Tự phát hiện qua RandR dựa trên độ phân giải và kích thước vật lý của màn hình

Nếu muốn tuỳ chỉnh hệ số tỉ lệ ngoài những gì Zode tự phát hiện, bạn có vài lựa chọn:

#### Kiểm tra hệ số tỉ lệ hiện tại

Bạn kiểm tra xem `Xft.dpi` đã được đặt chưa:

```sh
xrdb -query | grep Xft.dpi
```

Nếu lệnh này không in gì, Zode đang dùng RandR (extension quản lý màn hình của X11) để tự tính hệ số tỉ lệ dựa trên độ phân giải và kích thước vật lý mà màn hình báo.

#### Cách 1: Đặt Xft.dpi (X Resources Database)

`Xft.dpi` là cài đặt X11 chuẩn mà nhiều ứng dụng dùng để co giãn font và giao diện nhất quán. Đặt nó đảm bảo Zode co giãn giống các ứng dụng X11 khác tôn trọng cài đặt này.

Sửa hoặc tạo file `~/.Xresources`:

```sh
vim ~/.Xresources
```

Thêm dòng này với DPI bạn muốn:

```sh
Xft.dpi: 96
```

Các giá trị DPI thường gặp:

- `96` cho tỉ lệ chuẩn 1x
- `144` cho tỉ lệ 1.5x
- `192` cho tỉ lệ 2x
- `288` cho tỉ lệ 3x

Nạp cấu hình:

```sh
xrdb -merge ~/.Xresources
```

Khởi động lại Zode để thay đổi có hiệu lực.

#### Cách 2: Dùng biến môi trường GPUI_X11_SCALE_FACTOR

Biến môi trường riêng của Zode này đặt thẳng hệ số tỉ lệ, bỏ qua mọi cách tự phát hiện.

```sh
GPUI_X11_SCALE_FACTOR=1.5 zode
```

Bạn dùng được giá trị thập phân (ví dụ `1.25`, `1.5`, `2.0`) hoặc đặt `GPUI_X11_SCALE_FACTOR=randr` để ép phát hiện dựa trên RandR kể cả khi `Xft.dpi` đã được đặt.

Để giữ vĩnh viễn, thêm nó vào shell profile hoặc desktop entry.

#### Cách 3: Chỉnh DPI RandR toàn hệ thống

Cách này đổi DPI được báo cho toàn bộ phiên X11, ảnh hưởng cách RandR tính tỉ lệ cho mọi ứng dụng dùng nó.

Thêm dòng này vào `.xprofile` hoặc `.xinitrc`:

```sh
xrandr --dpi 192
```

Thay `192` bằng giá trị DPI bạn muốn. Điều này ảnh hưởng toàn hệ thống và sẽ được cơ chế tự phát hiện RandR của Zode dùng khi `Xft.dpi` chưa được đặt.

### Tham số render font

Trên Linux, Zode đọc các biến môi trường `ZODE_FONTS_GAMMA` và `ZODE_FONTS_GRAYSCALE_ENHANCED_CONTRAST` để lấy giá trị dùng cho render font.

`ZODE_FONTS_GAMMA` tương ứng với giá trị [getgamma](https://learn.microsoft.com/en-us/windows/win32/api/dwrite/nf-dwrite-idwriterenderingparams-getgamma).
Khoảng cho phép [1.0, 2.2], giá trị khác bị cắt về biên.
Mặc định: 1.8

`ZODE_FONTS_GRAYSCALE_ENHANCED_CONTRAST` tương ứng với giá trị [getgrayscaleenhancedcontrast](https://learn.microsoft.com/en-us/windows/win32/api/dwrite_1/nf-dwrite_1-idwriterenderingparams1-getgrayscaleenhancedcontrast).
Khoảng cho phép: [0.0, ..), giá trị khác bị cắt về biên.
Mặc định: 1.0
