# Phase 04 — `DatabasePanel` + nút rail; cột hiện ra, còn rỗng

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-15) · **Blocked by:** 01, 02
**File ownership:** `crates/database_ui/**` (mới) · `crates/sidebar/src/rail_database.rs` (mới) + `sidebar.rs` (dòng `mod`) · `crates/zed/src/zed.rs` (init) · `crates/zed_actions` (action)

Bước vô hình thứ hai: bấm nút trên rail, cột database mở ra, bên trong là trạng thái rỗng. Chưa có driver nào được gọi.

## Vì sao lại tách một bước rỗng nữa

Panel vào đúng cột, nút rail đúng chỗ, layout đúng bên, đóng-mở app nhớ đúng — bốn thứ đó dính vào `workspace`/`sidebar`, hoàn toàn khác họ với việc nói chuyện với DB. Gộp vào phase 05 thì một lỗi layout và một lỗi protocol trông giống hệt nhau từ phía người dùng.

## Nút rail phải viết tay — và vì sao

`rail_panels.rs:15-23` `rail_dock()` chỉ đọc `left_dock`/`right_dock`. Cột database không nằm trong đó, nên `rail_draws_panel` không bao giờ thấy nó. Mẫu đúng là `rail_agents.rs`: nút hard-code, dispatch action.

- [ ] `crates/sidebar/src/rail_database.rs` — `render_rail_database`, mượn hình dạng `render_rail_agents`
- [ ] Chèn vào `render_rail` (`rail.rs:132-134`) giữa khối panel và khối agent
- [ ] `toggle_state` = cột database đang mở
- [ ] `on_click` **dispatch action**, không gọi thẳng workspace — thân `cx.listener` chạy trong `Sidebar::update` và mở panel với tay vào workspace. Đây là bẫy đã làm crash rail một lần (`rail_panels.rs:113-119`)
- [ ] `DatabasePanel::icon()` trả **`None`**, y như `AgentPanel` (`agent_panel.rs:770`) — nếu không sẽ mọc thêm một nút thừa ở status bar cho cùng một thứ

## Việc

- [ ] `crates/database_ui/` — `[lib] path = "src/database_ui.rs"`, mỗi file dưới 200 dòng
- [ ] `DatabasePanel` impl `Panel`: `own_column()` → `Some(DockColumn::Database)` (từ phase 01), `persistent_name()` = `"DatabasePanel"`, `position` theo bên rail
- [ ] Action `zode_actions::database::ToggleDatabase`
- [ ] Đăng ký panel trong `zed.rs` cạnh chỗ các panel khác được thêm
- [ ] Trạng thái rỗng: "Chưa có kết nối nào" + nút "Thêm kết nối" (nút chưa làm gì, phase 05 nối vào)
- [ ] Icon: dùng `IconName::DatabaseZap` sẵn có, hoặc thêm `assets/icons/database.svg` trơn nếu glyph tia chớp gây hiểu nhầm

## Test

- [ ] Nút database hiện trên rail; bấm → cột mở; bấm lại → đóng
- [ ] Cột đứng **giữa** dock bên rail và center, ở cả hai giá trị `sidebar_side`
- [ ] Panel vào `database_dock`, **không** vào `left_dock` (khẳng định thẳng vào entity, không qua element tree)
- [ ] Rail vẽ được với panel đã đăng ký — đây là bẫy re-entrancy đã làm crash rail; chỉ lộ ra khi **vẽ thật**, dựng element bằng tay không thấy (mẫu: `rail_panels.rs:265-292`)
- [ ] `DatabasePanel::icon()` là `None` → status bar không mọc nút thứ hai
- [ ] Đóng-mở workspace: cột mở vẫn mở, bề ngang giữ nguyên

## Định nghĩa xong

Bấm nút 🗄️ trên rail → một cột rỗng mở ra đúng chỗ trong hình ở plan.md, bám đúng bên rail, nhớ trạng thái qua lần mở app sau. `cargo test -p database_ui -p sidebar -p workspace` xanh.

## Rủi ro

- **Bẫy re-entrancy của rail.** `rail_panels.rs:113-119` và `rail_agents.rs:89-91` đều chép lại cùng một lời cảnh báo vì nó đã cắn một lần. Test phải `cx.update(|window,_| window.refresh())` rồi `run_until_parked()`, không chỉ dựng element.
- Panel rỗng có nên tự đóng như `AgentPanel` không? **Không** — cột database rỗng là trạng thái hợp lệ (chưa thêm connection). Quyết định này phải nằm trong code dưới dạng comment, vì `Panel::set_active` của agent làm ngược lại.

---

## Xong (2026-08-15)

`cargo check -p zode` sạch (toàn bộ app build) · `database_ui` 5 xanh · `sidebar` 20 xanh (18 cũ + 2 mới) · clippy sạch.

### Lệch khỏi plan, có lý do

**Test vẽ rail nằm ở crate `sidebar`, không ở `database_ui`.** Plan đặt nó trong `database_ui`. Thử
rồi: thêm `sidebar` làm dev-dependency của `database_ui` kéo theo `agent_ui` và một tổ hợp feature
của `remote` **không build được** (`RemoteConnectionOptions::Mock` không được cover trong
`remote_connection.rs:234` — lỗi có sẵn, chỉ lộ ra ở tổ hợp feature đó).

Hoá ra không cần: **nút rail không phụ thuộc `database_ui` chút nào** — nó chỉ dùng
`workspace::dock::DockColumn` và `zed_actions`. Nên test nó bằng `TestPanel::new_database` ngay
trong `sidebar`, không crate nào phải phụ thuộc crate nào. Đây là dấu hiệu tốt về ranh giới: rail
biết "có cột database", không biết "database là gì".

**`database_ui` không phụ thuộc `sidebar`, và `sidebar` không phụ thuộc `database_ui`.**

### Test mới, và chúng bắt được gì

| Test | Ở đâu | Bắt được |
|---|---|---|
| `the_panel_lands_in_the_database_column` | `database_ui` | Định tuyến qua `own_column()`; và `all_docks()` với tới cột mới (`workspace.panel::<T>()` tìm thấy) |
| `the_panel_contributes_no_generic_button` | `database_ui` | `icon()` là `None` → status bar không mọc nút thứ hai cùng nghĩa với nút rail |
| `the_column_draws_with_the_panel_in_it` | `database_ui` | `render_centre_with_own_columns` + `measure_own_column` chạy trên panel thật, không chỉ `TestPanel` |
| `the_column_follows_the_rail` | `database_ui` | Đổi `sidebar_side` → cột đổi bên, panel không bị lôi vào tool dock |
| `an_empty_column_stays_open` | `database_ui` | Cột rỗng **không** tự đóng — khác `AgentPanel` cố ý |
| `the_rail_draws_with_the_database_column_open` | `sidebar` | Bẫy re-entrancy: `database_column_open` với tay qua workspace trong lúc `Sidebar::render` |
| `the_button_reads_the_columns_own_state` | `sidebar` | Nút sáng theo `is_open` **của cột**, không theo panel → hai thứ không thể bất đồng |

### Chưa làm, cố ý

Nút "Add Connection" chưa làm gì — phase 05 nối vào cùng lúc với cái store có chỗ để chứa câu trả lời.
