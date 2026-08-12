# Phase 02 — Nút rail + terminal mode ở center

**Context:** [plan.md](plan.md) · [brainstorm](../reports/brainstorm-260811-rail-agents-claude-code-codex.md)
**Priority:** P2 · **Status:** completed · **Effort:** 3-4d · **Blocked by:** 01

Phase đầu tiên **dùng được thật**: hai nút agent trên rail, click mở terminal chạy CLI ở center, đúng bên theo rail. Đây cũng là bài kiểm tra rẻ cho đường resolve của phase 01 trước khi phase 04 đổ 45k dòng vào.

## Key insights

- **Rail hôm nay chỉ vẽ được dock panel.** `rail_panels.rs:48-55` lấy entry từ `Panel::icon` của dock cùng phía; comment ở `:52-55` ghi rõ pane item "would each need wiring of their own". Phase này viết đúng đoạn wiring đó.
- **Rail phải dispatch action, không gọi thẳng.** `rail.rs:260-268` giải thích: thân `cx.listener` chạy trong `Sidebar::update`, chạm lại workspace là double-borrow. `window.dispatch_action` hoãn lại nên borrow đã nhả. Nút agent theo đúng khuôn này.
- **Action phải ở crate thứ ba.** `sidebar` và `agent_ui` đều depend `workspace`; cho `sidebar` depend `agent_ui` là mời vòng Cargo. `zed_actions` là nhà đã có sẵn cho action liên crate. `crates/sidebar/Cargo.toml` hiện **chưa** có `zed_actions` → phải thêm.
- **Bên của pane đọc cùng một setting với rail.** `rail_side()` (`rail.rs:12-14`) đọc `WorkspaceSettings::get_global(cx).multi_project.sidebar_side`. `agent_ui` đọc **chính setting đó**, không phải hằng riêng — cùng lý do `rail.rs:8-11` đã nêu: hai bên không được phép lệch nhau.
- `Workspace::split_item(SplitDirection, Box<dyn ItemHandle>, window, cx)` (`workspace.rs:4510`) là API cần. `SplitDirection::{Left,Right}` (`pane_group.rs:1015`). Map: rail `Left` → `SplitDirection::Left`, rail `Right` → `SplitDirection::Right`.
- `TerminalPanel::add_center_terminal` (`terminal_panel.rs:752`) là bản tham chiếu cho phần wiring — **đặc biệt** cái gate `is_enabled_in_workspace` (`:762`): terminal chưa hỗ trợ remote project. Agent terminal mode phải xử lý cùng trường hợp đó, không để panic hay im lặng.
- Chạy lệnh cụ thể (không phải shell trống): `Project::create_terminal_task(SpawnInTerminal, cx)` (`terminals.rs:63`).
- **Trong phase này click = Terminal**, vì chat UI chưa tồn tại. Phase 04 lật mặc định về Chat UI và đẩy Terminal vào menu chuột phải (quyết định #5).

## Requirements

**Functional**
1. Hai nút trên rail: Claude Code (`IconName::AiClaude`), Codex (`IconName::AiOpenAi`), nhóm riêng, có separator như `render_rail_panels` đang làm.
2. Tooltip mang tên agent; dùng `Tooltip::for_action` để keybinding hiện được.
3. Click → mở terminal chạy CLI của agent đó, cwd = project root, ở pane split đúng bên rail.
4. Chuột phải → menu: `Open as Terminal`, `New Session`. (Phase 04 thêm mục chọn mode.)
5. Agent đã mở, click lại nút đó → **focus** pane của nó, không mở thêm.
6. Mở agent thứ hai → split ngang thêm pane thứ hai (7b).
7. Đổi `sidebar_side` trong settings → pane agent mở lần sau sang đúng bên mới.
8. Remote project (terminal chưa hỗ trợ) → báo rõ ràng, không panic.

**Non-functional:** rail không được depend `agent_ui`; không thêm request mạng nào vào đường click.

## Architecture

```
crates/sidebar/src/rail_agents.rs        crates/zed_actions              crates/agent_ui
┌────────────────────────┐               ┌──────────────────┐            ┌──────────────────────┐
│ render_rail_agents()   │  dispatch →   │ OpenAgent {      │  handler → │ mở TerminalView      │
│  ▣ AiClaude            │               │   agent: AgentId │            │ split_item(dir, item)│
│  ▣ AiOpenAi            │               │   mode: Mode     │            │ dir ← sidebar_side   │
└────────────────────────┘               │ }                │            └──────────────────────┘
   ↑ hardcode 2 nút (#4)                 └──────────────────┘
```

Rail nằm dưới `render_rail_panels`, trên `render_rail_footer` — chèn vào `render_rail` (`rail.rs:124-125`).

## Related code files

**Tạo:**
- `crates/sidebar/src/rail_agents.rs` — hai nút + context menu, dispatch action
- `crates/agent_ui/` (khung tối thiểu) — crate mới, phase này chỉ có handler mở terminal; phase 04 đổ phần chat vào

**Sửa:**
- `crates/sidebar/src/rail.rs` — gọi `render_rail_agents` trong `render_rail`
- `crates/sidebar/src/sidebar.rs` — khai module mới
- `crates/sidebar/Cargo.toml` — thêm `zed_actions`
- `crates/zed_actions/src/…` — action `OpenAgent { agent, mode }`
- `crates/zed/…` — đăng ký `agent_ui::init`

**Xoá:** không

## Implementation steps

1. Action `OpenAgent { agent: SharedString, mode: AgentViewMode }` trong `zed_actions`; `AgentViewMode { Terminal, Chat }` — `Chat` khai từ bây giờ nhưng phase này trả lỗi "chưa hỗ trợ".
2. Khung crate `agent_ui` + `agent_ui::init(cx)` đăng ký handler `OpenAgent` trên `Workspace`.
3. Handler: resolve binary qua phase 01 → `SpawnInTerminal` (command = binary, cwd = project root) → `create_terminal_task` → `TerminalView::new` → `split_item(dir, Box::new(view))`, `dir` suy từ `sidebar_side`.
4. Sổ theo dõi pane đang mở theo `AgentId` để yêu cầu 5 (click lại = focus) đúng. Dùng `WeakEntity` để pane đóng không giữ entity sống.
5. `rail_agents.rs`: hai `IconButton`, `RAIL_ICON_SIZE` + `RAIL_ICON_GAP` dùng lại từ `rail.rs` (đừng đặt hằng mới), separator giống `render_rail_panels`.
6. Context menu chuột phải — theo khuôn `crates/sidebar/src/context_menu.rs` đang có.
7. Gate remote project theo `is_enabled_in_workspace` (`terminal_panel.rs:762`); thiếu hỗ trợ thì báo bằng toast, không im lặng.
8. Test: đổi `sidebar_side` → khẳng định `SplitDirection` tính ra đổi theo. Test này không cần vẽ.
9. Test vẽ thật: dock panel + rail có nút agent → `run_until_parked` + `window.refresh()`, theo đúng khuôn `rail_draws_with_panels_registered` (`rail_panels.rs:209`) — bài test này từng bắt được đúng bẫy re-entrancy của rail.

## Todo

- [x] Action `OpenAgent` trong `zed_actions`, `sidebar` thêm dep
- [x] Crate `agent_ui` + `init`, đăng ký ở **cả hai** chỗ init (`main.rs`, `zed.rs`)
- [x] Hai nút rail, icon đúng, tooltip có action
- [x] Click → terminal chạy CLI ở pane đúng bên
- [x] Click lại nút đang mở → focus, không nhân bản
- [x] Agent thứ hai → pane thứ hai (split ngang)
- [ ] ~~Menu chuột phải: `Open as Terminal`, `New Session`~~ — **hoãn sang phase 04, có chủ ý.** Xem điều #2 dưới
- [x] Remote project → báo rõ, không panic (`State::Failed` mang thông điệp của lỗi)
- [x] Test hướng split theo `sidebar_side` — `the_agent_pane_opens_on_the_rail_side`
- [x] Test vẽ rail có nút agent — `rail_draws_with_agent_buttons`
- [x] Thêm: `every_rail_agent_is_a_registered_builtin` — nút rail không thể trỏ vào agent không tồn tại
- [ ] **Còn nợ mắt người:** chưa mở app bấm thử. Compile + test xanh không thay được việc nhìn.

## Kết quả (2026-08-12)

`./script/clippy -p project -p agent_ui -p sidebar -p zed_actions -p zode` exit 0 · **16/16** test `sidebar`, **2/2** `agent_ui`, **13/13** `agent_server_store` · `cargo check -p zode` xanh.

| File | Δ |
|---|---|
| `crates/agent_ui/` (mới) | `agent_ui.rs` 17 · `agent_view.rs` ~290 · `Cargo.toml` |
| `crates/sidebar/src/rail_agents.rs` (mới) | ~100 |
| `crates/sidebar/src/rail.rs` | +7 / −7 |
| `crates/zed_actions/src/lib.rs` | +30 / −1 |
| `crates/zed/src/{main,zed}.rs`, `Cargo.toml` × 3 | +1 mỗi file |

## Ba điều phase này dạy lại cho plan

**1. `AgentView` phải là `Item` ngay từ phase này, không phải đợi phase 04.**
Plan viết phase 02 mở thẳng `TerminalView` vào pane, phase 04 mới dựng `AgentView`. Nhưng yêu cầu 5 (click lại = focus) cần biết *pane nào là của agent nào* — với `TerminalView` trần thì không có chỗ nào giữ `AgentId`, phải dựng sổ theo dõi riêng rồi phase 04 vứt đi. Đã làm: `AgentView` là `Item` ngay, ôm `TerminalView` bên trong; yêu cầu 5 rút về một dòng `items_of_type::<AgentView>().find(...)`. Phase 04 chỉ việc thêm nhánh `State::Chat` vào cùng struct, **không** phải thay item.

**2. Menu chuột phải chưa đáng làm ở phase này.**
Quyết định #5 mô tả trạng thái cuối: click = Chat, chuột phải = menu chứa Terminal. Ở phase 02 chat chưa tồn tại, nên menu sẽ có đúng một mục trùng với hành vi click và một mục (`New Session`) chưa có khái niệm session để nhân bản. Một menu gồm một mục thừa và một mục hỏng tệ hơn không có menu. **Dời trọn sang phase 04**, nơi nó lật mặc định sang Chat *và* dựng menu cùng lúc — một lần đổi, không hai.

**3. `zed_actions` đã có sẵn `pub mod agent`, và `render_rail_footer` có một tham số vừa chết.**
`pub mod agent` sót lại từ upstream (crate `zed_actions` không bị hard-fork đụng tới) — khai thêm module trùng tên là lỗi biên dịch, phải gộp vào. Còn `render_rail_footer(follows_panels)` vẽ đường kẻ trên **có điều kiện**; khối agent luôn được vẽ nên điều kiện đó vĩnh viễn đúng. Đã bỏ tham số thay vì truyền `true` — giữ lại là để một nhánh không bao giờ chạy nằm trong code.

## Success criteria

Mở app, bấm icon Claude trên rail → terminal chạy `claude` hiện ra **cạnh** editor, đúng bên với rail; đổi `sidebar_side` rồi mở lại → đổi bên. Bấm Codex → pane thứ hai. `./script/clippy` xanh, test `sidebar` xanh.

## Risks

| # | Rủi ro | Đối phó |
|---|---|---|
| — | Double-borrow khi rail chạm workspace | Dispatch action, không gọi trực tiếp — `rail.rs:260-268` đã ghi lý do |
| — | Vòng phụ thuộc `sidebar` → `agent_ui` | Action ở `zed_actions`; kiểm bằng `cargo tree` chứ không bằng niềm tin |
| R4 | Pane thứ hai bóp editor | Min-width cho pane agent ngay từ phase này, đừng để sang 06 |
| — | `codex` chưa cài trên máy dev | Đúng nguyên liệu cho phase 03; phase này chỉ cần lỗi có kiểu nổi lên tới UI |
