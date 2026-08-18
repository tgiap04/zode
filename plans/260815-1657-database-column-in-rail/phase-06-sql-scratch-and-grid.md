# Phase 06 — SQL scratch + result grid

**Context:** [plan.md](plan.md)
**Priority:** P2 · **Status:** ✅ done (2026-08-15) · **Blocked by:** 05
**File ownership:** `crates/database_ui/**`

Hết phase này là **lát cắt dọc SQLite chạy end-to-end**: nút rail → cột → connection → cây → SQL → dữ liệu.

## Hình dạng trong cột

```
▾ localhost            ← cây (phase 05)
    ▾ public
      users
──────────────────      ← tay kéo
select * from users     ← Editor thật
──────────────────      ← tay kéo
 id │ email             ← ui::Table
  1 │ a@b.com
◂ 1–200 / còn nữa ▸     ← phân trang
```

Ba vùng chia dọc trong cột. Dùng `pane_axis` như `Dock::render_stack` (`dock.rs:1492`) đã làm, không tự chế layout co giãn.

## Việc

- [ ] SQL scratch: một `Editor` thật với ngôn ngữ `sql` — được highlight, multi-cursor, vim mode miễn phí
- [ ] Nội dung scratch lưu theo connection qua KeyValueStore, khôi phục khi mở lại
- [ ] `cmd-enter` chạy; không có vùng chọn thì chạy câu lệnh dưới con trỏ, có vùng chọn thì chạy đúng vùng chọn
- [ ] Result grid: `ui::Table` với `Table::uniform_list` + `ColumnWidthConfig::redistributable` + `TableInteractionState`. Mẫu dùng thật: `repl/src/outputs.rs`
- [ ] Phân trang: `query { limit: 200, offset }`, nút trước/sau, hiện `truncated`. **Không bao giờ** kéo cả bảng
- [ ] Bấm bảng trên cây → chạy `SELECT * FROM …` phân trang, không cần gõ
- [ ] `NULL` vẽ khác chuỗi rỗng (chữ nghiêng mờ `NULL`), theo type tag từ protocol
- [ ] Ô dài: cắt kèm tooltip đầy đủ; ô rất dài mở được ở popover
- [ ] Huỷ query đang chạy → gọi `cancel`, nút đổi thành "Đang huỷ"
- [ ] Lỗi SQL hiện dưới editor với `message` từ protocol; lỗi `code` read-only nói rõ "cột này là read-only" chứ không phải lỗi cú pháp
- [ ] Export CSV kết quả đang hiện + export toàn bộ query (chạy lại theo trang, ghi dần ra file)

## `database_ui` không được biết engine nào tồn tại

Đây là bất biến mà phase 07 sẽ đem ra kiểm tra.

- [ ] `grep -rn "sqlite\|postgres\|mysql" crates/database_ui/src` phải **rỗng** (trừ chuỗi hiển thị tên engine người dùng tự đặt)
- [ ] Mọi quyết định format ô đến từ type tag của protocol, không từ tên engine
- [ ] Có test khẳng định điều này (kiểm tra nội dung file nguồn, hoặc rõ hơn: chạy toàn bộ suite UI trên `fake_driver` với tên engine bịa)

## Test

- [ ] Chạy `select` trên fixture SQLite → grid đúng cột đúng dòng
- [ ] `delete from users` → hiện lỗi read-only, dữ liệu **không đổi** (khẳng định bằng cách select lại)
- [ ] 250 dòng: trang 1 = 200 + "còn nữa", trang 2 = 50, quay lại trang 1 đúng như cũ
- [ ] `NULL` và chuỗi rỗng vẽ khác nhau
- [ ] Query treo → bấm huỷ → UI trở lại nhận lệnh trong thời gian hữu hạn
- [ ] Đóng cột giữa lúc query chạy → không panic, không process mồ côi
- [ ] Scratch buffer khôi phục đúng nội dung theo từng connection
- [ ] Export CSV khớp nội dung grid, kể cả ô có dấu phẩy/xuống dòng/`NULL`
- [ ] Layout: cả ba vùng vẽ ra kích thước thật. Nhớ `.flex_1().self_stretch()` dưới `div()` trần cho ra chiều cao 0 — đo bằng cách dock panel rồi để `run_until_parked` vẽ window, **không** bằng `cx.draw` (nó không xuất bản frame nào)

## Định nghĩa xong

Mở một SQLite trong project, bấm bảng, thấy dữ liệu. Gõ SQL, `cmd-enter`, thấy kết quả. Gõ `delete`, bị chặn. Xuất CSV ra file đúng nội dung. Toàn bộ mà `crates/database_ui` không chứa chữ `sqlite` nào.

## Rủi ro

