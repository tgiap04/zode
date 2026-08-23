# Phase 06 — Đóng lại tính năng

**Status:** ✅ done (2026-08-21) · **Priority:** P2 · **Người dùng thấy:** —

## Việc

1. **Full suite** trên các crate bị ảnh hưởng: `agent_usage`, `workspace`, `zode`.
   Sandbox này có test treo đã biết — bỏ qua **có nêu tên**, không bỏ chung chung,
   và không cho qua test đỏ để build xanh.
2. **`rustfmt`** từng file đã sửa, không phải `cargo fmt -p` (repo có drift sẵn;
   diff với HEAD và revert hunk không liên quan).
3. **Rà secret một lần nữa.** Đây là feature duy nhất trong repo đọc credential của
   một app khác. Grep diff tìm mọi đường token có thể lọt vào log, `{:?}`, hay đĩa.
4. **Docs** — `doc-writer` là chủ sở hữu docs, không phải suy đoán của phase này.
   Cái nó cần biết: có một chỉ báo mới trên thanh status, nguồn dữ liệu của mỗi
   agent, và những điều kiện khiến nó **không** hiện (env override, gói free, chưa
   login codex).

## Todo

- [x] Suite xanh, test treo nêu tên cụ thể
- [x] `rustfmt` không mang theo drift lạ
- [x] Rà secret: không đường nào token vào log/đĩa
- [x] `doc-writer` xem `docs/`
- [x] Binary build được (`cargo build --bin zode`)

## Success criteria

Mở app: thanh dưới hiện usage của Claude (và Codex nếu đã login). Thu nhỏ → không
request nào. Bấm ⟳ → cập nhật ngay. Bỏ credential đi → phần đó biến mất, không hiện
số sai, không log ồn.
