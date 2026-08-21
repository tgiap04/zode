# Agent về lại thanh tab của editor

**Status:** ✅ done (2026-08-21) · **Priority:** P1 · **Branch:** feat.release-v0.1.1

## Vấn đề

Agent và code editor đang là **hai cột**. Yêu cầu: một thanh tab chung — bấm agent thì
agent thành một tab của editor, bấm mở file thì file thành tab kế bên.

## Đây là một revert có chủ đích

`c056596` — *"move the agent out of the editor's tabs into its own dock"* — đã dời agent
đi, và nói rõ lý do:

> `Workspace::open_path` … falls back to the active one, so every way of opening a file —
> the project panel, the file finder, go-to-definition — put an editor tab **inside the
> agent** whenever the agent had focus.

Điều bị coi là **lỗi** lúc đó chính là điều **được yêu cầu** bây giờ. Nên đây không phải
tính năng mới: phần lớn việc là dựng lại thứ commit đó đã xoá, và lần này nó là hợp đồng
chứ không phải rò rỉ.

Ba thứ còn sống sót giúp đường về ngắn hơn nhiều so với ước lượng:

- `AgentView` **vẫn** implement `workspace::item::Item` (`agent_view.rs:685`) — tab label,
  icon, rename đều đã chạy. Nó vốn đã là một tab hợp lệ.
- Bảng `agent_views(workspace_id, item_id, agent, mode)` **vẫn còn** trong DB: migration là
  append-only nên `c056596` không xoá được nó.
- `impl SerializableItem for AgentView` lấy lại nguyên văn từ `c056596^`, không phải viết lại.

## Quyết định đã chốt

| Câu hỏi | Chốt |
|---------|------|
| Cột agent | **Bỏ hẳn.** Muốn agent cạnh code thì split center pane — split do người dùng điều khiển, và có chỗ rộng thật |
| Restore | **Đầy đủ qua `SerializableItem`.** Tab agent về đúng vị trí cũ *giữa* các tab editor |
| Rail toggle | **Về lại item vừa xem** trong cùng pane. Cất đi, không đóng — không mất session |

## Phase

| # | Việc | Người dùng thấy gì |
|---|------|--------------------|
| [01](phase-01-agent-remembers-itself-again.md) ✅ | `SerializableItem` + migration cột `name` | **Không gì cả** — cố ý |
| [02](phase-02-agent-opens-as-an-editor-tab.md) ✅ | `open_inner` → center pane, rail đọc panes | Agent thành tab của editor |
| [03](phase-03-the-rail-button-puts-it-away.md) ✅ | `ToggleAgent` → `Pane::activation_history` | Bấm lần nữa về lại tab code |
| [04](phase-04-the-plus-menu-starts-an-agent.md) ✅ | "New Agent" vào menu `+` mặc định | Mở session thứ hai từ tab bar editor |
| [05](phase-05-the-column-comes-down.md) ✅ | Xoá `AgentPanel`, `agent_dock`, `DockColumn::Agent` | Cột agent biến mất |
| [06](phase-06-finish-the-feature.md) ✅ | Full suite + docs | — |

Phase 01 vô hình có chủ đích: impl mới nằm đó không ai dùng cho tới 02, nên toàn bộ rủi ro
DB/migration đứng sau một bước đã xanh. Phase 04 đứng **trước** 05 để không có cửa sổ nào
mà "mở session thứ hai" không còn đường nào tới.

## Chỗ phải cẩn thận

- **Bẫy re-entrancy, crate này đã trả giá ba lần** (`ccd151f`, `Panel::position`,
  `register_action`). `open_inner` hiện đã đi qua `cx.spawn_in` rồi `update_in` — **giữ
  nguyên hình dạng đó**, đừng gọi thẳng vào workspace từ trong handler.
- **`should_serialize` cũ trả `false`** — đó chính là lý do tên tự đặt chưa bao giờ sống
  qua restart. Phải trả `true` cho `UpdateTab`, nếu không cột `name` mới thêm là vô nghĩa.
- **`OWN_COLUMN_ORDER` và `TestPanel::new_agent`**: workspace test dùng *cột agent* làm
  cỗ xe thử nghiệm cho own-column nói chung. Bỏ `DockColumn::Agent` là phải chuyển chúng
  sang `DockColumn::Database` — cột own duy nhất còn lại.
- **`fills_the_center` chỉ có agent dùng**; `fills_the_window` là của database. Bỏ agent thì
  method thứ nhất thành code chết, xoá luôn — nhưng đừng đụng cái thứ hai.
