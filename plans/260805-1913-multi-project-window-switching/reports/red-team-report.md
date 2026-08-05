# Red Team Review — 2026-08-05

4 lens (7 phase ⇒ full set): Security Adversary · Failure Mode Analyst · Assumption Destroyer ·
Scope & Complexity Critic. Mọi finding phải có citation `file:line` mới được xét; finding không có
citation bị loại trước khi bàn tới nội dung.

**Tổng:** 14 finding — **12 Accept**, 2 Reject. 1 Critical · 4 High · 7 Medium.

---

## Finding 1: `hibernate_after` dạng chuỗi thời lượng là hư cấu — CRITICAL

**Reviewer:** Assumption Destroyer
**Location:** Phase 2, FR5 + Implementation Steps bước 9; Phase 6 bước 5
**Flaw:** Plan viết default `"hibernate_after": "5m"`. Hệ settings của Zode không có kiểu duration
dạng chuỗi — nó dùng số nguyên millisecond.
**Failure scenario:** Người thực thi thêm field `Option<String>` rồi tự viết parser `"5m"`, hoặc phát
hiện ra giữa đường và phải thiết kế lại settings + default.json + migration.
**Evidence:** `crates/settings_content/src/workspace.rs:1024` — `pub debounce_ms: Option<u64>`;
`:541` — `AfterDelay { milliseconds: DelayMs }`. Đây là hai khuôn duy nhất có sẵn trong file.
**Disposition:** **Accept**
**Fix:** Dùng `hibernate_after_ms: Option<u64>` (hoặc kiểu `DelayMs` như `AfterDelay`), default
`300000`. Không phát minh parser.

## Finding 2: Phase 1 bỏ sót đường retention thứ ba — HIGH

**Reviewer:** Assumption Destroyer
**Location:** Phase 1, Related Code Files + Implementation Steps bước 4
**Flaw:** Plan chỉ kể `activate()` (có gate) và `add()` (luôn retain). Còn một đường thứ ba push
thẳng vào `retained_workspaces` không qua gate nào.
**Failure scenario:** Sau Phase 1, đặt `retain_background_projects: false` nhưng mở project qua đường
provisional vẫn thấy workspace bị retain ⇒ setting nói dối, và Phase 2 gắn timer vào một workspace mà
người dùng tưởng đã bị thả.
**Evidence:** `crates/workspace/src/multi_workspace.rs:675-690` `activate_provisional_workspace` —
`self.retained_workspaces.push(workspace.clone())` không kiểm `sidebar_open` lẫn setting; gọi từ
`crates/workspace/src/workspace.rs:10058`.
**Disposition:** **Accept**
**Fix:** Phase 1 phải xử lý cả 3 đường qua cùng một `should_retain()`; thêm test cho đường provisional.

## Finding 3: Autosave đua với hibernate, format-on-save chết im lặng — HIGH

**Reviewer:** Failure Mode Analyst
**Location:** Phase 3, Architecture + FR1
**Flaw:** Hibernate stop language server mà không xét autosave đang chờ. Format-on-save đi qua LSP.
**Failure scenario:** `autosave: { after_delay: { milliseconds: 30000 } }`, user rời project A sang B.
Cầu chì (Phase 6) hoặc ngưỡng idle ngắn đưa A vào `Hibernated`. Autosave của A nổ sau đó → format qua
LSP không có server → file được lưu **không format**, hoặc save treo. Người dùng không thấy gì bất
thường cho tới lần review code.
**Evidence:** `crates/settings_content/src/workspace.rs:537-554` — `AutosaveSetting::AfterDelay { .. }`,
`OnFocusChange`, `OnWindowChange`; `crates/project/src/lsp_store.rs:1629`, `:1651` — formatter chọn
theo `settings.format_on_save`.
**Disposition:** **Accept**
**Fix:** Không hibernate project còn buffer dirty **hoặc** còn autosave đang chờ. Nếu vẫn hibernate,
phải await request format/save đang bay trước khi stop server.

## Finding 4: Code salvage có thể mang lại symbol auth đã bị xoá — HIGH

