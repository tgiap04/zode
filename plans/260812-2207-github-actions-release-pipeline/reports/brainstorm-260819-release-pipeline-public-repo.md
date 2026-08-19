# Brainstorm — Release & build pipeline cho Zode (repo public)

**Ngày:** 2026-08-19 · **Lens:** CTO · **Level:** medium · **Trạng thái:** thiết kế đã chốt, chưa triển khai

**Thay thế:** `brainstorm-260812-github-actions-release-pipeline.md` — xem mục "Vì sao bản 260812 hết hiệu lực".

---

## Commission

1. Có bản build installer cho macOS / Windows / Linux mà không phải dựng tay.
2. Push release tag → các file installer tự được nối vào release tag đó.

---

## Vì sao bản 260812 hết hiệu lực

Bản 260812 dựng toàn bộ kiến trúc trên tiền đề **`tgiap04/zode` là private**: repo thứ hai `zode-dist` public làm phương tiện compute, hai PAT fine-grained, checkout chéo repo, chấp nhận vi phạm GPL-3 §6 và vùng xám điều khoản Actions.

Kiểm hôm nay: `gh repo view tgiap04/zode --json visibility` → **`PUBLIC`**.

Tiền đề đổi ⇒ toàn bộ lớp phức tạp đó tan biến, và tan biến theo hướng tốt:

| Bản 260812 | Hôm nay |
|---|---|
| Repo `zode-dist` thứ hai | không cần |
| Hai PAT fine-grained, hết hạn 90 ngày | không cần, dùng `secrets.GITHUB_TOKEN` |
| Vi phạm GPL-3 §6 (binary public + source private) | **hết** — source đã public |
| Vùng xám điều khoản Actions | **hết** — build đúng repo của mình |
| Log công khai rò mảnh source | **hết** — source vốn đã công khai |
| Phút runner | vẫn $0 (public repo, runner standard) |

Năm trong sáu rủi ro "đã biết và chấp nhận" của bản cũ không còn tồn tại. Đây là kết quả của việc repo chuyển public, không phải của thiết kế mới.

---

## Quyết định đã chốt

| # | Quyết định | Chọn | Ghi chú |
|---|---|---|---|
| 1 | Runner | GitHub-hosted **free** | public repo ⇒ phút không giới hạn trên runner standard |
| 2 | Build khi merge `main` | **Không build** ở merge; cron nightly build đủ 6 target | merge chỉ chạy test/clippy |
| 3 | Nguồn file lúc tag | **Build lại từ commit của tag** | không tái dùng artifact ⇒ không có bẫy SHA lệch |
| 4 | Ma trận | **đủ 6 target** như Zed | mac arm+x64, linux arm+x64, win arm+x64 |
| 5 | Quan hệ upstream | **đi đường riêng**, không rebase từ `zed-industries/zed` | được phép sửa thẳng file upstream |
| 6 | Version/tag | **bỏ `determine-release-channel`**, tag tự do `v0.1.0` / `v0.1.0-beta.1` | prerelease suy ra từ hậu tố tag |
| 7 | `remote_server` | **bỏ** | đòn giảm chi phí lớn nhất; mất tính năng SSH remote dev |

---

## Số đo đã xác minh

### Runner free cho public repo

| Label | vCPU | RAM | Disk (doc) |
|---|---|---|---|
| `ubuntu-22.04` / `ubuntu-24.04` | 4 | 16 GB | 14 GB (thực trống ~22 GB) |
| `ubuntu-24.04-arm` | 4 | 16 GB | 14 GB (thực trống ~45 GB) |
| `windows-2025` | 4 | 16 GB | 14 GB |
| `windows-11-arm` | 4 | 16 GB | 14 GB |
| `macos-15` (arm64) | **3** | **7 GB** | 14 GB |
| `macos-15-intel` | 4 | 14 GB | 14 GB |

Nghịch lý cần nhớ: **mac x86_64 có gấp đôi RAM của mac arm64.** Nền tảng chủ lực (arm64) là nền tảng chật nhất. Trần cứng: **6h/job**.

### Áp lực từ profile build

`Cargo.toml:876-879` (bản 260812 ghi 852-855 — file đã dịch dòng):

```toml
[profile.release]
debug = "limited"     # sinh debug info → target/ phình hàng chục GB
lto = "thin"
codegen-units = 1     # codegen tuần tự, RAM cao mỗi crate
```

