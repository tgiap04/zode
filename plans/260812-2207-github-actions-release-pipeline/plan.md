---
title: 'Release pipeline: nightly 6 target + tag tự nối installer vào release'
description: >-
  Tháo hạ tầng riêng của Zed (Namespace runner, self-hosted Windows, Sentry,
  sccache, compliance, Slack, DigitalOcean) ra khỏi bộ máy release có sẵn, ép
  profile build vừa runner GitHub-hosted free, rồi nối lại thành hai đường:
  cron nightly đẩy 6 asset vào một prerelease cuộn, và push tag v* build lại từ
  commit của tag rồi attach vào draft release.
status: pending
priority: P1
effort: 3-6d
branch: feat/release-pipeline
tags:
  - ci
  - release
  - build
  - xtask
  - fork
blockedBy: []
blocks: []
work_type: deliverable
spec_waived: 'SDD mode disabled (takumi.sddMode: off)'
created: 2026-08-19
---

# Release pipeline cho Zode trên runner GitHub-hosted free

**Status:** pending · **Priority:** P1 · **Branch:** `feat/release-pipeline`
**Bắt nguồn từ:** [brainstorm-260819-release-pipeline-public-repo.md](./reports/brainstorm-260819-release-pipeline-public-repo.md) — 7 quyết định đã chốt
**Đã hết hiệu lực:** [brainstorm-260812](./reports/brainstorm-260812-github-actions-release-pipeline.md) — dựng trên tiền đề repo private; repo giờ là PUBLIC

## Hình dạng

```
tgiap04/zode  (PUBLIC — một repo duy nhất, không repo phụ, không PAT)

  develop ──PR──> main
                   │
                   ├─ run_tests / clippy            (KHÔNG bundle ở merge)
                   │
                   ├─ cron 0 7 * * *  trên main
                   │    6 job song song, 1 target/job, runner native:
                   │      macos-15         → bundle-mac aarch64-apple-darwin
                   │      macos-15-intel   → bundle-mac x86_64-apple-darwin
                   │      ubuntu-22.04     → bundle-linux
                   │      ubuntu-22.04-arm → bundle-linux
                   │      windows-2025     → bundle-windows.ps1 -Architecture x86_64
                   │      windows-11-arm   → bundle-windows.ps1 -Architecture aarch64
                   │    → 1 prerelease CUỘN `nightly` (xoá asset cũ mỗi lần)
                   │
                   └─ push tag v*
                        cùng 6 job đó, build LẠI từ commit của tag
                        → draft release → attach 6 asset → người dùng bấm publish
```

Không tái dùng artifact của nightly ở đường tag — build lại là thứ bảo đảm binary khớp đúng source của tag.

## Phase

| # | Việc | Status | Blocked by |
|---|---|---|---|
| 01 | [Runner + cổng owner: workflow chạy được trên account này](phase-01-runners-and-owner-gate.md) | ✅ | — |
| 02 | [Script: signing Windows, bỏ `remote_server`, thay channel](phase-02-scripts-signing-and-remote-server.md) | ✅ | — |
| 03 | [Ép build vừa 14 GB / 7 GB: env override + dọn disk](phase-03-fit-the-build-on-free-runners.md) | 🔶 code xong, chưa chạy CI | 01, 02 |
| 04 | [Hai đường phát hành: nightly cuộn + draft release ở tag](phase-04-nightly-and-tag-release.md) | 🔶 code xong, chưa chạy CI | 03 |
| 05 | [Kiểm cài trên máy sạch từng OS + README](phase-05-verify-on-clean-machines.md) | ⬜ README xong, chưa kiểm máy | 04 |

**01 và 02 chạy song song được** — file ownership rời hẳn nhau (`tooling/xtask/**` vs `script/**`). Từ 03 trở đi là chuỗi.

Phase 03 là phase duy nhất mang **rủi ro thực nghiệm**: nó không xong khi code viết đúng, mà xong khi 6 job thật sự xanh dưới trần 6h. Ước lượng của nó rộng nhất trong plan.

## Dependencies

- Không có plan nào block/bị block bởi plan này (`blockedBy: []`, `blocks: []`).
- **Tương tác mềm** với [260726-1531-remove-auth-cloud-hard-fork](../260726-1531-remove-auth-cloud-hard-fork/plan.md) (`status: pending`): nếu plan đó xoá thêm crate, danh sách asset và bước bundle cần kiểm lại — nhưng nó không chặn plan này, vì pipeline build **bất kỳ** cây crate đang có.
- Cần `contents: write` cho `secrets.GITHUB_TOKEN` (mặc định có). Không PAT, không secret bên ngoài. Ký số (`MACOS_CERTIFICATE`, `AZURE_*`) là **tuỳ chọn về sau** — thêm secret là script tự nhận, không phải sửa lại thiết kế.

## Sai lệch giữa plan và thứ đã build

Bốn thứ chỉ lộ ra khi implement, đều đã xử lý:

| # | Plan nói | Thực tế |
|---|---|---|
| 1 | `ZED_WORKSPACE` cần workflow set | **Không cần.** `script/lib/workspace.ps1` tự suy từ `cargo metadata`. Nhưng nó tìm package `"zed"` trong khi crate đã đổi tên `zode` → `RELEASE_VERSION` rỗng → `exit 1`. Blocker Windows thật, đã sửa. |
| 2 | Bỏ `/sDefaultsign` là đủ để không ký | **Không đủ.** `zed.iss:35` khai `SignTool=Defaultsign` khi `CI` khác rỗng; bỏ argument mà giữ directive là Inno abort. Đã đổi cả hai đầu sang cùng một tín hiệu `ZODE_CODE_SIGN`. |
| 3 | Chỉ nightly cần `ref: main` | Gate test/clippy của nightly cũng cần — nếu không nó test `develop` rồi bundle `main`. Đã thêm biến thể `*_on_ref` cho hai helper. |
| 4 | Không nói gì về Nix | Bỏ Nix khỏi nightly làm `nix_build.rs` thành dead code, mà CI dùng `-D warnings` → build đỏ. Đã xoá module + 2 step cache + 2 secret. Nix cho dev local (`flake.nix`) không đụng tới. |

Một chỗ **cố ý làm khác plan**: plan bảo nâng cả ba `timeout_minutes(60)` lên 360. `release_nightly.rs:71` vẫn giữ 60, vì job duy nhất dùng nó là `check_style` và nó đã thu về chỉ `cargo fmt --check` — không biên dịch gì, nên trần 6h chỉ khiến một job treo đốt 6h.

Một câu hỏi mở của plan đã tự trả lời: `cargo-bundle v0.6.1-zed` **không cần bước CI nào** — `bundle-mac:60-62` tự `cargo install` từ git khi version lệch.

## Định nghĩa xong

Push `v0.1.0` → trong 6h có draft release mang đủ 6 asset. Cron nightly cập nhật prerelease `nightly` mỗi ngày, không tích lũy release rác. Máy sạch mac/win/linux tải về, cài theo README, mở được app. `cargo xtask workflows` không sinh diff sau khi commit. Merge vào `main` không kích hoạt bundle nào.
