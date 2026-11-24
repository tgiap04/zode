---
title: Rust
description: "Configure Rust language support in Zode, including language servers, formatting, and debugging."
---

# Rust

Rust được hỗ trợ sẵn trong Zode.

- Tree-sitter: [tree-sitter/tree-sitter-rust](https://github.com/tree-sitter/tree-sitter-rust)
- Language Server: [rust-lang/rust-analyzer](https://github.com/rust-lang/rust-analyzer)
- Debug Adapter: [CodeLLDB](https://github.com/vadimcn/codelldb) (chính), [GDB](https://sourceware.org/gdb/) (phụ, không có trên Apple silicon)

<!--
TBD: Polish Rust docs. Zode has strong Rust support, and the docs should reflect that clearly.
TBD: Users may not know what inlayHints, don't start there.
TBD: Provide explicit examples not just `....`
-->

## Inlay Hints

Có thể dùng cấu hình sau để đổi cài đặt inlay hint của `rust-analyzer` trong Rust:

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "inlayHints": {
          "maxLength": null,
          "lifetimeElisionHints": {
            "enable": "skip_trivial",
            "useParameterNames": true
          },
          "closureReturnTypeHints": {
            "enable": "always"
          }
        }
      }
    }
  }
}
```

Xem [Inlay Hints](https://rust-analyzer.github.io/book/features.html#inlay-hints) trong sách hướng dẫn Rust Analyzer để biết thêm.

## Thư mục target

Có thể đặt thư mục target của `rust-analyzer` trong `initialization_options`:

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "rust": {
          "analyzerTargetDir": true
        }
      }
    }
  }
}
```

Đặt `true` thì thư mục target là `target/rust-analyzer`. Bạn có thể chỉ định thư mục riêng bằng một chuỗi như `"target/analyzer"` thay cho `true`.

## Binary

Bạn có thể cấu hình Zode dùng binary `rust-analyzer` nào.

Mặc định, Zode tìm `rust-analyzer` trong `$PATH` và dùng nó. Nếu binary đó chạy được `rust-analyzer --help` thì dùng luôn. Nếu không, Zode tự cài bản `rust-analyzer` stable của riêng nó rồi dùng bản đó.

Nếu muốn cài bản `rust-analyzer` pre-release, bạn có thể bảo Zode làm vậy bằng cách đặt `pre_release` thành `true` trong `settings.json`:

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "fetch": {
        "pre_release": true
      }
    }
  }
}
```

Nếu muốn Zode không tìm binary `rust-analyzer` trong hệ thống, đặt `ignore_system_version` thành `true` trong `settings.json`:

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "binary": {
        "ignore_system_version": true
      }
    }
  }
}
```

Nếu muốn dùng binary ở vị trí tuỳ chọn, chỉ định `path` và `arguments` (không bắt buộc):

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "binary": {
        "path": "/Users/example/bin/rust-analyzer",
        "arguments": []
      }
    }
  }
}
```

`"path"` này phải là đường dẫn tuyệt đối.

## Target thay thế

Nếu muốn rust-analyzer báo chẩn đoán cho một target khác nền tảng hiện tại (ví dụ cho windows khi đang chạy trên macOS), dùng các cài đặt lsp sau của Zode:

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "cargo": {
          "target": "x86_64-pc-windows-msvc"
        }
      }
    }
  }
}
```

