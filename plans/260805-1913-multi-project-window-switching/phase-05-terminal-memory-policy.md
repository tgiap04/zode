# Phase 05 — Chính sách bộ nhớ terminal

## Context Links

- [plan.md](./plan.md) · [Phase 02](./phase-02-project-activity-governor.md)
- [reports/scout-report.md](./reports/scout-report.md) § Phase 5
- `crates/terminal/src/terminal.rs`, `crates/project/src/terminals.rs`

## Overview

- **Priority:** P3 — phase nhỏ nhất, và có thể kết luận là **không cần làm gì**
- **Status:** Pending
- **Effort:** 1 ngày (bao gồm phần đo để quyết định)
- **Phụ thuộc:** chạy **sau Phase 6** (red team Finding 7) — phần đo ở đây dùng hạ tầng `sysinfo` mà
  Phase 6 dựng. Hai đường đo riêng sẽ cho hai con số không so được với nhau.
- **Test bất biến "không bao giờ dừng tiến trình user" đã chuyển sang Phase 2** (red team Finding 12),
  vì phase này có thể đóng lại bằng no-op và test đó thì không được phép mất.

Quyết định đã chốt: tiến trình user trong terminal **luôn được giữ chạy**, chỉ chặn bộ nhớ log. Phase
này đo trước rồi mới siết, vì siết scrollback của terminal đang sống là hành vi **xoá log của người
dùng** — không được làm im lặng.

## Key Insights

- Bộ đệm vòng **đã có sẵn**: alacritty grid là ring buffer; `MAX_SCROLL_HISTORY_LINES = 100_000`
  (`terminal.rs:343`), default `10000` (`default.json:1604`). Đề bài ban đầu tưởng phải dựng cái này —
  không phải.
- Đường đổi config lúc chạy đã có: `self.term.lock().set_options(self.term_config.clone())`
  (`terminal.rs:1297`). **Chưa xác minh** alacritty fork của Zed (rev `9d9640d4`) có thu nhỏ grid
  history trong `set_options` hay không — bước 1 của phase này là thử.
- Terminal của task luôn dùng `MAX_SCROLL_HISTORY_LINES` (`terminal.rs:548-552`), bỏ qua setting user.
  Đây mới là chỗ có thể phình to nhất: một task `npm run dev` xả log liên tục giữ tới 100k dòng.
- Enumerate terminal theo project: `Project.terminals.local_handles` (`terminals.rs:27`, accessor `:585`).
- **Siết là mất mát không hoàn lại.** Hạ 10k → 2k xoá 8k dòng cũ; wake nới lại không lấy được về.

## Requirements

**Functional**

- FR1: Đo và ghi lại bộ nhớ thật của grid alacritty theo (số dòng × số cột) trên máy thật. Kết quả đo
  quyết định các FR còn lại có được thực thi hay không.
- FR2: Tiến trình trong terminal **không bao giờ** bị stop, suspend hay kill bởi governor. Đây là bất
  biến của cả plan — **test nằm ở Phase 2** để không mất khi phase này đóng bằng no-op.
- FR3 *(chỉ làm nếu FR1 cho thấy đáng)*: setting
  `workspace.multi_project.background_scroll_history_lines`, **default `null` = không siết**. Khi đặt
  số, terminal của project `Hibernated` hạ scrollback về mức đó.
- FR4: Nếu FR3 được bật, phải có cảnh báo rõ trong comment của `default.json`: siết sẽ **xoá** dòng log
  cũ và không hoàn lại được.
- FR5: Terminal của task (`MAX_SCROLL_HISTORY_LINES`) cũng chịu FR3 khi project ngủ — đây là ca hưởng
  lợi nhiều nhất.
- FR6: Wake nới scrollback trở lại giá trị bình thường cho terminal mới sinh; không hứa phục hồi dòng
  đã mất.

**Non-functional**

- NFR1: Không đổi hành vi terminal của project đang active, một chút cũng không.
- NFR2: Nếu `set_options` không thu nhỏ history, phase này **kết thúc ở kết luận đo** và ghi lại lý do —
  không hack vào alacritty fork.

## Architecture

