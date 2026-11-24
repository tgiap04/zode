---
title: Debugger Extensions
description: "Debugger Extensions for Zode extensions."
---

# Extension trình gỡ lỗi

Các server [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol) có thể được cung cấp dưới dạng extension để dùng trong [trình gỡ lỗi (debugger)](../debugger.md).

## Định nghĩa extension trình gỡ lỗi

Một extension có thể cung cấp một hoặc nhiều server DAP.
Mỗi server DAP phải được đăng ký trong `extension.toml`:

```toml
[debug_adapters.my-debug-adapter]
# Optional relative path to the JSON schema for the debug adapter configuration schema. Defaults to `debug_adapter_schemas/$DEBUG_ADAPTER_NAME_ID.json`.
# Note that while this field is optional, a schema is mandatory.
schema_path = "relative/path/to/schema.json"
```

Sau đó, trong mã Rust của extension, hãy cài đặt phương thức `get_dap_binary` cho extension của bạn:

```rust
impl zode::Extension for MyExtension {
    fn get_dap_binary(
        &mut self,
        adapter_name: String,
        config: DebugTaskDefinition,
        user_provided_debug_adapter_path: Option<String>,
        worktree: &Worktree,
    ) -> Result<DebugAdapterBinary, String>;
}
```

Phương thức này trả về lệnh để khởi động một server debug adapter protocol, cùng các đối số hoặc biến môi trường cần thiết để nó chạy.

Nếu cần tải server DAP từ nguồn bên ngoài (GitHub Releases, npm, ...), bạn cũng làm việc đó trong hàm này. Hãy chỉ kiểm tra bản cập nhật theo chu kỳ, vì hàm này được gọi mỗi khi người dùng mở một phiên gỡ lỗi mới với debug adapter của bạn.

Bạn cũng phải cài đặt `dap_request_kind`. Hàm này dùng để xác định một kịch bản gỡ lỗi sẽ _launch_ một debuggee mới hay _attach_ vào một debuggee đang có.
Zode cũng dùng nó để xác định một kịch bản gỡ lỗi có cần chạy _locator_ hay không.

```rust
impl zode::Extension for MyExtension {
    fn dap_request_kind(
        &mut self,
        _adapter_name: String,
        _config: Value,
    ) -> Result<StartDebuggingRequestArgumentsRequest, String>;
}
```

Hai hàm này đủ để đưa debug adapter của bạn vào các quy trình dùng `debug.json`, nhưng bạn nên cân nhắc nghiêm túc việc cài đặt thêm `dap_config_to_scenario`.

```rust
impl zode::Extension for MyExtension {
    fn dap_config_to_scenario(
        &mut self,
        _adapter_name: DebugConfig,
    ) -> Result<DebugScenario, String>;
}
```

`dap_config_to_scenario` được dùng khi người dùng mở phiên qua giao diện modal tiến trình mới. Nói gọn, nó nhận một cấu hình gỡ lỗi chung (không riêng cho debug adapter nào) và cố biến nó thành
một kịch bản gỡ lỗi cụ thể cho adapter của bạn.
Nói cách khác, nó phải trả lời câu hỏi: "Với một chương trình, danh sách đối số, thư mục làm việc hiện tại và biến môi trường, cấu hình để khởi chạy debug adapter này sẽ trông thế nào?".

## Định nghĩa debug locator

Zode có cách tự động tạo kịch bản gỡ lỗi bằng _debug locator_.
Locator xác định mục tiêu gỡ lỗi và tìm cách mở phiên gỡ lỗi cho nó. Nhờ locator, Zode có thể tự động chuyển các task có sẵn của người dùng (ví dụ `cargo run`) thành kịch bản gỡ lỗi (ví dụ `cargo build` rồi khởi chạy trình gỡ lỗi với `target/debug/my_program` làm chương trình cần gỡ).

> Extension của bạn có thể tự định nghĩa debug locator ngay cả khi nó không cung cấp debug adapter. Chúng tôi đặc biệt khuyến nghị làm vậy khi extension đã cung cấp task cho ngôn ngữ, vì nó cho phép người dùng mở phiên gỡ lỗi mà không phải cấu hình debug adapter thủ công.

Locator có thể (nhưng không bắt buộc) không phụ thuộc vào debug adapter mà nó đi cùng. Chúng chịu trách nhiệm xác định mục tiêu gỡ lỗi và tìm cách mở phiên gỡ lỗi cho nó. Nhờ vậy các extension có thể dùng chung logic locator giữa các adapter.

Extension của bạn có thể định nghĩa một hoặc nhiều debug locator. Mỗi debug locator phải được đăng ký trong `extension.toml`:

```toml
[debug_locators.my-debug-locator]
```

Locator có hai thành phần.
Thứ nhất, mỗi locator được chạy trên từng task có sẵn để xem có locator nào cung cấp được kịch bản gỡ lỗi cho task đó hay không. Việc này được thực hiện bằng cách gọi `dap_locator_create_scenario`.

```rust
impl zode::Extension for MyExtension {
    fn dap_locator_create_scenario(
        &mut self,
        _locator_name: String,
        _build_task: TaskTemplate,
        _resolved_label: String,
        _debug_adapter_name: String,
    ) -> Option<DebugScenario>;
}
```

Hàm này nên trả về `Some` kịch bản gỡ lỗi khi kịch bản đó là phiên bản gỡ lỗi tương ứng của một task do người dùng định nghĩa.
Lưu ý `DebugScenario` có thể kèm một [task build](../debugger.md#build-tasks). Nếu có, Zode sẽ chạy `run_dap_locator` sau khi task build kết thúc thành công.

```rust
impl zode::Extension for MyExtension {
    fn run_dap_locator(
        &mut self,
        _locator_name: String,
        _build_task: TaskTemplate,
    ) -> Result<DebugRequest, String>;
}
```

`run_dap_locator` hữu ích khi bạn không xác định được mục tiêu build một cách chắc chắn. Một số hệ thống build tạo ra artifact mà tên không biết trước.
Tuy nhiên lưu ý bạn _không_ cần đi qua quy trình hai giai đoạn; nếu xác định được toàn bộ cấu hình gỡ lỗi chỉ bằng `dap_locator_create_scenario`, bạn có thể bỏ thuộc tính `build` trên `DebugScenario` trả về. Cũng lưu ý locator của bạn **sẽ bị** gọi với những task mà nó khó có khả năng nhận; vì vậy bạn nên cố trả về `None` sớm trước khi làm bất kỳ thao tác tốn kém nào.

## Các extension có sẵn

Xem các server DAP được xuất bản dưới dạng extension [trên trang của Zed](https://zed.dev/extensions?filter=debug-adapters).

Hãy xem repository của chúng để biết các mẫu triển khai và cấu trúc phổ biến.

## Kiểm thử

Để thử extension server Debug Adapter Protocol mới của bạn, bạn có thể [cài nó như một dev extension](./developing-extensions.md#developing-an-extension-locally).
