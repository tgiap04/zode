# Phase 03 — LSP hibernate và wake, giữ diagnostic summary

## Context Links

- [plan.md](./plan.md) · [Phase 02](./phase-02-project-activity-governor.md)
- [reports/scout-report.md](./reports/scout-report.md) § Phase 3 — vị trí từng đoạn xoá diagnostics
- [research/researcher-01-lsp-hibernation-prior-art.md](./research/researcher-01-lsp-hibernation-prior-art.md)
- `crates/project/src/lsp_store.rs` (14.754 dòng — **phase này đứng riêng, không gộp**)

## Overview

- **Priority:** P1 — đây là phần tiết kiệm RAM thật, và là phần dễ gây regression nhất
- **Status:** Pending
- **Effort:** 5–6 ngày (3–4 ngày cho local + 2 ngày cho remote/SSH, vào scope theo Validation session 1)

Khi project vào `Hibernated`: stop toàn bộ language server + Prettier + DAP của project đó, nhưng
**giữ `diagnostic_summaries`** để cây file và status bar không nhìn như project vừa sạch lỗi. Khi
`Active` trở lại: restart server theo các buffer đang mở, không block UI.

## Key Insights

- `stop_local_language_server` (`lsp_store.rs:11023`) xoá diagnostics ở **4 chỗ**: buffer
  (`:11041`), `diagnostic_summaries` (`:11048`), `local.diagnostics` (`:11085`),
  `language_server_watched_paths` (`:11094`). Hibernate phải bỏ qua đúng chỗ thứ 2.
- **Bẫy test:** `project_tests.rs:3866` `test_diagnostic_summaries_cleared_on_server_restart` khẳng
  định hành vi xoá. Cùng nhóm `:3788`, `:3940`. Hibernate **phải là đường riêng** — tuyệt đối không sửa
  `stop_local_language_server` để nó thôi xoá, vì đó là hợp đồng của restart.
- Quyết định đã chốt: **chỉ giữ summary, bỏ squiggle per-line**. Lý do kỹ thuật: `server_id` đổi sau
  restart, map lại diagnostics per-line theo tên server là phần đắt nhất và dễ sai nhất.
- rust-analyzer không có cache index trên đĩa → wake là trả lại toàn bộ chi phí index. UI phải nói rõ.
- Server chỉ start khi có buffer của language đó được mở (`on_buffer_added`, `:4504`). Wake vì vậy =
  "đăng ký lại các buffer đang mở", không phải "start mọi server từng chạy".

## Requirements

**Functional**

- FR1: `LspStore::hibernate(cx) -> Task<()>` stop mọi server local của project, **không** xoá
  `diagnostic_summaries`.
- FR2: `LspStore::wake(cx)` đăng ký lại các buffer đang mở với language server, theo đúng đường
  `restart_language_servers_for_buffers` đã có.
- FR3: Summary còn lại phải được đánh dấu stale, và UI hiển thị được sự khác biệt đó (Phase 3 chỉ cần
  mang cờ; cách vẽ do project_panel/diagnostics quyết định — xem Risk).
- FR4: Lần publish diagnostics đầu tiên của server mới cho một file **thay thế** summary stale của file
  đó. Không cộng dồn, không nhân đôi.
- FR4b *(red team Finding 5)*: Khi server báo xong đợt index đầu tiên sau wake (progress end), xoá
  **toàn bộ** cờ stale còn lại của project — không chỉ các file đã publish. Lý do: file không tự đổi
  nhưng vỡ vì dependency đổi sẽ giữ nguyên summary sạch cũ, tức là che lỗi (kể cả lint bảo mật).
