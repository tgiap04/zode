# Phase 03 — Ép build vừa 14 GB disk / 7 GB RAM

**Priority:** P1 · **Status:** ⬜ pending · **Blocked by:** 01, 02
**Context:** [plan.md](plan.md) · [brainstorm 260819](./reports/brainstorm-260819-release-pipeline-public-repo.md)

## Mục tiêu

Sáu job bundle **thật sự chạy xong** trên runner GitHub-hosted free, dưới trần 6h, và upload đủ 6 asset.

Đây là phase duy nhất trong plan **không xong khi code viết đúng**. Nó xong khi 6 job xanh trên GitHub. Ước lượng rộng nhất trong plan nằm ở đây.

## Key insight — ba con số quyết định, hai đã chắc

**1. `timeout_minutes(60)` là thứ sẽ giết mọi lần chạy đầu tiên.** Ba chỗ đặt nó:
`run_bundling.rs:73` (mọi job bundle) · `steps.rs:348` (`release_job`) · `release_nightly.rs:85`.
Trên máy 32 core của Zed, 60 phút là dư. Trên 4 core (mac 3 core) với ~186 crate build lạnh, job sẽ bị cắt đúng phút 60 và **trông y như build treo**. Nâng lên `360` (trần cứng GitHub) là việc đầu tiên phải làm, trước cả khi tune profile.

**2. Profile release nguyên bản không vừa disk.** `Cargo.toml:876-879` giữ `debug = "limited"` + `codegen-units = 1` + `lto = "thin"`. `target/debug` đo được 86 GB trên máy local; runner có 14 GB. Ghi đè bằng **env**, không sửa `Cargo.toml` — vì tên profile vẫn phải là `release` để đường dẫn trong `script/bundle-*` không đổi.

**3. `bundle_envs()` ở `vars.rs:60-85` là điểm duy nhất cần chạm** để cả 6 job nhận cùng một tập env. Không đi thêm env vào từng job.

## Related code files

**Modify:**
- `tooling/xtask/src/tasks/workflows/vars.rs:60-85` — `bundle_envs()`: thêm 4 biến `CARGO_PROFILE_RELEASE_*`, thêm `ZED_WORKSPACE` cho Windows
- `tooling/xtask/src/tasks/workflows/vars.rs:378-410` — `assets`: đổi sang `Zode-*`/`zode-*`, bỏ 6 hằng `REMOTE_SERVER_*` khỏi `all()`
- `tooling/xtask/src/tasks/workflows/run_bundling.rs:73` — `timeout_minutes(60)` → `360`
- `tooling/xtask/src/tasks/workflows/run_bundling.rs:103,109-111,155,159-161,195,198-200` — bỏ `setup_sentry()` và bước upload remote_server
- `tooling/xtask/src/tasks/workflows/steps.rs:348` — `timeout_minutes(60)` → `360`
- `tooling/xtask/src/tasks/workflows/steps.rs` — **thêm** `free_disk_space(platform)`
- `tooling/xtask/src/tasks/workflows/release_nightly.rs:85` — `timeout_minutes(60)` → `360`

## Implementation steps

1. **Nâng cả ba `timeout_minutes(60)` → `360`.** Làm trước tiên. Không tune gì khác trước bước này, vì mọi kết luận "build không kịp" thu được trước đó đều là kết luận về cái timeout, không phải về build.

2. **`vars.rs:60-85` — thêm env override vào `bundle_envs()`**, phần dùng chung cho cả ba platform:

   ```rust
   let env = Env::default()
       .add("CARGO_INCREMENTAL", 0)
       .add("CARGO_PROFILE_RELEASE_DEBUG", "none")        // thắng disk lớn nhất
       .add("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", 16)    // giảm RAM đỉnh + thời gian
       .add("CARGO_PROFILE_RELEASE_LTO", "false")         // giảm RAM đỉnh lúc link
       .add("ZED_CLIENT_CHECKSUM_SEED", ZED_CLIENT_CHECKSUM_SEED)
       .add("ZED_MINIDUMP_ENDPOINT", ZED_SENTRY_MINIDUMP_ENDPOINT);
   ```

   Hai biến `ZED_*` giữ lại: secret rỗng trên account này, script tự bỏ qua. Xoá chúng là diff thừa.

   Nhánh `Platform::Windows` thêm `.add("ZED_WORKSPACE", "${{ github.workspace }}")` — `bundle-windows.ps1` bắt buộc có biến này, và trên runner GitHub-hosted không ai set nó.

