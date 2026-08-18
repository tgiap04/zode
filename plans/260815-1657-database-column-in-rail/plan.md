---
title: 'Database: nút trên rail, một cột riêng cạnh editor, driver là sidecar'
status: completed
priority: P2
branch: feat/database-column
blockedBy: []
blocks: []
work_type: feature
spec_waived: 'SDD mode disabled (takumi.sddMode: off)'
---

# Database: nút trên rail, một cột riêng cạnh editor

**Status:** ✅ done (2026-08-15) · **Priority:** P2 · **Branch:** `feat/database-column`
**Bắt nguồn từ:** [brainstorm-260815-database-panel.md](../reports/brainstorm-260815-database-panel.md) — 9 quyết định đã chốt

## Hình dạng

```
┌────┬─────────┬──────────────┬──────────┐
│rail│ Git    │ ▾ localhost  │          │
│ 📁 │ panel  │   ▾ public   │  editor  │
│ 🔍 │        │     users    │  .rs     │
│ 🗄️ │        │───SQL───────│          │
│ 🤖 │        │ select * ..  │          │
│    │        │──────────────│          │
│    │        │ id│email     │          │
└────┴─────────┴──────────────┴──────────┘
      tool dock   DATABASE      center
```

Cột database luôn nằm **giữa dock bên rail và editor**, bám bên của rail — đúng chỗ cột agent đứng, vì đó là chỗ duy nhất một cột không tranh chiều dọc với panel nào.

```
DatabasePanel ──JSON-RPC/stdio──> driver sidecar ──sqlx──> DB
     │
     ├─ ui::Table            (grid ảo hoá, đã có, 1.091 dòng)
     ├─ Editor               (SQL scratch, đã có)
     ├─ credentials_provider (keychain, đã có)
     └─ StdioTransport       (JSON-RPC, đã có trong context_server)
```

## Phase

| # | Việc | Status | Blocked by |
|---|---|---|---|
| 01 | [Dock biết khái niệm "cột riêng"; `database_dock` rỗng](phase-01-dock-learns-own-column.md) | ✅ | — |
| 02 | [crate `database`: protocol + client stdio](phase-02-protocol-and-driver-client.md) | ✅ | — |
| 03 | [Driver SQLite](phase-03-sqlite-driver.md) | ✅ | 02 |
| 04 | [`DatabasePanel` + nút rail — cột hiện ra, còn rỗng](phase-04-panel-and-rail-button.md) | ✅ | 01, 02 |
| 05 | [ConnectionStore + cây connection/schema](phase-05-connections-and-schema-tree.md) | ✅ | 03, 04 |
| 06 | [SQL scratch + result grid](phase-06-sql-scratch-and-grid.md) — **hết lát cắt dọc SQLite** | ✅ | 05 |
| 07 | [Driver PostgreSQL → **đóng băng spec**](phase-07-postgres-driver-freezes-spec.md) | ✅ | 06 |
| 08 | [Extension khai báo driver + driver MySQL](phase-08-extension-drivers-and-mysql.md) | ✅ | 07 |
| 09 | [Trả nốt bốn khoản nợ](phase-09-finish-the-feature.md) — driver extension chạy được, mật khẩu vào keychain, test chạy driver thật, tay kéo | ✅ | 08 |
| 10 | [Thêm kết nối bằng hộp thoại](phase-10-add-connection-dialog.md) — nút `+`, driver tự khai các ô của nó | ✅ | 09 |
| 11 | [Hộp thoại theo mẫu TablePro](phase-11-connection-dialog-ui.md) — danh mục engine + search + Not Installed, Test Connection, Save & Connect | ✅ | 10 |
| 12 | [Toàn màn hình + ngắt kết nối](phase-12-full-screen-and-disconnect.md) — `fills_the_window`, nút Power | ✅ | 11 |

**01 và 02 chạy song song được** — file ownership rời hẳn nhau (`workspace` vs crate mới). Từ 03 trở đi là chuỗi.

Phase 01 và 04 cố tình **không cho người dùng thấy dữ liệu nào**: 01 dựng cột rỗng, 04 dựng panel rỗng. Mọi rủi ro của 05–06 đứng sau hai bước đã xanh — đúng cách [cột agent](../260814-2318-agent-gets-its-own-column/plan.md) vừa được xây hôm nay.

## Điều đã khảo sát (không phải phỏng đoán)