- FR5: Prettier (`PrettierStore`) và DAP theo cùng nhịp hibernate/wake.
- FR6: Hibernate là idempotent — gọi 2 lần không sinh lỗi, không stop 2 lần.
- FR7 *(red team Finding 11 + quyết định Validation session 1)*: **Remote/SSH nằm trong scope v1.**
  - Hibernate: gửi `proto::StopLanguageServers { all: true }` qua upstream client — đường này đã có
    trong `shutdown_all_language_servers` (`lsp_store.rs:11140-11150`).
  - Wake: `proto::RestartLanguageServers` với buffer id đang mở — đã có tại
    `restart_language_servers_for_buffers` (`lsp_store.rs:11183-11211`).
  - Kết nối SSH **không** bị đóng. Không đường nào trong hai đường trên chạm tới connection.
  - **Vấn đề riêng của remote, phải xử lý tường minh:** với remote, việc xoá diagnostics diễn ra ở
    `LspStore` phía host, rồi host đẩy `proto::UpdateDiagnosticSummary` với count = 0 xuống client
    (khuôn thấy ở `lsp_store.rs:11055-11072`). Nghĩa là summary phía local **sẽ bị wipe bởi message
    đến**, chứ không phải bởi code local. Cách xử lý v1: trong lúc project ở `Hibernated`, bỏ qua các
    `UpdateDiagnosticSummary` có count = 0. Đánh đổi phải ghi rõ: một update "0 lỗi thật" đến đúng lúc
    đó cũng bị bỏ — chấp nhận được vì ta đang đánh dấu toàn bộ là stale.
  - **Ghi thẳng vào plan:** hibernate remote tiết kiệm RAM **trên máy remote**, không phải trên máy
    local. Lợi ích thật (dev box thường nhỏ) nhưng không phục vụ mục tiêu gốc.
- FR8 *(red team Finding 3 + quyết định Validation session 1)*: Hoãn hibernate **chỉ khi autosave đang
  bật** (`AutosaveSetting` khác `Off`) **và** có buffer dirty. Mặc định `"autosave": "off"`
  (`assets/settings/default.json:988`) nên phần lớn người dùng không có cuộc đua này. Buffer dirty với
  autosave tắt **không** chặn hibernate — nếu chặn thì ai hay để tab dirty sẽ không bao giờ được ngủ.
  Khi hoãn: thử lại sau một chu kỳ, không huỷ hẳn.

**Non-functional**

- NFR1: `wake()` không block main thread; UI (tab, pane, cursor) phải render trước khi server xanh.
- NFR2: Hành vi restart hiện tại không đổi: 3 test `diagnostic_summaries_cleared_*` còn xanh nguyên.
- NFR3: RSS của project sau hibernate phải giảm đo được (kiểm ở Phase 6, không phải ở đây).

## Architecture

```
Project::set_activity(Hibernated)
  └─ emit Event::ActivityChanged
       ├─ rào 1: mode != Local            ⇒ early-return + log debug (FR7)
       ├─ rào 2: có buffer dirty / autosave đang chờ ⇒ BỎ QUA hibernate (FR8)
       ├─ rào 3: có session debug đang chạy         ⇒ BỎ QUA hibernate
       ├─ LspStore::hibernate()      → stop server × N, giữ diagnostic_summaries
       └─ PrettierStore::hibernate() → drop prettier instance

Project::set_activity(Active)
  └─ LspStore::wake() → restart_language_servers_for_buffers(buffers đang mở)
                          ├─ publish đầu tiên cho file X ⇒ xoá cờ stale của X (FR4)
                          └─ index xong (progress end)   ⇒ xoá TOÀN BỘ cờ stale còn lại (FR4b)
```

**Cách tách đường mà không nhân bản logic (DRY):** rút phần thân của `stop_local_language_server` thành
`fn stop_local_language_server_inner(&mut self, server_id, clear_diagnostics: bool, cx)`. Hai wrapper:
`stop_local_language_server` (giữ nguyên chữ ký công khai, gọi với `true`) và đường hibernate (gọi với
`false`). Không copy-paste 100 dòng.

Cờ stale: thêm vào cấu trúc summary một `stale: bool`, hoặc giữ `HashSet<ProjectPath>` riêng trong
`LspStore` cho các path có summary stale — chọn phương án ít lan ra API công khai nhất sau khi đọc kiểu
thật của `diagnostic_summaries`.

## Related Code Files

**Modify**

- `crates/project/src/lsp_store.rs` — tách `stop_local_language_server` thành inner + 2 wrapper; thêm
  `hibernate()` / `wake()`; quản cờ stale