Đo thực tế trên máy local: `target/debug` = 86 GB. Upstream phải có `script/clear-target-dir-if-larger-than 350 200` chính vì lý do này. **Profile nguyên bản không vừa 14 GB disk / 7 GB RAM.** Đây là dự đoán vỡ đầu tiên, và nó là dự đoán về OOM/hết-disk, không phải về chậm.

### Cache

Trần cache GitHub Actions = 10 GB/repo. `target/` release lớn hơn nhiều lần ⇒ **không cache được compile**. Chỉ cache `~/.cargo/registry` + `~/.cargo/git` (~2 GB, tiết kiệm ~5–10 phút fetch). Mỗi lần build là build lạnh — đó là giá của runner free, không phải thứ tune được.

---

## Blocker thật (đều là fail cứng, không phải cảnh báo)

| # | Vấn đề | Vị trí |
|---|---|---|
| 1 | Runner không tồn tại trên account: mọi job build trỏ `namespace-profile-*` (Namespace.so, trả phí) + `self-32vcpu-windows-2022` (self-hosted). Job sẽ **treo vô hạn trong queue**, không fail. | `tooling/xtask/src/tasks/workflows/runners.rs:1-15` |
| 2 | 34 workflow có `if: github.repository_owner == 'zed-industries'` → job skip, mọi job `needs` nó skip theo | `.github/workflows/*.yml` |
| 3 | YAML là file **generated**; `run_tests.yml:705` có gate CI fail nếu YAML lệch với `cargo xtask workflows`. Sửa YAML trực tiếp sẽ vỡ ở lần chạy sau. | header mọi `.github/workflows/*.yml` |
| 4 | `determine-release-channel` **exit 1** với channel `dev` (giá trị hiện tại của `crates/zed/RELEASE_CHANNEL`); chỉ nhận `stable`/`preview`, và bắt tag khớp chính xác `v{version trong Cargo.toml}` | `script/determine-release-channel` |
| 5 | `bundle-windows.ps1` **exit 1** dưới `$env:CI` khi thiếu bất kỳ biến nào trong nhóm `AZURE_*` / `ACCOUNT_NAME` / `CERT_PROFILE_NAME` / `ENDPOINT` / `FILE_DIGEST` / `TIMESTAMP_*`. Ngoài ra `sign.ps1` được gọi vô điều kiện khi `$env:CI` (dòng 356). | `script/bundle-windows.ps1:68-86`, `:221`, `:356` |
| 6 | `--repo=zed-industries/zed` hardcode ở upload / validate / notify | `release.yml:640,652`, và Rust sinh ra chúng |
| 7 | Job `compliance_check`, `auto_release_preview`, Slack notify cần GitHub App `ZED_ZIPPY_*` + webhook của Zed | `release.yml:647-760` |
| 8 | `validate_release_assets` kỳ vọng đúng 12 tên asset `Zed-*` / `zed-remote-server-*` | `release.yml:652` |
| 9 | `bundle-linux` build **cả** target thường **và** target musl riêng cho `remote_server` → gần gấp đôi thời gian và disk | `script/bundle-linux:91,98` |
| 10 | Không có crate `auto_update` → app **không tự cập nhật**, người dùng phải tải lại thủ công | — |

### Đã sẵn sàng, không phải làm