3. **`vars.rs:378-410` — sửa tên asset và thu `all()`.**
   - `MAC_AARCH64` → `"Zode-aarch64.dmg"`, `MAC_X86_64` → `"Zode-x86_64.dmg"`
   - `LINUX_*` → `"zode-linux-{arch}.tar.gz"`
   - `WINDOWS_*` → `"Zode-{arch}.exe"` *(khớp cái `bundle-windows.ps1:269` vốn đã sinh ra)*
   - Bỏ 6 hằng `REMOTE_SERVER_*` khỏi `all()`. Xoá luôn hằng nếu không còn ai tham chiếu — `grep` trước.

   `all()` là nguồn duy nhất cho `validate_release_assets` (`release.rs:312`) và `prep_release_artifacts` (`release.rs:384`). Sửa một chỗ, ba chỗ đúng theo.

4. **`run_bundling.rs` — bỏ bước upload remote_server** ở cả ba builder (`:109-111` mac, `:159-161` linux, `:198-200` windows), và bỏ biến `remote_server_artifact_name` (`:89-92`, `:140-143`, `:182-185`).

5. **`run_bundling.rs` — bỏ `steps::setup_sentry()`** (`:103`, `:155`, `:195`). Không có token Sentry, và script bundle vốn đã tự bỏ qua phần upload → bước này là thời gian chết.

6. **`steps.rs` — thêm `free_disk_space(platform)`** và gọi nó ngay sau `checkout_repo()` trong cả ba builder:

   ```rust
   pub fn free_disk_space(platform: Platform) -> Step<Run> {
       match platform {
           Platform::Linux => named::bash(indoc!{r#"
               sudo rm -rf /usr/share/dotnet /opt/ghc /usr/local/lib/android /opt/hostedtoolcache/CodeQL
               sudo docker image prune --all --force || true
               df -h /
           "#}),
           Platform::Mac => named::bash(indoc!{r#"
               sudo rm -rf /Applications/Xcode_15*.app || true
               df -h /
           "#}),
           Platform::Windows => named::pwsh(indoc!{r#"
               Remove-Item -Recurse -Force "C:\Android" -ErrorAction SilentlyContinue
               Get-PSDrive -PSProvider FileSystem | Format-Table
           "#}),
       }
   }
   ```

   Ubuntu thu được ≈ **+25 GB**. macOS không có tương đương gọn — **đây là chỗ dự kiến vỡ đầu tiên.**

   `df -h` / `Get-PSDrive` ở cuối mỗi nhánh là **có ý**: nó đặt số dung lượng thật vào log, để lần tune sau dựa trên số đo thay vì phỏng đoán.

7. **`cargo xtask workflows`**, commit, push, và **chạy thật**. Dùng `workflow_dispatch` hoặc label `run-bundling` trên một PR nháp để không phải tạo tag rác.

8. **Vòng lặp tune** — theo thứ tự này, mỗi lần một biến, đọc `df -h` trong log trước khi đổi:
   1. Job nào chết? Vì disk, RAM (OOM killer / `signal: 9`), hay hết giờ?
   2. Hết disk → thêm mục dọn; hoặc `--no-default-features` nếu có feature nặng cắt được.
   3. OOM (gần chắc là macOS arm 7 GB) → hạ song song link: `CARGO_BUILD_JOBS=2`, rồi `1`.
   4. Hết giờ trên một platform → cân nhắc `sccache` với backend R2/S3 riêng (~$1/tháng). Đây là **fix thật**, không phải hack — nhưng chỉ dùng khi đã hết cách miễn phí.

## Todo

