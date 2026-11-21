---
title: Java
description: "Configure Java language support in Zode, including language servers, formatting, and debugging."
---

# Java

Hỗ trợ ngôn ngữ Java trong Zode do các thành phần sau cung cấp:

- Zode Java: [zed-extensions/java](https://github.com/zed-extensions/java)
- Tree-sitter: [tree-sitter/tree-sitter-java](https://github.com/tree-sitter/tree-sitter-java)
- Language server: [eclipse-jdtls/eclipse.jdt.ls](https://github.com/eclipse-jdtls/eclipse.jdt.ls)

## Cài OpenJDK

Bạn cần cài Java runtime (OpenJDK).

- macOS: `brew install openjdk`
- Ubuntu: `sudo add-apt-repository ppa:openjdk-23 && sudo apt-get install openjdk-23`
- Windows: `choco install openjdk`
- Arch Linux: `sudo pacman -S jre-openjdk-headless`

Hoặc tải và cài thủ công [OpenJDK 23](https://jdk.java.net/23/).

## Cài extension

Bạn cài bằng cách mở {#action zode::Extensions}({#kb zode::Extensions}) và tìm `java`.

## Bắt đầu nhanh và cấu hình

Với đa số người dùng, hỗ trợ Java chạy được ngay, không cần cấu hình.

- Nên mở dự án sao cho thư mục gốc dự án của Zode trùng với thư mục gốc dự án Java (nơi thường có file `pom.xml` hoặc `build.gradle`).

- Mặc định extension sẽ tải và chạy bản JDTLS chính thức mới nhất cho bạn, nhưng cần máy có Java 21 qua biến môi trường `$JAVA_HOME` hoặc file thực thi `java(.exe)` trong `$PATH`. Nếu dự án cần Java đời thấp hơn trong môi trường, bạn chỉ định JDK khác để chạy JDTLS qua tùy chọn cấu hình `java_home`.

- Bạn có thể cung cấp **script khởi chạy JDTLS riêng** bằng cách thêm file thực thi tên `jdtls` (hoặc `jdtls.bat` trên Windows) vào biến môi trường `$PATH`. Nếu có file này, extension sẽ bỏ qua việc tải và chạy bản do nó quản lý, và dùng bản trong môi trường.

- Để hỗ trợ [Lombok](https://projectlombok.org/), phải tải lombok-jar và đăng ký làm Java-Agent khi chạy JDTLS. Mặc định extension tự lo việc này, nhưng nếu không muốn, bạn đặt tùy chọn cấu hình `lombok_support` thành `false`.

Dưới đây là một `settings.json` thường gặp, gồm các cấu hình nói trên:

```jsonc
{
  "lsp": {
    "jdtls": {
      "settings": {
        "java_home": "/path/to/your/JDK21+",
        "lombok_support": true,
      },
    },
  },
}
```

## Gỡ lỗi

Hỗ trợ gỡ lỗi được bật qua [bản fork của Java Debug](https://github.com/zed-industries/java-debug), extension sẽ tự tải và chạy cho bạn. Xem [tài liệu Debugger](https://zodekit.com/docs/debugger#getting-started) để biết cách gỡ lỗi nói chung hoạt động ra sao trong Zode.

Để bắt đầu với Java, bấm nút `edit debug.json` trong menu Debug, rồi thay nội dung file bằng đoạn sau:

```jsonc
[
  {
    "adapter": "Java",
    "request": "launch",
    "label": "Launch Debugger",
    // if your project has multiple entry points, specify the one to use:
    // "mainClass": "com.myorganization.myproject.MyMainClass",
    //
    // this effectively sets a breakpoint at your program entry:
    "stopOnEntry": true,
    // the working directory for the debug process
    "cwd": "$ZODE_WORKTREE_ROOT",
  },
]
```

Sau đó bạn có thể bắt đầu một phiên gỡ lỗi mới bằng kịch bản "Launch Debugger" trong menu debug.

## Script khởi chạy (tức task) trên Windows

Extension này cung cấp các task để chạy ứng dụng và test ngay trong Zode, qua các nút play nhỏ cạnh test/điểm vào. Tuy nhiên, do giới hạn hiện tại của giao diện extension trong Zode, chúng tôi không thể cung cấp script chạy được trên cả Maven lẫn Gradle, ở cả Windows và hệ tương thích Unix, nên mặc định các script khởi chạy chỉ chạy trên Mac và Linux.

Có một cách sửa khá đơn giản để chạy được trên Windows: tự cung cấp script task của bạn. Xem [issue này](https://github.com/zed-extensions/java/issues/94) để biết cách làm, và đọc [phần Tasks trong tài liệu của Zed](https://zodekit.com/docs/tasks) để biết thêm.

## Cấu hình nâng cao / tùy chọn khởi tạo JDTLS

JDTLS có nhiều tùy chọn cấu hình, truyền được qua LSP-request `initialize`. Extension sẽ chuyển đối tượng JSON từ `lsp.jdtls.settings.initialization_options` trong setting của bạn sang JDTLS. Xem [trang wiki Cấu hình JDTLS](https://github.com/eclipse-jdtls/eclipse.jdt.ls/wiki/Running-the-JAVA-LS-server-from-the-command-line#initialize-request) để biết các tùy chọn và giá trị. Dưới đây là một `settings.json` ví dụ, chuyển cấu hình mẫu từ trang wiki trên sang JDTLS:

```jsonc
{
  "lsp": {
    "jdtls": {
      "settings": {
        // this will be sent to JDTLS as initializationOptions:
        "initialization_options": {
          "bundles": [],
          // use this if your Zode project root folder is not the same as the java project root:
          "workspaceFolders": ["file:///home/snjeza/Project"],
          "settings": {
            "java": {
              "home": "/usr/local/jdk-9.0.1",
              "errors": {
                "incompleteClasspath": {
                  "severity": "warning",
                },
              },
              "configuration": {
                "updateBuildConfiguration": "interactive",
                "maven": {
                  "userSettings": null,
                },
              },
              "import": {
                "gradle": {
                  "enabled": true,
                },
                "maven": {
                  "enabled": true,
                },
                "exclusions": [
                  "**/node_modules/**",
                  "**/.metadata/**",
                  "**/archetype-resources/**",
                  "**/META-INF/maven/**",
                  "/**/test/**",
                ],
              },
              "referencesCodeLens": {
                "enabled": false,
              },
              "signatureHelp": {
                "enabled": false,
              },
              "implementationCodeLens": "all",
              "format": {
                "enabled": true,
              },
              "saveActions": {
                "organizeImports": false,
              },
              "contentProvider": {
                "preferred": null,
              },
              "autobuild": {
                "enabled": false,
              },
              "completion": {
                "favoriteStaticMembers": [
                  "org.junit.Assert.*",
                  "org.junit.Assume.*",
                  "org.junit.jupiter.api.Assertions.*",
                  "org.junit.jupiter.api.Assumptions.*",
                  "org.junit.jupiter.api.DynamicContainer.*",
                  "org.junit.jupiter.api.DynamicTest.*",
                ],
                "importOrder": ["java", "javax", "com", "org"],
              },
            },
          },
        },
      },
    },
  },
}
```

## Xem thêm

[Zode Java Repo](https://github.com/zed-extensions/java)
[Eclipse JDTLS Repo](https://github.com/eclipse-jdtls/eclipse.jdt.ls)
