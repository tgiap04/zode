# Panel usage khi click, và menu bật/tắt khi chuột phải

**Status:** ✅ done (2026-08-21), chờ commit · **Priority:** P2 · **Branch:** feat.release-v0.1.1

## Yêu cầu

Click vào phần usage trên thanh status → panel như ảnh #3. Chuột phải → menu tick chọn
agent như ảnh #4. Nối tiếp
[feature usage](../260821-1821-agent-usage-on-the-status-bar/plan.md) vừa xong.

## Hai ảnh mẫu là của IDE khác — và đây là chỗ chúng lệch

| Trong ảnh | Trong zode |
|---|---|
| *Usage details & history* | không có store lịch sử, chỉ snapshot trong RAM → **bỏ** |
| *Manage Accounts…* | login nằm trong CLI, editor không quản account → **bỏ** |
| *OpenCode Go / MiniMax Usage* | không có agent nào như vậy → bỏ |
| *Remote Hosts / Resource Manager / Ports* | không có status item nào như vậy → thay bằng **5 item thật** đã có field trong `StatusBarSettings` |

Chép nguyên hai ảnh là ship bốn dòng menu dẫn tới chỗ trống. Người dùng đã chốt bỏ.

## Quyết định đã chốt

| Câu hỏi | Chốt |
|---|---|
| Footer panel | Bỏ *history* + *Manage Accounts* |
| Menu chuột phải | Claude/Codex Usage **+** 5 status item sẵn có |
| Detailed/Compact | Đổi **dòng trên thanh status**; Compact = chỉ cửa sổ căng nhất |
| Mũi tên `>` | Submenu chi tiết từng cửa sổ (tên đầy đủ, mốc reset tuyệt đối, lý do im) |

## Cái parser đang bỏ đi, mà panel thì cần

Payload của Claude mang `kind` trên mỗi phần tử `limits[]` — `session`, `weekly_all`,
`weekly_scoped` — và `parse_windows` đọc `percent`/`resets_at`/`scope` rồi **bỏ luôn
`kind`**. Nhưng đúng field đó là chữ `5h` / `wk` trong ảnh #3, và là "Session (5h)" /
"Weekly" trong submenu. Nên phase 01 không phải trang trí: thiếu nó, panel chỉ còn cách
đoán nhãn theo **vị trí trong mảng** — đúng loại giả định xanh trong test và sai trên máy
người khác.

Codex thì **không có `kind` nào cả**. Nó có `windowDurationMins`, và payload ghi thật đọc
ra `43200` — **30 ngày**, không phải 5 giờ cũng không phải 1 tuần. Nên nhãn của Codex
phải suy ra từ độ dài cửa sổ. Một cặp `5h`/`wk` hardcode sẽ in ra nhãn sai với vẻ tự tin
tuyệt đối.

## Phase

| # | Việc | Người dùng thấy gì |
|---|------|--------------------|
| [01](phase-01-every-window-learns-what-it-is.md) | `WindowKind` + hai parser đọc nó | **không gì cả** — cố ý |
| [02](phase-02-three-settings-and-the-compact-line.md) | 3 setting `status_bar.*` + thanh status tuân theo | Compact rút dòng lại, tắt được từng agent |
| [03](phase-03-the-panel-behind-the-click.md) | Panel: header, ⟳, toggle, dòng agent | Ảnh #3 |
| [04](phase-04-the-chevron-and-what-it-opens.md) | Submenu chi tiết từng cửa sổ | Mũi tên `>` mở ra |
| [05](phase-05-the-right-click-menu.md) | `right_click_menu` + ghi setting | Ảnh #4 |
| [06](phase-06-finish-the-feature.md) | Suite + fmt + docs | — |

Phase 01 vô hình có chủ đích: nó đổi kiểu dữ liệu mà 03/04 dựa vào, nên nó phải xanh
trước khi có gì được vẽ. Phase 02 gom **cả ba** setting vào một lượt sửa 4 file, để 03 và
05 chỉ đọc — thay vì chạm `default.json` ba lần ở ba phase.

## Chỗ phải cẩn thận

- **`assets/settings/default.json` là nguồn chốt.** `StatusBarSettings::from_settings`
  gọi `.unwrap()` trên từng field, nên thiếu một key trong JSON là panic lúc khởi động,
  không phải là default. Ba key mới phải vào đó cùng lúc với struct.
- **Tắt cả hai agent thì không còn gì để chuột phải vào.** Đây là lỗ đã biết và chấp
  nhận: đường về là settings, giống đúng cách `line_endings_button: false` hoạt động hôm
  nay. Phải viết vào docs, không để người dùng tự phát hiện.
- **`<dyn fs::Fs>::global(cx)`** là cách lấy fs để ghi setting mà không cần workspace.
  Nhưng trong test không có `GlobalFs` → panic. Nhánh ghi setting chỉ chạy khi click, nên
  test vẽ thì an toàn; test *bấm* thì cần fake fs.
- **`cx.draw` không publish frame** (bẫy đã ghi): đừng đo bounds của panel. Assert trên
  entity đã dựng và trên chuỗi render, không trên layout.
- **Progress bar cần bề rộng thật.** `flex_1`/`self_stretch` dưới `div()` trơn vẽ ra 0 —
  hai bẫy đã ghi. Dùng `w()` tường minh cho bar.