**Reviewer:** Security Adversary
**Location:** Phase 7, Implementation Steps bước 3-7
**Flaw:** Nguồn salvage là commit **trước** khi auth/cloud bị gỡ. Port thiếu cảnh giác sẽ kéo lại
tham chiếu tới đúng những symbol mà hard-fork đã cắt.
**Failure scenario:** Port `connect_remote` kèm nhánh gọi API đăng nhập → hoặc không compile (may),
hoặc compile được nhờ một survivor còn sót và fork âm thầm có lại một đường mạng ra ngoài (tệ).
**Evidence:** `plans/260726-1531-remove-auth-cloud-hard-fork/plan.md` § "four findings" #4 liệt kê
chính lớp lỗi này kèm lệnh `rg` (`sign_in_with_optional_connect|has_credentials|Plan::Zed…`);
sidebar cũ có `connect_remote` tại `c3e2ac3^:crates/sidebar/src/sidebar.rs:445`.
**Disposition:** **Accept**
**Fix:** Phase 7 thêm gate: chạy đúng lệnh `rg` của hard-fork plan trên `crates/sidebar/` mới, phải ra
0 kết quả, trước khi phase được coi là xong.

## Finding 5: Summary stale che lỗi ở file *không* đổi — HIGH

**Reviewer:** Security Adversary
**Location:** Phase 3 FR4 + Phase 4 FR6
**Flaw:** Cờ stale chỉ được xoá cho path mà rescan thấy đổi (Phase 4 FR6) hoặc khi server mới publish
cho đúng file đó (Phase 3 FR4). File **không** đổi nhưng vỡ vì dependency đổi thì giữ nguyên summary
sạch cũ.
**Failure scenario:** `git pull` trong lúc project ngủ đổi một type ở crate lõi. Wake: cây file hiện 0
lỗi ở 40 file phụ thuộc, người dùng tin là sạch, commit tiếp. Với lint bảo mật chạy qua LSP thì đây là
che cảnh báo bảo mật.
**Evidence:** `crates/project_panel/src/project_panel.rs:1015`, `:1044` đọc thẳng
`diagnostic_summaries`; `crates/diagnostics/src/diagnostics.rs:463` tương tự — cả hai không biết gì về
cờ stale.
**Disposition:** **Accept**
**Fix:** Xoá **toàn bộ** cờ stale của project khi server báo xong đợt index đầu tiên sau wake (progress
end), thay vì xoá lẻ theo từng file publish.

## Finding 6: `app_will_quit` không flush state từng workspace — MEDIUM

**Reviewer:** Failure Mode Analyst
**Location:** Phase 1, Risk Assessment
**Flaw:** Có hai đường quit. Đường trong `zed.rs` lặp qua `workspaces()` và flush từng cái. Đường
`app_will_quit` của `MultiWorkspace` chỉ await `_serialize_task` + `pending_removal_tasks`.
**Failure scenario:** Nếu tồn tại đường quit nào chỉ chạm `app_will_quit` (quit do OS, logout), N
workspace retained mất state tab của lần lưu debounce gần nhất.
**Evidence:** `crates/workspace/src/multi_workspace.rs:1532-1543` (chỉ 2 nguồn task) so với
`crates/zed/src/zed.rs:1431-1445` (lặp `multi_workspace.workspaces()`).
**Disposition:** **Accept** (dưới dạng bước xác minh, không phải đổi thiết kế)
**Fix:** Phase 1 thêm việc: liệt kê mọi đường quit, chứng minh đường nào cũng đi qua vòng lặp ở
`zed.rs`; nếu không, gọi `flush_all_serialization` trong `app_will_quit`.

## Finding 7: Phase 5 đứng sai thứ tự — MEDIUM

**Reviewer:** Scope & Complexity Critic
**Location:** Phase 5, Implementation Steps bước 1
**Flaw:** Phase 5 bước 1 đo RSS "qua `sysinfo`", nhưng hạ tầng đọc RSS lại do Phase 6 mới dựng.
**Failure scenario:** Người thực thi Phase 5 tự viết đường đo tạm, rồi Phase 6 viết đường thứ hai —
hai cách đo cho hai con số khác nhau, không ai biết tin cái nào.
**Evidence:** Phase 5 bước 1 vs Phase 6 bước 2; `crates/system_specs/src/system_specs.rs:38` hiện chỉ
đọc `total_memory()`, chưa có RSS per-process.
**Disposition:** **Accept**
**Fix:** Đổi thứ tự: Phase 5 chạy **sau** Phase 6, dùng chung hạ tầng đo. Cập nhật `plan.md`.

