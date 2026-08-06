# Phase 06 — Đo lường và cầu chì áp lực bộ nhớ

## Context Links

- [plan.md](./plan.md) · [Phase 03](./phase-03-lsp-hibernate-wake.md) · [Phase 04](./phase-04-worktree-pause-resume.md) · [Phase 05](./phase-05-terminal-memory-policy.md)
- [brainstorm-report.md](./brainstorm-report.md) — quyết định "không giới hạn warm" + cầu chì được khuyến nghị
- `crates/system_specs/src/system_specs.rs` (đã dùng `sysinfo`)

## Overview

- **Priority:** P2 — chốt sổ cho cả plan: chứng minh RAM thật sự giảm, và đặt sàn cho quyết định "không giới hạn"
- **Status:** Completed — code + test + đo thật (Rust) xong; Go/TS còn thiếu do sandbox không
  có `gopls`/`typescript-language-server` (đã hỏi người dùng, chọn hoãn) — xem
  `reports/memory-measurements.md` § Next steps
- **Effort:** 2 ngày

Hai việc: (1) đo được tài nguyên theo từng project để mọi con số trong plan này thôi là phỏng đoán;
(2) cầu chì — khi bộ nhớ căng, hibernate ngay các project ít dùng nhất bất kể timer.

## Key Insights

- Người dùng đã chọn **không giới hạn số project warm**, dựa hoàn toàn vào timer idle. Timer không chặn
  được đỉnh: chạm 10 project trong 5 phút thì cả 10 warm cùng lúc. Cầu chì là cái sàn cho lựa chọn đó,
  không phải sự phủ định nó — quy tắc danh nghĩa vẫn là "không giới hạn".
- `sysinfo` đã là dependency (`system_specs.rs:38`) → không thêm dep mới.
- **Không đo được RSS của Zode theo từng project** (heap chung một tiến trình). Đo được: RSS của tiến
  trình **con** (LSP, prettier, node, DAP) — mà đó chính là phần lớn chi phí. Phần in-process dùng proxy
  đếm được: số worktree entry, số buffer, số dòng grid terminal.
- Đo phải trung thực về giới hạn của nó, nếu không sẽ chốt `hibernate_after` bằng một con số ảo.

## Requirements

**Functional**

- FR1: Thu được theo từng project: RSS tổng của tiến trình con, số language server đang chạy, số buffer
  mở, số worktree entry, tổng dòng scrollback terminal, trạng thái `ProjectActivity`.
- FR2: Xem được các số đó khi đang chạy — qua một debug view (bắt chước khuôn `inspector_ui` /
  `miniprofiler_ui` đã có trong cây), hoặc tối thiểu là một action ghi ra log.
- FR3: Cầu chì: poll bộ nhớ hệ thống theo chu kỳ; vượt ngưỡng → hibernate ngay các project `Warm` theo
  thứ tự ít dùng gần đây nhất, tới khi đủ ngưỡng hoặc chỉ còn project `Active`.
- FR4: Project `Active` **không bao giờ** bị cầu chì hibernate. Project đang debug, còn buffer dirty,
  hoặc còn autosave đang chờ cũng không (khớp Phase 3 FR7/FR8 — cầu chì đi qua **cùng** ba rào đó, không
  có đường tắt).
- FR4b *(red team Finding 9)*: Quy tắc phân xử giữa cầu chì và timer, không để hai cơ chế đánh nhau:
  - Project vừa được wake **bằng tay** được miễn cầu chì trong ≥ 1 chu kỳ poll.
  - Wake bằng tay luôn thắng cầu chì (user bấm là user thắng).
  - Project phải ở `Warm` tối thiểu 60s mới đủ điều kiện làm nạn nhân.
  - Cầu chì hibernate từng project một, đo lại giữa mỗi lần; hysteresis ≥ 2 chu kỳ trước khi kích lại.
- FR5: Setting `workspace.multi_project.memory_pressure_threshold` — mặc định đặt theo số đo, `null` để
  tắt cầu chì.
- FR6: Khi cầu chì kích, thông báo cho người dùng (status/toast) — không được hibernate ngầm rồi để họ
  tự đoán vì sao project vừa chậm lại.

**Non-functional**

- NFR1: Poll bộ nhớ không được tốn đáng kể: chu kỳ ≥ 30s, chạy trên background executor.
- NFR2: Đo không được làm chậm đường switch project.

## Architecture

