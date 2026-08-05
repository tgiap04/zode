# Phase 04 — Worktree pause/resume và đối chiếu đĩa

## Context Links

- [plan.md](./plan.md) · [Phase 02](./phase-02-project-activity-governor.md)
- [reports/scout-report.md](./reports/scout-report.md) § Phase 4
- `crates/worktree/src/worktree.rs`, `crates/project/src/buffer_store.rs`

## Overview

- **Priority:** P2 — tiết kiệm ít RAM hơn Phase 3, nhưng giải phóng inotify watch (giới hạn thật trên Linux)
- **Status:** Pending
- **Effort:** 2 ngày

Project `Hibernated` → drop background scanner + fs watcher. Wake → `restart_background_scanners` và
**đối chiếu buffer đang mở với đĩa** trước khi cho người dùng tin vào nội dung trên màn hình.

## Key Insights

- Đường resume đã tồn tại và đã được dùng: `restart_background_scanners` (`worktree.rs:1121`) được gọi
  ở `:454`, `:2030`, `:2048`. Ta không phát minh gì, chỉ thêm một trigger.
- `_background_scanner_tasks: Vec<Task<()>>` (`:134`, set ở `:1228`) — drop là cancel, đúng ngữ nghĩa
  GPUI. Pause = `std::mem::take` cái Vec đó.
- **Đây là phase có mùi mất dữ liệu.** Buffer mở, project ngủ, `git checkout` bên ngoài → snapshot cũ,
  buffer báo clean, nội dung trên màn hình khác đĩa. Nếu user gõ tiếp rồi save, họ ghi đè thứ họ chưa
  từng thấy.
- Giới hạn inotify trên Linux (thường 8192 watch/user) là lợi ích thật: 6 project mở cùng lúc rất dễ
  chạm trần, và lỗi khi chạm trần rất khó hiểu với người dùng.
- `buffer_store.rs` đã có `reload_buffers` (3 chỗ: `:358`, `:742`, `:1501`) và `language/src/buffer.rs`
  đã có phát hiện `file_changed` (`:1665-1685`). Wake phải đi qua đường này, không viết đường mới.

## Requirements

**Functional**

- FR1: `Hibernated` → mọi worktree của project drop scanner + watcher.
- FR2: `Active` → restart scanner cho mọi worktree, ưu tiên worktree chứa buffer đang mở.
- FR3: Sau rescan, mọi buffer đang mở **không dirty** mà file trên đĩa đã đổi → reload im lặng (đúng
  hành vi hiện tại của Zed khi file đổi).
- FR4: Buffer **dirty** mà file trên đĩa đã đổi → cảnh báo xung đột theo đúng cơ chế hiện có, không tự
  ghi đè, không tự reload.
- FR5: File bị xoá trên đĩa lúc ngủ → theo `close_on_file_delete` setting hiện có, không tự phát minh.
- FR6: Summary diagnostics stale (Phase 3) của file đã đổi trên đĩa → xoá **sớm** ngay khi rescan phát
  hiện. Đây chỉ là đường xoá sớm; việc xoá dứt điểm toàn bộ cờ stale thuộc Phase 3 FR4b (khi index xong)
  — hai đường bổ sung cho nhau, không thay thế nhau.
- FR7: Git status của project ngủ không refresh; wake phải trigger refresh một lần.

**Non-functional**

- NFR1: Rescan chạy nền, không block frame đầu của wake.
- NFR2Trên project lớn (>100k file), rescan không được làm treo UI — dùng đúng cơ chế scan tăng dần đã có.

## Architecture

```
Hibernated:  Worktree::pause_scanning()
               └─ std::mem::take(&mut self._background_scanner_tasks)   // drop ⇒ cancel
Active:      Worktree::resume_scanning()
               └─ restart_background_scanners(cx)                        // đã tồn tại
                    └─ scan xong ⇒ worktree emit UpdatedEntries
                         ├─ buffer_store: reload_buffers cho buffer sạch có mtime đổi
                         ├─ buffer dirty + file đổi ⇒ đường xung đột hiện hành
                         └─ lsp_store: xoá cờ stale của path đã đổi
```

Điểm nối: dùng chính event `UpdatedEntries`/`UpdatedGitRepositories` mà worktree đã emit — không tự
diff snapshot bằng tay.

## Related Code Files

**Modify**

