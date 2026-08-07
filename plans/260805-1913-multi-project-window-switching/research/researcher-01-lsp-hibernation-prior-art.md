# Research 01 — Chi phí wake của LSP và tiền lệ hibernate

**Câu hỏi:** shutdown language server khi project ngủ rồi start lại khi wake — giá thật là bao nhiêu,
và ai đã làm kiểu này?

## rust-analyzer: không có cache index trên đĩa

Đây là ràng buộc nặng nhất, vì Zode tự build bằng Rust nên project Rust là ca dùng chính.

- rust-analyzer index lại **mỗi lần khởi động**; không có persistent cache. Issue mở từ lâu:
  [Indexing on every startup #9991](https://github.com/rust-lang/rust-analyzer/issues/9991).
- Lý do là chủ ý của maintainer: persistent cache làm giảm áp lực tối ưu initial analysis. Khuyến nghị
  chính thống là **để server chạy** và chia sẻ giữa các session editing —
  [A Plan for Making Rust Analyzer Faster #17491](https://github.com/rust-lang/rust-analyzer/issues/17491).
- Vẫn đang có việc đo startup: [PR #22581](https://github.com/rust-lang/rust-analyzer/pull/22581) thêm
  log "startup time to ready" + benchmark. Nghĩa là con số này chưa ổn định và phải tự đo trên máy thật.
- Re-index còn bị trigger cả khi build script / proc macro đổi
  ([#18631](https://github.com/rust-lang/rust-analyzer/issues/18631)) — tức là ngay cả khi giữ server
  sống, index không miễn phí vĩnh viễn.

**Kết luận cho thiết kế:** wake một project Rust đã hibernate = trả lại toàn bộ chi phí index. Không
có mẹo nào giảm được từ phía client. Ba hệ quả bắt buộc:

1. Ngưỡng `hibernate_after` phải đủ dài để switch qua lại thường ngày **không bao giờ** chạm tới nó.
2. UI phải nói rõ đang index, không để user đoán vì sao "go to definition" im lặng.
3. Wake không được block UI — state tab/pane/terminal phải hiện trước, LSP bò về sau.

## Chi phí wake thay đổi theo từng server — phải đo, không suy diễn

Các server phổ biến khác nhau về việc có cache trên đĩa hay không (clangd và gopls có dạng cache
file-based, tsserver/pyright thì không rõ ràng). Báo cáo này **không** chốt con số cho từng server —
Phase 6 phải đo thật trên các project của người dùng, vì kết luận sai chỗ này sẽ chọn sai
`hibernate_after`.

Việc cần đo, theo từng server: thời gian từ `initialize` tới lúc diagnostics đầu tiên trở lại, trên
một project cỡ vừa và một project cỡ lớn.

## Tiền lệ: không ai dùng SIGSTOP

| Hệ | Cách làm | Liên quan |
|---|---|---|
| Emacs `eglot` / `lsp-mode` | Shutdown server khi **không còn buffer nào** thuộc workspace đó (`lsp-keep-workspace-alive nil`) | Gần nhất với thiết kế đang chọn: vòng đời server gắn với mức độ "còn dùng", không gắn với process suspend |
| JetBrains | Một project một cửa sổ; có "unload modules" để loại bớt phần cây khỏi index | Chấp nhận trả giá index lại khi load module trở lại |
| VS Code | Một window một folder/workspace; không có khái niệm hibernate chéo project | Không có tiền lệ nào để mượn |
| Zed (upstream) | Server start khi buffer của language đó được mở trong project | Vòng đời đã là lazy theo buffer — thiết kế của ta chỉ thêm chiều "idle" |

**Không tìm thấy tiền lệ nào dùng SIGSTOP/SIGCONT cho language server.** Củng cố quyết định đã chốt
trong brainstorm: SIGSTOP chỉ tiết kiệm CPU, giữ nguyên RAM, và thêm rủi ro pipe buffer đầy + client
timeout.

## Điều đáng mượn từ eglot

Vòng đời "shutdown khi không còn buffer tham chiếu" nghĩa là: nếu project ngủ mà **không có buffer nào
đang mở**, shutdown là hành vi đúng đắn kể cả không có governor. Ngược lại, project ngủ mà còn 12 tab
mở thì shutdown là quyết định đánh đổi có ý thức — và đó chính là quyết định `hibernate_after` đang
mã hoá.

## Nguồn

- [Indexing on every startup · rust-analyzer #9991](https://github.com/rust-lang/rust-analyzer/issues/9991)
- [A Plan for Making Rust Analyzer Faster · #17491](https://github.com/rust-lang/rust-analyzer/issues/17491)
- [provide startup time to ready log point and associated benchmark · PR #22581](https://github.com/rust-lang/rust-analyzer/pull/22581)
- [r-a constantly reindexes workspace when build script or proc macro is changed · #18631](https://github.com/rust-lang/rust-analyzer/issues/18631)