- `crates/project/src/project.rs` — nối `set_activity` → `lsp_store.update(...)`, `prettier_store`, dap
- `crates/project/src/prettier_store.rs` — `hibernate()` (drop instance đang giữ)
- `crates/project_panel/src/project_panel.rs:1015`, `:1044` — đọc cờ stale để vẽ khác (làm mờ)
- `crates/diagnostics/src/diagnostics.rs:463` — banner "project đang index lại"
- `crates/project/tests/integration/project_tests.rs` — test mới cho hibernate (KHÔNG sửa 3 test cũ)

**Read for context**

- `crates/project/src/lsp_store.rs:11138` (`shutdown_all_language_servers`), `:11177`
  (`restart_language_servers_for_buffers`), `:4504` (`on_buffer_added`), `:296`/`:322`
  (`language_server_ids`, `lsp_tree`)

## Implementation Steps

1. Viết test **trước**: `test_hibernate_preserves_diagnostic_summaries` — mở buffer, đẩy diagnostics
   từ fake LSP, hibernate, khẳng định `project.diagnostic_summaries(...)` vẫn trả về số cũ và buffer
   không còn diagnostics per-line.
2. Test thứ hai: `test_wake_replaces_stale_summaries` — wake, fake server publish diagnostics khác cho
   cùng file, khẳng định không nhân đôi và cờ stale đã tắt.
3. Tách `stop_local_language_server` → inner có tham số `clear_diagnostics`. Chạy full
   `cargo test -p project` để chứng minh không đổi hành vi trước khi thêm gì.
4. `LspStore::hibernate()`: lấy danh sách server id như `shutdown_all_language_servers` làm
   (`local.language_server_ids.values()`), `lsp_tree.remove_nodes(...)`, rồi gọi inner với
   `clear_diagnostics: false`. Đánh cờ stale cho mọi path còn summary.
5. `LspStore::wake()`: gom buffer từ `buffer_store.buffers()`, gọi
   `restart_language_servers_for_buffers(buffers, HashSet::default(), cx)`.
6. Xoá cờ stale trong đường xử lý publish diagnostics (tìm nơi `diagnostic_summaries` được ghi khi
   server publish, không phải nơi nó bị xoá). **Và** xoá toàn bộ cờ còn lại khi nhận progress end của
   đợt index đầu tiên sau wake (FR4b).
6b. Hai rào trước khi hibernate: (a) autosave đang bật + có buffer dirty ⇒ hoãn, thử lại chu kỳ sau;
   (b) đang có session debug ⇒ bỏ qua. Mỗi rào một test riêng.
6c. **Đường remote (FR7):** hibernate gửi `StopLanguageServers { all: true }` qua upstream client; wake
   gửi `RestartLanguageServers` với buffer id đang mở; trong lúc `Hibernated` bỏ qua
   `UpdateDiagnosticSummary` count = 0. Test riêng với fake upstream client.
6d. **Flip default** `retain_background_projects` từ `false` sang `true` (quyết định Validation session
   1) — làm ở bước cuối của phase này, sau khi hibernate đã chạy thật.
7. Prettier: `PrettierStore` drop instance; wake để nó tự lazy-start như hiện tại (kiểm xem có lazy
   thật, nếu không thì để nguyên và ghi lại lý do).
8. DAP: nếu project có session debug đang chạy → **bỏ qua hibernate hoàn toàn** cho project đó
   (không stop LSP luôn). Debug đang chạy là công việc user đang làm.
9. `project_panel` + `diagnostics`: vẽ trạng thái stale (mờ hoặc icon) + banner đang index.
10. `./script/clippy`, `cargo test -p project`, và chạy tay: mở 2 project Rust, hibernate một, đo thời
    gian wake, xem badge có mờ và có trở lại đúng.

## Todo List

