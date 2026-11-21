---
title: SQL
description: "Configure SQL language support in Zode, including language servers, formatting, and debugging."
---

# SQL

File SQL do [SQL Extension](https://github.com/zed-extensions/sql) xử lý.

- Tree-sitter: [nervenes/tree-sitter-sql](https://github.com/nervenes/tree-sitter-sql)

### Định dạng

Zode hỗ trợ tự định dạng SQL bằng công cụ bên ngoài như [`sql-formatter`](https://github.com/sql-formatter-org/sql-formatter).

1. Cài `sql-formatter`:

```sh
npm install -g sql-formatter
```

2. Đảm bảo `sql-formatter` có trong path của bạn và kiểm tra phiên bản:

```sh
which sql-formatter
sql-formatter --version
```

3. Cấu hình định dạng trong Settings ({#kb zode::OpenSettings}) tại Languages > SQL, hoặc thêm vào file settings:

```json [settings]
  "languages": {
    "SQL": {
      "formatter": {
        "external": {
          "command": "sql-formatter",
          "arguments": ["--language", "mysql"]
        }
      }
    }
  },
```

Thay `mysql` ở trên bằng [SQL Dialect] bạn muốn dùng (`duckdb`, `hive`, `mariadb`, `postgresql`, `redshift`, `snowflake`, `sqlite`, `spark`, v.v.).

Bạn thêm vào cài đặt dự án của Zode (`.zode/settings.json`) hoặc cài đặt người dùng của Zode (`~/.config/zode/settings.json`).

### Định dạng nâng cao

Sql-formatter cũng cho kiểm soát chính xác hơn qua [các tuỳ chọn cấu hình của sql-formatter](https://github.com/sql-formatter-org/sql-formatter#configuration-options). Để đặt chúng, tạo file `.sql-formatter.json` trong dự án:

```json
{
  "language": "postgresql",
  "tabWidth": 2,
  "keywordCase": "upper",
  "linesBetweenQueries": 2
}
```

Khi dùng file `.sql-formatter.json`, bạn dùng được cấu hình settings Zode rút gọn:

```json [settings]
{
  "languages": {
    "SQL": {
      "formatter": {
        "external": {
          "command": "sql-formatter"
        }
      }
    }
  }
}
```