- [ ] `run_bundling.rs:73`, `steps.rs:348`, `release_nightly.rs:85`: `60` → `360`
- [ ] `vars.rs:60-85`: thêm 3 biến `CARGO_PROFILE_RELEASE_*`
- [ ] `vars.rs`: nhánh Windows thêm `ZED_WORKSPACE = ${{ github.workspace }}`
- [ ] `vars.rs:380-385`: đổi tên asset sang `Zode-*` / `zode-*`
- [ ] `vars.rs:394-409`: `all()` chỉ còn 6 asset
- [ ] `run_bundling.rs`: bỏ upload remote_server + biến tên của nó (3 chỗ)
- [ ] `run_bundling.rs`: bỏ `setup_sentry()` (3 chỗ)
- [ ] `steps.rs`: thêm `free_disk_space(platform)` + gọi trong 3 builder
- [ ] `cargo build -p xtask` xanh; `cargo xtask workflows`; commit
- [ ] Chạy thật; ghi lại **thời gian và `df -h`** của từng job vào một bảng trong phase file này
- [ ] Lặp tới khi cả 6 job xanh dưới 360 phút

## Bảng đo

### Đã đo — máy local, KHÔNG phải runner CI

Chạy `./script/bundle-mac aarch64-apple-darwin` với đúng bốn env override của CI, trên M1 Pro (10 core, 32 GB):

| Đo | Giá trị |
|---|---|
| Kết quả | **thành công**, exit 0 |
| Hiện vật | `target/aarch64-apple-darwin/release/Zode-aarch64.dmg`, 109 MB |
| Nội dung `.app/Contents/MacOS/` | `zode`, `cli`, `git`, 3 driver `zode-db-*` — **không có** `remote_server` |
| Ký số | `Signature=adhoc`, `TeamIdentifier=not set` → đường degrade khi thiếu cert hoạt động |
| Binary khởi động | có (clap parse được argv) |
| **`target/<triple>/release` sau build** | **5,2 GB** (quỹ đạo: 1,9 → 4,0 → 5,2 GB) |

**5,2 GB là con số quan trọng nhất ở đây**: nó vừa thoải mái trong 14 GB, nên rủi ro *disk* trên macOS thấp hơn dự đoán của brainstorm. So sánh: `target/debug` đo được 86 GB.

Điều này **chưa** nói gì về hai rủi ro còn lại trên runner CI: **RAM 7 GB** (máy local có 32 GB) và **thời gian trên 3 core** (máy local có 10 core). Hai cái đó chỉ CI trả lời được.

### Đã đo trên CI — run 32296843005, `conclusion: success`

| Job | Runner | Kết quả | Thời gian | Artifact |
|---|---|---|---|---|
| bundle_linux_aarch64 | `ubuntu-24.04-arm` | ✅ | 33 phút | `zode-linux-aarch64.tar.gz` 113 MB |
| bundle_linux_x86_64 | `ubuntu-24.04` | ✅ | 42 phút | `zode-linux-x86_64.tar.gz` 115 MB |
| bundle_mac_aarch64 | `macos-15` | ✅ | 73 phút | `Zode-aarch64.dmg` 108 MB |
| bundle_mac_x86_64 | `macos-15-intel` | ✅ | 99 phút | `Zode-x86_64.dmg` 116 MB |
| bundle_windows_x86_64 | `windows-2025` | ✅ | ~78 phút | `Zode-x86_64.exe` 56 MB |
| bundle_windows_aarch64 | `windows-2025` (cross) | ✅ | ~78 phút | `Zode-aarch64.exe` 49 MB |

**Hai rủi ro lớn nhất của brainstorm đã bị loại bằng bằng chứng:**

- **macOS arm 7 GB RAM không OOM.** Plan ghi "xác suất cao" và dự phòng "nếu vẫn OOM thì phải mở lại quyết định chỉ dùng runner free". Không cần mở lại.
- **Thời gian 33–99 phút**, xa trần 360. Nhưng cũng cho thấy `timeout_minutes(60)` gốc sẽ giết **cả sáu** job — kể cả job nhanh nhất chỉ dưới ngưỡng 18 phút.

Runner khác plan: Linux dùng 24.04 (không phải 22.04) và Windows arm cross-compile trên x64 (không phải `windows-11-arm`) — lý do ở mục dưới.

### Windows: sáu lượt mới xanh

Không lượt nào đoán trước được từ local (không có `pwsh`, không có Windows), và mỗi lượt sau bước signing tốn ~77 phút:

