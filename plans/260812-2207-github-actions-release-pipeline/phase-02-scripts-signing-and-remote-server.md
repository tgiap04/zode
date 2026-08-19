# Phase 02 — Script: signing Windows, bỏ `remote_server`, thay channel

**Priority:** P1 · **Status:** ⬜ pending · **Blocked by:** — · **Chạy song song với:** [Phase 01](phase-01-runners-and-owner-gate.md)
**Context:** [plan.md](plan.md) · [brainstorm 260819](./reports/brainstorm-260819-release-pipeline-public-repo.md)

## Mục tiêu

Ba script bundle chạy được trên CI **không có secret ký số nào**, không build `remote_server`, và đường release không còn bị `determine-release-channel` chặn.

File ownership rời hẳn Phase 01 (`script/**` vs `tooling/xtask/**`) → hai phase chạy song song an toàn.

## Key insight

**Ba script xử lý thiếu-secret theo ba cách khác nhau.** Đây là cái bẫy:

| Script | Thiếu secret ký số | |
|---|---|---|
| `bundle-mac:128` | kiểm 5 biến, thiếu thì ký ad-hoc `--sign -` | ✅ degrade đúng |
| `bundle-linux` | không ký gì | ✅ không liên quan |
| `bundle-windows.ps1:76-86` | **`exit 1`** | ❌ fail cứng |

Chỉ Windows phải sửa. `bundle-mac` là **mẫu** để sửa theo, không phải chỗ để sửa.

## Lỗi tiềm ẩn đã phát hiện (phải sửa ở phase này)

Việc rename Zed → Zode làm **nửa vời**, và `upload_artifact` dùng `if-no-files-found: error` nên nó sẽ fail ở bước upload dù build xong:

| Asset | Script sinh ra | `vars.rs` kỳ vọng | |
|---|---|---|---|
| Windows exe | `Zode-$Architecture.exe` (`bundle-windows.ps1:269,283`) | `Zed-x86_64.exe` (`vars.rs:384-385`) | ✗ |
| mac remote_server | `zode-remote-server-macos-*.gz` (`bundle-mac:357`) | `zed-remote-server-macos-*.gz` (`vars.rs:387`) | ✗ moot — sẽ bị xoá |
| mac dmg | `Zed-$arch_suffix.dmg` (`bundle-mac:261`) | `Zed-aarch64.dmg` | ✓ |
| linux tarball | `zed-linux-$arch.tar.gz` (`bundle-linux:213`) | `zed-linux-x86_64.tar.gz` | ✓ |

**Quyết:** đổi hết sang `Zode-*` / `zode-*`. Windows đã đi trước; kéo mac + linux theo, và `vars.rs` khớp lại ở [Phase 03](phase-03-fit-the-build-on-free-runners.md). Điều này chốt luôn mục "chưa quyết" về tên asset trong brainstorm.

## Phát hiện lúc implement (không có trong bản plan gốc)

1. **`script/lib/workspace.ps1` tìm package `"zed"`.** Crate đã đổi tên `zode`, nên `$env:RELEASE_VERSION` rỗng → `CheckEnvironmentVariables` `exit 1`. Blocker Windows độc lập với chuyện ký số. Đã sửa.
2. **`crates/zed/resources/windows/zed.iss:35` khai `SignTool=Defaultsign` khi `CI` khác rỗng.** Bỏ `/sDefaultsign` mà giữ directive → Inno abort vì tên SignTool không tồn tại. Hai điều kiện độc lập là chính cái bug. Đã gộp về một tín hiệu `ZODE_CODE_SIGN` do script đặt.
3. **`ZED_WORKSPACE` không cần workflow set** — `ParseZedWorkspace` (`bundle-windows.ps1:375`) tự suy từ `cargo metadata`. Ngược lại, `.working_directory("${{ env.ZED_WORKSPACE }}")` trong workflow resolve thành **rỗng** trên runner GitHub, nên đã bỏ.
4. **`cargo-bundle v0.6.1-zed` không cần bước CI** — `bundle-mac:60-62` tự `cargo install` từ git khi version lệch. Câu hỏi mở của plan tự trả lời.
5. **`script/generate-licenses` fail sẵn, chặn cả 6 job.** `webpki-roots 1.0.9` khai `CDLA-Permissive-2.0`, không có trong allowlist `script/licenses/zed-licenses.toml`. Cả ba script bundle gọi `generate-licenses` ở đầu → **mọi** job bundle sẽ đỏ. Đã xác minh fail ở cả baseline (không do plan này gây ra) và đã thêm license vào allowlist.