```
Bước 1 (đo):   1 terminal, 10k dòng × 200 cột → RSS delta = ?
               → nếu < ~5MB/terminal: KẾT LUẬN "không cần siết", đóng phase
               → nếu đáng kể: sang bước 2

Bước 2 (siết): ActivityChanged(Hibernated)
                 └─ for terminal in project.terminals.local_handles
                      └─ term_config.scrolling_history = N; term.lock().set_options(config)
```

## Related Code Files

**Modify (chỉ khi bước đo cho thấy đáng)**

- `crates/terminal/src/terminal.rs` — `pub fn set_scroll_history_limit(&mut self, lines: usize)` gói
  quanh `term_config` + `set_options` (:1297)
- `crates/project/src/terminals.rs` — helper áp cho toàn bộ `local_handles` của project
- `crates/project/src/project.rs` — nối vào `ActivityChanged`
- `crates/settings_content/src/workspace.rs`, `crates/workspace/src/workspace_settings.rs`,
  `assets/settings/default.json` — setting FR3

**Read for context**

- `terminal.rs:343`, `:354-365`, `:548-559`, `:1297`, `:2323`

## Implementation Steps

1. **Đo trước khi viết gì**, dùng `ProjectResourceStats` + đường đọc RSS do **Phase 6** dựng (không tự
   viết đường đo thứ hai). Mở terminal, xả đủ 10k dòng, đọc RSS trước/sau. Ghi số vào
   `reports/terminal-memory-measurement.md`.
2. Thử `set_options` với `scrolling_history` nhỏ hơn → đọc `term.history_size()` (`terminal.rs:1660`
   dùng nó) để xác nhận có thu nhỏ thật.
3. Nếu (1) nhỏ hoặc (2) không hoạt động → viết kết luận vào report, đánh phase là Completed-as-no-op,
   dừng. Đây là kết quả hợp lệ, không phải thất bại.
4. Nếu đáng: `set_scroll_history_limit` + helper theo project + setting default `null`.
5. Test bất biến FR2: project background có terminal chạy tiến trình dài → hibernate → tiến trình vẫn
   sống, vẫn nhận được output sau khi wake.
6. `./script/clippy`, `cargo test -p terminal -p project`.

## Todo List

- [ ] Đo RSS grid 10k dòng **bằng hạ tầng Phase 6**, ghi `reports/terminal-memory-measurement.md`
- [ ] Xác minh `set_options` thu nhỏ history (đọc `history_size()`)
- [ ] Quyết định: siết hay đóng phase là no-op
- [ ] *(nếu siết)* `set_scroll_history_limit` + helper + setting default `null`
- [ ] *(nếu siết)* Comment cảnh báo mất log trong `default.json`
- [ ] Xác nhận test bất biến FR2 đã tồn tại và xanh ở Phase 2 (không viết lại ở đây)
- [ ] `./script/clippy` sạch

## Success Criteria

- Có con số đo thật trong `reports/`, không phải suy diễn.
- Test bất biến FR2 xanh: hibernate không dừng tiến trình nào của user.
- Nếu siết được bật: terminal project active không đổi hành vi; project ngủ giảm RSS đo được.
- Nếu kết luận no-op: report nói rõ vì sao, để không ai phải đo lại.

## Risk Assessment

| Rủi ro | Mức | Giảm thiểu |
|---|---|---|
| Siết xoá log user đang cần (build log 30 phút trước) | **Cao** | Default `null` (tắt); comment cảnh báo; không bao giờ bật ngầm |
| `set_options` không thu nhỏ, hoặc panic khi grid đang bị đọc | Trung bình | Xác minh ở bước 2 trước khi thiết kế dựa vào nó; `term.lock()` giữ đúng khuôn gọi hiện có |
| Tiến trình user bị ảnh hưởng ngoài ý muốn (PTY resize side-effect) | Trung bình | `set_options` không đổi kích thước cửa sổ; test FR2 canh đúng chỗ này |
| Đo sai vì alacritty cấp phát grid lười (lazy) | Trung bình | Đo sau khi đã đẩy đủ dòng để lấp buffer, không đo lúc terminal vừa mở |

## Security Considerations

Log terminal có thể chứa secret (token in ra khi debug). Siết scrollback **giảm** thời gian secret nằm
trong RAM — một lợi ích phụ, nhưng không được dùng làm lý do bật ngầm.

## Next Steps

Số đo từ phase này là đầu vào cho Phase 6 (ngưỡng cầu chì) và cho việc chốt `hibernate_after`.
