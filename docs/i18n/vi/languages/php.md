---
title: PHP
description: "Configure PHP language support in Zode, including language servers, formatting, and debugging."
---

# PHP

Hỗ trợ PHP có sẵn qua [extension PHP](https://github.com/zed-extensions/php).

- Tree-sitter: [tree-sitter/tree-sitter-php](https://github.com/tree-sitter/tree-sitter-php)
- Language Server: [phpactor/phpactor](https://github.com/phpactor/phpactor)
- Alternate Language Server: [bmewburn/vscode-intelephense](https://github.com/bmewburn/vscode-intelephense/)

## Cài PHP

Extension PHP yêu cầu PHP đã được cài và có trong `PATH`:

```sh
# macOS via Homebrew
brew install php

# Debian/Ubuntu
sudo apt-get install php-cli

# CentOS 8+/RHEL
sudo dnf install php-cli

# Arch Linux
sudo pacman -S php

# check PHP path
## macOS and Linux
which php

## Windows
where php
```

## Chọn language server

Extension PHP dùng các [LSP language server](https://microsoft.github.io/language-server-protocol), mặc định là Phpactor. Nếu muốn dùng language server khác có hỗ trợ Zode (ví dụ Intelephense hoặc PHP Tools), hãy làm theo tài liệu về cách triển khai.

### Intelephense

[Intelephense](https://intelephense.com/) là language server [độc quyền](https://github.com/bmewburn/vscode-intelephense/blob/master/LICENSE.txt#L29) cho PHP theo mô hình freemium. Một số tính năng cần mua [giấy phép premium](https://intelephense.com/buy).

Cấu hình language server trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > PHP, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "PHP": {
      "language_servers": ["intelephense", "!phpactor", "!phptools", "..."]
    }
  }
}
```

Để dùng các tính năng premium, bạn có thể đặt file giấy phép trong thư mục home tại `~/intelephense/licence.txt` trên macOS và Linux, hoặc `%USERPROFILE%/intelephense/licence.txt` trên Windows.

Hoặc bạn có thể truyền khóa giấy phép, hay đường dẫn tới file chứa khóa, làm tùy chọn khởi tạo. Để làm vậy, thêm đoạn sau vào `settings.json`:

```json [settings]
{
  "lsp": {
    "intelephense": {
      "initialization_options": {
        "licenceKey": "/path/to/licence.txt"
      }
    }
  }
}
```

### PHP Tools

[PHP Tools](https://www.devsense.com/) là language server độc quyền có tính năng miễn phí và premium. Bạn cần [mua giấy phép](https://www.devsense.com/en/purchase) để kích hoạt tính năng premium.

Cấu hình language server trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > PHP, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "PHP": {
      "language_servers": ["phptools", "!intelephense", "!phpactor", "..."]
    }
  }
}
```

Để dùng các tính năng premium, bạn có thể thêm giấy phép vào `initialization_options` trong `settings.json`:

```json [settings]
{
  "lsp": {
    "phptools": {
      "initialization_options": {
        "0": "your_license_key"
      }
    }
  }
}
```

hoặc đặt biến môi trường `DEVSENSE_PHP_LS_LICENSE` trong file `.env` của dự án.

```env
DEVSENSE_PHP_LS_LICENSE="your_license_key"
```

Xem tài liệu [PHP Tools cho Zode](https://docs.devsense.com/other/zed/) để biết thêm chi tiết.

### Phpactor

Cấu hình language server trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > PHP, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "PHP": {
      "language_servers": ["phpactor", "!intelephense", "!phptools", "..."]
    }
  }
}
```

## PHPDoc

Zode hỗ trợ tô màu cú pháp cho comment PHPDoc.

- Tree-sitter: [claytonrcarter/tree-sitter-phpdoc](https://github.com/claytonrcarter/tree-sitter-phpdoc)

## Gỡ lỗi

Extension PHP cung cấp debug adapter cho PHP qua Xdebug. Có nhiều cách dùng:

```json
[
  {
    "label": "PHP: Listen to Xdebug",
    "adapter": "Xdebug",
    "request": "launch",
    "port": 9003
  },
  {
    "label": "PHP: Debug this test",
    "adapter": "Xdebug",
    "request": "launch",
    "program": "vendor/bin/phpunit",
    "args": ["--filter", "$ZODE_SYMBOL"]
  }
]
```

Đây là một số mẹo xử lý sự cố thường gặp, phòng khi bạn gặp vấn đề:

- Đảm bảo đã cài Xdebug cho đúng phiên bản PHP bạn đang chạy.
- Đảm bảo Xdebug được cấu hình chạy ở chế độ `debug`.
- Đảm bảo Xdebug thực sự bắt đầu một phiên gỡ lỗi.
- Đảm bảo host và port khớp giữa Xdebug và Zode.
- Xem log chẩn đoán bằng hàm `xdebug_info()` trong trang bạn đang gỡ lỗi.

## Dùng Tailwind CSS Language Server với PHP

Để có đủ tính năng (gợi ý tự động, lint, v.v.) từ [Tailwind CSS language server](https://github.com/tailwindlabs/tailwindcss-intellisense/tree/HEAD/packages/tailwindcss-language-server#readme) trong file PHP, bạn cần cấu hình language server để nó biết tìm class CSS ở đâu, bằng cách thêm đoạn sau vào `settings.json`:

```json [settings]
{
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "includeLanguages": {
          "php": "html"
        },
        "experimental": {
          "classRegex": [
            "class=\"([^\"]*)\"",
            "class='([^']*)'",
            "class=\\\"([^\\\"]*)\\\""
          ]
        }
      }
    }
  }
}
```

Với các cài đặt này, bạn sẽ có gợi ý cho class Tailwind CSS trong thuộc tính HTML bên trong file PHP. Ví dụ:

```php
<?php
// PHP file with HTML:
?>
<div class="flex items-center <completion here>">
  <p class="text-lg font-bold <completion here>">Hello World</p>
</div>
```

### Laravel/Blade

Với file Laravel/Blade, bạn có thể cần cấu hình thêm để xử lý directive của Blade:

```json [settings]
{
  "lsp": {
    "tailwindcss-language-server": {
      "settings": {
        "includeLanguages": {
          "php": "html",
          "blade": "html"
        },
        "experimental": {
          "classRegex": [
            "class=\"([^\"]*)\"",
            "class='([^']*)'",
            "class=\\\"([^\\\"]*)\\\"",
            "@class\\(\\[([^\\]]*)\\]\\)"
          ]
        }
      }
    }
  }
}
```

Cách này cũng cho gợi ý trong directive Blade như:

```blade
{{-- Blade file --}}
<div class="flex {{ $customClass }} <completion here>">
  @class(['flex', 'items-center', '<completion here>'])
</div>
```
