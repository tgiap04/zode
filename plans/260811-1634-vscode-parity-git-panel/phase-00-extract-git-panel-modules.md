# Phase 00 — Tách `git_panel.rs` thành module

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-vscode-parity-git-panel.md)
**Priority:** P2 · **Status:** pending · **Effort:** 2-3d · **Blocked by:** —

Thuần di chuyển code. **Zero behavior change.** Đây là phase nền: không có nó, thêm 4 section vào một file 7.6k dòng sẽ để lại thứ tệ hơn hiện tại (quyết định 4).

## Key insights

- `crates/git_ui/src/git_panel.rs` = **7.619 dòng**. Một `impl GitPanel` duy nhất trải từ dòng **688 → 5209** (~4.500 dòng), cộng `mod tests` từ **6080 → 7619** (~1.540 dòng).
- Vùng **3604 → 5208** gần như toàn bộ là rendering — cắt ở đây được nhiều nhất với ít xáo trộn nhất.
- **Rust cho phép chia một `impl` qua nhiều file trong cùng crate.** Submodule khai báo thêm `impl GitPanel { … }`.
- **Không cần đổi visibility bất kỳ field nào.** Module con thấy được toàn bộ private item của module cha, nên `self.entries`, `self.commit_editor`, `self.active_repository` … vẫn truy cập được từ `git_panel::render_header`. Đây là dữ kiện làm phase này thành một phép move thuần.
- CLAUDE.md cấm `mod.rs`. Layout đúng: giữ `src/git_panel.rs` làm cha, thêm `src/git_panel/<child>.rs`. Đây là layout Rust 2018, hợp lệ.

## Requirements

**Functional:** không có. Panel phải hành xử **giống hệt** trước và sau phase này.
**Non-functional:** mỗi file mới nằm trong mức đọc được; `./script/clippy` sạch; test `git_ui` xanh giữa **từng** commit.

## Architecture — đường cắt

```
crates/git_ui/src/
├── git_panel.rs                      ← giữ: struct defs, GitPanel struct, state,
│                                        actions/logic, Render, Panel, Focusable,
│                                        PanelHeader, GitPanelAddon
└── git_panel/
    ├── render_header.rs              ← render_panel_header, render_overflow_menu,
    │                                    render_git_commit_menu
    ├── render_commit_box.rs          ← render_footer, render_commit_button,
    │                                    render_pending_amend, render_previous_commit,
    │                                    render_remote_button
    ├── render_entries.rs             ← render_entries, render_list_header,
    │                                    render_status_entry, render_directory_entry,
    │                                    render_empty_state
    └── tests.rs                      ← mod tests hiện tại (6080–7619)
```

Hàm nguồn và dòng hiện tại:

| Hàm | Dòng | Đi đâu |
|---|---|---|
| `render_overflow_menu` | 3604 | `render_header.rs` |
| `render_git_commit_menu` | 3637 | `render_header.rs` |
| `render_panel_header` | 3749 | `render_header.rs` |
| `render_remote_button` | 3814 | `render_commit_box.rs` |
| `render_footer` | 3837 | `render_commit_box.rs` |
| `render_commit_button` | 3955 | `render_commit_box.rs` |
| `render_pending_amend` | 4036 | `render_commit_box.rs` |
| `render_previous_commit` | 4062 | `render_commit_box.rs` |
| `render_empty_state` | 4163 | `render_entries.rs` |
| `render_entries` | 4286 | `render_entries.rs` |
| `render_list_header` | 4447 | `render_entries.rs` |
| `render_status_entry` | 4615 | `render_entries.rs` |
| `render_directory_entry` | 4863 | `render_entries.rs` |

**Ở lại `git_panel.rs`** dù nằm giữa vùng cắt: `render_buffer_header_controls` (4231, thuộc `GitPanelAddon`), `load_commit_details` (4530), `deploy_entry_context_menu`, `is_on_main_branch` — logic, không phải render tree.

## Related code files

**Sửa:** `crates/git_ui/src/git_panel.rs` (thêm 4 khai báo `mod`, xoá phần đã move)
**Tạo:** `crates/git_ui/src/git_panel/render_header.rs`, `render_commit_box.rs`, `render_entries.rs`, `tests.rs`
**Xoá:** không

## Implementation steps

1. Tạo `git_panel/render_entries.rs`. Đầu file: `use super::*;`. Move 5 hàm entries. Trong `git_panel.rs` thêm `mod render_entries;`. Chạy `./script/clippy` → xử hết import thiếu bằng cách thêm `use` cụ thể vào file con, **không** nới `pub` ở cha.
2. Commit. Test `git_ui` xanh.
3. Lặp cho `render_commit_box.rs` (5 hàm). Commit. Test xanh.
4. Lặp cho `render_header.rs` (3 hàm). Commit. Test xanh.
5. Move `mod tests` → `git_panel/tests.rs`, khai báo `#[cfg(test)] mod tests;` trong `git_panel.rs`. Test dùng `use super::super::*;` hoặc `use crate::git_panel::*;` tuỳ item. Commit. Test xanh.
6. Đo lại: `wc -l crates/git_ui/src/git_panel.rs crates/git_ui/src/git_panel/*.rs`. Ghi số vào commit message cuối.

