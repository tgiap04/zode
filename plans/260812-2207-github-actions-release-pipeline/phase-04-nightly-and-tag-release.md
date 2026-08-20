# Phase 04 — Hai đường phát hành: nightly cuộn + draft release ở tag

**Priority:** P1 · **Status:** ⬜ pending · **Blocked by:** 03
**Context:** [plan.md](plan.md) · [brainstorm 260819](./reports/brainstorm-260819-release-pipeline-public-repo.md)

## Mục tiêu

Nối 6 job đã xanh ở Phase 03 vào hai đích:
- **cron mỗi ngày** → một prerelease **cuộn** tên `nightly`, asset bị ghi đè, không tích lũy.
- **push tag `v*`** → build lại từ commit của tag → **draft** release mang 6 asset, người dùng tự bấm publish.

Và dọn hết job cần hạ tầng riêng của Zed ra khỏi hai workflow này.

## Key insight — hai cái bẫy

**1. `schedule` chỉ chạy trên default branch.** Default branch của repo này là **`develop`**, không phải `main`. Nghĩa là cron sẽ đọc workflow file từ `develop` **và** checkout `develop` — nightly sẽ âm thầm build `develop` chứ không phải `main`. Hai cách chữa, phải chọn một dứt khoát:

| | Cách | Đánh đổi |
|---|---|---|
| a | `checkout_repo().with_ref("main")` trong mọi job nightly | Workflow **file** vẫn phải có mặt và đúng trên `develop`. Không đổi thói quen làm việc. |
| b | Đổi default branch sang `main` | Sạch hơn, nhưng đổi hành vi mặc định của PR và của mọi thứ khác. |

**Chọn (a)** — đây là plan về release, không phải nơi đổi mô hình branch. Nhưng phải ghi rõ trong README rằng workflow file cần được merge vào `develop` để cron thấy nó.

**2. `create_draft_release` chạy `script/draft-release-notes`** (`release.rs:412`), script này diff giữa tag hiện tại và tag trước. **Release đầu tiên không có tag trước** → khả năng cao fail hoặc sinh notes rỗng. Phải có đường lùi.

## Related code files

**Modify:**
- `tooling/xtask/src/tasks/workflows/release.rs` — bỏ `compliance_check` (`:25`, `:99-102`), `auto_release_preview` (`:66`, `:111`, `:354-371`), `push_release_update_notification` (`:77-84`, `:112`, `:440-554`); `validate_release_assets` bỏ tham số compliance (`:311-352`); `--repo` → `${{ github.repository }}` (`:319`, `:403`); test gate thu về Linux
- `tooling/xtask/src/tasks/workflows/release_nightly.rs` — `update_nightly_tag_job` (`:93-129`): thay `script/upload-nightly` + DigitalOcean bằng `gh release`; bỏ `create_sentry_release()`; `checkout_repo().with_ref("main")`
- `README.md` — bảng download, hướng dẫn mở app unsigned, ghi rõ không tự cập nhật và không có remote dev

**Có thể xoá** (chỉ khi `grep` chứng minh không còn ai gọi): `release.rs:148-158` `create_sentry_release`, `:160-291` `add_compliance_steps`, `:556-619` khối Slack.

## Implementation steps

1. **`release.rs` — thu test gate về Linux.** Sáu job test/clippy hiện chặn trước bundling; trên runner free đó là hàng giờ cộng thêm cho mỗi lần release. Theo quyết định đã chốt (Đường 1), giữ `linux_tests` + `linux_clippy` + `check_scripts`; bỏ `macos_tests`, `windows_tests`, `macos_clippy`, `windows_clippy` khỏi cả `deps` của 6 bundle job lẫn `add_job`.

   Đây là **giảm độ phủ có ý thức**, không phải tối ưu. Ghi vào README: lỗi chỉ-xuất-hiện-trên-Windows sẽ không bị chặn ở release.

