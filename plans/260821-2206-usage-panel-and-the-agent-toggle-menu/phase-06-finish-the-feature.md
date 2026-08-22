# Phase 06 — Đóng lại tính năng

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** —

## Việc

1. **Full suite** trên crate bị ảnh hưởng: `agent_usage`, `workspace`, `zode`, và
   `encoding_selector`/`line_ending_selector`/`go_to_line`/`language_selector` vì phase 02
   chạm struct setting chúng đọc. Test treo đã biết thì **nêu tên**, không bỏ chung chung.
   `multi_workspace_tests::test_hibernate_after_ms_zero_disables_hibernation` là lỗi
   order-dependent có sẵn — không tính vào feature này, và cũng không gọi là "flake".
2. **`rustfmt` từng file đã sửa**, không `cargo fmt -p` (repo có drift sẵn — diff với HEAD
   và revert hunk không liên quan).
3. **Docs.** `doc-writer` là chủ sở hữu, không phải suy đoán của phase này. Ba thứ nó cần
   biết: ba setting mới trong `status_bar`, panel + menu chuột phải là surface mới cho dữ
   liệu **đã có** (không request thêm, `telemetry.md` không đổi), và **lỗ đã biết** — tắt
   cả hai agent thì đường về là settings.
4. **Commit.** Cây đang mang cả feature usage của plan trước chưa commit. Tách theo scope,
   không dồn một commit.

## Todo

- [x] Suite xanh: `agent_usage` 61 · `zode` 56 · `workspace` 222 (+1 đỏ có sẵn, nêu tên)
      · `settings` 32 · `settings_ui` 15 · `go_to_line` 7 · `language_selector` 2
- [x] `rustfmt` không mang theo drift lạ (diff file tracked chỉ có insertion)
- [x] `cargo build --bin zode` exit 0 · clippy 0 warning
- [x] `reviewer` xem toàn bộ diff — 0 critical, 0 high, 2 medium (đã sửa cả hai)
- [x] `doc-writer` cập nhật `docs/src/agent-usage.md` + `visual-customization.md`
- [ ] `git-manager` commit theo scope — **chờ bạn quyết**

## Đã tìm ra một regression thật, và chỉ vì chạy lại đúng target

`tester` báo `cargo test -p zode --lib` là "no lib target → Skipped". Test của crate đó
nằm ở **bin target**, nên 56 test vừa bị bỏ qua chứ không phải không tồn tại. Chạy
`cargo test -p zode` (không `--lib`) thì đỏ ngay:

```
zed::tests::test_action_namespaces panicked at crates/zed/src/zed.rs:5099
```

Action mới `agent_usage::ToggleUsagePanel` thêm một namespace, và test đó **khẳng định
danh sách namespace đầy đủ** — nó tồn tại đúng để bắt việc thêm namespace mà không ai để
ý. Sửa: thêm `"agent_usage"` vào danh sách. 56/56 xanh.

Bài học không phải về namespace: **"suite xanh" của một agent chỉ đúng với target nó thật
sự chạy.** Một dòng "Skipped" trong bảng kết quả là một khoảng trống, không phải một số 0.

## Success criteria

Mở app: click ra panel, chuột phải ra menu, Compact rút dòng và sống qua restart. Tắt hết
credential → panel nói lý do, thanh status không hiện số sai. Không có request nào mới so
với trước feature này.
