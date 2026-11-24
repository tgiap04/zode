---
title: Python
description: "Configure Python language support in Zode, including language servers, formatting, and debugging."
---

# Cách thiết lập Python trong Zode

Zode hỗ trợ Python sẵn có.

- Tree-sitter: [tree-sitter-python](https://github.com/zed-industries/tree-sitter-python)
- Language Server:
  - [DetachHead/basedpyright](https://github.com/DetachHead/basedpyright)
  - [astral-sh/ruff](https://github.com/astral-sh/ruff)
  - [astral-sh/ty](https://github.com/astral-sh/ty)
  - [microsoft/pyright](https://github.com/microsoft/pyright)
  - [python-lsp/python-lsp-server](https://github.com/python-lsp/python-lsp-server) (PyLSP)
- Debug Adapter: [debugpy](https://github.com/microsoft/debugpy)

## Cài Python

Bạn cần cài cả Zode lẫn Python trước khi bắt đầu.

### Bước 1: Cài Python

Zode không đi kèm runtime Python, nên bạn cần tự cài.
Chọn một trong các cách sau:

- uv (khuyến nghị)

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

Tìm hiểu thêm tại [hướng dẫn cài đặt của Astral](https://docs.astral.sh/uv/getting-started/installation/).

- Homebrew:

```bash
brew install python
```

- Trình cài đặt từ Python.org: Tải bản mới nhất tại [python.org/downloads](https://python.org/downloads).

### Bước 2: Kiểm tra bản cài Python

Xác nhận Python đã được cài và dùng được trong shell:

```bash
python3 --version
```

Bạn sẽ thấy đầu ra như `Python 3.x.x`.

## Mở dự án Python đầu tiên trong Zode

Khi đã cài Zode và Python, hãy mở một thư mục chứa mã Python để bắt đầu làm việc.

### Bước 1: Mở Zode với một dự án Python

Mở Zode.
Từ thanh menu, chọn File > Open Folder, hoặc mở từ terminal:

```bash
zode path/to/your/project
```

Zode nhận diện file `.py` tự động bằng bộ phân tích tree-sitter-python có sẵn, không cần plugin hay thiết lập thủ công.

### Bước 2: Dùng terminal tích hợp (tùy chọn)

Zode có terminal tích hợp, mở từ bảng phía dưới. Nếu Zode phát hiện dự án đang dùng [môi trường ảo](#virtual-environments), môi trường đó sẽ tự được kích hoạt trong các terminal mới tạo. Bạn có thể cấu hình hành vi này bằng cài đặt [`detect_venv`](../reference/all-settings.md#terminal-detect_venv).

## Cấu hình Python language server trong Zode

Zode cung cấp sẵn nhiều Python language server. Mặc định, [basedpyright](https://github.com/DetachHead/basedpyright) là language server chính, còn [Ruff](https://github.com/astral-sh/ruff) dùng để định dạng và lint.

Các language server tích hợp khác gồm:

- [ty](https://docs.astral.sh/ty/)&mdash;Language server đầy triển vọng từ Astral, xây dựng để chạy nhanh.
- [Pyright](https://github.com/microsoft/pyright)&mdash;Nền tảng của basedpyright.
- [PyLSP](https://github.com/python-lsp/python-lsp-server)&mdash;Language server dựa trên plugin, tích hợp với các công cụ như `pycodestyle`, `autopep8` và `yapf`.

Các language server này mặc định bị tắt, nhưng có thể bật trong cài đặt.

Cấu hình language server trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > Python, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Python": {
      "language_servers": [
        // Enable ty, disable basedpyright, and enable all
        // other registered language servers (ruff, pylsp, pyright).
        "ty",
        "!basedpyright",
        "..."
      ]
    }
  }
}
```

Xem: [Làm việc với language server](https://zodekit.com/docs/configuring-languages#working-with-language-servers) để biết thêm cách bật và tắt language server.

### Basedpyright

[basedpyright](https://docs.basedpyright.com/latest/) là Python language server chính trong Zode từ Zode v0.204.0. Nó cung cấp các chức năng cốt lõi của language server như điều hướng (đi tới định nghĩa/tìm mọi tham chiếu) và kiểm tra kiểu. So với Pyright, nó hỗ trợ thêm các tính năng language server (như inlay hint) và các quy tắc kiểm tra.

Lưu ý rằng khi chạy riêng, basedpyright mặc định dùng [chế độ kiểm tra kiểu](https://docs.basedpyright.com/latest/benefits-over-pyright/better-defaults/#typecheckingmode) `recommended`, còn Zode cấu hình nó dùng chế độ `standard` ít nghiêm ngặt hơn theo mặc định, khớp với hành vi của Pyright. Bạn có thể đặt chế độ kiểm tra kiểu cho dự án bằng cài đặt `typeCheckingMode` trong `pyrightconfig.json` hoặc `pyproject.toml`, và nó sẽ ghi đè mặc định của Zode. Đọc tiếp để biết chi tiết cách cấu hình basedpyright.

#### Cấu hình basedpyright

basedpyright đọc tùy chọn cấu hình từ hai loại nguồn:

- Cài đặt language server ("workspace configuration"), phải cấu hình theo từng trình soạn thảo (với Zode là dùng `settings.json`) nhưng áp dụng cho mọi dự án mở trong trình soạn thảo đó
- File cấu hình (`pyrightconfig.json`, `pyproject.toml`), không phụ thuộc trình soạn thảo nhưng chỉ áp dụng cho dự án chứa chúng

Nguyên tắc chung: tùy chọn chỉ liên quan khi dùng basedpyright từ trình soạn thảo thì đặt trong cài đặt language server, còn tùy chọn có ý nghĩa cả khi bạn chạy nó [như công cụ dòng lệnh](https://docs.basedpyright.com/latest/configuration/command-line/) thì đặt trong file cấu hình. Các cài đặt liên quan đến inlay hint là ví dụ của loại thứ nhất, còn các cài đặt [loại chẩn đoán](https://docs.basedpyright.com/latest/configuration/config-files/#diagnostic-categories) là ví dụ của loại thứ hai.

Ví dụ cho cả hai loại cấu hình có bên dưới. Xem tài liệu basedpyright về [cài đặt language server](https://docs.basedpyright.com/latest/configuration/language-server-settings/) và [file cấu hình](https://docs.basedpyright.com/latest/configuration/config-files/) để biết danh sách đầy đủ các tùy chọn.

##### Cài đặt language server

Cài đặt language server cho basedpyright trong Zode có thể đặt ở mục `lsp` của `settings.json`.

Ví dụ, để:

- chẩn đoán mọi file trong workspace thay vì mặc định chỉ các file đang mở
- tắt inlay hint trên đối số hàm

Bạn có thể dùng cấu hình sau:

```json [settings]
{
  "lsp": {
    "basedpyright": {
      "settings": {
        "basedpyright.analysis": {
          "diagnosticMode": "workspace",
          "inlayHints": {
            "callArgumentNames": false
          }
        }
      }
    }
  }
}
```

##### File cấu hình

basedpyright đọc cấu hình riêng của dự án từ file cấu hình `pyrightconfig.json` và từ các mục `[tool.basedpyright]` và `[tool.pyright]` trong manifest `pyproject.toml`. `pyrightconfig.json` ghi đè `pyproject.toml` nếu cấu hình có ở cả hai nơi.

Đây là ví dụ file `pyrightconfig.json` cấu hình basedpyright dùng chế độ kiểm tra kiểu `strict` và không báo chẩn đoán cho file nào trong thư mục `__pycache__`:

```json
{
  "typeCheckingMode": "strict",
  "ignore": ["**/__pycache__"]
}
```

### PyLSP

[python-lsp-server](https://github.com/python-lsp/python-lsp-server/), thường gọi là PyLSP, mặc định tích hợp với một số công cụ bên ngoài (autopep8, mccabe, pycodestyle, yapf), còn các công cụ khác là tùy chọn và phải bật và cấu hình rõ ràng (flake8, pylint).

Xem [Cấu hình Python Language Server](https://github.com/python-lsp/python-lsp-server/blob/develop/CONFIGURATION.md) để biết thêm.

## Môi trường ảo

[Môi trường ảo](https://docs.python.org/3/library/venv.html) là công cụ hữu ích để cố định phiên bản Python và bộ thư viện phụ thuộc cho một dự án cụ thể, tách biệt với các dự án khác trên cùng máy. Zode có hỗ trợ sẵn việc tìm, cấu hình và kích hoạt môi trường ảo, dựa trên khái niệm [toolchain](../toolchains.md) không phụ thuộc ngôn ngữ.

Lưu ý rằng nếu có bản Python cài toàn cục, nó cũng được tính là một toolchain đối với Zode.

### Tạo môi trường ảo

Nếu dự án chưa có môi trường ảo, bạn có thể tạo như sau:

```bash
python3 -m venv .venv
```

Hoặc nếu dùng `uv`, chạy `uv sync` sẽ tạo môi trường ảo ở lần chạy đầu tiên.

### Cách Zode dùng toolchain Python

Zode dùng toolchain Python đã chọn cho dự án theo các cách sau:

- Language server tích hợp sẽ tự được cấu hình với đường dẫn tới trình thông dịch Python của toolchain và, nếu có, môi trường ảo. Điều này quan trọng để chúng phân giải được các thư viện phụ thuộc. (Lưu ý language server do extension cung cấp hiện chưa tự cấu hình như vậy được.)
- Task Python (như test pytest) sẽ chạy bằng trình thông dịch Python của toolchain.
- Nếu toolchain là môi trường ảo, script kích hoạt của môi trường sẽ tự chạy khi bạn mở shell mới trong terminal tích hợp của Zode, giúp bạn dùng thuận tiện trình thông dịch Python đã chọn và bộ thư viện phụ thuộc.
- Nếu language server tích hợp được cài trong môi trường ảo đang kích hoạt, binary đó sẽ được dùng thay cho binary riêng do Zode tự cài. Điều này cũng áp dụng cho debugpy.

### Chọn toolchain

Với hầu hết dự án, Zode tự chọn đúng toolchain Python. Trong dự án phức tạp có nhiều môi trường ảo, có thể cần ghi đè lựa chọn này. Bạn có thể dùng [bộ chọn toolchain](../toolchains.md#selecting-toolchains) để chọn một toolchain trong danh sách Zode tìm thấy, hoặc [chỉ định đường dẫn tới toolchain thủ công](../toolchains.md#adding-toolchains-manually) nếu nó không có trong danh sách.

## Định dạng mã và lint

Zode dùng [Ruff](https://github.com/astral-sh/ruff) để định dạng và lint mã Python. Cụ thể, nó chạy Ruff như một LSP server bằng lệnh con `ruff server`.

### Cấu hình định dạng

Định dạng trong Zode theo quy trình hai pha: trước tiên chạy các code action khi định dạng (`code_actions_on_format`), sau đó đến trình định dạng đã cấu hình.

Cấu hình định dạng trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > Python, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Python": {
      "code_actions_on_format": {
        "source.organizeImports.ruff": true
      },
      "formatter": {
        "language_server": {
          "name": "ruff"
        }
      }
    }
  }
}
```

Hai pha này độc lập với nhau. Ví dụ, nếu bạn thích dùng [Black](https://github.com/psf/black) để định dạng mã nhưng vẫn muốn giữ việc sắp xếp import của Ruff, bạn chỉ cần đổi pha định dạng.

Cấu hình trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > Python, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Python": {
      "code_actions_on_format": {
        // Phase 1: Ruff still handles organize imports
        "source.organizeImports.ruff": true
      },
      "formatter": {
        // Phase 2: Black handles formatting
        "external": {
          "command": "black",
          "arguments": ["--stdin-filename", "{buffer_path}", "-"]
        }
      }
    }
  }
}
```

Để chuyển hẳn sang công cụ khác và ngăn Ruff sửa mã của bạn hoàn toàn, bạn phải đặt rõ `source.organizeImports.ruff` thành false trong mục `code_actions_on_format`, ngoài việc đổi trình định dạng.

Để không có thao tác định dạng nào khi lưu, bạn có thể tắt định dạng khi lưu cho file Python.

Cấu hình trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > Python, hoặc thêm vào file cài đặt:

```json [settings]
{
  "languages": {
    "Python": {
      "format_on_save": "off"
    }
  }
}
```

### Cấu hình Ruff

Giống basedpyright, khi dùng trong Zode, Ruff đọc tùy chọn từ cả cài đặt language server của Zode lẫn file cấu hình (`ruff.toml`). Khác basedpyright, _mọi_ tùy chọn đều có thể cấu hình ở một trong hai nơi này, nên chọn đặt cấu hình Ruff ở đâu tùy vào việc bạn muốn nó dùng chung giữa các dự án nhưng riêng cho Zode (khi đó dùng cài đặt language server), hay riêng cho một dự án nhưng chung cho mọi lần gọi Ruff (khi đó dùng `ruff.toml`).

Đây là ví dụ dùng cài đặt language server trong `settings.json` của Zode để tắt mọi lint của Ruff trong Zode (vẫn dùng Ruff làm trình định dạng):

```json [settings]
{
  "lsp": {
    "ruff": {
      "initialization_options": {
        "settings": {
          "exclude": ["*"]
        }
      }
    }
  }
}
```

Và đây là ví dụ `ruff.toml` với các tùy chọn lint và định dạng, lấy từ tài liệu Ruff:

```toml
[lint]
# Avoid enforcing line-length violations (`E501`)
ignore = ["E501"]

[format]
# Use single quotes when formatting.
quote-style = "single"
```

Để biết thêm chi tiết, xem tài liệu Ruff về [file cấu hình](https://docs.astral.sh/ruff/configuration/) và [cài đặt language server](https://docs.astral.sh/ruff/editors/settings/), cùng [danh sách tùy chọn](https://docs.astral.sh/ruff/settings/).

### Tô màu ngôn ngữ nhúng

Zode hỗ trợ tô màu cú pháp cho mã nhúng trong chuỗi Python bằng cách thêm comment ghi tên ngôn ngữ.

```python
# sql
query = "SELECT * FROM users"

#sql
query = """
    SELECT *
    FROM users
"""

result = func( #sql
    "SELECT * FROM users"
)
```

## Gỡ lỗi

Zode hỗ trợ gỡ lỗi Python qua adapter `debugpy`. Bạn có thể bắt đầu không cần cấu hình hoặc tự định nghĩa profile chạy trong `.zode/debug.json`.

### Bắt đầu gỡ lỗi không cần thiết lập

Zode có thể tự phát hiện các điểm vào Python gỡ lỗi được. Nhấn F4 (hoặc chạy debugger: start từ bảng lệnh (Command Palette)) để xem các lựa chọn cho dự án hiện tại.
Cách này dùng được cho:

- Script Python
- Module
- Test pytest

Zode dùng `debugpy` bên dưới, nhưng không cần cấu hình adapter thủ công.

### Định nghĩa cấu hình gỡ lỗi tùy chỉnh

Với thiết lập dùng lại nhiều lần, hãy tạo file `.zode/debug.json` ở thư mục gốc dự án. Cách này cho bạn kiểm soát nhiều hơn cách Zode chạy và gỡ lỗi mã.

- [Tài liệu cấu hình debugpy](https://github.com/microsoft/debugpy/wiki/Debug-configuration-settings#launchattach-settings)

#### Gỡ lỗi file đang mở

```json [debug]
[
  {
    "label": "Python Active File",
    "adapter": "Debugpy",
    "program": "$ZODE_FILE",
    "request": "launch"
  }
]
```

Lệnh này chạy file đang mở trong trình soạn thảo.

#### Gỡ lỗi ứng dụng Flask

Với dự án dùng Flask, bạn có thể định nghĩa cấu hình chạy đầy đủ:

```
.venv/
app/
  init.py
  main.py
  routes.py
templates/
  index.html
static/
  style.css
requirements.txt
```

…có thể dùng cấu hình sau:

```json [debug]
[
  {
    "label": "Python: Flask",
    "adapter": "Debugpy",
    "request": "launch",
    "module": "app",
    "cwd": "$ZODE_WORKTREE_ROOT",
    "env": {
      "FLASK_APP": "app",
      "FLASK_DEBUG": "1"
    },
    "args": [
      "run",
      "--reload", // Enables Flask reloader that watches for file changes
      "--debugger" // Enables Flask debugger
    ],
    "autoReload": {
      "enable": true
    },
    "jinja": true,
    "justMyCode": true
  }
]
```

Có thể kết hợp các cấu hình này để phù hợp với web server, trình chạy test hoặc script tùy chỉnh.

#### Gỡ lỗi ứng dụng Django

Với dự án dùng Django có cấu trúc tương tự như sau:

```
my_django_project/
  manage.py
  …
  my_django_app/
    migrations/
    templates/
    models.py
    urls.py
    …
```

…có thể dùng cấu hình sau:

```json [debug]
[
  {
    "label": "Python: Django",
    "adapter": "Debugpy",
    "request": "launch",
    "program": "manage.py",
    "args": ["runserver"],
    "django": true
  }
]
```

## Xử lý sự cố

Các vấn đề với Python trong Zode thường liên quan đến môi trường ảo, language server hoặc cấu hình công cụ.

### Xử lý lỗi khởi động language server

Nếu language server không phản hồi hoặc các tính năng như chẩn đoán hay gợi ý tự động không dùng được:

- Kiểm tra log Zode (bằng action {#action zode::OpenLog}) để tìm lỗi liên quan đến language server bạn đang dùng. Đây là nơi bạn dễ tìm thấy thông tin hữu ích nếu language server hoàn toàn không khởi động được.
- Dùng chế độ xem log language server để hiểu vòng đời của language server bị ảnh hưởng. Bạn mở chế độ xem này bằng action {#action dev::OpenLanguageServerLogs}, hoặc bấm biểu tượng tia chớp trên thanh trạng thái rồi chọn language server. Hai phần dữ liệu hữu ích nhất trong chế độ xem này là:
  - "Server Logs", hiện mọi lỗi mà language server in ra
  - "Server Info", hiện chi tiết về cách language server được khởi động
- Kiểm tra `settings.json` hoặc `pyrightconfig.json` có đúng cú pháp không.
- Khởi động lại Zode để khởi tạo lại kết nối language server, hoặc thử khởi động lại language server bằng action {#action editor::RestartLanguageServer}

Nếu language server không phân giải được import và bạn đang dùng môi trường ảo, hãy chắc chắn đã chọn đúng môi trường trong bộ chọn. Bạn có thể dùng chế độ xem "Server Info" để xác nhận Zode đang gửi môi trường ảo nào cho language server&mdash;tìm mục `* Configuration` ở cuối.
