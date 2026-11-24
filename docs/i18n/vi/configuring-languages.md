---
title: Language Server and Tree-sitter Config - Zode
description: Configure language support in Zode with Tree-sitter for syntax highlighting and LSP for diagnostics, completion, and formatting.
---

# Cấu hình các ngôn ngữ được hỗ trợ

Hỗ trợ ngôn ngữ của Zode xây dựng trên hai công nghệ:

1. **Tree-sitter** lo việc tô màu cú pháp và các tính năng dựa trên cấu trúc như bảng outline.
2. **Language Server Protocol (LSP)** cung cấp các tính năng ngữ nghĩa: gợi ý code, chẩn đoán, đi tới định nghĩa và refactor.

Trang này nói về cài đặt riêng cho từng ngôn ngữ, liên kết file, cấu hình language server, format, lint và tô màu cú pháp.

Danh sách ngôn ngữ được hỗ trợ xem ở [Ngôn ngữ được hỗ trợ](./languages.md). Để thêm hỗ trợ ngôn ngữ mới, xem [Extension ngôn ngữ](./extensions/languages.md).

## Cài đặt riêng cho từng ngôn ngữ

Zode cho phép bạn ghi đè cài đặt toàn cục cho từng ngôn ngữ. Các cấu hình tùy chỉnh này khai báo trong file `settings.json` dưới khóa `languages`.

Ví dụ về cài đặt riêng cho ngôn ngữ:

```json [settings]
"languages": {
  "Python": {
    "tab_size": 4,
    "formatter": "language_server",
    "format_on_save": "on"
  },
  "JavaScript": {
    "tab_size": 2,
    "formatter": {
      "external": {
        "command": "prettier",
        "arguments": ["--stdin-filepath", "{buffer_path}"]
      }
    }
  }
}
```

Bạn có thể tùy chỉnh nhiều cài đặt cho từng ngôn ngữ, gồm:

