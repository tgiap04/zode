---
title: YAML
description: "Configure YAML language support in Zode, including language servers, formatting, and debugging."
---

# YAML

Zode hỗ trợ YAML sẵn có.

- Tree-sitter: [zed-industries/tree-sitter-yaml](https://github.com/zed-industries/tree-sitter-yaml)
- Language server: [redhat-developer/yaml-language-server](https://github.com/redhat-developer/yaml-language-server)

## Cấu hình

Bạn có thể cấu hình nhiều [setting của yaml-language-server](https://github.com/redhat-developer/yaml-language-server?tab=readme-ov-file#language-server-settings) bằng cách thêm chúng vào settings.json của Zode, trong khối `yaml-language-server` dưới khóa `lsp`.

Bạn có thể cấu hình schema YAML riêng bằng đường dẫn tương đối. Zode phân giải đường dẫn tương đối theo thư mục gốc của dự án:

```json [settings]
  "lsp": {
    "yaml-language-server": {
      "settings": {
        "yaml": {
          "keyOrdering": true,
          "format": {
            "singleQuote": true
          },
          "schemas": {
              "https://getcomposer.org/schema.json": ["/*"],
              "./schemas/kubernetes.yaml": "k8s/**/*.yaml",
              "~/global-schemas/docker-compose.yaml": "docker-compose*.yml"
          }
        }
      }
    }
  }
```

Đường dẫn bắt đầu bằng `./` được phân giải theo thư mục gốc của worktree. Đường dẫn bắt đầu bằng `~/` được mở rộng thành thư mục home của bạn.

Lưu ý: các khóa setting phải lồng nhau, nên `yaml.keyOrdering` trở thành `{"yaml": { "keyOrdering": true }}`.

## Định dạng

Mặc định, Zode dùng Prettier để định dạng file YAML.

### Định dạng bằng Prettier

Bạn có thể tùy chỉnh cách Prettier định dạng. Ví dụ để dùng nháy đơn trong file yaml, thêm đoạn sau vào file cấu hình `.prettierrc`:

```json
{
  "overrides": [
    {
      "files": ["*.yaml", "*.yml"],
      "options": {
        "singleQuote": false
      }
    }
  ]
}
```

### Định dạng bằng yaml-language-server

Để dùng `yaml-language-server` thay cho Prettier khi định dạng YAML, cấu hình trong Cài đặt ({#kb zode::OpenSettings}) mục Languages > YAML, hoặc thêm vào file settings:

```json [settings]
  "languages": {
    "YAML": {
      "formatter": "language_server"
    }
  }
```

## Schema

Mặc định, yaml-language-server sẽ thử xác định schema đúng cho từng file yaml và lấy JSON Schema phù hợp từ [Json Schema Store](https://schemastore.org/).

Bạn có thể ghi đè schema tự phát hiện qua khóa setting `schemas` (như ví dụ ở trên) hoặc cung cấp tham chiếu [schema nội tuyến](https://github.com/redhat-developer/yaml-language-server#using-inlined-schema) bằng một comment modeline ở đầu file yaml:

```yaml
# yaml-language-server: $schema=https://www.schemastore.org/github-action.json
name: Issue Assignment
on:
  issues:
    types: [opened]
```

Nếu muốn, bạn có thể tắt việc tự phát hiện và lấy schema từ JSON Schema:

```json [settings]
  "lsp": {
    "yaml-language-server": {
      "settings": {
        "yaml": {
          "schemaStore": {
            "enable": false
          }
        }
      }
    }
  }
```

## Tag tùy chỉnh

Yaml-language-server hỗ trợ [tag tùy chỉnh](https://github.com/redhat-developer/yaml-language-server#adding-custom-tags), dùng để đưa chức năng tùy biến của ứng dụng vào file yaml lúc chạy.

Ví dụ YAML của Amazon CloudFormation dùng khá nhiều tag tùy chỉnh. Để hỗ trợ chúng, bạn thêm đoạn sau vào settings.json:

```json [settings]
  "lsp": {
    "yaml-language-server": {
      "settings": {
        "yaml": {
          "customTags": [
            "!And scalar",
            "!And mapping",
            "!And sequence",
            "!If scalar",
            "!If mapping",
            "!If sequence",
            "!Not scalar",
            "!Not mapping",
            "!Not sequence",
            "!Equals scalar",
            "!Equals mapping",
            "!Equals sequence",
            "!Or scalar",
            "!Or mapping",
            "!Or sequence",
            "!FindInMap scalar",
            "!FindInMap mapping",
            "!FindInMap sequence",
            "!Base64 scalar",
            "!Base64 mapping",
            "!Base64 sequence",
            "!Cidr scalar",
            "!Cidr mapping",
            "!Cidr sequence",
            "!Ref scalar",
            "!Ref mapping",
            "!Ref sequence",
            "!Sub scalar",
            "!Sub mapping",
            "!Sub sequence",
            "!GetAtt scalar",
            "!GetAtt mapping",
            "!GetAtt sequence",
            "!GetAZs scalar",
            "!GetAZs mapping",
            "!GetAZs sequence",
            "!ImportValue scalar",
            "!ImportValue mapping",
            "!ImportValue sequence",
            "!Select scalar",
            "!Select mapping",
            "!Select sequence",
            "!Split scalar",
            "!Split mapping",
            "!Split sequence",
            "!Join scalar",
            "!Join mapping",
            "!Join sequence",
            "!Condition scalar",
            "!Condition mapping",
            "!Condition sequence"
          ]
        }
      }
    }
  }
```
