# Phase 04 — Poll khi focus, và bấm để làm mới

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** số luôn mới, ⟳ bấm được

## Mục tiêu

Số trên thanh không cũ đi trong im lặng, mà cũng không có request nào đi ra khi
người dùng không nhìn.

## Việc

**Poll**

- Chu kỳ ~60s. Claude không có push nên poll là cách duy nhất.
- **Chỉ khi cửa sổ đang focus.** Mất focus → dừng hẳn; lấy lại focus → fetch ngay
  một lần rồi vào nhịp (số sau khi quay lại máy phải mới, không phải cũ 60s).
- Một task duy nhất, huỷ khi indicator bị drop — không để rò task theo cửa sổ.

**Bấm để làm mới**

- Cả nhóm là một vùng bấm được, `⟳` là affordance (đúng như ảnh).
- Đang fetch → vô hiệu hoá để không xếp hàng nhiều request.
- Tooltip nói khi nào lấy lần cuối — một con số không rõ cũ hay mới thì khó tin.

**Trạng thái**

| trạng thái | hiện |
|---|---|
| chưa bao giờ có dữ liệu | không gì cả |
| có dữ liệu | các cửa sổ |
| đang refresh, đã có dữ liệu | dữ liệu cũ + dấu hiệu đang tải |
| fetch lỗi, từng có dữ liệu | giữ dữ liệu cuối + tooltip nói lỗi |

Hàng cuối là một đánh đổi có chủ đích: một con số hơi cũ kèm lời giải thích hữu ích
hơn là thanh trống mỗi lần mạng chớp. Nhưng **không bao giờ** giữ dữ liệu qua một
lần đổi credential/eligibility — lúc đó phải trống thật.

## Files

- sửa: `crates/agent_usage/src/agent_usage.rs`

## Todo

- [x] Task poll 60s, chỉ chạy khi focus, huỷ khi mất focus
- [x] Lấy lại focus → fetch ngay rồi vào nhịp
- [x] Bấm để refresh, vô hiệu khi đang fetch
- [x] Tooltip: thời điểm lấy lần cuối, hoặc lý do lỗi
- [x] Test: lỗi sau khi đã có dữ liệu → vẫn hiện dữ liệu cũ, tooltip đổi
- [x] Test: eligibility mất → xoá trắng, không giữ số cũ
  (`keep_holds_the_last_numbers_and_clear_takes_them_away`)
- [x] Test: apply luôn hạ cờ đang-fetch, dù kết quả thế nào
- [ ] ~~Test: mất focus → không có request nào đi ra~~
- [ ] ~~Test: bấm hai lần liên tiếp → chỉ một request~~

**Hai ô cuối không test được từ crate này, và đó là hệ quả trực tiếp của việc sửa
suite `zode`.** Phase 06 phát hiện indicator làm IO thật ngay trong `new()`, làm 30
test của `zode` đỏ. Cách chặn là feature `test-support`: trong build test,
`may_read_usage()` trả `false` và **vòng poll không bao giờ khởi động**. Điều đó chữa
được suite, nhưng cũng có nghĩa đường poll/click không chạy trong unit test của chính
crate này.

Cái *đã* kiểm là nơi logic thật sự nằm: `apply` (giữ/xoá/hạ cờ), parser hai nguồn, và
luật render. Cái chưa kiểm là 10 dòng timer + guard. Đánh đổi có ý thức, không phải
bỏ quên.
- [x] `cargo check -p agent_usage` xanh

## Success criteria

Thu nhỏ app một lúc rồi mở lại: số mới ngay, và không có request nào trong lúc thu
nhỏ. Bấm ⟳: cập nhật ngay, không xếp hàng.

## Rủi ro

| Rủi ro | Chặn thế nào |
|--------|--------------|
| Task poll rò theo mỗi cửa sổ | Task thuộc entity, huỷ khi drop; test mở/đóng nhiều cửa sổ |
| Xếp hàng request khi bấm liên tục | Cờ đang-fetch, có test |
| Giữ số cũ sau khi mất quyền hiện | Đổi eligibility là xoá trắng, có test riêng |