## Finding 8: "Hai workspace chia một Project" là giả định chưa chứng minh — MEDIUM

**Reviewer:** Assumption Destroyer
**Location:** Phase 2, Architecture ("Vì sao trạng thái nằm ở Project") + Risk row 1
**Flaw:** Plan lấy việc hai workspace chia một `Entity<Project>` làm lý do đặt state ở `Project`, và
làm luôn một risk row. Không có citation nào chứng minh việc chia sẻ đó xảy ra.
**Failure scenario:** Nếu mỗi workspace luôn có `Project` riêng, risk row đó là hư cấu và test viết cho
nó là test cho một trạng thái không tồn tại — lãng phí, và tạo cảm giác an toàn sai.
**Evidence:** `crates/workspace/src/multi_workspace.rs:1113-1139`
`find_or_create_workspace_with_source_workspace` chỉ nhận `provisional_project_group_key`; không có chỗ
nào trong file cho thấy `Entity<Project>` được dùng lại giữa hai workspace.
**Disposition:** **Accept**
**Fix:** Phase 2 đổi câu khẳng định thành bước xác minh: đọc đường tạo `Project` trước, rồi mới quyết
định đặt state ở `Project` hay ở `Workspace`. Giữ lý do "LSP/worktree/terminal đều thuộc Project" —
lý do đó tự đứng được.

## Finding 9: Cầu chì và timer đánh nhau, không ai định nghĩa ai thắng — MEDIUM

**Reviewer:** Failure Mode Analyst
**Location:** Phase 6 FR3 + Risk row 3
**Flaw:** Cầu chì có sàn 60s, timer có ngưỡng 5 phút. Không có mô tả nào về việc sau khi cầu chì
hibernate một project rồi user wake nó lại thì timer chạy từ đâu, và cầu chì có được kích lại ngay không.
**Failure scenario:** Bộ nhớ vẫn căng sau khi wake → cầu chì hibernate lại ngay project user vừa mở →
vòng lặp wake/hibernate, mỗi lần trả tiền index.
**Evidence:** Phase 6 FR3/FR4 không nói gì về tương tác với `hibernate_timers`
(`multi_workspace.rs` field do Phase 2 thêm).
**Disposition:** **Accept**
**Fix:** Quy tắc rõ: project vừa được wake bằng tay được miễn cầu chì trong ≥ 1 chu kỳ; cầu chì không
bao giờ chọn project `Active`; wake bằng tay luôn thắng cầu chì.

## Finding 10: Phase 7 có thể không thuộc plan này — MEDIUM

**Reviewer:** Scope & Complexity Critic
**Location:** Phase 7 toàn phase (4–5 ngày, 1 crate mới, ~2.500 dòng port)
**Flaw:** Giá trị cốt lõi của plan (retention + hibernate) hoàn chỉnh mà không cần Phase 7 — chính
`plan.md` viết rằng Phase 1 đã cho đường chuyển bằng keybinding.
**Failure scenario:** Phase 7 chiếm gần một nửa effort của plan; nó trượt thì cả plan nhìn như chưa
xong, dù 6 phase kia đã cho hết giá trị kỹ thuật.
**Evidence:** `plan.md` § Overview + § Phases (Phase 7 ghi "không chặn 2–6"); Phase 1 FR1 cho
`NextProject`/`PreviousProject` chạy khi `sidebar = None`.
**Disposition:** **Accept — đưa ra Validation để người dùng quyết**
**Fix:** Hoặc giữ trong plan (một tính năng, một plan), hoặc tách thành plan riêng nối `blockedBy`.
Quyết định thuộc người dùng, không thuộc red team.

## Finding 11: Phase 3 FR7 (remote/SSH) được khẳng định nhưng không được thiết kế — MEDIUM