- [`tab_size`](./reference/all-settings.md#tab-size): Số khoảng trắng cho mỗi cấp thụt lề
- [`formatter`](./reference/all-settings.md#formatter): Công cụ dùng để format code
- [`format_on_save`](./reference/all-settings.md#format-on-save): Có tự động format code khi lưu hay không
- [`enable_language_server`](./reference/all-settings.md#enable-language-server): Bật/tắt hỗ trợ language server
- [`hard_tabs`](./reference/all-settings.md#hard-tabs): Dùng tab thay vì khoảng trắng để thụt lề
- [`preferred_line_length`](./reference/all-settings.md#preferred-line-length): Độ dài dòng tối đa được khuyến nghị
- [`soft_wrap`](./reference/all-settings.md#soft-wrap): Cách xuống dòng cho các dòng code dài
- [`show_completions_on_input`](./reference/all-settings.md#show-completions-on-input): Có hiện gợi ý khi bạn gõ hay không
- [`show_completion_documentation`](./reference/all-settings.md#show-completion-documentation): Có hiện tài liệu ngay trong dòng và bên cạnh cho các mục trong menu gợi ý hay không
- [`colorize_brackets`](./reference/all-settings.md#colorize-brackets): Có dùng truy vấn bracket của tree-sitter để phát hiện và tô màu dấu ngoặc trong trình soạn thảo hay không (còn gọi là "rainbow brackets")

Các cài đặt này giúp bạn giữ phong cách code riêng cho từng ngôn ngữ và dự án.

## Liên kết file

Zode tự phát hiện loại file theo phần mở rộng, nhưng bạn có thể tùy chỉnh các liên kết này cho hợp quy trình làm việc.

Để thiết lập liên kết file tùy chỉnh, dùng setting [`file_types`](./reference/all-settings.md#file-types) trong `settings.json`:

```json [settings]
"file_types": {
  "C++": ["c"],
  "TOML": ["MyLockFile"],
  "Dockerfile": ["Dockerfile*"]
}
```

Cấu hình này bảo Zode:

- Coi file `.c` là C++ thay vì C
- Nhận file tên "MyLockFile" là TOML
- Áp dụng cú pháp Dockerfile cho mọi file bắt đầu bằng "Dockerfile"

Bạn có thể dùng glob pattern để khớp linh hoạt hơn, xử lý các quy ước đặt tên phức tạp trong dự án.

## Làm việc với language server

Language server là phần quan trọng trong các tính năng code thông minh của Zode, cung cấp khả năng như tự hoàn thành, đi tới định nghĩa và kiểm tra lỗi theo thời gian thực.

### Language server là gì?

Language server triển khai Language Server Protocol (LSP), giao thức chuẩn hóa giao tiếp giữa trình soạn thảo và công cụ riêng của từng ngôn ngữ. Nhờ đó Zode hỗ trợ các tính năng nâng cao cho nhiều ngôn ngữ lập trình mà không phải tự cài đặt từng tính năng.

Một số tính năng chính do language server cung cấp:

- Gợi ý code
- Kiểm tra lỗi và chẩn đoán
- Điều hướng code (đi tới định nghĩa, tìm tham chiếu)
- Code action (Rename, extract method)
- Thông tin khi rê chuột (hover)
- Tìm symbol trong workspace

### Quản lý language server

Zode đơn giản hóa việc quản lý language server cho người dùng:

1. Tự động tải: Khi bạn mở file có loại file khớp, Zode tự tải language server phù hợp. Zode có thể gợi ý bạn cài extension cho các loại file đã biết.

2. Nơi lưu:

   - macOS: `~/Library/Application Support/Zode/languages`
   - Linux: `$XDG_DATA_HOME/zode/languages`, `$FLATPAK_XDG_DATA_HOME/zode/languages`, hoặc `$HOME/.local/share/zode/languages`

3. Tự động cập nhật: Zode luôn cập nhật language server của bạn, để bạn luôn có tính năng và cải tiến mới nhất.

### Chọn language server

Một số ngôn ngữ trong Zode có nhiều lựa chọn language server. Bạn có thể cài nhiều extension cùng gói language server cho một ngôn ngữ, dẫn tới khả năng chồng chéo. Để có đúng chức năng mong muốn, Zode cho phép bạn ưu tiên language server nào được dùng và theo thứ tự nào.

Bạn chỉ định ưu tiên bằng setting `language_servers`:

```json [settings]
  "languages": {
    "PHP": {
      "language_servers": ["intelephense", "!phpactor", "!phptools", "..."]
    }
  }
```

Trong ví dụ này:

- `intelephense` được đặt làm language server chính.
- `phpactor` và `phptools` bị tắt (chú ý tiền tố `!`).
- `"..."` mở rộng thành phần còn lại của các language server đăng ký cho PHP mà chưa được liệt kê.

Mục `"..."` đóng vai ký tự đại diện, gồm mọi language server đã đăng ký mà bạn chưa nêu rõ. Các server bạn liệt kê theo tên giữ nguyên vị trí, và `"..."` điền các server còn lại vào đúng chỗ đó trong danh sách. Server có tiền tố `!` bị loại hoàn toàn. Nghĩa là nếu cài thêm extension language server mới hoặc có server mới đăng ký cho ngôn ngữ, `"..."` sẽ tự động gồm nó. Nếu muốn kiểm soát hoàn toàn server nào được bật, hãy bỏ `"..."` — chỉ các server bạn liệt kê theo tên mới được dùng.

#### Ví dụ

Giả sử bạn làm việc với Ruby. Cấu hình mặc định là:

```json [settings]
{
  "language_servers": [
    "solargraph",
    "!ruby-lsp",
    "!rubocop",
    "!sorbet",
    "!steep",
    "!kanayago",
    "..."
  ]
}
```

Khi bạn ghi đè `language_servers` trong cài đặt, danh sách của bạn **thay thế** hoàn toàn mặc định. Nghĩa là các server mặc định bị tắt như `kanayago` sẽ được `"..."` bật lại trừ khi bạn tắt chúng lần nữa.

| Cấu hình                                          | Kết quả                                                            |
| ------------------------------------------------- | ------------------------------------------------------------------ |
| `["..."]`                                         | `solargraph`, `ruby-lsp`, `rubocop`, `sorbet`, `steep`, `kanayago` |
| `["ruby-lsp", "..."]`                             | `ruby-lsp`, `solargraph`, `rubocop`, `sorbet`, `steep`, `kanayago` |
| `["ruby-lsp", "!solargraph", "!kanayago", "..."]` | `ruby-lsp`, `rubocop`, `sorbet`, `steep`                           |
| `["ruby-lsp", "solargraph"]`                      | `ruby-lsp`, `solargraph`                                           |

> Lưu ý: Ở ví dụ đầu, `"..."` gồm cả `kanayago` dù nó bị tắt mặc định. Phần ghi đè đã thay danh sách mặc định, nên mục `"!kanayago"` không còn. Để giữ nó bị tắt, bạn phải thêm `"!kanayago"` vào cấu hình.

### Toolchain

Một số language server cần được cấu hình với "toolchain" hiện tại, tức bản cài đặt của một phiên bản cụ thể của compiler hoặc/và interpreter của ngôn ngữ lập trình, có thể gồm cả bộ dependency đầy đủ của dự án.
Ví dụ về thứ Zode coi là toolchain là virtual environment trong Python.
Không phải ngôn ngữ nào trong Zode cũng hỗ trợ tìm và chọn toolchain, nhưng với những ngôn ngữ có hỗ trợ, bạn có thể chỉ định toolchain từ bộ chọn toolchain (qua {#action toolchain::Select}). Để tìm hiểu thêm về toolchain trong Zode, xem [`toolchains`](./toolchains.md).

### Cấu hình language server

Khi cấu hình language server trong `settings.json`, gợi ý tự động gồm mọi LSP adapter có sẵn mà Zode nhận biết, không chỉ những cái đang hoạt động cho các ngôn ngữ đã tải. Điều này giúp bạn khám phá và cấu hình language server trước khi mở file dùng chúng.

Nhiều language server nhận các tùy chọn cấu hình tùy chỉnh. Bạn đặt chúng trong mục `lsp` của `settings.json`:

```json [settings]
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "check": {
          "command": "clippy"
        }
      }
    }
  }
```

Ví dụ này cấu hình Rust Analyzer dùng Clippy để lint thêm khi lưu file.

#### Đối tượng lồng nhau

Khi cấu hình tùy chọn language server trong Zode, điều quan trọng là dùng đối tượng lồng nhau thay vì chuỗi phân tách bằng dấu chấm. Điều này đặc biệt đáng chú ý với các cấu hình phức tạp. Hãy xem ví dụ thực tế với language server của TypeScript:

Giả sử bạn muốn cấu hình các cài đặt sau cho TypeScript:

- Bật strict null checks
- Đặt phiên bản ECMAScript đích là ES2020

Đây là cách bạn cấu trúc các cài đặt này trong `settings.json` của Zode:

```json [settings]
"lsp": {
  "typescript-language-server": {
    "initialization_options": {
      // These are not supported (VSCode dotted style):
      // "preferences.strictNullChecks": true,
      // "preferences.target": "ES2020"
      //
      // These is correct (nested notation):
      "preferences": {
        "strictNullChecks": true,
        "target": "ES2020"
      },
    }
  }
}
```

#### Các tùy chọn cấu hình có thể dùng

Tùy cách triển khai của từng language server, chúng có thể dựa trên các tùy chọn cấu hình khác nhau, đều được nêu trong LSP.

- [initializationOptions](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#version_3_17_0)

Gửi một lần khi language server khởi động, cần khởi động lại server để áp dụng thay đổi.

Ví dụ, rust-analyzer và clangd chỉ cấu hình theo cách này.

```json [settings]
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "checkOnSave": false
      }
    }
  }
```

- [Configuration Request](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#workspace_configuration)

Server có thể truy vấn nhiều lần.
Phần lớn server chỉ cấu hình theo cách này.

```json [settings]
"lsp": {
  "tailwindcss-language-server": {
    "settings": {
      "tailwindCSS": {
        "emmetCompletions": true,
      },
    }
  }
}
```

Ngoài các tùy chọn cấu hình server liên quan LSP, một số server trong Zode cho phép cấu hình cách Zode khởi chạy binary.

Language server được tự động tải hoặc khởi chạy nếu tìm thấy trong path của bạn; nếu muốn chỉ định rõ một binary khác, bạn có thể khai báo trong cài đặt:

```json [settings]
  "lsp": {
    "rust-analyzer": {
      "binary": {
        // Whether to fetch the binary from the internet, or attempt to find locally.
        "ignore_system_version": false,
        "path": "/path/to/langserver/bin",
        "arguments": ["--option", "value"],
        "env": {
          "FOO": "BAR"
        }
      }
    }
  }
```

### Bật hoặc tắt language server

Bạn có thể bật/tắt hỗ trợ language server toàn cục hoặc theo từng ngôn ngữ:

```json [settings]
  "languages": {
    "Markdown": {
      "enable_language_server": false
    }
  }
```

Cách này tắt language server cho file Markdown, có ích cho hiệu năng ở các dự án tài liệu lớn. Bạn có thể cấu hình toàn cục trong `~/.config/zode/settings.json` hoặc trong `.zode/settings.json` ở thư mục dự án.

## Format và lint

Zode hỗ trợ format và lint code để giữ phong cách code nhất quán và bắt sớm các vấn đề tiềm ẩn.

### Cấu hình formatter

Zode hỗ trợ cả formatter tích hợp sẵn và bên ngoài. Xem tài liệu [`formatter`](./reference/all-settings.md#formatter) để biết thêm. Bạn có thể cấu hình formatter toàn cục hoặc theo ngôn ngữ trong `settings.json`:

```json [settings]
"languages": {
  "JavaScript": {
    "formatter": {
      "external": {
        "command": "prettier",
        "arguments": ["--stdin-filepath", "{buffer_path}"]
      }
    },
    "format_on_save": "on"
  },
  "Rust": {
    "formatter": "language_server",
    "format_on_save": "on"
  }
}
```

Ví dụ này dùng Prettier cho JavaScript và formatter của language server cho Rust, cả hai đều format khi lưu.

Để tắt format cho một ngôn ngữ cụ thể:

```json [settings]
"languages": {
  "Markdown": {
    "format_on_save": "off"
  }
}
```

### Thiết lập linter

Việc lint trong Zode thường do language server đảm nhiệm. Nhiều language server cho phép bạn cấu hình luật lint:

```json [settings]
"lsp": {
  "eslint": {
    "settings": {
      "codeActionOnSave": {
        "rules": ["import/order"]
      }
    }
  }
}
```

Cấu hình này đặt ESLint sắp xếp import khi lưu cho file JavaScript.

Để tự động chạy sửa lỗi của linter khi lưu:

```json [settings]
"languages": {
  "JavaScript": {
    "formatter": {
      "code_action": "source.fixAll.eslint"
    }
  }
}
```

### Format vùng chọn

Zode hỗ trợ chỉ format phần văn bản được chọn qua `editor: format selections` ({#kb editor::FormatSelections}). Cách
này hoạt động tùy theo formatter được cấu hình:

- Action chỉ hiện khi formatter đang dùng thực sự format được theo vùng cho ít nhất một
  buffer được chọn.
- **Language server**: Gửi yêu cầu range formatting của LSP cho mỗi vùng chọn. Cách này cho kết quả
  format chính xác nhất chỉ trong vùng chọn, và chỉ dùng được khi language server được cấu hình
  công bố hỗ trợ range formatting.
- **Prettier**: Dùng range formatting tích hợp của Prettier để format vùng bao quanh mọi vùng chọn. Mọi
  chỉnh sửa rơi ngoài các vùng được chọn đều bị bỏ, nên chỉ code được chọn bị sửa.
- **Lệnh bên ngoài**: Formatter dạng lệnh bên ngoài không hỗ trợ range formatting và bị bỏ qua khi format
  vùng chọn.
- **Formatter dạng code action**: Code action tác động lên toàn bộ buffer, nên tự chúng không bật
  `format selections`.

### Kết hợp format và lint

Zode cho phép chạy cả format và lint khi lưu. Đây là ví dụ dùng Prettier để format và ESLint để lint file JavaScript:

```json [settings]
"languages": {
  "JavaScript": {
    "formatter": [
      {
        "code_action": "source.fixAll.eslint"
      },
      {
        "external": {
          "command": "prettier",
          "arguments": ["--stdin-filepath", "{buffer_path}"]
        }
      }
    ],
    "format_on_save": "on"
  }
}
```

### Xử lý sự cố

Nếu gặp vấn đề với format hoặc lint:

1. Kiểm tra file log của Zode xem có thông báo lỗi không (dùng bảng lệnh (command palette): `zode: open log`)
2. Đảm bảo công cụ bên ngoài (formatter, linter) đã cài đúng và nằm trong PATH
3. Kiểm tra cấu hình ở cả cài đặt Zode và file cấu hình riêng của ngôn ngữ (ví dụ `.eslintrc`, `.prettierrc`)

## Tô màu cú pháp và giao diện

Zode có các tùy chọn tùy biến tô màu cú pháp và giao diện (theme), cho phép bạn chỉnh diện mạo code theo ý mình.

### Tùy chỉnh tô màu cú pháp

Zode dùng ngữ pháp Tree-sitter để tô màu cú pháp. Ghi đè cách tô màu mặc định bằng setting `theme_overrides`.

Ví dụ này làm comment in nghiêng và đổi màu của string:

```json [settings]
"theme_overrides": {
  "One Dark": {
    "syntax": {
      "comment": {
        "font_style": "italic"
      },
      "string": {
        "color": "#00AA00"
      }
    }
  }
}
```

### Chọn và tùy chỉnh theme

Đổi theme:

1. Dùng bộ chọn theme ({#kb theme_selector::Toggle})
2. Hoặc đặt trong `settings.json`:

```json [settings]
"theme": {
  "mode": "dark",
  "dark": "One Dark",
  "light": "GitHub Light"
}
```

Tạo theme riêng bằng cách tạo file JSON trong `~/.config/zode/themes/`. Zode sẽ tự phát hiện và cho dùng mọi theme trong thư mục này.

### Dùng extension theme

Zode hỗ trợ extension theme. Duyệt và cài extension theme từ bảng Extensions ({#kb zode::Extensions}).

Để tạo extension theme riêng, xem hướng dẫn [Phát triển extension theme](./extensions/themes.md).

## Dùng các tính năng của language server

### Semantic token

Semantic token cho phép tô màu cú pháp phong phú hơn bằng thông tin kiểu và phạm vi từ language server. Bật chúng bằng setting `semantic_tokens`:

```json [settings]
"semantic_tokens": "combined"
```

- `"off"` — Chỉ tô màu bằng Tree-sitter (mặc định)
- `"combined"` — Semantic token của LSP phủ lên tree-sitter
- `"full"` — Semantic token của LSP thay thế hoàn toàn tree-sitter

Bạn có thể tùy chỉnh màu và kiểu của token qua `global_lsp_settings.semantic_token_rules` trong cài đặt.

→ [Tài liệu Semantic Tokens](./semantic-tokens.md)

### Inlay hint

Inlay hint cho thêm thông tin ngay trong dòng code, như tên tham số hoặc kiểu suy luận. Cấu hình inlay hint trong `settings.json`:

```json [settings]
"inlay_hints": {
  "enabled": true,
  "show_type_hints": true,
  "show_parameter_hints": true,
  "show_other_hints": true
}
```

Với cài đặt inlay hint riêng cho từng ngôn ngữ, xem tài liệu của ngôn ngữ đó.

### Code action

Code action cung cấp sửa nhanh và tùy chọn refactor. Mở code action bằng lệnh `editor: Toggle Code Actions` hoặc nhấn biểu tượng bóng đèn hiện cạnh con trỏ khi có action.

### Đi tới định nghĩa và tham chiếu

Dùng các lệnh này để điều hướng codebase:

- `editor: Go to Definition` (<kbd>f12|f12</kbd>)
- `editor: Go to Type Definition` (<kbd>cmd-f12|ctrl-f12</kbd>)
- `editor: Find All References` (<kbd>shift-f12|shift-f12</kbd>)

### Đổi tên symbol

Để đổi tên một symbol trong toàn dự án:

1. Đặt con trỏ lên symbol
2. Dùng lệnh `editor: Rename Symbol` (<kbd>f2|f2</kbd>)
3. Nhập tên mới và nhấn Enter

Các tính năng này phụ thuộc vào khả năng của language server của từng ngôn ngữ.

Khi đổi tên một symbol trải trên nhiều file, Zode mở bản xem trước trong một multibuffer. Nhờ đó bạn xem lại mọi thay đổi trong dự án trước khi áp dụng. Để xác nhận đổi tên, chỉ cần lưu multibuffer. Nếu quyết định không tiếp tục, bạn có thể hoàn tác thay đổi hoặc đóng multibuffer mà không lưu.

### Thông tin hover

Dùng lệnh `editor: Hover` để hiện thông tin về symbol dưới con trỏ. Thường gồm thông tin kiểu, tài liệu và liên kết tới tài nguyên liên quan.

### Tìm symbol trong workspace

Lệnh {#action project_symbols::Toggle} cho phép bạn tìm symbol (hàm, class, biến) trong toàn bộ dự án. Rất hữu ích để di chuyển nhanh trong codebase lớn.

### Gợi ý code

Zode đưa ra gợi ý code thông minh khi bạn gõ. Bạn có thể gọi gợi ý thủ công bằng lệnh `editor: Show Completions`. Dùng <kbd>tab|tab</kbd> hoặc <kbd>enter|enter</kbd> để chấp nhận gợi ý.

### Chẩn đoán (Diagnostics)

Language server cung cấp chẩn đoán (lỗi, cảnh báo, gợi ý) theo thời gian thực khi bạn code. Xem mọi chẩn đoán của dự án bằng lệnh {#action diagnostics::Deploy}.
