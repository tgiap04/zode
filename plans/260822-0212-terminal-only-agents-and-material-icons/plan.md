# Agent chỉ còn terminal, icon là Material và khoá cứng

**Status:** ✅ done (2026-08-22), chờ commit · **Priority:** P2 · **Branch:** feat.release-v0.1.1

> Bản ghi này viết **sau** khi forge xong, không phải trước. Người dùng yêu cầu implement
> ngay và làm song song nếu được; blueprint không đi trước code ở lần này, nên đây là bản
> ghi lại quyết định thật, không phải kế hoạch đã được phê duyệt.

## Yêu cầu

Hai việc trong một lượt:

1. Bỏ tính năng hiển thị UI cho agent — chỉ dùng terminal, **không cho chuyển sang dạng UI**.
2. Thay bộ icon project panel bằng Material Icon Theme (giống VS Code), **không sửa được**,
   là mặc định luôn.

Hai lựa chọn người dùng chốt qua `AskUserQuestion`, cả hai đều là phương án tối đa:

| Câu | Chốt | Kéo theo |
|---|---|---|
| Chat: khoá lại hay xoá? | **Xoá hản code** | 4 crate biến mất, không còn đường quay lại |
| Icon: giữ màu hay đơn sắc? | **Giữ màu** | phải đổi đường render, `svg()` là mặt nạ đơn sắc |

## Track A — xoá chat, agent là một phiên terminal

Terminal mode **chưa bao giờ** đi qua ACP: nó là PTY thuần
(`project::AgentServerStore` → `project.create_terminal_task`). Nên cả tầng Agent Client
Protocol nằm dưới chat là thứ chỉ chat dùng, và xoá được sạch.

```
crates/agent_ui       26,503 → 2,270 dòng   (còn actions.rs, agent_ui.rs, agent_view.rs, missing_binary.rs)
crates/acp_thread      8,009 → xoá hẳn
crates/agent_servers   4,199 → xoá hẳn
crates/acp_tools         902 → xoá hẳn
crates/agent_settings    153 → xoá hẳn
```

`AgentViewMode` mất biến thể `Chat`; `mode_from_name` nhánh mặc định trả `Terminal`.
`agent_view.rs` mất `State::Chat`, `start_chat`, `conversation_view()`, `render_mode_switch`,
hàng header và `ensure_prompt_store`. Workspace `Cargo.toml` mất 4 member, 4 path dep và
`agent-client-protocol`. `agent_ui/Cargo.toml` mất 41 dependency.

### Hai lần researcher chỉ sai đường, và bắt được vì đi kiểm chứ không đi theo

**`settings_content/src/agent.rs` phải giữ.** Report nói xoá. Nhưng
`project::agent_server_store` cần `AllAgentServersSettings` và `CustomAgentServerSettings`
cho **terminal mode** — xoá file đó là giết đúng tính năng đang được giữ lại.

**Bỏ block `agent` trong `default.json` làm đỏ một test.**
`settings::the_agent_does_not_dock_beside_the_panel_that_starts_open` — một test mà **tiền
đề của nó (agent dock cạnh panel) đã chết hai feature trước**, và nó xanh suốt từ đó chỉ vì
settings key còn tồn tại. Xoá test, không phải xoá key.

### Tôi ước lượng sai gấp 3 lần, và đã nói ra trước khi làm

Tôi báo ~13.000 dòng; thật là ~37.900. Nguyên nhân: `wc -l crates/agent_ui/src/*.rs` **không
đệ quy vào `ui/`**. Glob một tầng trên một cây nhiều tầng là một cách đếm sai đủ tự tin để
không ai kiểm lại.

## Track B — Material Icon Theme, có màu, khoá ở registry

1126 SVG dưới `assets/icons/file_icons/material/` (kèm LICENSE MIT nguyên văn, Material
commit `e355348dd69ad2b7fed0c394af54f1e80c5e36e0`), bảng sinh ra trong
`crates/theme/src/icon_theme_material.rs` (5.985 dòng).

Bảng: 1377 suffix · 2135 stem · 587 khoá file-icon · 4654 named-directory — toàn chữ
thường, **0 collision**.

### Đổi đường render hoá ra là ~6 dòng, không phải rủi ro trên 5 chỗ

Tôi cảnh báo người dùng rằng giữ màu nghĩa là đổi renderer ở nhiều nơi. Sai.
`Icon::from_path` **vốn đã** phân nhánh embedded→`svg()` / external→`img()`, và `img()`
**vốn đã** xử lý `Resource::Embedded`. Nên chỉ cần thêm `IconSource::EmbeddedColor` trong
`crates/ui/src/components/icon.rs` để embedded đi qua `img()` (usvg/resvg, màu đủ).
**0 trên 32 chỗ render** phải sửa. Tôi đã nói lại chỗ này với người dùng.

### Khoá ở registry, không mổ settings — và có lý do