## Related code files

**Modify:**
- `script/bundle-windows.ps1` — required-vars (`:73-86`), signing (`:143-144`, `:221`, `:356-357`), remote_server (`:136-160`), `appSetupName`
- `script/bundle-mac` — remote_server (`:99-101`, `:319-331`, `:356-357`), dmg name (`:261`)
- `script/bundle-linux` — remote_server (`:61`, `:71`, `:93-98`, `:113`, `:136-143`, `:220`), archive name (`:213`)
- `script/determine-release-channel` — viết lại hoặc bỏ khỏi đường release

**Không tạo file mới. Không xoá file nào** — `determine-release-channel` còn được `release_nightly` và người khác gọi; đổi hành vi, đừng xoá.

## Implementation steps

1. **`bundle-windows.ps1` — tách nhóm ký số ra khỏi required-vars.** Giữ `$requiredVars = @('ZED_WORKSPACE', 'RELEASE_VERSION', 'ZED_RELEASE_CHANNEL')`. Chín biến còn lại (`AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `ACCOUNT_NAME`, `CERT_PROFILE_NAME`, `ENDPOINT`, `FILE_DIGEST`, `TIMESTAMP_DIGEST`, `TIMESTAMP_SERVER`) chuyển thành **cờ**:

   ```powershell
   $script:canCodeSign = -not ([string]::IsNullOrEmpty($env:AZURE_TENANT_ID) -or
                               [string]::IsNullOrEmpty($env:AZURE_CLIENT_ID) -or
                               [string]::IsNullOrEmpty($env:AZURE_CLIENT_SECRET))
   ```

   Theo đúng hình dạng `bundle-mac:128`. Nếu không ký được thì **in một dòng nói rõ**, đừng im lặng.

2. **`bundle-windows.ps1` — guard ba chỗ gọi ký.** `:221` (`sign.ps1 $files`) và `:356-357` (`/sDefaultsign=`) hiện rẽ theo `$env:CI`; đổi sang rẽ theo `$script:canCodeSign`. `:143-144` biến mất cùng remote_server ở bước sau.

3. **`bundle-windows.ps1` — `ZED_WORKSPACE`.** Biến này là quy ước dev-drive của runner self-hosted Zed. Trên runner GitHub-hosted, workflow phải set nó = `${{ github.workspace }}`. Việc set nằm ở Phase 03 (`vars.rs`); ở đây chỉ **kiểm** rằng script không giả định gì hơn ngoài "đường dẫn tuyệt đối tới cây source".

4. **Bỏ `remote_server` ở cả ba script.** Đây là đòn giảm chi phí lớn nhất — `bundle-linux` hiện build **hai** target (target thường + musl):
   - `bundle-linux`: xoá `:61` (`remote_server_triple`), `:71` (`rustup target add`), `:93-98` (`cargo build --package remote_server`), `:113` (kiểm), `:136-143` (strip + kiểm libssl), `:220` (gzip).
   - `bundle-mac`: xoá `:99-101` (`cargo build --package remote_server`), `:319-331` (dsymutil), `:356-357` (sign + gzip).
   - `bundle-windows.ps1`: xoá `:136-160` (build, sign, zip, pdb).

   Xoá **cả** bước build lẫn bước đóng gói. Bỏ sót bước build là mất công mà không được lợi gì.

5. **Đổi tên asset sang Zode.** `bundle-mac:261` → `Zode-${arch_suffix}.dmg`. `bundle-linux:213` → `zode-linux-${arch}.tar.gz`. Windows đã đúng.

6. **`determine-release-channel` — viết lại.** Hiện `exit 1` với channel `dev` và bắt tag khớp `v{Cargo version}`. Thay bằng: suy channel từ hình dạng tag, không đọc `RELEASE_CHANNEL`, không so version.

   ```bash
   # v0.1.0        → stable, không prerelease
   # v0.1.0-beta.1 → preview, prerelease
   version="${GITHUB_REF_NAME#v}"
   if [[ "$version" == *-* ]]; then channel=preview; else channel=stable; fi
   echo "RELEASE_CHANNEL=${channel}" >> "$GITHUB_ENV"
   echo "RELEASE_VERSION=${version}" >> "$GITHUB_ENV"
   ```

   Giữ nguyên hợp đồng ra (`RELEASE_CHANNEL` + `RELEASE_VERSION` vào `$GITHUB_ENV`) để `create_draft_release` và `draft-release-notes` không phải đổi.

7. **`set -euxo pipefail` → `set -euo pipefail`** ở `bundle-linux:3` và `bundle-mac`. Thuần giảm ồn log; repo đã public nên không còn là vấn đề bảo mật.

8. **`script/shellcheck-scripts`** và `script/check-keymaps` phải xanh (job `check_scripts` gọi chúng).

## Todo

- [ ] `bundle-windows.ps1`: required-vars còn 3; nhóm `AZURE_*` thành cờ `$canCodeSign`
- [ ] `bundle-windows.ps1`: guard `:221` và `:356` theo `$canCodeSign` thay vì `$env:CI`
- [ ] `bundle-windows.ps1`: xoá khối remote_server `:136-160`
- [ ] `bundle-mac`: xoá remote_server `:99-101`, `:319-331`, `:356-357`
- [ ] `bundle-mac:261`: `Zed-` → `Zode-`
- [ ] `bundle-linux`: xoá remote_server `:61`, `:71`, `:93-98`, `:113`, `:136-143`, `:220`
- [ ] `bundle-linux:213`: `zed-linux-` → `zode-linux-`
- [ ] `determine-release-channel`: suy channel từ tag, bỏ kiểm version
- [ ] `set -euxo` → `set -euo` ở `bundle-linux:3` và `bundle-mac`
- [ ] `script/shellcheck-scripts` xanh
- [ ] **Chạy thật `./script/bundle-mac aarch64-apple-darwin` trên máy local** (M1 Pro, 32 GB) — chứng minh script còn build được sau khi cắt

## Success criteria

- `./script/bundle-mac aarch64-apple-darwin` trên máy local sinh ra `Zode-aarch64.dmg` và **không** sinh file remote_server nào.
- `grep -rn "remote_server" script/bundle-*` không còn kết quả.
- `script/shellcheck-scripts` xanh.
- `GITHUB_REF_NAME=v0.1.0 GITHUB_ACTIONS=1 GITHUB_ENV=/tmp/e script/determine-release-channel` → exit 0, `/tmp/e` chứa `RELEASE_CHANNEL=stable` và `RELEASE_VERSION=0.1.0`. Lặp lại với `v0.1.0-beta.1` → `preview`.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| Bỏ `remote_server` là **mất tính năng SSH remote dev**, không phải tối ưu build | Người dùng đã chấp nhận (quyết định #7). Ghi vào README để không ai đi tìm. |
| Cắt remote_server khỏi `bundle-linux` có thể làm hỏng bước strip/kiểm dùng chung biến `remote_server_triple` | Chạy shellcheck; và không thể kiểm bundle-linux trên macOS → phải đợi CI ở Phase 03. **Nói rõ đây là chỗ mù.** |
| `determine-release-channel` mới có thể phá `release_nightly` (đường nightly có gọi nó?) | `grep -rn "determine-release-channel" .github/ script/ tooling/` trước khi sửa. Nếu nightly có gọi, giữ nhánh tương thích. |
| Không có secret ký → không ai xác minh được đường ký còn hoạt động | Đúng, và không sửa được ở phase này. Đường ký ngủ đông tới khi có cert. |

## Security considerations

- Không hardcode secret nào vào script.
- Khi không ký được, script phải **in ra là không ký** — im lặng bỏ qua ký số là cách người ta ship binary không ký mà không biết.
- Bỏ `-x` giảm rò biến môi trường vào log công khai. Không phải biện pháp bảo mật thật (repo đã public), chỉ là vệ sinh.

## Next steps

→ [Phase 03](phase-03-fit-the-build-on-free-runners.md) (cần cả 01 và 02 xong)