**Reviewer:** Assumption Destroyer
**Location:** Phase 3 FR7
**Flaw:** FR7 nói "hibernate không đóng kết nối SSH" nhưng không có bước thực thi nào cho remote, và
`hibernate()` dựng trên `stop_local_language_server` — hàm này return sớm nếu mode không phải Local.
**Failure scenario:** Người thực thi tưởng đã lo remote, thực tế hibernate là no-op im lặng cho project
remote; RAM không giảm, và không ai biết vì sao.
**Evidence:** `crates/project/src/lsp_store.rs:11029-11033` — `let local = match &mut self.mode { LspStoreMode::Local(local) => local, _ => return Task::ready(()) }`.
**Disposition:** **Accept**
**Fix:** Phase 3 nói thẳng: v1 chỉ local; `hibernate()` early-return cho remote mode **và log ở debug**;
ghi "hibernate cho remote" thành việc ngoài scope trong Next Steps.

## Finding 12: Test bất biến quan trọng nhất có thể không bao giờ được viết — MEDIUM

**Reviewer:** Failure Mode Analyst
**Location:** Phase 5 FR2 + Success Criteria
**Flaw:** "Tiến trình user không bao giờ bị governor dừng" là bất biến quan trọng nhất của cả plan,
nhưng nó nằm trong phase được thiết kế để có thể kết thúc bằng no-op.
**Failure scenario:** Phase 5 đóng lại với kết luận "không cần siết" ⇒ test bất biến không được viết ⇒
một phase sau (hay một refactor sau) thêm đường stop terminal, không có gì bắt được.
**Evidence:** Phase 5 Implementation Steps bước 3 ("đánh phase là Completed-as-no-op, dừng") đặt trước
bước 5 (test bất biến).
**Disposition:** **Accept**
**Fix:** Chuyển test bất biến sang Phase 2 (governor) — nơi nó thuộc về, và nơi nó chắc chắn được viết.

---

## Bị loại

### R1: "Đường quit làm mất state của workspace retained" — REJECT
**Reviewer:** Failure Mode Analyst (chính tôi nêu, rồi tự bác)
**Lý do loại:** Bằng chứng bác bỏ: `crates/zed/src/zed.rs:1431-1445` lặp qua
`multi_workspace.workspaces()` và gọi `workspace.flush_serialization(window, cx)` cho **từng** cái, cộng
`multi_workspace.flush_serialization()`. `workspaces()` (`multi_workspace.rs:1344`) trả về toàn bộ
retained + active. Đường quit đã đúng. Phần còn lại của lo ngại này sống tiếp ở Finding 6, hẹp hơn.

### R2: "4 setting cho một tính năng là over-configuration" — REJECT
**Reviewer:** Scope & Complexity Critic
**Lý do loại:** Cả 4 đều là công tắc tắt cho một chính sách có thể sai trên máy người dùng
(`retain_background_projects`, `hibernate_after_ms`, `background_scroll_history_lines`,
`memory_pressure_threshold`). YAGNI áp cho code, không áp cho đường thoát của một cơ chế tự động
quản lý bộ nhớ. Mặc định đều bảo thủ; 2 trong 4 mặc định là tắt.

---

## Sweep nhất quán toàn plan (Step 10)

Sau khi áp 12 finding, rà lại `plan.md` + 7 phase file:

- `plan.md` § Phases: thứ tự phụ thuộc phải đổi theo Finding 7 (Phase 5 sau Phase 6). **Đã sửa.**
- Mọi chỗ viết `"5m"` / `hibernate_after` dạng chuỗi: Phase 2 FR5 + bước 9, Phase 6 bước 5. **Đã sửa
  thành `hibernate_after_ms`.**
- Phase 5 FR2 + Success Criteria trỏ tới test bất biến: đã chuyển sang Phase 2, Phase 5 giữ tham chiếu
  chéo. **Đã sửa.**
- Phase 3 FR4 và Phase 4 FR6 nói về cùng cơ chế xoá cờ stale: sau Finding 5, Phase 3 là nơi quyết định
  (xoá cả loạt khi index xong), Phase 4 chỉ còn xoá sớm cho file đã đổi. **Đã đồng bộ.**
- Không còn mâu thuẫn nào chưa giải quyết.