| Dữ kiện | Ý nghĩa |
|---|---|
| `workspace.rs:2204` `all_docks()` trả **4**, đã có `agent_dock` (`:1357`, `:1709`) | Mẫu "cột riêng cạnh center bám bên rail" đã chạy. Không phát minh lại |
| `dock.rs:304` `is_agent_column: **bool**`; `:1111` `mark_as_agent_column()`; `:1123`/`:1138` layout rẽ nhánh theo nó | Cờ boolean, không phải kind. Cột thứ hai buộc phải **tổng quát hoá** nó — đây là toàn bộ nội dung phase 01 |
| `workspace.rs:2638` `add_panel` định tuyến bằng `panel.is_agent_panel(cx)` | Điểm rẽ phải thành "panel này muốn cột riêng nào", không phải thêm một `if` thứ hai |
| `rail_panels.rs:15-23` `rail_dock()` chỉ đọc `left_dock`/`right_dock` | Panel ở cột riêng **không** được nút rail miễn phí. Phải viết tay ~30 dòng, mẫu có sẵn ở `rail_agents.rs` |
| `agent_panel.rs:770` `Panel::icon()` trả `None` **có chủ ý** | `DatabasePanel` làm y hệt, nếu không sẽ mọc nút thừa ở status bar |
| `dock.rs:1492` `render_stack()` → `pane_axis` | Left dock đã xếp chồng nhiều panel. Đây là lý do database **không** vào đó |
| `context_server/src/client.rs:169` `Client::stdio(id, binary, cwd, cx)` + `transport/stdio_transport.rs` | Tầng JSON-RPC-over-stdio đã chạy production. Đây là thứ được sao chép, không viết mới |
| `extension_manifest.rs:118` `debug_adapters: BTreeMap<Arc<str>, DebugAdapterManifestEntry>` (`:355`, chỉ có `schema_path`) | `database_drivers` là entry thứ sáu cùng khuôn, entry struct nhỏ |
| `ui/src/components/data_table.rs` — `Table::uniform_list`, `ColumnWidthConfig::redistributable`, `TableInteractionState` | Result grid ảo hoá đã có. Ví dụ dùng thật: `repl/src/outputs.rs` |
| `credentials_provider.rs:11` `trait CredentialsProvider` — `read/write/delete_credentials(url, …)` | Khoá theo URL → dùng chính connection URL làm key |
| `libsqlite3-sys` bundled đã có trong workspace (cho `sqlez`) | Driver SQLite gần như không thêm chi phí build |
| `icons.rs:61` `DatabaseZap` (`database_zap.svg`) | Có icon dùng ngay; `database.svg` trơn là việc phụ của phase 04 |

## Rủi ro nền

- **`dock.rs`/`workspace.rs` là code dùng chung với upstream Zed.** Mọi diff ở đó làm merge upstream khó hơn. Phase 01 phải giữ diff nhỏ và có hình dạng dễ đọc — tổng quát hoá một cờ sẵn có, không thêm khái niệm song song.
- **`all_docks()` lên 5 là đổi hành vi diện rộng**: `focus_or_unfocus_panel`, `panel::<T>()`, `close_panel`, `capture_dock_state`, serialize đều lặp qua nó. Phase 01 phải quyết dứt khoát cái nào tính cột database và có test cho từng cái.
- **Bịa protocol mù.** Ba driver là số tối thiểu để spec không rò rỉ chi tiết một engine, nhưng cũng là ba lần trả giá mỗi lần đổi spec. Thứ tự là ràng buộc cứng: SQLite → Postgres → **đóng băng** → MySQL là bài *kiểm tra* spec, không phải nơi sửa spec.
- **Kết quả lớn qua stdio.** Không bao giờ kéo cả bảng: `LIMIT/OFFSET` phía server, mặc định 200 dòng/trang.
- **Query treo.** `cancel` trong protocol, timeout mặc định mỗi query, và kill process là biện pháp cuối.
- **"Read-only" cưỡng chế ở driver, không ở UI.** Zode không đoán câu lệnh; DB tự từ chối.

## Định nghĩa xong

Bấm nút database trên rail → cột database mở ra giữa git panel và editor, bám đúng bên của rail. Kết nối một file SQLite trong project, duyệt tới bảng, thấy dữ liệu phân trang. Gõ `select` vào scratch buffer thì chạy, gõ `delete` thì **chính DB** từ chối. Đổi sang một Postgres thật: cùng luồng đó chạy mà `database_ui` không đổi một dòng. Giết driver giữa chừng thì editor sống và cột báo lỗi tử tế. Đóng app mở lại vẫn đúng layout và đúng connection đang mở.