- [ ] Test-first: 2 test hibernate/wake summary
- [ ] Tách inner + 2 wrapper, chứng minh không đổi hành vi
- [ ] `LspStore::hibernate()`
- [ ] `LspStore::wake()`
- [ ] Cờ stale + xoá cờ khi publish
- [ ] Xoá toàn bộ cờ stale khi index xong (FR4b)
- [ ] Prettier hibernate
- [ ] Đường remote: stop + restart qua proto, test với fake upstream client
- [ ] Remote: bỏ qua `UpdateDiagnosticSummary` count = 0 khi đang `Hibernated`
- [ ] Rào autosave-bật + buffer dirty ⇒ hoãn (FR8) + test cho `AfterDelay` và `OnFocusChange`
- [ ] Rào DAP: đang debug ⇒ không hibernate
- [ ] Flip default `retain_background_projects` → `true` (bước cuối phase)
- [ ] UI: badge mờ + banner đang index
- [ ] 3 test `diagnostic_summaries_cleared_*` còn xanh
- [ ] Đo tay thời gian wake trên project Rust cỡ thật
- [ ] `./script/clippy` sạch

## Success Criteria

- `test_diagnostic_summaries_cleared_on_server_restart` (`:3866`), `:3788`, `:3940` xanh **không sửa**.
- Hibernate rồi wake một project Rust: badge lỗi không biến mất khỏi cây file; sau khi index xong, số
  liệu khớp với trước hibernate.
- `cargo test -p project` xanh.
- Không có `unwrap()` mới trên đường fallible; mọi lỗi stop/restart đi qua `?` hoặc `.log_err()`.

## Risk Assessment

| Rủi ro | Mức | Giảm thiểu |
|---|---|---|
| Summary stale sai sự thật (file đã sửa ngoài editor lúc ngủ) | **Cao** | Vẽ khác (mờ) + banner; Phase 4 xoá sớm cho file đã đổi; FR4b xoá toàn bộ khi index xong — đây mới là cái chặn được ca "file không đổi nhưng vỡ vì dependency" |
| Autosave nổ sau khi server đã stop ⇒ save không format, im lặng | **Cao** | FR8: không hibernate khi còn buffer dirty / autosave chờ. Test riêng cho `AfterDelay` và `OnFocusChange` |
| Remote: summary bị wipe bởi message từ host, không phải bởi code local | **Cao** | FR7: bỏ qua `UpdateDiagnosticSummary` count = 0 khi `Hibernated`; test bằng fake upstream client. Đây là phần đội thêm 2 ngày vào phase |
| Remote hibernate tiết kiệm RAM ở máy remote, không phải máy local | Trung bình | Ghi rõ trong FR7 để không ai kỳ vọng sai; mốc RAM ở Phase 6 chỉ đo project local |
| Sửa `stop_local_language_server` làm vỡ ngữ nghĩa restart | **Cao** | Đường riêng qua tham số, không sửa hành vi wrapper cũ; bước 3 chạy full test trước khi thêm tính năng |
| `lsp_tree.remove_nodes` để lại node mồ côi khiến wake không start lại | Cao | Bắt chước đúng trình tự của `shutdown_all_language_servers`; test wake phải khẳng định server thật sự start lại |
| Wake đúng lúc user vừa bấm "go to definition" → im lặng | Trung bình | Toast/status "đang index lại", không im lặng thất bại |
| File lớn 14.7k dòng, dễ sửa lệch chỗ | Trung bình | Phase đứng riêng, một commit một việc, không refactor kèm |
| Prettier/DAP có vòng đời riêng chưa rõ | Trung bình | Đọc trước khi sửa; nếu Prettier không lazy-start thì để nguyên và ghi lý do vào plan, đừng đoán |

## Security Considerations

- Stop server phải qua đường shutdown của LSP (`shutdown_language_server`, `:10989`) — không kill
  process thẳng, tránh để lại file tạm/lock của server.
- Không log nội dung diagnostics (có thể chứa mã nguồn) khi hibernate/wake.

## Next Steps

- Phase 4 xử lý phần sự thật của summary stale: phát hiện file đổi trên đĩa lúc ngủ.
- Phase 6 đo mức RAM thật giảm được, và hiệu chỉnh `hibernate_after_ms` theo chi phí wake đo được.
- Bước cuối của phase này flip `retain_background_projects` sang `true`.
