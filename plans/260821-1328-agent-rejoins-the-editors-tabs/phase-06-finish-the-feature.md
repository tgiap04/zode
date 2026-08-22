# Phase 06 — Đóng lại tính năng

**Status:** ⬜ todo · **Priority:** P2 · **Người dùng thấy:** —

## Mục tiêu

Chạy hết suite trên code cuối, và để lại tài liệu đúng với thứ vừa dựng.

## Việc

1. **Full suite.** `cargo test -p agent_ui -p workspace -p sidebar -p database_ui`, rồi
   rộng hơn nếu cần. Trong sandbox này có test treo đã biết — bỏ qua **có nêu tên**, không
   bỏ qua chung chung, và không cho qua test đỏ để build xanh.
2. **`cargo fmt`** — cẩn thận: `cargo fmt -p <crate>` format cả crate, và repo này có drift
   sẵn. Diff với HEAD và revert những hunk không liên quan.
3. **Docs.** `docs/` có `features/`, `flows/`, `system/`, `generated/`. Bất cứ trang nào mô tả
   agent là một cột/dock panel đều đã sai. Đây là việc của `doc-writer` ở stage Delivery —
   nó là chủ sở hữu docs, không phải suy đoán của phase này.
4. **Changelog** (`docs/project-changelog.md`): agent về thanh tab editor; cột agent bỏ; bản
   ghi chiều rộng cột cũ thành mồ côi và không được migrate.

## Todo

- [ ] Suite xanh, test treo được nêu tên cụ thể
- [ ] `cargo fmt` không mang theo drift lạ
- [ ] Trang docs nói agent-là-cột được sửa
- [ ] Changelog có entry

## Success criteria

Mở app: một thanh tab giữ cả code và agent. Bấm rail đi rồi bấm lại — không mất session.
Mở file khi đang ở tab agent — tab mới kế bên. Restart — đúng tab đó, đúng thứ tự, đúng tên.
Không còn cột agent. Cột database không hề bị ảnh hưởng.