## Todo

- [x] `render_entries.rs` + commit + test xanh — `fe4963e`
- [x] `render_commit_box.rs` + commit + test xanh — `7875a3a`
- [x] `render_header.rs` + commit + test xanh — `b2cd0f2`
- [x] `tests.rs` + commit + test xanh — `42763c2`
- [x] Không có `pub`/`pub(crate)` nào được nới rộng so với HEAD — xác minh: 5 dòng `+pub fn` trong diff là artifact re-align, cả 5 đã `pub` ở `main`
- [x] `wc -l` ghi vào commit cuối — `git_panel.rs` 7619 → 4813; tổng 5 file = **7619**, net zero

## Kết quả (2026-08-11)

`git_panel.rs` **7619 → 4813**. Tổng 5 file = 7619 dòng, đúng bằng file gốc (+12 dòng wrapper/`mod`, −12 dòng do rustfmt gộp dòng). Clippy exit 0, **60/60 test xanh** sau từng commit. 5 commit trên `feat/vscode-parity-git-panel`.

| File | Dòng |
|---|---|
| `git_panel.rs` | 4813 |
| `git_panel/tests.rs` | 1530 |
| `git_panel/render_entries.rs` | 731 |
| `git_panel/render_commit_box.rs` | 348 |
| `git_panel/render_header.rs` | 197 |

### Hai điều phase này dạy lại cho plan

**1. Giả định về visibility đúng nửa, và là nửa ít quan trọng hơn.** Plan viết "module con thấy được toàn bộ private item của module cha, nên phép move là thuần". Đúng — không lỗi nào về `self.entries` / `self.commit_editor`. Nhưng **chiều ngược lại không đúng**: cha *không* thấy private của con. Sáu hàm được gọi từ ngoài module mới phải mang `pub(super)`:

| Hàm | Gọi từ |
|---|---|
| `render_entries`, `render_empty_state` | `Render` (cha) |
| `render_pending_amend`, `render_previous_commit` | `Render` (cha) |
| `render_panel_header` | `Render` (cha) |
| `render_git_commit_menu` | `render_commit_box` (**sibling**) |

`pub(super)` **không phải nới rộng**: một `fn` trần trong module cha đã thấy được trong `git_panel` và con cháu; `pub(super)` từ module con phủ đúng tập đó. Mức hiệu dụng y hệt. Phase 01–05 sẽ gặp lại điều này với mọi hàm mới đặt trong submodule.

**2. Nhóm commit-box không tự chứa.** `render_commit_box` với sang `render_header::render_git_commit_menu`. Không phải lỗi cắt — nút Commit *cần* menu của nó — nhưng phase 02 dời cả hai nên cần biết chúng dính nhau.

### Drift rustfmt

`git_panel.rs` ở HEAD có 3 hunk drift. Một hunk nằm trong `render_footer` nên theo code sang `render_commit_box.rs` và đã được dọn ở đó. Hai hunk còn lại (dòng 63 `use ui::{…}`, dòng 647 blank trong struct) nằm trong code phase này không chạm — **để nguyên**, theo đúng tiền lệ commit `f2f58c4` của repo. Cả 4 file mới: 0 hunk. Net crate bớt 1 hunk.

## Success criteria

- `./script/clippy` sạch sau **mỗi** commit, không chỉ commit cuối.
- Test `git_ui` xanh sau mỗi commit.
- `git diff HEAD~5 --stat` cho thấy tổng số dòng thay đổi ≈ 0 (move, không viết mới).
- **Không** field/hàm nào phải nới visibility. Nếu buộc phải nới → đường cắt sai, cắt lại.
- Mở panel: hành vi không phân biệt được với trước.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| Diff lớn dạng no-op, khó review (R6) | 5 commit riêng biệt, mỗi commit một file, test xanh giữa từng cái |
| Cắt sai chỗ → phải nới `pub` khắp nơi | Nếu một hàm đòi nới visibility, nó thuộc nhóm logic chứ không phải render → để lại `git_panel.rs` |
| `mod tests` tham chiếu private item của cha | Module con thấy private của cha; chỉ cần `use` đúng path |
| Lỡ tay sửa logic trong lúc move | `git diff` từng commit phải là move thuần; không chấp nhận "sửa nhẹ cho gọn" ở phase này |

## Security

Không có bề mặt mới. Không đổi đường dẫn chạy lệnh git.

## Next steps

Phase 01 dựng `PanelSection` lên trên nền module vừa tách.
