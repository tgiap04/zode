# Brainstorm — GitHub Actions release pipeline cho Zode

**Ngày:** 2026-08-12 · **Lens:** CTO · **Level:** medium · **Trạng thái:** thiết kế đã chốt, chưa triển khai

---

## Commission

Merge vào `main` → GitHub tự build IDE và phát hành bản public cho macOS, Linux, Windows. GitHub chịu phần chạy build.

**Đã chốt qua consultation:**

| Quyết định | Chọn |
|---|---|
| Trigger | Mọi merge vào `main` → build đủ 3 OS → publish pre-release |
| Hạ tầng build | Repo CI **public** + source **private** (checkout qua PAT) |
| Phát hành | Repo public thứ hai chỉ chứa Releases |
| Code signing | Không sign, ship kèm hướng dẫn mở app |
| Matrix | macOS arm64 + Linux x86_64 + Windows x86_64 |

---

## Workshop findings

### Đã có sẵn (không phải viết từ đầu)

- `.github/workflows/release.yml` (36 KB), `release_nightly.yml`, `run_bundling.yml` — machinery release 3 nền tảng từ upstream Zed.
- `script/bundle-mac`, `bundle-linux`, `bundle-windows.ps1` chạy được cho cả 4 channel.
- `crates/zed/Cargo.toml:213-243` — metadata bundle đã rename sang `io.github.tgiap04.zode*`, tên `Zode` / `Zode Dev` / `Zode Nightly` / `Zode Preview`.
- `crates/zed/contents/{dev,nightly,preview,stable}/embedded.provisionprofile` đủ 4 channel.
- Sentry upload **đã được guard** — tự skip khi thiếu token (`script/bundle-linux:118`, `script/bundle-windows.ps1:160`). Không cần sửa.

### Blocker thật

| # | Vấn đề | Vị trí |
|---|---|---|
| 1 | Không runner nào tồn tại trên account: mọi job build trỏ `namespace-profile-*` (Namespace.so, trả phí) + `self-32vcpu-windows-2022` (self-hosted) | `tooling/xtask/src/tasks/workflows/runners.rs:1-15` |
| 2 | YAML là file **generated** — sửa tay sẽ bị ghi đè | header mọi file `.github/workflows/*.yml` |
| 3 | `bundle-windows.ps1` `exit 1` trên CI khi thiếu `AZURE_*` | `script/bundle-windows.ps1:64-83` |
| 4 | `[profile.release]` = `codegen-units = 1` + `lto = "thin"` + `debug = "limited"` — tối ưu cho binary, tàn khốc cho build lạnh 4 core, và phình `target/` | `Cargo.toml:852-855` |
| 5 | Không có crate `auto_update` → không tự cập nhật trong app | — |
| 6 | Fork chưa từng chạy build nào trên GitHub — chỉ workflow bot dùng runner hosted | — |

### Số đo thực tế

- `target/debug` = 86 GB · `target/release` = 2.6 GB (build dở). Release đầy đủ 186 crate vượt trần cache 10 GB/repo.
- Máy local: M1 Pro, 10 core, 32 GB RAM, 595 GB trống — mạnh hơn runner macOS hosted (3 core, 7 GB) ~3x core, ~4.5x RAM.
- Chi phí private-repo hosted, ước 4h/nền tảng, build lạnh: Linux ~$1.92 + Windows ~$3.84 + **macOS ~$19.20** = **~$25/merge**. macOS chiếm 77%.

---

## Paths examined

| Path | Chi phí | Private? | Điều khoản | Kết |
|---|---|---|---|---|
| Hybrid: macOS self-hosted + Linux/Win hosted | ~$6/merge | ✅ | ✅ sạch | không chọn |
| Self-hosted cả ba | $0 phút + hardware | ✅ | ✅ sạch | không chọn |
| Namespace.so | trả theo dùng, ít sửa code nhất | ✅ | ✅ sạch | không chọn |
| Chuyển `zode` sang public | $0, miễn phí không giới hạn | ❌ | ✅ sạch | không chọn |
| **Repo CI public + source private** | **$0** | ✅ source | ⚠️ vùng xám | **CHỌN** |

---

## Agreed direction

```
tgiap04/zode  (PRIVATE — source)
  .github/workflows/dispatch_dist_build.yml     ← xtask sinh ra
  on: push → branches: [main]
  1 job ubuntu, ~5s: POST /repos/tgiap04/zode-dist/dispatches
     event_type: main-push · client_payload: { sha }
     token: DIST_DISPATCH_TOKEN (fine-grained, write Contents trên zode-dist)
        │
        ▼
tgiap04/zode-dist  (PUBLIC — CI + Releases + install docs)
  .github/workflows/build-release.yml           ← viết tay, xtask không sở hữu repo này
  on: repository_dispatch [main-push] + workflow_dispatch     ← KHÔNG pull_request
  concurrency: { group: build, cancel-in-progress: true }
  matrix (fail-fast: false):
     macos-14      → ./script/bundle-mac
     ubuntu-22.04  → ./script/bundle-linux
     windows-2022  → ./script/bundle-windows.ps1
  mỗi job: actions/checkout { repository: tgiap04/zode, ref: sha,
                              token: SOURCE_READ_PAT }
  job publish: gh release create --prerelease
     tag: nightly-0.1.0-<sha7>
```

Billing tính theo **repo chạy workflow**, không theo repo chứa code → `zode-dist` public ⇒ phút miễn phí không giới hạn trên runner tiêu chuẩn. Source vào filesystem tạm của runner rồi biến mất cùng runner.

### Thay đổi code cần làm

**Trong `zode` (private):**

