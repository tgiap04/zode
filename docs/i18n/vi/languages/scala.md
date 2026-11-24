---
title: Scala
description: "Configure Scala language support in Zode, including language servers, formatting, and debugging."
---

# Scala

Scala trong Zode được hỗ trợ bởi [extension Scala](https://github.com/scalameta/metals-zed) do cộng đồng duy trì.
Báo lỗi tại: [https://github.com/scalameta/metals-zed/issues](https://github.com/scalameta/metals-zed/issues)

- Tree-sitter: [tree-sitter/tree-sitter-scala](https://github.com/tree-sitter/tree-sitter-scala)
- Language Server: [scalameta/metals](https://github.com/scalameta/metals)

## Thiết lập

- Cài Scala bằng `cs setup` (Coursier): https://www.scala-lang.org/download/
  - `brew install coursier/formulas/coursier && cs setup`
- Hướng dẫn thiết lập REPL (Almond) https://almond.sh/docs/quick-start-install
  - `brew install --cask temurin` (bản OpenJDK chính thức của Eclipse foundation)
  - `brew install coursier/formulas/coursier && cs setup`
  - `coursier launch --use-bootstrap almond -- --install`

## Cấu hình

Hành vi của language server Metals có thể điều khiển bằng:

- File `.scalafix.conf` - Xem [Scalafix Configuration](https://scalacenter.github.io/scalafix/docs/users/configuration.html)
- File `.scalafmt.conf` - Xem [Scalafmt Configuration](https://scalameta.org/scalafmt/docs/configuration.html)

Bạn có thể đặt các file này ở thư mục gốc của dự án hoặc chỉ định vị trí của chúng trong cấu hình Metals. Xem thêm [Metals User Configuration](https://scalameta.org/metals/docs/editors/user-configuration).

<!--
TBD: Provide LSP configuration example for metals in Zode settings.json. metals.{javaHome,excludedPackages,customProjectRoot} etc.
-->