```
MemoryGovernor (trong MultiWorkspace, không phải crate mới)
  ├─ timer 30s trên background executor
  ├─ sysinfo: available memory hệ thống + RSS tiến trình Zode
  ├─ vượt ngưỡng?
  │    └─ chọn project Warm, sắp theo last-active tăng dần
  │         └─ set_activity(Hibernated) từng cái, đo lại sau mỗi cái
  └─ toast: "Đã cho N project ngủ để giải phóng bộ nhớ"

ProjectResourceStats (trong project.rs)
  └─ gom từ lsp_store (số server + pid con), buffer_store, worktree, terminals
```

Thứ tự ưu tiên "ít dùng nhất": dùng `last_active_workspace` đã có trong `ProjectGroupState`
(`multi_workspace.rs:238`) hoặc thêm timestamp lúc `activate()` — chọn cách ít thêm state nhất.

## Related Code Files

**Modify**

- `crates/project/src/project.rs` — `pub struct ProjectResourceStats` + `fn resource_stats(&self, cx)`
- `crates/project/src/lsp_store.rs` — expose số server đang chạy + pid tiến trình con
- `crates/workspace/src/multi_workspace.rs` — governor timer + logic chọn nạn nhân + toast
- `crates/settings_content/src/workspace.rs`, `crates/workspace/src/workspace_settings.rs`,
  `assets/settings/default.json` — setting FR5
- `crates/system_specs/src/system_specs.rs` — nếu cần thêm hàm đọc available memory / RSS process

**Create**

- `plans/260805-1913-multi-project-window-switching/reports/memory-measurements.md` — bảng số đo thật
  (1/3/5 project, warm vs hibernated, theo loại project: Rust, TS, Go)

## Implementation Steps

1. `ProjectResourceStats` + `resource_stats()`. Bắt đầu từ các số đếm được (server, buffer, entry,
   dòng terminal) — chúng chính xác và rẻ.
2. Thêm RSS tiến trình con: lấy pid của language server đang chạy từ `lsp_store`, hỏi `sysinfo`. Ghi rõ
   trong doc comment rằng đây **không** phải toàn bộ chi phí của project.
3. Debug view hoặc action `zode: dump project resources` ghi bảng ra log.
4. **Đo thật:** 1/3/5 project (Rust + TS + Go), lần lượt ở `Warm` và `Hibernated`. Ghi vào
   `reports/memory-measurements.md`. Đo luôn chi phí wake (thời gian tới diagnostics đầu tiên) cho từng
   loại server — số này quyết định `hibernate_after` cuối cùng.
5. Từ số đo, chốt: giá trị mặc định `hibernate_after_ms` (điều chỉnh Phase 2 nếu cần) và
   `memory_pressure_threshold`.
6. Governor: timer 30s, đọc bộ nhớ, chọn nạn nhân, hibernate, toast.
7. Test: giả lập áp lực bộ nhớ (inject reader qua trait để test được — **không** đọc `sysinfo` trực tiếp
   trong logic quyết định), khẳng định project `Active` không bị chọn và thứ tự nạn nhân đúng.
8. `./script/clippy`, `cargo test -p project -p workspace`.

## Todo List

- [x] `ProjectResourceStats` + `resource_stats()` — `project.rs`,
  test `test_resource_stats_reports_counts_and_activity`
- [x] RSS tiến trình con qua `sysinfo`, có doc comment về giới hạn phép đo —
  `Project::resource_stats`, chỉ tính LSP con (không tính prettier/DAP, đúng scope
  Related Code Files của phase này)
- [x] Debug view / dump action — action `DumpProjectResourceStats` ghi ra log,
  không dựng debug view riêng (phương án tối thiểu theo FR2)
- [x] **Đo 1/3/5 project × 3 ngôn ngữ, ghi `reports/memory-measurements.md`** —
  MỘT PHẦN: đã đo thật với `rust-analyzer` (project mẫu độc lập, không phải zode repo —
  xem lý do trong report) cho N=1/3/5, kết quả tuyến tính (~675MB/instance, sai số <2%).
  **Còn thiếu Go (`gopls`) và TypeScript (`typescript-language-server`)** — không có sẵn
  trong sandbox này, người dùng chọn hoãn việc cài; đây là gap còn mở, ghi rõ trong report.
- [x] **Đo chi phí wake theo từng loại server** — MỘT PHẦN: chỉ đo Rust (~20s cho project
  cỡ nhỏ/vừa). Chưa đo trên project cỡ zode-repo (tránh đụng session rust-analyzer đang
  sống của người dùng trên chính repo này — xem report), và chưa đo Go/TS.