- **Mất tính năng, phải nói thẳng:** hai agent cạnh nhau hiện là tự động (tới 3 pane).
  Sau đây người dùng phải tự split. `MAX_SIDE_BY_SIDE_AGENTS` vốn là cách chống đỡ cho một
  cột hẹp; center split thì rộng thật, nên đây là đổi chứ không hẳn là mất.
- **Hệ quả đã nhận:** agent giờ nằm trong `workspace.panes()`, nên "Close All Items" đóng
  luôn nó, và file mở khi agent đang focus vào cùng pane. Đó là hợp đồng, không phải sót.

## Định nghĩa xong

Một thanh tab: `main.rs │ Claude Code │ README.md`. Bấm rail → agent thành tab hiện hành;
bấm nữa → về `main.rs`. Mở file khi đang ở tab agent → file thành tab mới cạnh nó. Đóng app
mở lại → đúng những tab đó, đúng thứ tự, đúng tên đã đặt. Không còn cột agent ở đâu cả.

---

## Xong (2026-08-21)

Rẻ hơn ước lượng ban đầu vì phần lớn là **dựng lại**: `AgentView` đã là `Item`, bảng
`agent_views` còn nguyên trong DB, và `impl SerializableItem` lấy nguyên văn từ `c056596^`.

**Diff:** 10 file, ~876 thêm / ~2767 xoá. Xoá gấp ba lần thêm — cột agent mang theo
`PaneGroup` riêng, zoom riêng, hai predicate drag-drop, một menu `+` tự làm và một đường
persistence KVP song song, tất cả đều tan khi agent về chỗ mà editor đã có sẵn máy móc.

**Test:** 389 → 390 xanh (agent_ui 43, workspace 223, sidebar 20, database_ui 48, zode 56).

### Một bug lộ ra khi đọc code cũ

`should_serialize` của bản `SerializableItem` cũ trả `false`. Đó chính là lý do tên tự đặt
cho một session chưa bao giờ sống qua restart — chỉ có pass serialize toàn workspace ghi
hàng đó, mà một lần rename không kích hoạt pass đó. Thêm cột `name` mà không sửa chỗ này thì
cột mới vô nghĩa.

### `+` menu: lý do tự làm đã tự tan

`AgentPanel` phải thay hẳn menu `+` mặc định vì *"every one of which opens something into the
centre group, not into this column"*. Agent về center thì câu đó hết đúng — nên thay vì dựng
menu riêng, chỉ cần **thêm** mục `New <agent>` vào menu mặc định, dựng từ `BUILTIN_AGENTS`.
`crates/workspace` không phải depend `agent_ui`: mục menu chỉ dispatch một action đã đăng ký,
đúng lối `NewTerminal` vẫn làm.

### Ba test workspace bị xoá, và tại sao

Workspace test dùng *cột agent* làm cỗ xe thử nghiệm cho own-column nói chung. Còn một cột
thì ba test mất tiền đề:

- `flipping_the_rail_leaves_the_agent_in_its_own_column` — `an_own_column_follows_the_rail`
  đã assert đúng cả hai claim (theo rail + không bị lôi vào tool dock).
- `dragging_the_database_handle_leaves_the_agent_column_alone` — claim phân biệt của nó là
  cột-vs-cột; phần còn sống (cột vs tool dock cùng side) *là*
  `dragging_an_own_columns_handle_resizes_that_column` sau khi retarget.
- `an_outer_column_cannot_grow_through_the_one_beside_it` — tiền đề đúng nghĩa là hai cột own
  cạnh nhau. Với một cột, vòng `taken` trong `own_column_max_width` luôn ra 0.

`OWN_COLUMN_ORDER` giữ dạng array (giờ 1 phần tử) chứ không thu về một cột: mọi thứ đọc nó
đều viết dưới dạng fold, và đặc biệt hoá cho N=1 thì người thêm cột thứ hai phải tự đúng lại.

### Review bắt được ba thứ, một trong đó là kết luận sai của tôi

`reviewer` đọc adversarial toàn diff. Ba phát hiện, đã sửa cả ba:

