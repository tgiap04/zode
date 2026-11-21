---
title: Ansible
description: "Configure Ansible language support in Zode, including language servers, formatting, and debugging."
---

# Ansible

Hỗ trợ Ansible trong Zode do [Ansible extension](https://github.com/kartikvashistha/zed-ansible) do cộng đồng duy trì cung cấp.

- Tree-sitter: [zed-industries/tree-sitter-yaml](https://github.com/zed-industries/tree-sitter-yaml)
- Language server: [ansible/vscode-ansible](https://github.com/ansible/vscode-ansible/tree/main/packages/ansible-language-server)

## Thiết lập

### Nhận diện file

Để tránh xử lý nhầm các file YAML không phải Ansible, ngôn ngữ Ansible mặc định không gắn với đuôi file nào.

Để đổi hành vi này, bạn thêm mục `"file_types"` vào cài đặt Zode trong dự án (`.zode/settings.json`) hoặc cài đặt người dùng của Zode (`~/.config/zode/settings.json`) cho khớp quy ước thư mục/đặt tên của bạn. Ví dụ:

```json [settings]
{
  "file_types": {
    "Ansible": [
      "**.ansible.yml",
      "**.ansible.yaml",
      "**/defaults/*.yml",
      "**/defaults/*.yaml",
      "**/meta/*.yml",
      "**/meta/*.yaml",
      "**/tasks/*.yml",
      "**/tasks/*.yaml",
      "**/handlers/*.yml",
      "**/handlers/*.yaml",
      "**/group_vars/*.yml",
      "**/group_vars/*.yaml",
      "**/host_vars/*.yml",
      "**/host_vars/*.yaml",
      "**/playbooks/*.yml",
      "**/playbooks/*.yaml",
      "**playbook*.yml",
      "**playbook*.yaml"
    ]
  }
}
```

Bạn cứ sửa danh sách này theo nhu cầu.

#### Inventory

Nếu file inventory của bạn ở định dạng YAML, bạn có thể:

- Gắn JSON schema inventory của `ansible-lint` vào nó bằng comment sau ở đầu file inventory:

```yml
# yaml-language-server: $schema=https://raw.githubusercontent.com/ansible/ansible-lint/main/src/ansiblelint/schemas/inventory.json
```

- hoặc, cấu hình YAML language server trong cài đặt Zode để đặt schema này cho mọi file inventory khớp mẫu inventory của bạn ([tham khảo](https://zodekit.com/docs/languages/yaml)):

```json [settings]
{
  "lsp": {
    "yaml-language-server": {
      "settings": {
        "yaml": {
          "schemas": {
            "https://raw.githubusercontent.com/ansible/ansible-lint/main/src/ansiblelint/schemas/inventory.json": [
              "./inventory/*.yaml",
              "hosts.yml"
            ]
          }
        }
      }
    }
  }
}
```

### Cấu hình LSP

Mặc định, cấu hình sau được truyền cho Ansible language server. Nó tiện là phản chiếu mặc định mà [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig/blob/03bc581e05e81d33808b42b2d7e76d70adb3b595/lua/lspconfig/configs/ansiblels.lua) đặt cho Ansible language server:

```json
{
  "ansible": {
    "ansible": {
      "path": "ansible"
    },
    "executionEnvironment": {
      "enabled": false
    },
    "python": {
      "interpreterPath": "python3"
    },
    "validation": {
      "enabled": true,
      "lint": {
        "enabled": true,
        "path": "ansible-lint"
      }
    }
  }
}
```

> **Lưu ý:** Để linting chạy được, hãy đảm bảo `ansible-lint` đã được cài và tìm thấy được trong `$PATH` của bạn.

Khi cần, bạn ghi đè được bất kỳ mặc định nào ở trên trong mục `"lsp"` của file settings Zode. Ví dụ:

```json [settings]
{
  "lsp": {
    // The Zed Ansible extension prefixes all settings with `ansible`
    // so use `ansible.path` instead of `ansible.ansible.path`.
    "ansible-language-server": {
      "settings": {
        "ansible": {
          "path": "ansible"
        },
        "executionEnvironment": {
          "enabled": false
        },
        "python": {
          "interpreterPath": "python3"
        },
        "validation": {
          "enabled": false,
          "lint": {
            "enabled": false,
            "path": "ansible-lint"
          }
        }
      }
    }
  }
}
```

Danh sách đầy đủ các tuỳ chọn/cài đặt truyền được cho server nằm ở trang của dự án [tại đây](https://github.com/ansible/vscode-ansible/blob/main/docs/als/settings.md).