2. **`release.rs` — cắt ba job cần hạ tầng Zed.** `compliance_check` cần GitHub App `ZED_ZIPPY_*`; `auto_release_preview` cần cùng App đó; `push_release_update_notification` cần Slack webhook. Cả ba **không thể** hoạt động trên account này.

   Cắt cả hàm sinh ra chúng, đừng chỉ bỏ `add_job` — để hàm chết lại là mời người sau nối lại.

   `validate_release_assets` phải giữ (nó là thứ bắt lỗi tên asset lệch), nhưng bỏ tham số `context_check_result` và hai bước `checkout_repo` + `cache_rust_dependencies_namespace` vốn chỉ có ở đó để phục vụ compliance.

3. **`release.rs` — repo-neutral.** Ba chỗ `--repo=zed-industries/zed` (`:319`, `:403`, và trong khối Slack sẽ bị cắt) → `--repo=${{ github.repository }}`. Đừng hardcode `tgiap04/zode`: `github.repository` đúng cả khi ai đó fork.

4. **`release.rs` — đường lùi cho release notes.** Bọc `generate_release_notes()` (`:410-414`) để không giết cả job:

   ```bash
   node --redirect-warnings=/dev/null ./script/draft-release-notes "$RELEASE_VERSION" "$RELEASE_CHANNEL" \
     > target/release-notes.md \
     || echo "Release $RELEASE_VERSION" > target/release-notes.md
   ```

   Release đầu tiên không có tag trước để diff. Notes rỗng là chấp nhận được; job đỏ thì không.

5. **`release_nightly.rs` — đổi đích phát hành.** `update_nightly_tag_job` (`:93-129`) hiện đẩy lên DigitalOcean Spaces qua `script/upload-nightly`. Thay bằng prerelease cuộn:

   ```bash
   gh release view nightly \
     || gh release create nightly --prerelease --target main \
          -t "Nightly" -n "Bản build tự động từ main. Không ký số, không tự cập nhật."
   gh release upload nightly release-artifacts/* --clobber
   ```

   `--clobber` là thứ làm nó **cuộn**: một release, asset bị thay mỗi ngày, không tích lũy. Giữ nguyên bước `update_nightly_tag()` (`:94-105`) đang có — nó đã dời tag `nightly` sang HEAD đúng cách. Bỏ `create_sentry_release()` (`:127`).

6. **`release_nightly.rs` — checkout `main`.** Mọi job trong workflow nightly: `steps::checkout_repo().with_ref("main")`. Không có bước này, cron build `develop`.

7. **`release_nightly.rs` — thu test gate.** Nightly hiện chạy test + clippy trên **Windows** (`:19-21`, chọn vì đó là platform nhanh nhất của Zed). Trên runner free thì Linux nhanh nhất → đổi sang `Platform::Linux`. Bỏ hai job `build_nix` (`:33-46`, `:66-67`) — chúng cần Nix cache của Zed.

8. **`README.md`** — bảng 6 asset kèm nền tảng; hướng dẫn mở app unsigned cho từng OS (macOS: `xattr -d com.apple.quarantine`, Windows: SmartScreen → More info → Run anyway); và ba dòng nói thẳng: **không ký số**, **không tự cập nhật**, **không có SSH remote development**.

9. **`cargo xtask workflows`**, commit, và kiểm thật:
   - `workflow_dispatch` đường nightly → prerelease `nightly` xuất hiện đủ 6 asset.
   - Chạy lại lần hai → vẫn **một** release `nightly`, asset được thay, không sinh release thứ hai.
   - Tạo tag `v0.0.1-test` → draft release đủ 6 asset → xoá tag và draft sau khi kiểm.

## Todo

