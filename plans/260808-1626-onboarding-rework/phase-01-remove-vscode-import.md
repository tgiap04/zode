# Phase 01 — Xoá import từ VS Code / Cursor, tận gốc

## Context Links

- Biên bản: [`brainstorm-260808-onboarding-rework.md`](../reports/brainstorm-260808-onboarding-rework.md) § "Chỗ tôi phản đối"
- **Phase đầu tiên** — mọi phase sau làm việc trên code đã gọn hơn

## Overview

**Priority:** P2 · **Status:** pending · **Depends:** — · **Effort:** ~0.5d

Bỏ hẳn đường nhập settings từ VS Code và Cursor: UI, action, và toàn bộ code nền.

## Key Insights

- **Đây là phase bị phản đối và vẫn được chọn.** Biên bản ghi rõ: zode là fork hướng VSCode, nên
  nhập settings từ VSCode hợp với zode *hơn* cả với Zed. Người dùng đã nghe và vẫn chốt xoá.
  Ghi lại ở đây để người thi công không tưởng là sót.
- **`get_vscode_edits` đã rà: chỉ `settings_store.rs` tự gọi** (`:636` bởi `import_vscode_settings`,
  và `:2295` trong test). Không crate nào ngoài gọi tới ⇒ cắt cả cụm là an toàn.
- **`actions.json` là file SINH RA**, không phải viết tay. `script/generate-action-metadata` chạy
  `cargo run -p zode -- --dump-all-actions`. Sửa tay là để lại một artifact lệch với nguồn.
- 8 test trong `settings_store.rs` dùng helper `check_vscode_import` — chúng đi cùng.
- Phần lớn số dòng bị xoá là **code upstream Zed**. Đây là điểm conflict vĩnh viễn, không phải
  một lần.

## Requirements

**Chức năng:** Không còn đường nào trong app dẫn tới nhập settings từ VS Code/Cursor.
**Phi chức năng:** Không để lại import chết, action mồ côi, hay test tham chiếu thứ đã xoá.

## Architecture

Xoá theo tầng, từ ngoài vào trong: UI → action/handler → code nền.

## Related Code Files

**Xoá hẳn:** `crates/settings/src/vscode_import.rs` (1068 dòng)
**Sửa:** `crates/settings/src/settings_store.rs`, `crates/settings/src/settings.rs` (khai báo mod),
`crates/onboarding/src/onboarding.rs`, `crates/onboarding/src/basics_page.rs`
**Sinh lại:** `crates/docs_preprocessor/actions.json`
**Không sửa:** bất kỳ file nào khác

## Implementation Steps

1. **Rà lại trước khi cắt** — biên bản đã rà một lần, nhưng code có thể đã đổi:
   ```
   grep -rn "VsCodeSettings\|vscode_import\|ImportVsCodeSettings\|ImportCursorSettings\|SettingsImportState\|get_vscode_edits" crates/
   ```
   Nếu xuất hiện crate ngoài `settings`/`onboarding`/`docs_preprocessor` ⇒ **dừng lại**, phạm vi đã
   khác bản vẽ, báo cáo trước khi tiếp.

2. **UI** — xoá `render_import_settings_section` (`basics_page.rs:487`) và lời gọi nó (`:532`).

3. **Action + handler** (`onboarding.rs`) — hai action `ImportVsCodeSettings` (`:39`),
   `ImportCursorSettings` (`:48`), hai `register_action` (`:131`, `:151`),
   `handle_import_vscode_settings` (`:411`), `SettingsImportState` (`:514`).

4. **Code nền** — xoá `crates/settings/src/vscode_import.rs`, khai báo `mod vscode_import` và mọi
   `pub use` của nó; trong `settings_store.rs` xoá `import_vscode_settings` (`:629`),
   `get_vscode_edits` (`:837`), và import ở `:37`.

5. **Test đi kèm** — helper `check_vscode_import` và 8 chỗ gọi trong `settings_store.rs`.
   **Xoá, không `#[ignore]`** — test bị vô hiệu là rác, không phải an toàn.

6. **Sinh lại `actions.json`** — `./script/generate-action-metadata`. Đối chiếu số action giảm đúng 2.

7. **Rà sót** — chạy lại lệnh grep bước 1, phải rỗng.

## Todo List

- [ ] Rà trước, xác nhận không crate ngoài dự kiến
- [ ] UI section + lời gọi
- [ ] 2 action + 2 register + handler + `SettingsImportState`
- [ ] `vscode_import.rs` + khai báo mod + `pub use`
- [ ] `import_vscode_settings` + `get_vscode_edits` + import
- [ ] `check_vscode_import` + 8 chỗ gọi
- [ ] Sinh lại `actions.json`, xác nhận giảm đúng 2 action
- [ ] Rà sót lần cuối — grep rỗng

## Success Criteria

- `grep -rn "VsCodeSettings\|vscode_import\|ImportVsCodeSettings\|ImportCursorSettings" crates/` **rỗng**
- `cargo test -p settings -p onboarding -p zode` xanh
- `./script/clippy` sạch — kể cả `cargo-machete` (xoá code có thể làm một dependency thành thừa)
- `actions.json` giảm **đúng 2** action, và được sinh ra chứ không sửa tay

## Test Design

**Phase này không thêm test — nó xoá test.** Đó là điều đúng: 8 test kia kiểm đúng thứ vừa biến mất.

Cái thay thế chúng là hai phép đo cơ học ở Success Criteria: grep rỗng và biên dịch sạch. Với việc
xoá, trình biên dịch **là** bộ test — nó bắt mọi tham chiếu mồ côi. `cargo-machete` bắt phần
dependency thừa mà trình biên dịch không thấy.

## Risk Assessment

| Rủi ro | Đối phó |
|---|---|
| Sót một tham chiếu ⇒ không biên dịch được | Bước 1 và 7 kẹp hai đầu bằng cùng một lệnh grep |
| Sửa tay `actions.json` ⇒ artifact lệch nguồn | Bước 6 bắt chạy script |
| `#[ignore]` test thay vì xoá | Bước 5 nói thẳng |
| Xoá quá tay sang phần không liên quan trong `settings_store.rs` | Bước 4 liệt kê đúng số dòng |
| Dependency thành thừa sau khi xoá | `cargo-machete` trong `./script/clippy` |

## Security Considerations

`vscode_import.rs` đọc file settings của người dùng từ đường dẫn cài VS Code. Xoá nó **giảm** bề
mặt đọc file, không tăng.

## Next Steps

Mở khoá phase 02. Sau phase này `basics_page.rs` còn 3 section.