1. **NEW** `tooling/xtask/src/tasks/workflows/dispatch_dist_build.rs`, đăng ký ở `tooling/xtask/src/tasks/workflows.rs` (`mod` ~dòng 27, `WorkflowFile::zed(...)` ~dòng 209), rồi `cargo xtask workflows`. **Viết tay YAML sẽ bị xóa lần regenerate sau.**
2. `script/bundle-windows.ps1:64-83` — giữ required set còn `ZED_WORKSPACE, RELEASE_VERSION, ZED_RELEASE_CHANNEL`; đẩy nhóm `AZURE_*` sang nhánh skip `SignZedAndItsFriends` (dòng 371) khi thiếu. Theo đúng pattern `script/bundle-mac:120` đã dùng.
3. Channel: workflow **ghi file lúc build** (`echo nightly > crates/zed/RELEASE_CHANNEL`), không commit thay đổi — giữ dev local ở `dev`. Chọn `nightly` vì icon, identifier `io.github.tgiap04.zode.nightly`, provisionprofile đều đã có → bản rolling cài song song, không đè bản stable tương lai.
4. Giảm rò log: `script/bundle-linux:3` và `bundle-mac` `set -euxo pipefail` → `set -euo pipefail`.

**Trong `zode-dist` (public, tạo mới):**

5. `README.md` — Zode là gì, bảng download, hướng dẫn mở app unsigned từng OS, trỏ NOTICE/license. README thật cũng cải thiện đáng kể vị thế điều khoản so với repo vỏ rỗng.
6. `.github/workflows/build-release.yml` theo sơ đồ trên.
7. Settings: chỉ hai trigger; **không bao giờ** `pull_request_target`; workflow permissions read-only, `contents: write` chỉ ở job publish.

### Đòn bẩy thời gian build (rủi ro kỹ thuật lớn nhất)

Trần cứng 6h/job · disk ~14 GB trên `macos-14` · runner 4 core (mac 3 core).

Override profile bằng env, không sửa `Cargo.toml`:

```
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
CARGO_PROFILE_RELEASE_LTO=false
CARGO_PROFILE_RELEASE_DEBUG=none      # thắng disk lớn nhất; an toàn vì Sentry đã tự skip
CARGO_INCREMENTAL=0                   # upstream cũng set trên job bundling
```

Đánh đổi: binary chậm/lớn hơn chút. Với pre-release nightly đó là đánh đổi đúng.

- Ubuntu: `rm -rf /usr/share/dotnet /opt/ghc /usr/local/lib/android` ≈ +25 GB.
- `macos-14` không có tương đương → **đây là chỗ tôi dự đoán vỡ đầu tiên.**
- Cache: chỉ cache `~/.cargo/registry` + `~/.cargo/git` (~2 GB, hit cao). **Không** cache `target/` cho 3 nền tảng — trần 10 GB/repo sẽ thrash.
- Nếu wall-clock không chịu được sau phase 1: `sccache` với backend R2/S3 (không giới hạn, ~$1/tháng). Đó là fix thật, để phase 3.

---

## Rủi ro người dùng đã biết và chấp nhận

Ghi lại nguyên trạng, đã nêu trong consultation và được chỉ đạo tiếp tục:

1. **GPL-3 §6** — 154/177 crate là `GPL-3.0-or-later` (gồm package `zode`); `NOTICE` cũng ghi nhận. Phát hành binary công khai mà giữ source private là vi phạm §6. Nêu hai lần, người dùng chỉ đạo bỏ qua và tiếp tục.
2. **Điều khoản GitHub Actions** — dùng repo public làm phương tiện compute cho source private. Rủi ro là treo account, không phải bị tính tiền.
3. **Log build công khai rò mảnh source** qua diagnostics compiler. Bỏ `-x` giảm được phần echo lệnh; phần diagnostics **không xóa được**.
4. **Binary không sign** — Gatekeeper chặn trên macOS, SmartScreen cảnh báo trên Windows.
5. **Hai PAT fine-grained hết hạn 90 ngày** — pipeline chết im lặng khi lapse, không có cảnh báo.

## Rủi ro cần giải, chưa giải

- Trần 6h/job + disk 14 GB macOS → điểm vỡ đầu tiên dự kiến.
- Build lạnh mỗi merge; `cancel-in-progress` giữ queue không phình.
- **Nền glibc dâng lên**: upstream cố ý dùng Ubuntu 20.04 cho glibc tối thiểu (`runners.rs:7-8`); GitHub-hosted cũ nhất là 22.04 → binary không chạy trên glibc < 2.35.

---

## Success criteria

- Merge vào `main` → trong 6h, `zode-dist` có release mang đủ 3 artifact.
- Máy sạch mac/win/linux tải về, cài theo README, mở được Zode Nightly.
- Không secret nào trong log công khai; kiểm bằng `gh api` rằng không workflow nào chạy từ fork PR.
- Merge liên tiếp không xếp chồng build.

## Next steps

| Phase | Nội dung |
|---|---|
| 1 | Dispatch workflow phía private qua xtask + patch signing `bundle-windows.ps1` |
| 2 | Tạo `zode-dist`, README, `build-release.yml`, hai PAT |
| 3 | Tuning profile/disk/cache đến khi 3 nền tảng xanh dưới 6h |
| 4 | Kiểm hướng dẫn cài trên máy sạch từng OS |

## Chưa giải quyết

- Rotation của hai PAT: chưa có cơ chế cảnh báo trước khi hết hạn.
- Chưa quyết dọn release cũ: mỗi merge một pre-release sẽ tích lũy vô hạn — cần policy giữ N bản gần nhất.
