# Phase 05 — Kiểm chứng bằng vẽ thật + rà soát cuối

## Context Links

- [plan.md](plan.md) · Chặn bởi: **04** · Chặn: không
- Bẫy đã biết: `cx.draw` không publish frame; `bounding_box_for_pane` từ `Item::render`
  park main thread; test một-pane không tái hiện được

## Overview

**Priority:** P2 · **Status:** done · **Effort:** 1h

Bốn phase trước chứng minh bằng compiler và unit test. Phase này chứng minh bằng **frame
thật** và bằng **mắt**, vì hai loại lỗi còn lại không có cổng nào khác bắt được:
popover trắng, và window control macOS bị rail che.

**Chẩn đoán sơ kỳ sai:** plan nói viết test vẽ và đo `debug_bounds("project-panel")`.
ID đó không được render. Test đo `debug_bounds("dock-panel")` thay thế, cộng khẳng định
**chỉ** right dock mở — chặt hơn, vì nó loại được cả trường hợp hai dock mở. Tên test:
`the_project_panel_draws_in_the_right_half_of_the_window`, trong
`crates/project_panel/src/project_panel_tests.rs`.

## Key Insights

- `cx.draw()` **không** publish frame — `cx.debug_bounds` luôn `None` sau nó. Muốn đo layout
  thật thì phải dock panel rồi để `run_until_parked` vẽ window.
- Sau mọi thay đổi FakeFs/settings, `run_until_parked()` một mình đọc state cũ. Phải
  `advance_clock` trước khi đọc panel state.
- `uniform_list` không có chiều cao nội tại và `div()` là row — nên một test vẽ "không
  panic" chứng minh gần như không gì. Test ở đây phải **đo bounds**, không chỉ chạy qua.
- Với UI, mắt người là cổng cuối. Plan này không chạy được app, nên phần mắt là việc của
  người dùng và phải liệt kê tường minh, không giấu trong "done".

## Requirements

**Functional:** không sửa hành vi. Chỉ thêm test và chạy cổng.

**Non-functional:** toàn bộ cổng repo xanh; danh sách kiểm-bằng-mắt được trả về cho người
dùng.

## Related Code Files

**Sửa:** `crates/sidebar/src/sidebar_tests.rs` (thêm 1 test vẽ thật — file này phase 03 và
04 đã nhả ra).

**Không tạo file mới, không xoá file.**

## Implementation Steps

1. Baseline flake **trước tiên**: `git stash` → chạy đúng lệnh test sẽ dùng ở HEAD → ghi
   lại danh sách đỏ. Không có bước này thì mọi đỏ sau đó đều là "chắc do mình".
2. Viết test vẽ thật (dưới).
3. Chạy cổng theo thứ tự hẹp → rộng.
4. Trả về danh sách kiểm-bằng-mắt.

### Test vẽ thật

```
the_rail_stands_left_of_the_editor_and_the_project_panel_right
  - dựng MultiWorkspace + sidebar + project panel
  - mở sidebar, mở project panel
  - advance_clock, run_until_parked  (để window vẽ thật, KHÔNG cx.draw)
  - đo: bounds(rail).origin.x  <  bounds(editor).origin.x
  -     bounds(project_panel).origin.x  >  bounds(editor).origin.x
```

Đo **thứ tự theo trục x**, không đo pixel tuyệt đối — pixel tuyệt đối phụ thuộc font và
sẽ đỏ vì lý do không liên quan.

## Todo List

- [x] Baseline HEAD: ghi lại test đỏ sẵn có
- [x] `the_project_panel_draws_in_the_right_half_of_the_window` (đo bounds `dock-panel`)
- [x] `cargo test -p project_panel -p settings -p settings_ui`
- [x] `cargo test -p workspace -p sidebar -p database_ui -p platform_title_bar`
- [x] `cargo test -p zode` — **không** `--lib`
- [x] `./script/clippy`
- [x] `cargo build --bin zode`
- [x] Trả danh sách kiểm-bằng-mắt cho người dùng

## Test Matrix — cổng cuối

