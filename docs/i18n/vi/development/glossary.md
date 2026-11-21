---
title: "Zode Development: Glossary"
description: "Guide to Zode development: glossary for Zode development."
---

# Phát triển Zode: Bảng thuật ngữ

Trang này định nghĩa các thuật ngữ và cấu trúc dùng xuyên suốt codebase của Zode.

Đây là danh sách cố gắng hết mức và vẫn đang được hoàn thiện.

<!--
TBD: Glossary Improvement

Questions:

- Can we generate this list from doc comments throughout Zode?
- We should have a section that shows the various UI parts and their names. (Can't do that in the channel.)
-->

## Quy ước đặt tên

Đây là các mẫu đặt tên phổ biến trong codebase. `Name` là chỗ giữ chỗ
cho tên kiểu bất kỳ, chẳng hạn `AnyElement` hoặc `LspStore`.

- `AnyName`: Phiên bản xoá kiểu của _name_. Hình dung như `Box<dyn NameTrait>`.
- `NameStore`: Kiểu bao bọc che đi việc thao tác đang chạy cục bộ hay trên máy từ xa.

## GPUI

### Quản lý trạng thái

- `App`: Singleton giữ toàn bộ trạng thái ứng dụng, gồm mọi entity. `App` không `Send`, nên chỉ tồn tại trên thread đã tạo ra nó (thường là thread chính/UI). Thấy `&mut App` nghĩa là bạn đang ở thread UI.
- `Context`: Lớp bao quanh `App` với hành vi chuyên biệt cho một `Entity` cụ thể. Có thể hình dung nó là `(&mut App, Entity<V>)`. Ví dụ, `App::spawn` nhận `AsyncFnOnce(AsyncApp) -> Ret`, còn `Context::spawn` nhận `AsyncFnOnce(WeakEntity<V>, AsyncApp) -> Ret`.
- `AsyncApp`: Phiên bản sở hữu của `App` dùng trong ngữ cảnh async. Nó vẫn không `Send`, nên vẫn chạy trên thread chính, và thao tác có thể lỗi nếu `App` đã kết thúc.
  `AsyncApp` tồn tại vì `App` thường được truy cập dưới dạng `&mut App`, khó giữ qua các ranh giới async.
- `AppContext`: Trait che đi sự khác biệt giữa `App`, `AsyncApp`, `Context` và các biến thể dùng cho test.
- `Task`: Future đang chạy (hoặc đã lên lịch chạy) trên executor nền hoặc tiền cảnh. Khác với future thông thường, task không cần `.await` để bắt đầu. Bạn vẫn phải await để lấy kết quả.
- `Executor`: Dùng để spawn task chạy trên thread tiền cảnh hoặc thread nền. Cố chạy task trên thread nền.
  - `BackgroundExecutor`: Một threadpool chạy các `Task`.
  - `ForegroundExecutor`: Thread chính chạy các `Task`.
- `Entity`: Tham chiếu mạnh, đúng kiểu tới một struct do gpui quản lý. Về bản chất là con trỏ/khoá map trỏ vào `App::EntityMap`.
- `WeakEntity`: Tham chiếu được kiểm tra lúc chạy tới một `Entity` có thể đã không còn. Tương tự [`std::rc::Weak`](https://doc.rust-lang.org/std/rc/struct.Weak.html).
- `Global`: Kiểu singleton chỉ có một giá trị, lưu trong `App`.
- `Event`: Kiểu dữ liệu mà một `Entity` có thể gửi tới các bên đăng ký.
- `Action`: Sự kiện biểu diễn thao tác bàn phím của người dùng mà các listener có thể xử lý
  Ví dụ: `file finder: toggle`
- `Observing`: Phản ứng với thông báo rằng entity đã thay đổi.
- `Subscription`: Trình xử lý sự kiện dùng để phản ứng với thay đổi trạng thái trong ứng dụng.
  1. Xử lý sự kiện được emit
  2. Quan sát `{new,release,on notify}` của một entity

### UI

- `View`: Một `Entity` có thể tạo ra `Element` thông qua việc triển khai `Render`.
- `Element`: Kiểu có thể được bố cục và vẽ lên màn hình.
- `element expression`: Biểu thức dựng cây element, ví dụ:

```rust
h_flex()
    .id(text[i])
    .relative()
    .when(selected, |this| {
        this.child(
            div()
                .h_4()
                .absolute()
                etc etc
```

- `Component`: Builder có thể được render bằng cách biến nó thành `Element`.
- `Dispatch tree`: TODO
- `Focus`: Nơi xử lý phím bấm đầu tiên
- `Focus tree`: Đường đi từ nơi đang có focus tới gốc của UI. Ví dụ <img> TODO

## UI của Zode

- `Window`: Struct biểu diễn một cửa sổ Zode trong môi trường desktop của bạn (xem ảnh bên dưới). Bạn có thể mở nhiều cửa sổ. Chủ yếu nó được truyền đi để render.
- `Modal`: Thành phần UI nổi lên trên phần còn lại của UI
- `Picker`: Struct biểu diễn danh sách mục nổi trên UI (Modal). Bạn chọn một mục rồi xác nhận. Điều xảy ra khi chọn hoặc xác nhận do delegate của picker quyết định. (Modal trong ảnh bên dưới là một picker.)
- `PickerDelegate`: Trait dùng để chuyên biệt hoá hành vi của `Picker`. `Picker` lưu `PickerDelegate` trong field delegate.
- `Center`: Phần giữa của cửa sổ Zode, được chia thành nhiều `Pane`. Trong codebase đây là một field của struct `Workspace` (xem ảnh bên dưới).
- `Pane`: Vùng trong `Center` nơi ta đặt các mục như editor, multi-buffer hoặc terminal (xem ảnh bên dưới).
- `Panel`: Một `Entity` triển khai trait `Panel`. Panel có thể đặt trong `Dock`. Trong ảnh bên dưới: `ProjectPanel` ở dock trái, `DebugPanel` ở dock dưới và `GitPanel` ở dock phải (phía dock mặc định của mỗi panel, xem `assets/settings/default.json`). `Editor` không triển khai `Panel`.
- `Dock`: Thành phần UI giống `Pane`, có thể mở và ẩn. Có thể mở tối đa ba dock cùng lúc: trái, phải và dưới. Một dock chứa một hoặc nhiều `Panel`, không phải `Pane`.

<img width="1921" height="auto" alt="Screenshot for the Pane and Dock features" src="https://github.com/user-attachments/assets/2cb1170e-2850-450d-89bb-73622b5d07b2" />

- `Project`: Một hoặc nhiều `Worktree`
- `Worktree`: Biểu diễn file cục bộ hoặc từ xa.

<img width="552" height="auto" alt="Screenshot for the Worktree feature" src="https://github.com/user-attachments/assets/da5c58e4-b02e-4038-9736-27e3509fdbfa" />

- [Multibuffer](https://zodekit.com/docs/multibuffers): Danh sách các Editor, cho phép sửa nhiều file cùng lúc. Multi-buffer mở ra khi một thao tác trong Zode trả về nhiều vị trí, ví dụ: _search_ hoặc _go to definition_. Xem tìm kiếm trong dự án ở ảnh bên dưới.

<img width="800" height="auto" alt="Screenshot for the MultiBuffer feature" src="https://github.com/user-attachments/assets/d59dcecd-8ab6-4172-8fb6-b1fc3c3eaf9d" />

## Editor

- `Editor`: Kiểu trình soạn thảo văn bản. Hầu hết các bề mặt sửa được trong Zode là một `Editor`, kể cả ô nhập một dòng. Mỗi pane trong ảnh trên chứa một hoặc nhiều `Editor`.
- `Workspace`: Gốc của cửa sổ
- `Entry`: Một file, thư mục, thư mục đang chờ hoặc thư mục chưa tải.
- `Buffer`: Biểu diễn trong bộ nhớ của một 'file' cùng các dữ liệu liên quan như cây cú pháp, trạng thái git và chẩn đoán.
- `pending selection`: Bạn đang nhấn chuột và kéo nhưng chưa thả.

## Collab

- `Collab session`: Nhiều người dùng cùng làm việc trong một `Project` chia sẻ
- `Upstream client`: Client Zode đã chia sẻ workspace của họ
- `Downstream client`: Client Zode tham gia một workspace được chia sẻ

## Debugger

- `DapStore`: Là entity quản lý các phiên gỡ lỗi
- `debugger::Session`: Entity quản lý vòng đời của một phiên gỡ lỗi và giao tiếp với DAP.
- `BreakpointStore`: Là entity quản lý trạng thái breakpoint trong các bản Zode cục bộ và từ xa
- `DebugSession`: Quản lý UI và trạng thái chạy của một phiên gỡ lỗi
- `RunningState`: Trực tiếp quản lý mọi view của một phiên gỡ lỗi
- `VariableList`: View danh sách biến và watch của một phiên gỡ lỗi
- `Console`: TODO
- `Terminal`: TODO
- `BreakpointList`: TODO