- **Panel là surface mới cho dữ liệu đã có.** Không thêm request, không đọc thêm
  credential, và tuyệt đối không hiện `spend`/`extra_usage` — vẫn là tiền, vẫn không ai
  yêu cầu.

## Định nghĩa xong

Click vào usage → panel mở ra đúng ảnh #3 (trừ hai dòng footer đã bỏ). Bấm *Compact* →
thanh status rút lại còn cửa sổ căng nhất, và giữ nguyên sau khi khởi động lại. Chuột
phải → menu 2 + 5 item, bỏ tick Codex thì phần Codex biến khỏi thanh status ngay. Mũi
tên `>` mở ra chi tiết từng cửa sổ. Không agent nào trả lời → panel nói tại sao, không
im lặng.

---

## Đã forge (2026-08-21)

**61 test** trong `agent_usage` (từ 36 trước feature này). Clippy 0 warning trên
`agent_usage` / `workspace` / `settings_content` / `zode`. `cargo build --bin zode` exit 0.

### Phase 01 tưởng là kiểu dữ liệu, thực ra là chỗ feature này sẽ nói dối

`limits[]` của Claude mang `kind` và parser **đang bỏ nó**. Panel cần đúng field đó. Nếu
không thêm, đường duy nhất còn lại là đoán nhãn theo **vị trí trong mảng** — xanh trong
test hôm nay, sai vào ngày endpoint đổi thứ tự, mà endpoint đó thì không có tài liệu nên
nó được phép đổi.

Codex đắt hơn: nó **không có `kind` nào cả**. Payload ghi thật đọc ra
`windowDurationMins: 43200` = **30 ngày**. Nếu tôi hardcode cặp `5h`/`wk` theo Claude thì
Codex sẽ hiện `5h` cho một cửa sổ 30 ngày — sai, và sai một cách tự tin. Nhãn phải suy ra
từ độ dài.

`Unknown` tồn tại vì `parse_windows` dùng `filter_map`: một `kind` lạ rất dễ vô tình
thành row bị bỏ, tức tổng quota thấp hơn sự thật, im lặng. Có test riêng cho nó.

### Hai chỗ tôi viết sai trong blueprint và code đi ngược lại

1. **Tiebreak của `most_constrained`.** Blueprint ghi "bằng percent thì reset *gần* thắng".
   Sai: hai cửa sổ đầy bằng nhau thì cái giải phóng **sớm** hơn là cái *ít* cấp bách. Code
   lấy reset xa hơn, và có test cho cả hai thứ tự đầu vào.
2. **`active_encoding_button` "không phá `NonUtf8`".** Không đạt được. Checkbox có 2 trạng
   thái, setting có 3. Tắt rồi bật lại rơi về `Enabled`. `NonUtf8` vẫn được *đọc* là đang
   bật nên tick đúng, chỉ không quay lại được qua tick. Có test khẳng định đúng chỗ mất.

### Ba chỗ ảnh mẫu không mang sang được, và đó là quyết định

*Usage details & history* (không có store lịch sử), *Manage Accounts…* (login ở CLI), và 5
dòng menu của IDE khác (OpenCode Go, MiniMax, Remote Hosts, Resource Manager, Ports). Bốn
dòng menu dẫn tới chỗ trống thì tệ hơn một menu ngắn hơn.

### Đổi thiết kế so với blueprint

- **Mũi tên `>` mở rộng tại chỗ**, không phải submenu neo cạnh — đúng preview đã chọn, và
  không phải lồng một `PopoverMenu` vào một `PopoverMenu` đang ở deferred layer.
- **Trigger là `ButtonLike`**, không phải `h_flex` trần: `PopoverTrigger` đòi
  `ButtonCommon + Toggleable`, và `ButtonLike` là button duy nhất trong repo nhận child tự
  do — mà một dãy số thì đúng là child tự do.
- **Menu dùng `ContextMenuEntry`**, không phải `toggleable_entry`: cái sau nhận tick *hoặc*
  icon, menu này cần cả hai (tick nói bật/tắt, icon nói đó là item nào).
- **⟳ trên thanh status không còn refresh trực tiếp** — nó nằm trong trigger nên bấm là mở
  panel. Refresh ở ⟳ trong panel. Tách nó ra thành nút riêng là phá đúng cái "cả nhóm là
  một control" mà ảnh mẫu thể hiện.

### Một chỗ tôi tự sửa trước khi review chỉ ra

Handler mỗi dòng menu ban đầu bắt `!showing` từ lúc **dựng** menu, tức ghi một giá trị cũ
bằng tuổi cái menu. Đọc lại setting ngay trong handler thì rẻ hơn và xoá hẳn cả lớp lỗi.

### Lỗ đã biết, chấp nhận, không sửa

Tắt tick cả Claude lẫn Codex → thanh status không còn gì để chuột phải vào, đường về chỉ
còn settings. Sửa đúng chỗ là cho **cả thanh status** nhận chuột phải, tức đổi crate
`workspace` và tạo phụ thuộc vòng về `agent_usage` — nặng hơn cả năm phase còn lại. Có
test khẳng định trạng thái đó, và docs phải nói ra.
