# Cột agent: menu "+" của riêng nó, và mặt phẳng tách khỏi git panel

**Status:** ✅ done (2026-08-15) · **Priority:** P2 · **Branch:** feat/rail-agents-claude-code-codex

## Bốn việc người dùng nêu

1. Menu `+` đang là menu của **editor pane** (New File / Open File / Search / Terminal) — không món nào thuộc về cột này
2. Muốn mở **nhiều bản của cùng một agent** (hai Claude Code cùng lúc)
3. Cột agent **chưa thấy border radius**
4. Cột agent **chưa cách** cột git panel

## Chỗ 3 và 4 là **cùng một lỗi**

`AgentPanel::render` đã có `p(SURFACE_MARGIN)` + `rounded(SURFACE_ROUNDING)` — y hệt centre group. Nhưng `Dock::render` tô `bg(panel_background)` và kẻ `border_l_1`/`border_r_1` **bao ngoài** nó. Kết quả:

- 3px khe bị lấp bằng **đúng màu nền của git panel bên cạnh** → không thấy khe
- 5px bo góc không có gì để bo vào → không thấy bo

Cột agent là **surface riêng** (như centre group), không phải tool dock. Nó không nên mang chrome của dock. Bỏ chrome đi là cả hai hiện ra, dùng lại **đúng hằng số** centre group đang dùng — không đẻ số mới.

## Quyết định đã chốt với người dùng

| Hỏi | Chốt |
|---|---|
| Menu `+` còn gì | **Chỉ agent**: New Claude Code / New Codex |
| Bấm rail khi đã mở | **Focus cái đang có** — mở thêm là hành động cố ý qua `+` |

Rail giữ nghĩa "đang chạy"; lỡ tay bấm rail không đẻ thêm process CLI.

## Phase

| # | Việc | Người dùng thấy gì |
|---|---|---|
| 01 | Bỏ chrome của dock cho cột agent | Cột bo góc, tách khỏi git panel |
| 02 | `NewAgent` action + `AgentPanel::show_new` | (chưa gì — chưa có chỗ bấm) |
| 03 | Menu `+` của pane agent | Mở được Claude thứ hai |

02 vô hình có chủ đích: đường mở-bản-mới xanh trước khi có nút gọi nó.

## Chỗ phải cẩn thận

- **Menu dựng từ `BUILTIN_AGENTS`**, không hardcode hai tên — thêm agent thứ ba là nó tự có mặt.
- `set_render_tab_bar_buttons` thay **cả** cụm nút phải, nên phải dựng lại nút split và zoom, không chỉ `+`.
- Mỗi `AgentView` tự sở hữu process của nó, và `create_terminal_task` **không** gộp theo `TaskId` — nên hai bản terminal là an toàn. Đường ACP (conversation) cần kiểm: `AgentConnectionStore` giữ **một connection cho mỗi agent**, hai view phải là hai *session* trên cùng connection đó.
- `remember_mode` lưu theo agent id → hai bản dùng chung mode nhớ. Chấp nhận: đó là giá trị mặc định, không phải trạng thái phiên.

## Định nghĩa xong

Bấm `+` trong cột agent → chỉ thấy New Claude Code / New Codex. Bấm New Claude Code khi đã có một Claude → **hai** section Claude cùng chạy. Cột agent bo góc và có khe thật ngăn với git panel.

## Không kiểm được bằng test

Bỏ chrome là thay đổi **màu và nét vẽ** — `cx.debug_bounds` đo được hình hộp, không đo được cái gì tô lên nó. Phase 01 phải nói thẳng điều đó thay vì dựng một test lặp lại chính cờ vừa đặt.

---

## Xong (2026-08-15) — `ad1ef85`, `8738da8`

Gộp còn **hai** commit thay vì ba: phase 02 một mình là code chết (một action đã đăng ký nhưng không có nút nào gọi), nên nó đi cùng phase 03.

### Ba trong bốn yêu cầu là **một** nguyên nhân

Menu sai, không bo góc, không có khe — nhìn thì ba việc, nhưng bo góc và khe là **cùng một dòng code**: `Dock::render` tô `panel_background` + kẻ border **đè lên** `p(3px)` mà panel đã có sẵn. Khe bị lấp bằng đúng màu nền git panel bên cạnh → mắt không thấy khe; góc 5px không có gì để bo vào → mắt không thấy bo. Bỏ chrome là cả hai hiện ra, **không đẻ hằng số mới** — vẫn `SURFACE_MARGIN`/`SURFACE_ROUNDING` centre group đang dùng.

### Menu dựng từ `BUILTIN_AGENTS`

Không hardcode "Claude Code"/"Codex". Thêm agent thứ ba vào `BUILTIN_AGENTS` là nó tự có mặt trong menu — không ai phải nhớ quay lại sửa chỗ này.

`set_render_tab_bar_buttons` thay **cả cụm** nút phải, nên nút split và zoom phải dựng lại chứ không chỉ đổi `+`. Nút split ở đây chỉ mở `MovePane` — `handle_pane_event` vốn đã từ chối clone, vì một agent nhân đôi là process thứ hai giả làm session cũ.

### Hai lực kéo ngược nhau, và test giữ cả hai

`show` **gộp** (bấm rail lần nữa → quay lại cái đang chạy), `show_new` **không gộp** (menu `+` → session mới). Nếu `show_new` thừa hưởng phép gộp thì không cách nào mở Claude thứ hai. Test giữ đúng cả hai chiều; kiểm chứng ngược bằng cách cho `show_new` gộp lại → đỏ `left: 1, right: 2`.

### Kiểm được và không kiểm được

- **Kiểm được:** hai session cùng agent, rail vẫn quay về cái cũ (`a_second_session_of_one_agent_stands_beside_the_first`).
- **Đọc code để chắc, không phải test:** `request_connection` trả **chung một connection cho mỗi agent**, còn mỗi `ConversationView` tự gọi `new_session` — nên hai Claude là hai *session* trên một connection, đúng mô hình ACP. Đường terminal thì `create_terminal_task` không gộp theo `TaskId`, mỗi view một process.
- **Không kiểm được:** việc bỏ chrome. `cx.debug_bounds` đo hộp, không đo màu. Đây là thay đổi phải nhìn bằng mắt — nói thẳng thay vì dựng test lặp lại chính cờ vừa đặt.

### Còn nợ

`remember_mode` lưu theo agent id, nên hai bản Claude dùng chung mode đã nhớ. Chấp nhận: đó là **giá trị mặc định lúc mở**, không phải trạng thái của phiên đang chạy — đổi mode ở bản này không kéo bản kia theo.