**1. `activate_for_agent` chọn agent bừa (High) — hồi quy do tôi gây ra.** Nó lấy
`items_of_type::<AgentView>().next()`, tức tab đầu tiên theo thứ tự pane, không lọc theo
agent. Bản dock cũ đúng vì `focus_panel` mở *cả cột* chứa mọi agent nên "agent nào" không có
nghĩa; một tab thì chỉ một cái ở trước, nên notification từ Codex sẽ đưa tab Claude Code lên
— im lặng, và chỉ khi hai agent *khác nhau* cùng mở. Không test nào cũ chạm tới: mọi test
trước đó chỉ mở một agent, hoặc hai session của *cùng* một agent.
Đã sửa: `activate_for_agent` nhận `&AgentId` và `.find()`. Call site truyền `&agent.id()`
(giá trị đó vốn đã có trong tay và đang bị `let _ =` bỏ đi từ trước).
Test mới `a_notification_surfaces_the_agent_that_raised_it` — đã kiểm bằng cách tạm trả code
về `.next()` và **xác nhận nó đỏ**, nên đây là regression test thật, không phải trang trí.

**2. `agent_is_showing` là code chết (Medium).** Phase 03 viết lại nó thay vì xoá. Không còn
caller nào, và vì là `pub fn` nên clippy không bắt — đó chính là lý do "clippy sạch" không
phát hiện. Đã xoá. `Sidebar::agent_is_open` là bản còn dùng, và scope đúng hơn.

**3. Tôi kết luận sai là không test được round-trip DB (Medium).** Tôi bảo `agent_views` có FK
vào `workspaces` mà `save_workspace` là `pub(crate)` nên không crate nào ngoài
`crates/workspace` chèn được hàng cha. Sai: **`WorkspaceDb::next_id()` là `pub`** (sinh qua
macro `query!`) và `crates/editor/src/items.rs` đã dùng đúng cách đó từ crate khác trên đúng
schema có FK này. Test round-trip thật giờ đã có, gồm cả `name` là `Some` và `None`, rename
đè lên hàng cũ, và `delete_unloaded`. Đó là acceptance criterion của phase 01, trước đó
chưa đạt.

### Hai lỗ hổng test tôi từng bảo "không làm được" — đều làm được

Tôi tuyên bố không kiểm được hai chỗ, và **cả hai lần đều sai vì chưa tra đủ**:

1. **Vòng serialize → deserialize.** Lý do sai lần một: `save_workspace` là `pub(crate)` nên
   không chèn được hàng `workspaces` cha. Reviewer chỉ ra `WorkspaceDb::next_id()` là `pub`.
   Lý do sai lần hai (của tôi, sau đó): vòng đầy đủ cần máy móc restore của workspace. Cũng
   sai — `Workspace::set_database_id()` là `pub`.
   Giờ có `a_named_tab_survives_being_written_down_and_brought_back`: dựng workspace trên một
   hàng thật, đặt tên tab qua dispatch rename thật, `serialize`, rồi `deserialize` và assert
   đúng agent + mode + tên.

2. **Menu `+`.** Nhãn của mục menu thì thật sự không với tới (closure `PopoverMenu::menu` chỉ
   chạy khi popover mở, `ContextMenu.items` private trong `crates/ui`). Nhưng thứ *thật sự vỡ
   được* thì kiểm được: `every_agent_the_new_menu_offers_can_actually_be_opened` lặp qua
   `BUILTIN_AGENTS`, dispatch đúng action menu dispatch, và assert đúng tab đó mở ra — một id
   không handler nào nhận sẽ là mục menu bấm vào không làm gì.

Bài học ghi lại: "không test được" là một khẳng định cần tra, không phải một cảm giác. Hai
lần liền cái chặn đường là một `pub fn` nằm cách đó một file.

### Còn lại thật sự chưa kiểm bằng test

- *Nhãn chữ* trên mục menu `+` (lý do ở trên).
- *Vị trí* tab agent giữa các tab editor sau restart — cái đó do máy móc restore của
  workspace quyết, không phải `SerializableItem` này.

### Nợ đã ghi cho người thêm cột own thứ hai

Vòng `taken` trong `own_column_max_width` giờ luôn ra 0, và ba test bị xoá là những test duy
nhất chạm tới nhánh nhiều-cột đó. Không phải hồi quy ẩn — nhưng ai thêm cột own thứ hai phải
**viết lại** những test đó, đừng cho rằng chúng còn.

### Có sẵn từ trước, không phải hồi quy — không sửa trong phạm vi này

- `cargo check -p workspace --all-targets` đỏ ở `RemoteConnectionIdentity::Mock`
  (`persistence.rs`). Đã dựng worktree ở HEAD để đối chiếu: **y hệt**.
- `agent::ToggleFocus` trong keymap không có handler nào, ở HEAD cũng vậy.
- Không có keybinding mặc định nào cho `OpenAgent`/`ToggleAgent`/`NewAgent` — agent chỉ tới
  được bằng chuột (rail, và giờ thêm menu `+`).