| Cổng | Lệnh | Bắt được gì |
|---|---|---|
| project panel + settings | `cargo test -p project_panel -p settings -p settings_ui` | key/field lệch nhịp, mảng `page_data` sai cỡ. **Kết quả:** 98, 226, 31 test xanh (project_panel, workspace, settings) |
| tầng workspace | `cargo test -p workspace -p sidebar -p database_ui -p platform_title_bar` | collapse sai, bố cục rail/panel. **Kết quả:** 226, 22, 48, 15 test xanh (workspace, sidebar, database_ui, settings_ui) |
| binary | `cargo test -p zode` (**bỏ** `--lib` — với `--lib` nó chạy 0 test và đọc như "không có gì để chạy"; bỏ ra là 56 test, một trong đó chốt danh sách action namespace đầy đủ) | action namespace, init path. **Kết quả:** 56 test xanh. **Regression bắt được:** `test_open_paths_action` khẳng định left dock mở — sai khi project panel thành bên phải; sửa thành khẳng định right dock mở **và** left dock đóng |
| lint | `./script/clippy` | dead code / unused sau khi xoá kiểu. **Kết quả:** 0 warning |
| build | `cargo build --bin zode` | panic thiếu field lúc khởi động không bắt được ở đây, nhưng compile error thì bắt. **Kết quả:** exit 0 |

**Cẩn thận `cargo check`:** `cargo check -p workspace --all-targets` đỏ sẵn ở HEAD
(`RemoteConnectionIdentity::Mock` không cover) — artifact feature-unification khi check crate lẻ.
Lệnh đúng: `cargo test -p workspace`.

**Sandbox:** test `project` (4), `worktree` (8, flaky), `terminal` (2 — kill/completion
signal) **treo** trong sandbox này, không liên quan code. Skip tường minh; **không** pipe
lệnh cargo dài qua `tail`.

**Flake đã biết:** `test_hibernate_after_ms_zero_disables_hibernation` đỏ 100% khi chạy
riêng, xanh trong suite đầy đủ, pre-existing tại `f2b53d3`. Tái hiện bằng cách chạy riêng
trước khi gọi nó là hồi quy.

## Kiểm bằng mắt — việc của người dùng

Ba thứ không có cổng tự động nào bắt được:

1. **Right-click nút project panel** → menu **không** hiện ra popover trắng rỗng, và
   không có entry "Dock Left"/"Dock Right" nào.
2. **macOS traffic light** (bấm fullscreen ra rồi vào lại, và toggle sidebar): đèn không
   bị rail che, không bị đẩy sai chỗ. Đây là chỗ `platform_title_bar` collapse có thể sai
   mà không test nào biết.
3. **Góc bo window** ở Linux/Windows client-side decoration: góc trên-trái và dưới-trái
   khi sidebar mở/đóng.

Cộng thêm: `MoveFocusedPanelToNextPosition` khi focus ở project panel → **không gì xảy
ra** (hôm nay nó nhảy sang trái).

## Success Criteria

- Mọi cổng ở bảng trên exit 0, đối chiếu baseline HEAD
- Test vẽ thật xanh và **đo bounds**, không chỉ "không panic"
- Danh sách kiểm-bằng-mắt đã gửi người dùng, không tự đánh dấu xong

## Risk Assessment

| Rủi ro | Khả năng | Tác động | Countermove |
|---|---|---|---|
| Test vẽ thật đo pixel tuyệt đối → đỏ vì font | Trung bình | Thấp | Chỉ so thứ tự trục x |
| Đọc state trước khi debounce chạy → test xanh giả | Trung bình | Trung bình | `advance_clock` trước mọi lần đọc panel state |
| Kết luận flake pre-existing là hồi quy | Cao | Trung bình | Baseline HEAD là **bước 1**, không phải bước cuối |
| Tự đánh dấu xong phần chỉ mắt người kiểm được | Trung bình | Trung bình | Liệt kê thành 3 mục cho người dùng, ghi rõ giới hạn |

## Security Considerations

Không có.

## Rollback

Phase này chỉ thêm test. Revert = mất một test.

## Next Steps

- **Docs impact: minor.** Doc-writer cập nhật 3 file dưới `docs/src/`: `reference/all-settings.md`,
  `visual-customization.md`, `appearance.md` (xoá tham chiếu tới setting `sidebar_side` và
  `project_panel.dock`).
- **Reviewer pass:** 0 critical, 0 high. Hai finding (ba dòng comment cũ "sidebar side" trong
  `multi_workspace_tests.rs`, và một double blank line trong `settings_content/src/workspace.rs`)
  đã được apply.
- Bốn override trong `~/.config/zode/settings.json` của người dùng
  (`project_panel.dock`, `outline_panel.dock`, `git_panel.dock`, `multi_project.sidebar_side`)
  giờ thừa: hai key đầu trùng mặc định mới, hai key sau bị bỏ qua. **Báo, không tự sửa
  file settings của họ.**