| Lượt | Chết ở | Sau | Nguồn |
|---|---|---|---|
| 1 | `...\2022\Community\...Launch-VsDevShell.ps1` không tồn tại | 16s | hạ tầng Zed — runner GitHub có Enterprise |
| 2 | `-HostArch arm64` không thuộc `x86,amd64` | 28s | **lỗi khi triển khai** — cho chạy native thay vì cross-compile |
| 3 | `path too long` (261 ký tự) khi checkout git dep `pet` | 5,5 phút | hạ tầng Zed — dev drive của họ ngắn hơn 17 ký tự |
| 4 | `sign.ps1: The 'ENDPOINT' env is required` | 77 phút | **lỗi khi triển khai** — `Test-Path env:X` đúng với biến rỗng |
| 5 | `No files found matching "...\tools\*"` | 77 phút | có sẵn từ lần xoá auto-update |
| 6 | — | ✅ | — |

Lượt 4 là bài học đáng ghi: tôi khẳng định làm theo `bundle-mac:128` nhưng nó dùng `-n "${MACOS_CERTIFICATE:-}"` — kiểm **giá trị**, không kiểm sự tồn tại. Workflow map một secret không tồn tại vẫn **định nghĩa** biến, với giá trị rỗng.

Lượt 5 phát hiện thêm hai lỗi có sẵn: `zed.iss` đòi `tools\*` (từng chứa `auto_update_helper.exe`) nên **installer Windows của fork không build được kể từ lần xoá auto-update**; và `zed.iss` **không có entry nào cho 3 driver `zode-db-*.exe`**, nên bản Windows sẽ ship thiếu chúng trong khi mac/linux có.

### Câu hỏi mở của plan — đã trả lời

- **Inno Setup có sẵn** trên `windows-2025` tại `C:\Program Files (x86)\Inno Setup 6\ISCC.exe`.
- **`cargo-bundle v0.6.1-zed` không cần bước CI** — `bundle-mac:60-62` tự `cargo install` từ git khi version lệch.
- **`ZED_WORKSPACE` không cần workflow set** — `ParseZedWorkspace` tự suy từ `cargo metadata`.

## Success criteria

- Cả 6 job kết thúc **success**, mỗi job dưới 360 phút.
- Sáu artifact xuất hiện trong tab Artifacts của run, tên khớp đúng `assets::all()`.
- Bảng đo ở trên được điền — số thật, không phải ước lượng.
- Tải `Zode-aarch64.dmg` từ artifact, mở được app trên máy local (bỏ qua Gatekeeper).

## Risk assessment

| Rủi ro | Xác suất | Đối phó |
|---|---|---|
| **macOS arm 7 GB RAM OOM lúc link** | cao | `CARGO_BUILD_JOBS=2` → `1`. Nếu vẫn OOM: đây là lúc phải quay lại nói chuyện về larger runner cho riêng mac, tức mở lại quyết định #1. |
| Inno Setup không có trên image `windows-2025` | chưa rõ | Thêm bước `choco install innosetup`. Chưa xác minh được từ ngoài — phải chạy mới biết. |
| `cargo-bundle v0.6.1-zed` (fork riêng) không cài được trong CI | chưa rõ | `bundle-mac:59` kiểm đúng chuỗi version này. Tìm nguồn cài trong `script/bootstrap`; nếu không có, thêm bước `cargo install --git`. |
| Windows disk 14 GB chật nhất trong 6 target | cao | `Get-PSDrive` trong log sẽ nói. Cân nhắc `CARGO_TARGET_DIR` sang ổ `D:` nếu ổ đó rộng hơn. |
| 6 job × 4 lần thử = 24 lượt build 2h. Vòng lặp debug rất dài. | chắc chắn | Rủi ro **đã được nêu và người dùng chọn nhận** (bật cả 6 target ngay thay vì theo pha). Giảm đau: chạy `workflow_dispatch` với một target trước khi bật cả 6, dù plan cho phép cả 6. |
| Tắt LTO làm binary chậm hơn ~10-20% | chắc chắn | Đã chấp nhận (quyết định về profile). Không dùng số này để mở lại đường 2. |

## Security considerations

- `CARGO_PROFILE_RELEASE_DEBUG=none` bỏ debug symbol → crash report không symbolize được. Không có Sentry nên không mất gì; nhưng ghi vào README để người báo bug biết backtrace sẽ trống.
- Bước dọn disk chạy `sudo rm -rf` với đường dẫn **cố định, viết thẳng**. Không bao giờ dựng đường dẫn xoá từ biến hay input.

## Next steps

→ [Phase 04](phase-04-nightly-and-tag-release.md)
