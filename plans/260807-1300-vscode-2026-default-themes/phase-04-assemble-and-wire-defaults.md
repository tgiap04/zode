# Phase 04 — Lắp ráp file theme + đổi mặc định

## Context Links

- [phase 02](phase-02-build-dark-theme.md) → `build/dark-2026.json`
- [phase 03](phase-03-build-light-theme.md) → `build/light-2026.json`
- Nạp theme bundled: `crates/assets/src/assets.rs:12` (`#[include = "themes/**/*"]`)

## Overview

**Priority:** P1 · **Status:** pending · **Depends:** phase 02 + 03 · **Effort:** ~0.5d

Gộp hai theme vào **một** file family, rồi trỏ mặc định của app sang chúng.

## ⚠️ Key Insight — điểm sửa đã chốt bị sai, đã hiệu chỉnh

Buổi brainstorm chốt sửa `crates/theme/src/theme.rs:45` (`DEFAULT_DARK_THEME`). Trace lại toàn bộ
call site cho thấy **đó là hằng số sai** cho mục đích này. Trong cây code có **hai** hằng số trùng tên,
vai trò khác hẳn nhau:

| Hằng số | Vai trò | Hành động |
|---|---|---|
| `crates/settings_content/src/theme.rs:272-273`<br/>`DEFAULT_LIGHT_THEME` / `DEFAULT_DARK_THEME` | **Mặc định thật của settings** — `ThemeSelection::default()` (`:278-281`) dùng nó; `theme_settings/src/settings.rs:82-83` cũng vậy | ✅ **SỬA** |
| `crates/theme/src/theme.rs:45`<br/>`DEFAULT_DARK_THEME` | **Fallback khẩn cấp** — đặt tên cho theme Rust hardcode ở `fallback_themes.rs:109`, dùng khi asset JSON không nạp được (`theme.rs:106`, `theme_settings.rs:145`) | ❌ **GIỮ NGUYÊN** |

**Vì sao không được sửa cái thứ hai:** `fallback_themes.rs:109` đặt `name: DEFAULT_DARK_THEME.into()`
cho một theme Rust hardcode mang **màu One Dark**. Đổi hằng số này thành `"Dark 2026"` sẽ khiến
**hai theme khác nhau cùng mang tên "Dark 2026"** trong registry — bản JSON thật và bản hardcode
mang màu One Dark. Bên nào thắng phụ thuộc thứ tự đăng ký. Và `theme_settings.rs:145` gọi
`themes.get(DEFAULT_DARK_THEME).unwrap()` ở nhánh chót — nhánh này phải luôn trỏ vào một theme
**chắc chắn tồn tại không phụ thuộc asset**, tức là bản hardcode.

→ Sửa `settings_content` là đủ để app khởi động bằng Dark 2026. `theme.rs:45` giữ `"One Dark"` để
lưới an toàn còn nguyên.

## Requirements

**Chức năng:**
- `assets/themes/vscode-2026/vscode-2026.json` chứa `themes: ["Dark 2026", "Light 2026"]`
- App khởi động mặc định vào Dark 2026 (dark) / Light 2026 (light)
- One Dark / One Light **còn nguyên** trong theme selector

**Phi chức năng:** `./script/clippy` sạch. **Không** dùng `cargo clippy` (theo `CLAUDE.md`).

## Architecture

```
assets/themes/vscode-2026/
└── vscode-2026.json     { "$schema", "name": "VSCode 2026", "author", "themes": [dark, light] }
```

`crates/assets` đã có `#[include = "themes/**/*"]` ⇒ **chỉ cần đặt file vào đúng chỗ**, không phải
đăng ký thêm ở đâu cả.

## Related Code Files

**Tạo:** `assets/themes/vscode-2026/vscode-2026.json`
**Sửa:**
- `assets/settings/default.json:11-12` — `"light": "Light 2026"`, `"dark": "Dark 2026"`
- `crates/settings_content/src/theme.rs:272-273` — hai hằng số mặc định

**KHÔNG sửa:** `crates/theme/src/theme.rs:45` · `crates/theme/src/fallback_themes.rs` ·
`crates/theme_importer/**` · `assets/themes/one/**`

## Implementation Steps

1. Tạo `assets/themes/vscode-2026/vscode-2026.json`:
   ```jsonc
   {
     "$schema": "https://zed.dev/schema/themes/v0.2.0.json",
     "name": "VSCode 2026",
     "author": "Ported from Visual Studio Code default themes",
     "themes": [ /* dark-2026.json */, /* light-2026.json */ ]
   }
   ```
2. Sửa `assets/settings/default.json:11-12`. **Giữ nguyên `"mode": "system"`** và mọi comment quanh đó.
3. Sửa `crates/settings_content/src/theme.rs:272-273` sang `"Light 2026"` / `"Dark 2026"`.
4. Xác nhận `crates/theme/src/theme.rs:45` **không đổi** — `git diff` phải không đụng file này.
5. Chạy `./script/clippy`.
6. Khởi động zode, mở theme selector, xác nhận **bốn** theme: Dark 2026, Light 2026, One Dark, One Light.

## Todo List

- [ ] Tạo `assets/themes/vscode-2026/vscode-2026.json` với đủ hai theme
- [ ] Sửa `assets/settings/default.json:11-12`, giữ nguyên `mode` và comment
- [ ] Sửa `crates/settings_content/src/theme.rs:272-273`
- [ ] `git diff --stat` xác nhận **không** chạm `crates/theme/src/theme.rs`
- [ ] `./script/clippy` sạch
- [ ] zode khởi động, theme selector có đủ 4 theme, mặc định là Dark 2026
- [ ] Đổi tay sang One Dark rồi quay lại — xác nhận rollback được

## Success Criteria

- `./script/clippy` sạch, không warning mới
- zode khởi động vào Dark 2026 mà không cần sửa settings người dùng
- Theme selector liệt kê đủ 4 theme; One Dark/One Light chọn được và hiển thị đúng như trước
- `git diff` chỉ chạm **3 file**: một file mới + `default.json` + `settings_content/src/theme.rs`

## Risk Assessment

| Rủi ro | Mức | Đối phó |
|---|---|---|
| Sửa nhầm `theme.rs:45` ⇒ trùng tên theme trong registry, nhánh fallback trả về màu One Dark dưới tên Dark 2026 | **Cao** | Todo có mục `git diff` kiểm riêng; phần Key Insight giải thích đầy đủ lý do |
| `settings_content` là code upstream Zed ⇒ điểm conflict khi rebase | Trung bình | Chỉ 2 dòng hằng số; ghi vào `docs/project-changelog.md` để nhớ |
| Merge conflict trên `assets/settings/default.json` với 2 plan đang mở | Trung bình | Chỉ đụng `:11-12`; hai plan kia đụng key khác — xử lý ở mức git |
| Đổi mặc định là thay đổi thấy ngay với mọi người dùng | Trung bình | One Dark/One Light giữ nguyên ⇒ rollback tức thì bằng settings |

## Security Considerations

Không có bề mặt mới. `default.json` không thêm key mạng nào.

## Next Steps

Phase 05 — đối chiếu thị giác với VSCode.