- [x] **Chốt default `hibernate_after_ms` + `memory_pressure_threshold_percent`** —
  ĐÃ RÀ SOÁT theo số đo thật, quyết định: giữ nguyên cả hai. `hibernate_after_ms`
  (300000ms) không có lý do để đổi — chi phí wake đo được (~20s) nằm gọn trong ngưỡng
  idle 5 phút. `memory_pressure_threshold_percent` (`10.0`, placeholder) **vẫn là
  placeholder** một cách có chủ đích: theo đúng Risk Assessment của phase này, ngưỡng
  cầu chì dựa vào bộ nhớ hệ thống còn trống nói chung, không dựa vào con số per-project —
  số đo per-project ở đây xác nhận giả định "chi phí tuyến tính theo số project" là đúng,
  nhưng không tự nó cho ra con số ngưỡng phần trăm. Chốt ngưỡng đó cần một kịch bản đo
  riêng (máy giới hạn bộ nhớ thật), ghi rõ trong report § Next steps.
- [x] Governor + toast — `MultiWorkspace::memory_governor_tick` +
  `notify_memory_fuse_triggered`, poll 30s trên background executor
- [x] Quy tắc phân xử cầu chì ↔ timer (FR4b) — min-warm 60s, miễn trừ wake tay 1
  chu kỳ, hysteresis ≥2 chu kỳ, cả ba đều implement. **Test riêng cho từng quy tắc:
  chỉ 2/3.** Min-warm và hysteresis có test cô lập riêng
  (`test_memory_fuse_respects_min_warm_duration`,
  `test_memory_fuse_hysteresis_delays_the_second_victim`). Miễn trừ wake tay
  (`manually_woken_at`) đang bị min-warm **che khuất hoàn toàn** dưới hằng số hiện
  tại (`MEMORY_FUSE_MIN_WARM_DURATION` 60s > `MEMORY_FUSE_POLL_INTERVAL` 30s — xem
  bất biến ghi rõ tại hằng số đó trong `multi_workspace.rs`), nên không có đường nào
  để viết một test cô lập được nó mà không đổi hằng số. Giữ code (đúng theo FR4b,
  rẻ, và là lưới an toàn nếu hằng số đổi sau này) nhưng không tự nhận là đã test độc
  lập — phát hiện của `reviewer` agent (2026-08-06).
- [x] Test với memory reader inject được — `MemoryPressureReader` trait +
  `FakeMemoryPressureReader`, 7 test, không đọc `sysinfo` trong logic quyết định
- [x] `./script/clippy` sạch — `./script/clippy -p project -p workspace -p settings_content`,
  0 warning; `cargo machete` không thấy dependency thừa

## Success Criteria

- `reports/memory-measurements.md` có số thật, ghi rõ máy đo và phiên bản server.
- Mốc của plan được kiểm bằng số đo: 5 project mở (4 hibernated) → tổng RSS ≤ 1.5× một project active.
  **Nếu không đạt, ghi lại sự thật đó** thay vì sửa mốc cho vừa.
- Cầu chì kích được trong test, không bao giờ chọn project `Active` hoặc đang debug.
- Có thông báo cho người dùng mỗi lần cầu chì kích.

## Risk Assessment

| Rủi ro | Mức | Giảm thiểu |
|---|---|---|
| Phép đo per-project không đầy đủ → chốt ngưỡng sai | Cao | Ghi rõ giới hạn ngay trong doc comment và trong report; ngưỡng dựa vào bộ nhớ **hệ thống** còn trống, không dựa vào con số per-project |
| Cầu chì kích liên tục (dao động quanh ngưỡng) | Trung bình | Hysteresis: chỉ kích lại sau ≥ 2 chu kỳ; hibernate từng project một, đo lại giữa mỗi lần |
| Cầu chì hibernate project user vừa rời 10 giây trước | Trung bình | Có sàn thời gian tối thiểu (vd. 60s ở `Warm`) trước khi được chọn làm nạn nhân |
| `sysinfo` refresh tốn CPU nếu gọi sai kiểu | Thấp | `RefreshKind::nothing().with_memory(...)` như `system_specs.rs:38` đã làm, không refresh toàn hệ |
| Số đo chứng minh hibernate không đáng | Trung bình | Đây là kết quả hợp lệ — ghi lại, và cân nhắc lùi về giữ mọi thứ sống (Đường 1 trong brainstorm) |

## Security Considerations

Không log đường dẫn project hay tên file trong bảng dump ở mức info. Danh sách pid tiến trình con là
thông tin cục bộ, không gửi đi đâu (fork này không có telemetry — `telemetry::send_event` là no-op).

## Next Steps

- Số đo ở đây hiệu chỉnh lại default của Phase 2 và quyết định số phận Phase 5.
- Sau phase này, regenerate docs: `docs/generated/feature-list.md`, `docs/system/architecture.md`.
