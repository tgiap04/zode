---
title: Extension Capabilities
description: "Extension Capabilities for Zode extensions."
---

# Capability của extension

Những thao tác mà extension của Zode được phép thực hiện do một hệ thống capability quản lý.

## Hạn chế capability

Là người dùng, bạn có thể hạn chế các capability cấp cho extension.

Việc này điều khiển qua cài đặt `granted_extension_capabilities`.

Khi hạn chế hoặc gỡ một capability, extension gọi API tương ứng mà không đủ capability sẽ nhận lỗi.

Ví dụ, để chỉ cho tải file từ GitHub, đặt `host` cho capability `download_file`:

```diff
{
  "granted_extension_capabilities": [
    { "kind": "process:exec", "command": "*", "args": ["**"] },
-   { "kind": "download_file", "host": "*", "path": ["**"] },
+   { "kind": "download_file", "host": "github.com", "path": ["**"] },
    { "kind": "npm:install", "package": "*" }
  ]
}
```

Nếu không muốn extension dùng _bất kỳ_ capability nào, bạn có thể gỡ hết các capability đã cấp:

```json
{
  "granted_extension_capabilities": []
}
```

> Lưu ý: làm vậy nhiều khả năng khiến nhiều extension không chạy được, ít nhất ở cấu hình mặc định.

## Các capability

### `process:exec`

Capability `process:exec` cho phép extension chạy lệnh bằng [`zed_extension_api::process::Command`](https://docs.rs/zed_extension_api/latest/zed_extension_api/process/struct.Command.html).

#### Ví dụ

Cho phép chạy bất kỳ lệnh nào với mọi đối số:

```toml
{ kind = "process:exec", command = "*", args = ["**"] }
```

Cho phép một lệnh cụ thể (ví dụ `gem`) chạy với mọi đối số:

```toml
{ kind = "process:exec", command = "gem", args = ["**"] }
```

### `download_file`

Capability `download_file` cho phép extension tải file bằng [`zed_extension_api::download_file`](https://docs.rs/zed_extension_api/latest/zed_extension_api/fn.download_file.html).

#### Ví dụ

Cho phép tải bất kỳ file nào:

```toml
{ kind = "download_file", host = "*", path = ["**"] }
```

Cho phép tải bất kỳ file nào từ `github.com`:

```toml
{ kind = "download_file", host = "github.com", path = ["**"] }
```

Cho phép tải bất kỳ file nào từ một repository GitHub cụ thể:

```toml
{ kind = "download_file", host = "github.com", path = ["zed-industries", "zed", "**"] }
```

### `npm:install`

Capability `npm:install` cho phép extension cài package npm bằng [`zed_extension_api::npm_install_package`](https://docs.rs/zed_extension_api/latest/zed_extension_api/fn.npm_install_package.html).

#### Ví dụ

Cho phép cài bất kỳ package npm nào:

```toml
{ kind = "npm:install", package = "*" }
```

Cho phép cài một package npm cụ thể (ví dụ `typescript`):

```toml
{ kind = "npm:install", package = "typescript" }
```