- `crates/zed/Cargo.toml:213-246` — bundle metadata đã rename `io.github.tgiap04.zode*`, tên `Zode` / `Zode Dev` / `Zode Nightly` / `Zode Preview`.
- `crates/zed/contents/{dev,nightly,preview,stable}/embedded.provisionprofile` đủ 4 channel.
- `script/bundle-mac:128` — ký số **tự degrade**: thiếu secret thì ký ad-hoc `--sign -`. Không cần sửa. (Trái với Windows ở blocker #5.)
- Sentry upload trong script đã guard, tự skip khi thiếu token.

---

## Paths examined

| Path | Profile ở nightly | Profile ở tag | Nhận định |
|---|---|---|---|
| **1 — Tối giản, một profile** | `debug=none, cu=16, lto=false` | **giống nightly** | **CHỌN** |
| 2 — Hai profile | như trên | `lto=thin, cu=1` đầy đủ | loại |
| 3 — Từng nền tảng một | như đường 1 | như đường 1 | không phải đối thủ — là thứ tự thi công |

**Vì sao loại đường 2:** đường tag là đường *ít được chạy nhất* lại có cấu hình *khác nhất* so với nightly ⇒ lỗi chỉ lộ đúng lúc release. Cộng thêm `lto=thin` + `codegen-units=1` khi link trên máy 7 GB RAM là rủi ro OOM thật, buộc phải mua larger runner riêng cho mac — tức phá quyết định #1.

**Về đường 3:** đã trình rằng đây là thứ tự thi công đúng (linux x86_64 → macOS arm → Windows x64 → 3 arch còn lại), vì debug CI cần vòng lặp ngắn và mỗi lần đoán sai phải chờ ~2h để biết. **Người dùng đã cân nhắc và chọn bật cả 6 target ngay.** Quyết định của người dùng, ghi nhận và thực hiện; rủi ro vòng-lặp-debug ghi ở mục Rủi ro.

---

## Agreed direction

```
tgiap04/zode  (PUBLIC — một repo duy nhất, không repo phụ)

  develop ──PR──> main
                   │
                   ├─ workflow test/clippy  (không bundle)
                   │
                   ├─ cron 1 lần/ngày trên main
                   │    6 job song song, 1 target/job, runner native:
                   │      macos-15         → bundle-mac aarch64-apple-darwin
                   │      macos-15-intel   → bundle-mac x86_64-apple-darwin
                   │      ubuntu-22.04     → bundle-linux
                   │      ubuntu-24.04-arm → bundle-linux
                   │      windows-2025     → bundle-windows.ps1 -Architecture x86_64
                   │      windows-11-arm   → bundle-windows.ps1 -Architecture aarch64
                   │    → 1 prerelease CUỘN tên `nightly`, xóa asset cũ mỗi lần
                   │
                   └─ push tag v*
                        cùng 6 job trên, build LẠI từ commit của tag
                        → gh release create --draft → attach 6 asset
                        → người dùng tự bấm publish
```

Mọi job dùng `secrets.GITHUB_TOKEN`, không PAT. Billing tính theo repo chạy workflow — public ⇒ $0 trên runner standard.

### Đòn bẩy khả thi (không sửa `Cargo.toml`)

Ghi đè profile bằng env trong workflow, giữ tên profile là `release` để đường dẫn trong `script/bundle-*` không phải sửa:

```
CARGO_PROFILE_RELEASE_DEBUG=none          # thắng disk lớn nhất; an toàn vì không dùng Sentry
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16    # giảm RAM đỉnh và thời gian
CARGO_PROFILE_RELEASE_LTO=false           # giảm RAM đỉnh lúc link
CARGO_INCREMENTAL=0                       # upstream cũng set trên job bundling
```

Đánh đổi: binary chậm hơn ~10–20% và lớn hơn, mất symbol cho crash report. Với fork không dùng Sentry, đây là đánh đổi đúng.

Dọn disk trước khi build:
- Ubuntu: `rm -rf /usr/share/dotnet /opt/ghc /usr/local/lib/android` ≈ **+25 GB**.
- macOS: xóa Xcode phiên bản không dùng ≈ +10–20 GB. **Không có tương đương gọn như Ubuntu.**
- Windows: cần khảo sát dung lượng trống thật trên `C:` và `D:` của image `windows-2025`.

### Danh sách thay đổi cụ thể

**Rust (nguồn thật của workflow):**

1. `tooling/xtask/src/tasks/workflows/runners.rs` — đổi toàn bộ constant sang runner GitHub-hosted; thêm constant cho `macos-15-intel`, `ubuntu-24.04-arm`, `windows-11-arm`.
2. `tooling/xtask/src/tasks/workflows/release.rs` — bỏ cổng `repository_owner`; bỏ job `compliance_check` / `auto_release_preview` / Slack notify; đổi `--repo` sang `${{ github.repository }}`; bỏ 6 asset `remote_server` khỏi `validate_release_assets`; đổi tên asset kỳ vọng nếu bundle output đổi sang `Zode-*`; thêm env override profile + bước dọn disk; bỏ bước `setup_sccache` và `setup_sentry`.
3. `tooling/xtask/src/tasks/workflows/release_nightly.rs` — đổi trigger sang cron trên `main`; đích là 1 prerelease cuộn `nightly` trong cùng repo (thay vì blob store của Zed); cùng tập override như trên.
4. Chạy `cargo xtask workflows` và commit YAML sinh ra — nếu không, gate `run_tests.yml:705` sẽ đỏ.

**Script:**

5. `script/determine-release-channel` — bỏ khỏi đường release, hoặc viết lại để suy channel từ hậu tố tag và không bắt khớp version Cargo.
6. `script/bundle-windows.ps1:68-86` — giữ required set còn `ZED_WORKSPACE, RELEASE_VERSION, ZED_RELEASE_CHANNEL`; đẩy nhóm `AZURE_*` sang nhánh skip; guard `sign.ps1` ở `:221` và `:356` theo đúng pattern `bundle-mac:128`.
7. `script/bundle-linux` + `bundle-mac` + `bundle-windows.ps1` — bỏ đường build `remote_server`.
8. `script/bundle-linux:3` và `bundle-mac` — `set -euxo pipefail` → `set -euo pipefail` (giảm ồn log; không còn là vấn đề bảo mật vì repo đã public).

**Tài liệu:**

9. `README.md` — bảng download, hướng dẫn mở app unsigned từng OS, và **nói rõ app không tự cập nhật**.

---

## Rủi ro

### Người dùng đã biết và chấp nhận

1. **Bật cả 6 target ngay lần đầu.** Đã trình rằng thi công theo pha (đường 3) cho vòng lặp debug ngắn hơn; người dùng chọn bật hết. Hệ quả: mỗi lần đoán sai phải chờ ~2h, và 6 biến số cùng lúc.
2. **Binary không sign** — Gatekeeper chặn trên macOS, SmartScreen cảnh báo trên Windows. Có thể thêm secret sau mà không đổi thiết kế (`bundle-mac` đã tự nhận).
3. **Không LTO** → binary chậm hơn ~10–20%.
4. **Không tự cập nhật** (không có crate `auto_update`) → người dùng phải tải lại thủ công mỗi bản.
5. **Bỏ `remote_server`** → mất tính năng SSH remote development.

### Chưa giải, phải thử mới biết

- **macOS arm 7 GB RAM / 14 GB disk** — điểm vỡ đầu tiên dự kiến, kể cả sau khi override profile.
- **Trần 6h/job** trên 4 core (mac 3 core), ~186 crate, build lạnh. Ước 60–90 phút/target sau override, nhưng chưa đo.
- **Inno Setup** có sẵn trên image `windows-2025` hay phải tự cài — chưa xác minh.
- **`cargo-bundle v0.6.1-zed`** (fork riêng; `bundle-mac:59` kiểm tra đúng chuỗi version này) cài từ đâu trong CI — chưa xác minh.
- **Nền glibc dâng lên**: upstream cố ý dùng Ubuntu 20.04 cho glibc tối thiểu (`runners.rs:7-8`); GitHub-hosted cũ nhất là 22.04 (glibc 2.35) ⇒ binary không chạy trên glibc < 2.35. Nếu cần hạ nền, phải build trong container — repo đã có `Dockerfile-distros`.
- **`bundle-linux` build native theo `uname -m`** ⇒ linux arm buộc phải chạy trên `ubuntu-24.04-arm`, không cross được. Runner arm chỉ có trên public repo — nếu sau này repo về private, target này chết.

---

## Success criteria

- Push tag `v0.1.0` → trong 6h có draft release mang đủ 6 asset (2 mac, 2 linux, 2 windows).
- Cron nightly chạy 1 lần/ngày, cập nhật prerelease `nightly` với 6 asset mới, không tích lũy release rác.
- Máy sạch mac/win/linux tải về, cài theo README, mở được app.
- `cargo xtask workflows` không sinh diff sau khi commit (gate `run_tests.yml:705` xanh).
- Merge vào `main` **không** kích hoạt build bundle nào.

## Next steps

| Phase | Nội dung |
|---|---|
| 1 | Patch `runners.rs` + bỏ cổng owner + dọn job Zed-only trong `release.rs`/`release_nightly.rs`; `cargo xtask workflows` |
| 2 | Patch script: signing Windows, bỏ `remote_server`, bỏ/thay `determine-release-channel` |
| 3 | Thêm bước dọn disk + env override profile; chạy thật đến khi 6 target xanh dưới 6h |
| 4 | Nightly cuộn + draft release ở tag; README hướng dẫn cài |
| 5 | Kiểm cài trên máy sạch từng OS |

## Chưa quyết

- Đổi tên asset `Zed-*.dmg` → `Zode-*.dmg` hay giữ nguyên (ảnh hưởng `validate_release_assets` và README).
- Policy dọn: prerelease `nightly` cuộn thì không tích lũy, nhưng chưa quyết có giữ bản nightly theo ngày nào không.
- Có bổ sung `auto_update` về sau không — quyết định riêng, ngoài phạm vi consultation này.