- [ ] `release.rs`: test gate còn `linux_tests` + `linux_clippy` + `check_scripts`
- [ ] `release.rs`: cắt `compliance_check`, `auto_release_preview`, `push_release_update_notification` (cả hàm)
- [ ] `release.rs`: `validate_release_assets` bỏ tham số compliance
- [ ] `release.rs`: `--repo` → `${{ github.repository }}` (mọi chỗ)
- [ ] `release.rs`: release notes có đường lùi `|| echo`
- [ ] `release_nightly.rs`: `update_nightly_tag_job` dùng `gh release ... --clobber`
- [ ] `release_nightly.rs`: mọi job `checkout_repo().with_ref("main")`
- [ ] `release_nightly.rs`: test/clippy → `Platform::Linux`; bỏ 2 job `build_nix`
- [ ] `release_nightly.rs`: bỏ `create_sentry_release()`
- [ ] `grep -rn "zed-industries\|ZED_ZIPPY\|SLACK_WEBHOOK\|DIGITALOCEAN" .github/workflows/release.yml .github/workflows/release_nightly.yml` = 0
- [ ] `README.md`: bảng download + hướng dẫn mở app + 3 giới hạn đã biết
- [ ] `cargo xtask workflows`; commit
- [ ] Kiểm: nightly chạy 2 lần → vẫn 1 release, asset bị thay
- [ ] Kiểm: tag `v0.0.1-test` → draft release đủ 6 asset; dọn sau khi kiểm

## Success criteria

- `workflow_dispatch` nightly → prerelease `nightly` mang đúng 6 asset tên `Zode-*` / `zode-*`.
- Chạy nightly lần thứ hai → vẫn **một** release `nightly`; không có release thứ hai; asset mới thay asset cũ.
- Push `v0.0.1-test` → **draft** release (không tự publish) mang đủ 6 asset; `validate_release_assets` xanh.
- Merge một PR vào `main` → **không** job bundle nào chạy.
- Không còn tham chiếu nào tới `zed-industries`, `ZED_ZIPPY`, `SLACK_WEBHOOK`, `DIGITALOCEAN` trong hai workflow này.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| Cron đọc workflow file từ `develop` → sửa ở nhánh feature không có tác dụng cho tới khi merge vào `develop` | Ghi vào README. Kiểm bằng `workflow_dispatch` (đọc từ nhánh đang chạy) trước, rồi mới tin cron. |
| `script/draft-release-notes` fail ở release đầu tiên | Đường lùi `|| echo` ở bước 4. |
| `gh release upload --clobber` có thể để lại asset **cũ** nếu lần này thiếu file | `validate_release_assets` bắt được ở đường tag; đường nightly thì không. Chấp nhận, hoặc `gh release delete-asset` trước khi upload. |
| Thu test gate về Linux → lỗi riêng Windows/macOS lọt tới release | Đã chấp nhận có ý thức. `run_tests` vẫn chạy đủ 3 platform ở PR — gate chỉ bị thu ở **đường release**. |
| Cắt hàm ở `release.rs` có thể vỡ `release_nightly.rs` (nó `use` `create_sentry_release`, `notify_on_failure`, `prep_release_artifacts`, `download_workflow_artifacts` từ `release.rs`) | Sửa hai file **cùng một commit**. `cargo build -p xtask` là cái bắt lỗi này, không phải mắt người. |
| `--target main` khi tạo release `nightly` trong lúc tag `nightly` đã tồn tại | Bước `update_nightly_tag()` đã dời tag trước; thứ tự phải là dời tag **trước** khi tạo release. |

## Security considerations

- Đường tag tạo **draft**, không publish. Con người là cổng cuối trước khi asset ra công khai. Không tự động hoá bước bấm publish.
- `secrets.GITHUB_TOKEN` cần `contents: write` **chỉ** ở job upload/publish. Các job bundle giữ mặc định read-only.
- Không bao giờ dùng `pull_request_target` trong hai workflow này — nó cho code của PR lạ chạy với token có quyền ghi.

## Next steps

→ [Phase 05](phase-05-verify-on-clean-machines.md)
