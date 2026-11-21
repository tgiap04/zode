---
title: PowerShell
description: "Configure PowerShell language support in Zode, including language servers, formatting, and debugging."
---

# PowerShell

Hỗ trợ ngôn ngữ PowerShell trong Zode do [Zode PowerShell extension](https://github.com/wingyplus/zed-powershell) do cộng đồng duy trì cung cấp. Vui lòng báo lỗi tại: [github.com/wingyplus/zed-powershell/issues](https://github.com/wingyplus/zed-powershell/issues)

- Tree-sitter: [airbus-cert/tree-sitter-powershell](https://github.com/airbus-cert/tree-sitter-powershell)
- Language server: [PowerShell/PowerShellEditorServices](https://github.com/PowerShell/PowerShellEditorServices)

## Thiết lập

### Cài PowerShell 7+ {#powershell-install}

- macOS: `brew install powershell/tap/powershell`
- Alpine: [Cài PowerShell trên Alpine Linux](https://learn.microsoft.com/en-us/powershell/scripting/install/install-alpine)
- Debian: [Cài PowerShell trên Debian Linux](https://learn.microsoft.com/en-us/powershell/scripting/install/install-debian)
- RedHat: [Cài PowerShell trên RHEL](https://learn.microsoft.com/en-us/powershell/scripting/install/install-rhel)
- Ubuntu: [Cài PowerShell trên RHEL](https://learn.microsoft.com/en-us/powershell/scripting/install/install-ubuntu)
- Windows: [Cài PowerShell trên Windows](https://learn.microsoft.com/en-us/powershell/scripting/install/installing-powershell-on-windows)

Zode PowerShell extension mặc định dùng file thực thi `pwsh` tìm thấy trong path của bạn.

### Cài PowerShell Editor Services (tuỳ chọn) {#powershell-editor-services}

Zode PowerShell extension sẽ tự tải [PowerShell Editor Services](https://github.com/PowerShell/PowerShellEditorServices).

Nếu muốn dùng một file binary cụ thể, bạn khai báo trong settings.json của Zode:

```json [settings]
  "lsp": {
    "powershell-es": {
      "binary": {
        "path": "/path/to/PowerShellEditorServices"
      }
    }
  }
```