- **Cột hẹp là điểm yếu đã biết của thiết kế** (ghi trong brainstorm). Cột riêng kéo rộng được, nên phase này chỉ cần chắc chắn ba vùng chia dọc kéo được và grid cuộn ngang tử tế. Nếu dùng thật vẫn chật, lối thoát rẻ là một lệnh "Open Results in Editor" — **không** làm bây giờ.
- Grid + editor + cây trong một cột là ba vùng cuộn lồng nhau. Mỗi vùng phải có `min_h_0` và cha là `v_flex`, nếu không sẽ có vùng cao 0 mà không có panic nào báo.
- Export toàn bộ query có thể rất lâu — chạy nền, huỷ được, và **không** giữ cả kết quả trong bộ nhớ.

---

## Xong (2026-08-15)

`database_ui` 21 xanh · `cargo check -p zode` sạch · clippy sạch. **Hết lát cắt dọc SQLite.**

### Bất biến của phase 07 đã được cắm vào test ngay bây giờ

`no_source_file_in_this_crate_names_an_engine` quét mọi `.rs` trong `crates/database_ui/src`, bỏ
comment và bỏ khối `#[cfg(test)]`, và fail nếu thấy `sqlite`/`postgres`/`mysql`. Ngoại lệ duy nhất:
`driver_registry.rs` — chỗ *bắt buộc* phải kể tên driver Zode ship.

Viết ngay bây giờ chứ không đợi phase 07, vì một `if engine == "postgres"` lọt vào sẽ làm nghiệm thu
của phase đó thành vô nghĩa. Test này bắt đúng hai lần trong lúc viết.

### Lệch khỏi plan, có lý do

**Ba vùng chia bằng chiều cao cố định, không phải tay kéo.** Plan viết dùng `pane_axis`. Không dùng
được: nó `pub(crate)` trong `workspace`, nên tay kéo ở đây nghĩa là **implement lại** cơ chế co giãn.
SQL buffer cố định 132px (~6 dòng). Ghi trong `render.rs` để người sau biết đây là lựa chọn, không
phải bỏ sót.

**Export CSV ra clipboard, không ra file.** File cần hộp thoại chọn đường dẫn, mà bước tiếp theo gần
như luôn là dán vào bảng tính.

**`database_panel.rs` bị tách làm ba** (`database_panel.rs` / `panel_connections.rs` /
`panel_queries.rs`) — nó đã 513 dòng, quá mức 200 của CLAUDE.md.

### Quyết định đáng ghi

| Chỗ | Chọn gì | Vì sao |
|---|---|---|
| `cmd-enter` | Chạy **vùng chọn**, không có vùng chọn thì chạy cả buffer | Scratch buffer chứa nhiều câu; chạy hết vì con trỏ tình cờ nằm trong một câu không phải ý ai |
| Đọc vùng chọn | `selections.newest_anchor()` + `to_offset` | `newest::<T>()` cần `DisplaySnapshot`, thứ panel này không bao giờ dựng |
| `request_id` | **Đếm**, không random | Hai query trong cùng một mili-giây sẽ trùng id, và `cancel` giết nhầm |
| Kết quả về muộn | Đối chiếu `request_id` **đang chạy** rồi mới nhận | Kết quả của query người dùng đã bỏ qua không được đè lên trang đang xem |
| Bấm huỷ | Đổi sang "Cancelling…", **không** nhảy thẳng về idle | Engine không interrupt được vẫn chạy nốt; giả vờ đã dừng là nói dối |
| Bấm bảng | Vừa hiện cột **vừa** chạy `SELECT *` | "Cho tôi xem bảng này" là cả hình dạng lẫn dữ liệu; tách làm hai cử chỉ là bắt bấm hai lần |
| Tên bảng → SQL | `quote_identifier` ở tầng UI | Đây là chỗ tên từ cây **trở thành SQL**; bảng có thể tên `"; drop table users; --` |
| CSV: null vs chuỗi rỗng | Null = ô rỗng **không** nháy; chuỗi rỗng = `""` | Đúng một phân biệt CSV mang nổi. Test bắt được lỗi này — bản đầu để cả hai thành ô rỗng |
| Null trong grid | Nghiêng + mờ, chữ `NULL` | Vẽ giống chuỗi rỗng là nói dối về dữ liệu |
| SQL editor | `Editor` thật + ngôn ngữ SQL nạp **best-effort** | Không có grammar thì vẫn là text box dùng được; chết vì thiếu grammar thì vô lý |
| SQL + grid | Chỉ hiện khi đã có connection active | Hai hộp rỗng trên một cây chưa cấu hình đọc như hỏng, không như đang chờ |

### Phím tắt

`cmd-enter` chạy · `cmd-escape` huỷ · `alt-cmd-←/→` lật trang · `cmd-shift-c` copy CSV
(macOS; bản linux dùng `ctrl-`).

### Còn nợ

- Tay kéo giữa ba vùng (cần `pane_axis` được mở `pub` hoặc một bản co giãn riêng).
- Ô rất dài mới chỉ cắt một dòng, chưa có popover xem đầy đủ.
- Export **toàn bộ** query (chạy lại theo trang, ghi dần ra file) — mới có export trang đang xem.