Nếu dùng `rustup`, bạn có thể xem danh sách target triple có sẵn (`aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, v.v.) bằng cách chạy:

```sh
rustup target list --installed
```

## Task từ LSP

Zode cung cấp task dựa trên tree-sitter, nhưng rust-analyzer có một method mở rộng LSP để truy vấn các task liên quan tới file qua LSP.
Tính năng này bật mặc định và có thể cấu hình như sau

```json [settings]
"lsp": {
  "rust-analyzer": {
    "enable_lsp_tasks": true,
  }
}
```

## Tự lấy chẩn đoán Cargo

Mặc định rust-analyzer bật `checkOnSave: true`, nên mỗi lần lưu buffer sẽ chạy lệnh `cargo check --workspace --all-targets`.
Nếu tắt bằng `checkOnSave: false` (xem ví dụ cấu hình server ở trên), vẫn có thể lấy chẩn đoán thủ công bằng các lệnh `editor: run/clear/cancel flycheck` trong file Rust để làm mới chẩn đoán cargo; trình soạn thảo chẩn đoán dự án cũng làm mới chẩn đoán cargo bằng lệnh `editor: run flycheck` khi cài đặt này được bật.

## Cấu hình server thêm

<!--
TBD: Is it possible to specify RUSTFLAGS? https://github.com/zed-industries/zed/issues/14334
-->

[Sách hướng dẫn](https://rust-analyzer.github.io/book/) của Rust-analyzer mô tả nhiều tính năng và tuỳ chọn cấu hình cho language server rust-analyzer.
Rust-analyzer trong Zode chạy với tham số mặc định.

### Dự án lớn và hiệu năng

Một trong những nguyên nhân chính có thể ngốn nhiều tài nguyên trên dự án lớn là sự kết hợp của các tính năng sau:

```
rust-analyzer.checkOnSave (default: true)
    Run the check command for diagnostics on save.
```

```
rust-analyzer.check.workspace (default: true)
    Whether --workspace should be passed to cargo check. If false, -p <package> will be passed instead.
```

```
rust-analyzer.cargo.allTargets (default: true)
    Pass --all-targets to cargo invocation
```

Nghĩa là mỗi lần Zode lưu, lệnh `cargo check --workspace --all-targets` được chạy, kiểm tra toàn bộ dự án (workspace), lib, doc, test, bin, bench và [các target khác](https://doc.rust-lang.org/cargo/reference/cargo-targets.html).

Với dự án nhỏ thì không sao, nhưng nó không mở rộng tốt.

Phương án thay thế là dùng [task](../tasks.md), vì Zode đã có sẵn task `cargo check --workspace --all-targets` và cho phép cmd/ctrl-click vào đầu ra terminal để nhảy tới lỗi, đồng thời giới hạn hoặc tắt hẳn tính năng check khi lưu.

Tính năng check khi lưu chịu trách nhiệm trả về một phần chẩn đoán dựa trên đầu ra của cargo check, nên tắt nó thì rust-analyzer chỉ còn [chẩn đoán](https://rust-analyzer.github.io/book/diagnostics.html) của riêng nó.

Hãy xem thêm các cài đặt `rust-analyzer.cargo.`, `rust-analyzer.check.` và `rust-analyzer.diagnostics.` trong sách hướng dẫn để cấu hình chi tiết hơn.
Đây là đoạn mẫu cho settings.json của Zode (language server tự khởi động lại sau khi phần `lsp.rust-analyzer` được sửa và lưu):

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        // get more cargo-less diagnostics from rust-analyzer,
        // which might include false-positives (those can be turned off by their names)
        "diagnostics": {
          "experimental": {
            "enable": true
          }
        },
        // To disable the checking entirely
        // (ignores all cargo and check settings below)
        "checkOnSave": false,
        // To check the `lib` target only.
        "cargo": {
          "allTargets": false
        },
        // Use `-p` instead of `--workspace` for cargo check
        "check": {
          "workspace": false
        }
      }
    }
  }
}
```

### Workspace nhiều dự án

Nếu muốn rust-analyzer phân tích nhiều dự án Rust trong cùng một thư mục mà không nằm trong `[members]` của Cargo workspace,
bạn có thể liệt kê chúng trong `linkedProjects` ở cài đặt dự án cục bộ:

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "linkedProjects": ["./path/to/a/Cargo.toml", "./path/to/b/Cargo.toml"]
      }
    }
  }
}
```

### Snippet

Có cách để rust-analyzer đưa ra các mục gợi ý tuỳ chỉnh, biến đổi code theo nội dung snippet:

```json [settings]
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "completion": {
          "snippets": {
            "custom": {
              "Arc::new": {
                "postfix": "arc",
                "body": ["Arc::new(${receiver})"],
                "requires": "std::sync::Arc",
                "scope": "expr"
              },
              "Some": {
                "postfix": "some",
                "body": ["Some(${receiver})"],
                "scope": "expr"
              },
              "Ok": {
                "postfix": "ok",
                "body": ["Ok(${receiver})"],
                "scope": "expr"
              },
              "Rc::new": {
                "postfix": "rc",
                "body": ["Rc::new(${receiver})"],
                "requires": "std::rc::Rc",
                "scope": "expr"
              },
              "Box::pin": {
                "postfix": "boxpin",
                "body": ["Box::pin(${receiver})"],
                "requires": "std::boxed::Box",
                "scope": "expr"
              },
              "vec!": {
                "postfix": "vec",
                "body": ["vec![${receiver}]"],
                "description": "vec![]",
                "scope": "expr"
              }
            }
          }
        }
      }
    }
  }
}
```

## Gỡ lỗi

Zode hỗ trợ gỡ lỗi binary và test Rust ngay từ đầu với `CodeLLDB` và `GDB`. Chạy {#action debugger::Start} ({#kb debugger::Start}) để khởi chạy một trong các debug task cấu hình sẵn này.

Để kiểm soát nhiều hơn, bạn có thể thêm cấu hình gỡ lỗi vào `.zode/debug.json`. Xem các ví dụ bên dưới.

- [Tài liệu cấu hình CodeLLDB](https://github.com/vadimcn/codelldb/blob/master/MANUAL.md#starting-a-new-debug-session)
- [Tài liệu cấu hình GDB](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Debugger-Adapter-Protocol.html)

### Build binary rồi gỡ lỗi

```json [debug]
[
  {
    "label": "Build & Debug native binary",
    "build": {
      "command": "cargo",
      "args": ["build"]
    },
    "program": "$ZODE_WORKTREE_ROOT/target/debug/binary",
    // sourceLanguages is required for CodeLLDB (not GDB) when using Rust
    "sourceLanguages": ["rust"],
    "request": "launch",
    "adapter": "CodeLLDB"
  }
]
```

### Tự xác định đích gỡ lỗi từ lệnh build

Khi dùng `cargo build` hoặc `cargo test` làm lệnh build, Zode có thể suy ra đường dẫn tới binary đầu ra.

```json [debug]
[
  {
    "label": "Build & Debug native binary",
    "adapter": "CodeLLDB",
    "build": {
      "command": "cargo",
      "args": ["build"]
    },
    // sourceLanguages is required for CodeLLDB (not GDB) when using Rust
    "sourceLanguages": ["rust"]
  }
]
```