Đường resolve icon theme nằm **cùng hàm** với theme màu (`theme_settings/src/settings.rs`),
mà theme màu thì người dùng vẫn đổi được và đang dùng. Rút nửa icon ra khỏi những hàm đó là
đặt một feature đang chạy vào rủi ro để xoá một feature đã chết. Nên khoá đặt ở registry:

```rust
pub fn get_icon_theme(&self, _name: &str) -> Result<Arc<IconTheme>, IconThemeNotFoundError> {
    self.state.read().icon_themes
        .get(DEFAULT_ICON_THEME_NAME)
        .ok_or_else(|| IconThemeNotFoundError(DEFAULT_ICON_THEME_NAME.into()))
        .cloned()
}
```

Settings, extension, picker — mọi đường resolve về một bộ. `list_icon_themes()` trả đúng
một phần tử, nên **icon theme selector bị xoá hẳn** (495 dòng) cùng action, entry trong app
menu và prompt trong extensions UI: một picker liệt kê một dòng là control không cho gì cả.

### Tôi rơi vào đúng cái bẫy tôi tự ghi vào memory hôm nay

Bỏ `"icon_theme"` khỏi `assets/settings/default.json` làm **332 test đỏ** cùng lúc
(`workspace` 191, `project_panel` 95, `sidebar` 18, `agent_ui` 14, `settings_ui` 14).
`from_settings` unwrap không điều kiện → **thiếu key là panic lúc khởi động, không phải
fallback**. Key phục hồi, ghim `"Material Icon Theme"`, và nó trơ vì khoá nằm ở registry.

### Một lỗi agent tìm ra mà tôi không brief — và tôi sửa ở chỗ khác

`fileNames` của Material gần như toàn chữ thường vì VS Code match **case-insensitive**;
zode tra `HashMap` đúng hoa thường. Nên `Dockerfile`, `Makefile`, `LICENSE`, `README` sẽ
**âm thầm mất icon**. Agent bù bằng cách đăng ký thêm biến thể hoa (2135 → 5641 entry).

Hai cơ chế cho một vấn đề, và biến thể nào không ai nghĩ ra thì vẫn sai. Tôi sửa ở **chỗ
tra** — `icon_key_for` lowercase một lần — rồi cho sinh lại bảng chỉ chữ thường. Và nó sửa
luôn một lỗi **có sẵn**: `.PNG`, `README.MD` hôm nay cũng mất icon. `get_folder_icon` cùng
lớp lỗi, sửa cùng.

`file_icons` từ **0 lên 5 test**, canh đúng luật đó, gồm cả trường hợp stem thắng suffix.

## File chính

- `crates/agent_ui/` — 4 file còn lại; 17 file + thư mục `ui/` xoá
- `crates/{acp_thread,agent_servers,acp_tools,agent_settings}/` — xoá
- `crates/theme/src/icon_theme_material.rs` (mới) · `icon_theme.rs` (viết lại) · `registry.rs`
- `crates/file_icons/src/file_icons.rs` — `icon_key_for` + 5 test
- `crates/ui/src/components/icon.rs` — `IconSource::EmbeddedColor`
- `crates/theme_selector/src/icon_theme_selector.rs` — xoá; `theme_selector.rs`, `app_menus.rs`,
  `zed_actions/src/lib.rs`, `extensions_ui.rs` — bỏ tham chiếu
- `assets/icons/file_icons/material/` — 1126 SVG + LICENSE
- `plans/reports/gen-icon-theme-material.py` — script sinh bảng

Tổng: **66 file, +519 / −39.409**.

Suite xanh: `zode` 56 · `workspace` 223 · `project_panel` 97 · `ui` 38 · `settings` 31 ·
`sidebar` 20 · `agent_ui` 18 · `settings_ui` 15 · `theme` 9 · `file_icons` 5 ·
`theme_selector` 1 · `extensions_ui` 1. `cargo check --workspace --all-targets` 0 lỗi 0
warning. `cargo build --bin zode` exit 0.

## Giới hạn — nói ra thay vì để phát hiện sau

- **Rasterize SVG theo DPI tĩnh.** `img()` rasterize ở kích thước yêu cầu; ở
  `IconSize::XLarge` sẽ mềm hơn `svg()`. Hiện **không chỗ nào** dùng file icon ở size đó.
- **Chỉ ship biến thể Dark.** Material có bộ light; dùng được nó cần sửa `theme_settings` +
  `settings_content` để chọn theo appearance. Ngoài phạm vi yêu cầu.
- **Asset nặng lên: 392 KB → 4.5 MB.** Đây là giá của "giống VS Code".
- **Extension không còn cấp được icon theme.** Cài vào sẽ không có tác dụng, im lặng.
- Tôi không chạy được app, nên diện mạo icon thật trên project panel là **mắt người dùng**
  mới xác nhận được.

## Định nghĩa xong

Mở app: project panel hiện icon Material **có màu**, folder có icon riêng theo tên. Không
còn mục chọn icon theme trong menu hay command palette. Agent mở ra là terminal, không có
chỗ nào chuyển sang chat. Theme màu vẫn đổi được như thường.
