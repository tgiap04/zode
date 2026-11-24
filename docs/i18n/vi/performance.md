---
title: Rough quick CPU profiling (Flamechart)
description: "Performance profiling and optimization for Zode development."
---

Cách dùng công cụ nội bộ để profile và giữ cho Zode nhanh.

# Profile CPU nhanh (Flamechart)

Xem CPU tốn nhiều thời gian nhất ở đâu. Chúng tôi rất khuyên dùng
[samply](https://github.com/mstange/samply). Nó mở một profile tương tác trong
trình duyệt (cụ thể là một instance cục bộ của [firefox_profiler](https://profiler.firefox.com/)).

Xem README của [samply](https://github.com/mstange/samply) để biết cách cài và chạy.

File profile.json không chứa symbol nào. Firefox profiler có thể thêm symbol cục bộ vào profile. Để làm vậy, nhấn nút upload local profile ở góc trên bên phải.

<img width="851" height="auto" alt="image" src="https://github.com/user-attachments/assets/cbef2b51-0442-4ee9-bc5c-95f6ccf9be2c" style="display: block; margin: 0 auto;" />

# Profile CPU chuyên sâu (Tracing)

Xem mỗi lần gọi hàm được chú thích tốn bao lâu và tham số của nó (nếu
đã cấu hình).

Chú thích bất kỳ hàm nào bạn muốn hiện trong profile bằng instrument. Để biết
thêm chi tiết xem
[tracing-instrument](https://docs.rs/tracing/latest/tracing/attr.instrument.html):

```rust
#[instrument(skip_all)]
fn should_appear_in_profile(kitty: Cat) {
    sleep(QUITE_LONG)
}
```

Sau đó compile Zode với `ZTRACING=1 cargo r --features tracy --release`. Bản release là tùy chọn nhưng rất nên dùng, vì như mọi chương trình khác, đặc tính hiệu năng của Zed thay đổi mạnh khi có tối ưu hóa. Bạn không muốn đuổi theo những chỗ chậm không tồn tại trên bản release.

## Thiết lập một lần/Build profiler:

Tải profiler:
[linux x86_64](https://zed-tracy-import-miniprofiler.nyc3.digitaloceanspaces.com/tracy-profiler-linux-x86_64)
[macos aarch64](https://zed-tracy-import-miniprofiler.nyc3.digitaloceanspaces.com/tracy-profiler-0.13.0-macos-aarch64)

### Cách khác: Tự build

- Clone repo tại git@github.com:wolfpld/tracy.git
- `cd profiler && mkdir build && cd build`
- Chạy cmake để tạo file build: `cmake -G Ninja -DCMAKE_BUILD_TYPE=Release ..`
- Build profiler: `ninja`
- [Tùy chọn] chuyển profiler tới chỗ gọn gàng như ~/.local/bin trên linux

## Cách dùng

Mở profiler (tracy-profiler), bạn sẽ thấy Zode trong danh sách `Discovered clients`, hãy nhấn vào đó.

<img width="392" height="auto" alt="image" src="https://github.com/user-attachments/assets/b6f06fc3-6b25-41c7-ade9-558cc93d6033" style="display: block; margin: 0 auto;"/>

Tracy là profiler cực mạnh, làm được rất nhiều thứ nhưng giao diện không thân thiện lắm. Đây không phải chỗ để hướng dẫn sâu về Tracy, nhưng tôi muốn nêu một quy trình hữu ích khi tìm hiểu vì sao một đoạn code _thỉnh thoảng_ chậm.

Các bước:

1. Nhấn nút flamechart ở trên cùng.

<img width="1815" height="auto" alt="Click flamechart" src="https://github.com/user-attachments/assets/9b488c60-90fa-4013-a663-f4e35ea753d2" />

2. Nhấn vào một hàm tốn nhiều thời gian.

<img width="2001" height="auto" alt="Click snapshot" src="https://github.com/user-attachments/assets/ddb838ed-2c83-4dba-a750-b8a2d4ac6202" />

3. Mở rộng danh sách lời gọi hàm bằng cách nhấn vào main thread.

<img width="2313" height="auto" alt="Click main thread" src="https://github.com/user-attachments/assets/465dd883-9d3c-4384-a396-fce68b872d1a" />

4. Lọc danh sách xuống các lần gọi chậm hơn rồi nhấn vào một lần gọi chậm trong danh sách

<img width="2264" height="auto" alt="Select the tail calls in the histogram to filter down the list of calls then click on one call" src="https://github.com/user-attachments/assets/a8fddc7c-f40a-4f11-a648-ca7cc193ff6f" />

5. Nhấn zoom to zone để tới đúng lần gọi hàm đó trên timeline

<img width="1822" height="auto" alt="Click zoom to zone" src="https://github.com/user-attachments/assets/3391664d-7297-41d4-be17-ac9b2e2c85d1" />

6. Cuộn để phóng to và xem thêm chi tiết về các hàm gọi

<img width="1964" height="auto" alt="Scroll to zoom in" src="https://github.com/user-attachments/assets/625c2bf4-a68d-40c4-becb-ade16bc9a8bc" />

7. Nhấn vào một hàm gọi để xem thống kê của _chính nó_.

<img width="1888" height="auto" alt="Click on any of the zones to get statistics" src="https://github.com/user-attachments/assets/7e578825-2b63-4b7f-88f7-0cb16b8a3387" />

Thông thường các thanh xanh trên timeline của Tracy ứng với các lần gọi hàm, nhưng chúng có thể đo thời gian của bất kỳ phần nào trong codebase. Trong ví dụ dưới đây ta thêm một span "for block in edits" kèm metadata: block_height. Bạn có thể làm như sau:

```rust
let span = ztracing::debug_span!("for block in edits", block_height = block.height());
let _enter = span.enter(); // span guard, when this is dropped the span ends (and its duration is recorded)
```

# Profile Task/Async

Lấy profile của foreground executor và các background executor của Zode. Kiểm tra xem
có gì chặn foreground quá lâu hoặc tốn quá nhiều thời gian (đồng hồ) ở
background hay không.

Profiler luôn chạy ở background. Bạn có thể lưu trace từ giao diện của nó hoặc
xem kết quả trực tiếp.

## Thiết lập/Build importer:

Tải importer
[linux x86_64](https://zed-tracy-import-miniprofiler.nyc3.digitaloceanspaces.com/tracy-import-miniprofiler-linux-x86_64)
[mac aarch64](https://zed-tracy-import-miniprofiler.nyc3.digitaloceanspaces.com/tracy-import-miniprofiler-macos-aarch64)

### Cách khác: Tự build

- Clone repo tại git@github.com:zed-industries/tracy.git, nhánh v0.12.2
- `cd import && mkdir build && cd build`
- Chạy cmake để tạo file build: `cmake -G Ninja -DCMAKE_BUILD_TYPE=Release ..`
- Build importer: `ninja`
- Chạy importer trên file trace: `./tracy-import-miniprofiler /path/to/trace.miniprof.json /path/to/output.tracy`
- Mở trace trong tracy:
  - Nếu bạn dùng windows, tải bản v0.12.2 từ trang releases của repo upstream
  - Nếu dùng nền tảng khác, mở trên website: https://tracy.nereid.pl/ (phiên bản có thể lệch nên may rủi, lý tưởng là ta tự host..)

## Để lưu trace:

- Chạy action: `zode: open performance profiler`
- Nhấn nút save. Hộp thoại lưu sẽ mở, hoặc nếu không mở được thì trace được lưu vào thư mục làm việc của bạn.
- Chuyển đổi profile để import được vào tracy bằng importer: `./tracy-import-miniprofiler <path to performance_profile.miniprof.json> output.tracy`
- Vào <https://tracy.nereid.pl/>, nhấn 'power button' ở góc trên bên trái rồi mở trace đã lưu.
- Giờ phóng to để xem các task và thời gian chạy của chúng

# Cảnh báo khi hàm chạy chậm

```rust
let _timer = zlog::time!("my_function_name").warn_if_gt(std::time::Duration::from_millis(100));
```