- `crates/worktree/src/worktree.rs` — `pub fn pause_scanning()`, `pub fn resume_scanning()`; cờ
  `scanning_paused` để `resume` idempotent
- `crates/project/src/project.rs` — trong xử lý `set_activity`, gọi pause/resume cho mọi worktree
- `crates/project/src/buffer_store.rs` — đường reconcile sau resume (nếu event hiện có chưa đủ để
  trigger, thêm một lần gọi `reload_buffers` cho buffer sạch)
- `crates/project/src/lsp_store.rs` — xoá cờ stale cho path đã đổi (khớp Phase 3)
- `crates/project/src/git_store.rs` — refresh một lần khi resume

**Read for context**

- `worktree.rs:1121-1228`, `:2030`, `:2048`; `buffer_store.rs:358-440`; `buffer.rs:1665-1685`

## Implementation Steps

1. Test-first: `test_hibernate_pauses_scanning` — hibernate, ghi file qua fake fs, khẳng định snapshot
   worktree **không** đổi (chứng minh watcher thật sự tắt).
2. Test: `test_wake_reconciles_external_change` — cùng bối cảnh, wake, khẳng định snapshot cập nhật và
   buffer sạch đã reload.
3. Test: `test_wake_does_not_clobber_dirty_buffer` — buffer dirty + file đổi ⇒ không reload, đi đúng
   đường xung đột.
4. `pause_scanning` / `resume_scanning` trên `Worktree`, có cờ để gọi lặp không sinh scanner trùng.
5. Nối vào `Project` theo `ActivityChanged`. Chỉ áp cho worktree local; worktree remote đi qua kết nối
   SSH — xem FR7 của Phase 3, không đóng kết nối.
6. Nối reconcile: ưu tiên dựa vào event worktree phát ra sau rescan. Chỉ thêm gọi tay nếu test bước 2
   chứng minh event không đủ.
7. Xoá cờ stale diagnostics cho path đã đổi.
8. Git refresh một lần khi resume.
9. `./script/clippy`, `cargo test -p worktree -p project`.

## Todo List

- [ ] 3 test (pause, reconcile, không ghi đè buffer dirty)
- [ ] `pause_scanning` / `resume_scanning` + cờ idempotent
- [ ] Nối vào `ActivityChanged`, chỉ worktree local
- [ ] Reconcile buffer sau rescan
- [ ] Xoá cờ stale cho file đã đổi
- [ ] Git status refresh khi wake
- [ ] Thử tay: hibernate → `git checkout` nhánh khác → wake → kiểm nội dung buffer
- [ ] `./script/clippy` sạch

## Success Criteria

- Hibernate rồi sửa file ngoài editor: snapshot không đổi (watcher tắt thật).
- Wake: buffer sạch phản ánh đĩa; buffer dirty không bị ghi đè và có cảnh báo.
- Thử tay `git checkout` một nhánh lớn trong lúc project ngủ → wake không hiển thị nội dung sai im lặng.
- `cargo test -p worktree -p project` xanh.

## Risk Assessment

| Rủi ro | Mức | Giảm thiểu |
|---|---|---|
| Buffer sạch nhưng nội dung khác đĩa, user gõ tiếp rồi save ⇒ **mất dữ liệu** | **Cao** | Test bước 2/3 là điều kiện chặn merge; thử tay với `git checkout` bắt buộc |
| Rescan project lớn khi wake làm giật UI | Trung bình | Rescan nền; frame đầu vẽ từ snapshot cũ, cập nhật khi scan xong |
| `restart_background_scanners` tạo channel mới ⇒ request scan cũ mất | Trung bình | Đọc `:1121-1128`; hàng đợi scan lúc pause phải coi như bỏ, không giữ để replay |
| Worktree remote/SSH: pause làm mất trạng thái kết nối | Trung bình | Chỉ áp cho local ở phase này; remote ghi thành việc riêng |
| Giới hạn inotify vẫn chạm nếu user mở nhiều project cùng lúc và tất cả đều warm | Thấp | Phase 6 (cầu chì) là chỗ xử lý |

## Security Considerations

Không xoá/ghi file nào trong phase này. Reload buffer phải đi qua `fs` trait sẵn có, không đọc thẳng
`std::fs`, để giữ được hành vi trong test và trên remote.

## Next Steps

Phase 6 đo phần RAM/watch tiết kiệm được từ phase này để biết nó có đáng giữ hay không.
