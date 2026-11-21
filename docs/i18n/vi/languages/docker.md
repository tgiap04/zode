---
title: Docker
description: "Configure Docker language support in Zode, including language servers, formatting, and debugging."
---

# Docker

Hỗ trợ `Dockerfile` và `docker-compose.yaml` trong Zode do các extension cộng đồng duy trì.

## Docker Compose

Hỗ trợ ngôn ngữ cho `compose.yaml` của Docker trong Zode đến từ [extension Docker Compose](https://github.com/eth0net/zed-docker-compose). Báo lỗi tại: [https://github.com/eth0net/zed-docker-compose/issues](https://github.com/eth0net/zed-docker-compose/issues).

- Language Server: [microsoft/compose-language-service](https://github.com/microsoft/compose-language-service)

## Dockerfile

Hỗ trợ ngôn ngữ cho `Dockerfile` trong Zode đến từ [extension Dockerfile](https://github.com/d1y/dockerfile.zed). Báo lỗi tại: [https://github.com/d1y/dockerfile.zed/issues](https://github.com/d1y/dockerfile.zed/issues).

- Tree-sitter: [camdencheek/tree-sitter-dockerfile](https://github.com/camdencheek/tree-sitter-dockerfile)
- Language Server: [rcjsuen/dockerfile-language-server](https://github.com/rcjsuen/dockerfile-language-server)
