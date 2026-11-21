# Điều khiển từ xa — giao thức

Trang này là toàn bộ đặc tả. Mọi thứ Zode và trình duyệt trao đổi khi bạn
điều khiển một Zode đang chạy từ thiết bị khác đều được mô tả ở đây, đủ chi tiết
để viết một client tương thích mà không cần đọc mã nguồn Zode — và các vector cố định
ở cuối trang được bộ test của chính Zode kiểm tra, nên nếu trang này lệch với code
thì build sẽ hỏng, thay vì đánh lừa bạn.

Lý do ghi lại ra đây rất cụ thể: một relay server nằm giữa trình duyệt và trình
soạn thảo của bạn, và bạn không nên phải tin nó. Nó chuyển tiếp các byte mà nó
không đọc được. Nó chỉ thấy ai kết nối, tới thiết bị nào, bao nhiêu byte và vào
lúc nào. Chỉ có vậy.

## Các lớp

```
browser  ◄──── WebSocket ────►  relay  ◄──── WebSocket ────►  Zode
   │                                                            │
   └──────────── Noise KK channel (end to end) ─────────────────┘
                  carries: control messages, data streams
```

| Lớp             | Relay có thấy không | Định nghĩa tại                             |
| --------------- | ------------------- | ------------------------------------------ |
| Relay frame     | có                  | [Relay frame](#relay-frame)                |
| Noise transport | chỉ thấy bản mã     | [Kênh mã hoá](#encrypted-channel)          |
| Inner frame     | không               | [Inner frame](#inner-frame)                |
| Control và data | không               | [Thông điệp điều khiển](#control-messages) |

## Relay frame

Một frame WebSocket **text** là một object JSON mà chính relay hiểu. Cấu trúc của
nó thuộc về API riêng của relay và không được mô tả ở đây, trừ một
ngoại lệ: các thông điệp ghép cặp bên dưới đi trong frame text, và relay
chuyển tiếp chúng mà không diễn giải.

Một frame WebSocket **binary** gồm:

```
offset 0   session_id   u32, big-endian
offset 4   payload      1 to 65535 bytes — opaque to the relay
```

`session_id` cho relay biết các byte thuộc cuộc hội thoại nào trong số hai peer
của nó. Payload là một thông điệp handshake Noise hoặc một thông điệp
transport Noise. Frame ngắn hơn 5 byte, hoặc có payload quá 65535 byte, là
sai định dạng và làm đóng kết nối.

## Vai trò của relay

Relay gán cho mỗi WebSocket một trong hai vai trò, và một phiên luôn diễn ra
giữa một host và một client của cùng một tài khoản.

- **Host.** Zode được điều khiển. Kết nối bằng access token làm bearer, và
  không có gì khác trong URL, hoặc kèm `role=host`.
- **Client.** Thiết bị điều khiển. Trình duyệt trình một ticket dùng một lần.
  Zode trình access token của nó làm bearer **và** `?role=client` trên
  URL relay.

Các quy tắc quanh vai trò là hợp đồng của relay:

- `role` chỉ được đọc ở đường bearer. Vắng mặt hoặc `host` nghĩa là host; mọi giá
  trị khác — rỗng, sai hoa thường, lặp lại — bị từ chối trước khi nâng cấp với HTTP
  400 và lỗi `invalid_role`.
- Socket client được chào bằng `hello` mang `role` `client`, rồi một danh sách
  `presence` các host của tài khoản đang kết nối, không
  gồm thiết bị của chính client.
- Zode kiểm tra `role` trong `hello` đó với vai trò nó đã yêu cầu. Một
  `hello.role` không khớp với vai trò của socket sẽ dừng client như một
  relay không tương thích, giống như `relay_protocol` không rõ. Nếu không có bước kiểm tra, một
  relay bỏ qua `?role=` có thể đăng ký socket client làm host
  của thiết bị và chiếm chỗ host của host thật.
- Client mở phiên bằng `open`, nêu id thiết bị của một host và một `mode`
  là `pair` hoặc `session`. Mở tới chính id thiết bị của mình sẽ được trả lời bằng `error`
  `unauthorized`.
- Host được báo `opened` kèm id thiết bị của client và `peerKind` là `ide` với
  Zode, `web` với trình duyệt.
- Peer được định danh theo người dùng, thiết bị **và vai trò**, nên một Zode có thể vừa là host vừa là
  client. Socket thứ hai cùng vai trò sẽ thay socket thứ nhất, socket đó
  bị đóng với mã 4409.
- Thu hồi một thiết bị sẽ đóng cả socket host và socket client của nó với mã 4403
  và lý do `device_revoked`.
- Trong danh sách thiết bị, `online` nghĩa là "có thể kết nối tới như một host": một trình soạn thảo
  chỉ giữ socket client được liệt kê là offline.

## Kênh mã hoá

Kênh là `Noise_KK_25519_AESGCM_SHA256`:

- **KK** — hai phía đều đã biết khoá công khai tĩnh của phía kia. Việc ghép cặp
  đã thiết lập điều đó, nên handshake chỉ có hai thông điệp và xác thực cả hai đầu.
- **25519** — X25519 cho Diffie–Hellman.
- **AESGCM** — AES-256-GCM, với nonce của Noise: bốn byte không theo sau là
  bộ đếm thông điệp 64-bit, big-endian.
- **SHA256** — SHA-256 cho băm và HKDF.

**Initiator là thiết bị điều khiển** — trình duyệt, hoặc một Zode khác
(xem [Điều khiển Zode](#controlling-zode)); responder là Zode được
điều khiển.

```
  <- s                       (known before the handshake)
  -> s
  ...
  -> e, es, ss               message 1, may carry a payload
  <- e, ee, se               message 2, may carry a payload
```

Cả hai khoá pre-message được trộn vào transcript hash theo thứ tự đó:
khoá tĩnh của initiator trước, rồi tới khoá tĩnh của responder.

### Prologue

```
"zode-remote/1" ‖ 0x1F ‖ user_id ‖ 0x1F ‖ initiator_device_id ‖ 0x1F ‖ responder_device_id
```

Các trường là UTF-8. Dùng `0x1F` (ký tự phân tách đơn vị ASCII) thay vì ký tự
in được vì không id nào chứa nó, nên không có hai bộ ba khác nhau nào ghép ra
cùng một prologue. Trường rỗng, hoặc chứa `0x1F`, bị từ chối.

Prologue được trộn vào transcript hash trước mọi thứ khác, nên một
handshake làm cho một tài khoản và một cặp thiết bị sẽ không mở được dưới cặp khác:
thông điệp đầu tiên xác thực thất bại. Relay nào cố ghép một
handshake dành cho thiết bị của người khác vào của bạn sẽ chẳng được gì.

### Quy tắc handshake

- Payload handshake tối đa 65487 byte: giới hạn thông điệp 65535 byte trừ
  khoá ephemeral 32 byte và tag 16 byte.
- Handshake xác thực thất bại là handshake chết. Nó bị bỏ, không thử lại
  trên cùng trạng thái.
- Khoá công khai — đã ghim, hoặc khoá ephemeral ở đầu một thông điệp
  handshake — mà toàn số không hoặc là bất kỳ điểm Curve25519 bậc nhỏ nào khác
  đều bị từ chối. Khoá như vậy khiến kết quả Diffie–Hellman là thứ người quan sát có thể
  tính ra. Phép kiểm tra bỏ qua bit cao, giống như X25519.
- Responder bỏ handshake chưa hoàn tất trong 10 giây, và
  giữ tối đa 8 handshake mở dở cùng lúc. Relay có thể khởi tạo handshake thay
  một thiết bị mà không tốn gì, nên nếu không có giới hạn thì mỗi cái là bộ nhớ mà relay được
  tiêu xài.

### Transport

Sau handshake mỗi bên giữ hai khoá, mỗi chiều một khoá. Một thông điệp là
kết quả AES-GCM seal của plaintext, với associated data rỗng và giá trị
bộ đếm kế tiếp làm nonce, cho ra `ciphertext ‖ 16-byte tag`.

- **Responder không được hành động trên một phiên cho tới khi giải mã được thông điệp
  transport hợp lệ đầu tiên** — thực tế là `hello`. Thông điệp handshake 1 không còn mới: một
  relay đã ghi lại có thể phát lại nó, và responder không phân biệt được
  cho tới khi peer chứng minh nó giữ các khoá dẫn xuất bằng cách gửi
  một thứ gì đó dưới các khoá đó. Cho tới lúc đó responder không mở terminal,
  không liệt kê file, cũng không trả lời yêu cầu nào. Initiator không phải chờ như vậy, vì
  thông điệp 2 mang một khoá ephemeral mới nên không thể là bản phát lại.
- Bộ đếm là ngầm định, nên thông điệp phải đến đúng thứ tự. Một WebSocket
  duy nhất mỗi peer của relay đảm bảo điều đó.
- Một thông điệp xác thực thất bại **đóng phiên vĩnh viễn**. Không có chuyện
  bỏ qua thông điệp hỏng rồi đi tiếp.
- Phiên kết thúc sau 2^32 thông điệp một chiều. Các peer chạy handshake mới
  thay vì rekey. Bên gọi giữ cả một stream trước khi
  gửi có thể yêu cầu seal nó cùng lúc (`encrypt_data_chunks` trong
  `remote_relay_protocol`); một stream không vừa số thông điệp còn lại sẽ
  bị từ chối toàn bộ, trước khi gửi bất kỳ phần nào, và phiên bị đóng. Relay client
  và host của Zode không làm vậy. Chúng seal từng frame khi byte
  đến, nên hết thông điệp thì phiên kết thúc giữa chừng một stream,
  và peer thấy đó là phiên đóng chứ không phải một stream kết thúc
  trọn vẹn.
- Noise giới hạn một thông điệp ở 65535 byte. Plaintext mỗi thông điệp được giữ ở 61440
  byte (60 KiB), gồm cả header.

## Inner frame

Plaintext của mỗi thông điệp transport:

```
offset 0   kind         u8      0 = control, 1 = data
offset 1   stream_id    u32, big-endian
offset 5   payload      0 to 61435 bytes
```

- Frame **control** luôn dùng `stream_id` 0 và mang một thông điệp JSON. Payload
  control rỗng là sai định dạng.
- Frame **data** dùng `stream_id` khác không do bên mở stream chọn.
  Payload data **rỗng** đánh dấu stream kết thúc.
- Nội dung lớn hơn một frame được cắt thành nhiều frame data trên cùng một
  stream. Frame không bao giờ bị cắt ngang qua các thông điệp Noise.

## Thông điệp điều khiển

JSON UTF-8, gắn thẻ bằng `t`. Trường mà một peer không nhận ra thì **bị bỏ qua**, nên
peer mới có thể bổ sung vào thông điệp mà không làm hỏng peer cũ. Một `t` không ai
nhận ra là lỗi: đoán một thông điệp lạ có nghĩa gì là cách để một yêu cầu
bị xử lý hai lần hoặc không lần nào.

Yêu cầu mang `request_id` do bên gửi chọn, và phản hồi lặp lại nó.
Nội dung file, diff và đầu ra terminal không đi trong thông điệp control mà đi
trong frame data trên `stream_id` mà phản hồi nêu tên.

| `t`                   | Trường                                                                              | Hướng              |
| --------------------- | ----------------------------------------------------------------------------------- | ------------------ |
| `hello`               | `relay_protocol`, `app_version`, `rpc_protocol`, `capabilities`                     | trình duyệt → Zode |
| `hello_ack`           | giống `hello`                                                                       | Zode → trình duyệt |
| `error`               | `code`, `message`, `request_id` tuỳ chọn                                            | cả hai             |
| `ping` / `pong`       | không có                                                                            | cả hai             |
| `agent_list`          | `agents`: `[{id, name, status, title?, session_id?, agent?}]`                       | Zode → trình duyệt |
| `agent_update`        | `agent`: `{id, name, status, title?, session_id?, agent?}`                          | Zode → trình duyệt |
| `terminal_list`       | `terminals`: `[{id, title, columns, rows}]`                                         | Zode → trình duyệt |
| `terminal_attach`     | `terminal_id`, `stream_id`                                                          | trình duyệt → Zode |
| `terminal_attached`   | `terminal_id`, `stream_id`, `columns`, `rows`                                       | Zode → trình duyệt |
| `terminal_detach`     | `terminal_id`                                                                       | trình duyệt → Zode |
| `terminal_resized`    | `terminal_id`, `columns`, `rows`                                                    | cả hai             |
| `terminal_closed`     | `terminal_id`, `exit_code` tuỳ chọn                                                 | Zode → trình duyệt |
| `ide_open`            | `request_id`, `path`, `line` tuỳ chọn, `stream_id`, `app_version`, `proto_version`  | controller → Zode  |
| `ide_opened`          | `request_id`, `stream_id`, `path_style`, `shell`, `default_shell`                   | Zode → controller  |
| `files_list`          | `request_id`, `worktree_id` tuỳ chọn, `path`                                        | trình duyệt → Zode |
| `files_list_reply`    | `request_id`, `entries`: `[{name, kind, size?, worktree_id?}]`, `truncated`, `more` | Zode → trình duyệt |
| `file_read`           | `request_id`, `worktree_id`, `path`                                                 | trình duyệt → Zode |
| `file_read_reply`     | `request_id`, `size`, `stream_id`                                                   | Zode → trình duyệt |
| `diff_request`        | `request_id`, `worktree_id`                                                         | trình duyệt → Zode |
| `diff_reply`          | `request_id`, `stream_id`, `truncated`                                              | Zode → trình duyệt |
| `sessions_list`       | `request_id`, `before` và `limit` tuỳ chọn                                          | trình duyệt → Zode |
| `sessions_list_reply` | `request_id`, `sessions`: `[entry]`, `more`, `next` và `unavailable_hosts` tuỳ chọn | Zode → trình duyệt |
| `session_resume`      | `request_id`, `session_id`, `fork`                                                  | trình duyệt → Zode |
| `session_resumed`     | `request_id`, `open_agent_id` tuỳ chọn                                              | Zode → trình duyệt |

`status` là một trong `working`, `waiting_for_input`, `idle`, `finished`,
`failed`; giá trị mà bên đọc không biết được đọc là unknown thay vì
làm hỏng cả danh sách. `kind` là một trong `file`, `directory`, `symlink`, và
cũng suy biến thành `other`.

Bản tóm tắt agent còn có thể mang `session_id`, id của cuộc hội thoại mà
tab đang ghi như `sessions_list` gọi tên, khi tab có, và `agent`,
là `claude`, `codex`, `copilot` hoặc `opencode` và bị bỏ với agent tuỳ chỉnh.
Tab mở dưới dạng fork không có `session_id`. Bên đọc không biết hai
trường này thì bỏ qua chúng.

`capabilities` trong `hello_ack` liệt kê những gì host phục vụ: luôn có `terminal`,
rồi `files`, `ide` và `sessions` theo thứ tự đó, mỗi cái chỉ khi nó được phục vụ.
Yêu cầu mà capability của nó thiếu sẽ được trả lời bằng `error` code
`unsupported`.

Sau `terminal_attach`, đầu ra terminal chảy Zode → trình duyệt và phím gõ
trình duyệt → Zode dưới dạng frame data trên `stream_id` của lần attach, cho tới khi
`terminal_detach` hoặc `terminal_closed`.

Những byte đầu tiên trên một stream terminal là snapshot của màn hình: một lần reset
đầy đủ (`ESC c`), scrollback và màn hình được vẽ lại, rồi các chế độ của terminal
(gồm cả cờ bàn phím kitty mà chương trình yêu cầu), con trỏ, và
trạng thái pending-wrap. Nó cũng mang những gì một chương trình chỉ vẽ lại phần
thay đổi phụ thuộc vào: vùng cuộn, con trỏ được lưu bằng `ESC 7` cùng
thuộc tính và bộ ký tự của nó, và các bộ ký tự đang dùng cùng bộ đang
shift-in. Sau đó là đầu ra thô, và hai phần không bao giờ chồng lên nhau:
đầu ra mà snapshot đã vẽ thì không gửi lại. Hãy coi stream là **byte**,
không phải văn bản: snapshot lớn hơn một frame bị cắt thành các frame ở
offset tuỳ ý, có thể rơi vào giữa một ký tự UTF-8 hoặc một escape sequence, và
đầu ra trực tiếp cũng vậy.

Một snapshot mới (lại bắt đầu bằng `ESC c`) thay màn hình mỗi khi
kích thước của host đổi, một bên đọc chậm tụt quá xa, đầu ra bị mất, chương trình
rời alternate screen, hoặc đến hạn gửi settle snapshot (bên dưới); tối đa
một lần mỗi 250 ms cho mỗi terminal và thiết bị. Khoảng cách này giữ nguyên qua
detach rồi attach lại: thiết bị attach lại không được nợ snapshot sớm hơn
thiết bị ở lại.

Hai trường hợp trong số đó cần giải thích:

- **Rời alternate screen.** Snapshot chụp khi chương trình đang ở
  alternate screen mang màn hình đó chứ không phải màn hình chính phía sau, nên
  thiết bị không có gì để quay về khi chương trình thoát. Khi đó host gửi một
  snapshot mới tới từng thiết bị đã nhận snapshot như vậy.
- **Bên đọc chậm.** Khi đầu ra trực tiếp làm tràn hàng đợi của thiết bị, hàng đợi
  bị bỏ và cần một snapshot mới. Snapshot đang gửi dở
  được gửi trọn, và snapshot mới theo sau: cắt ngang nó sẽ để
  thiết bị không theo kịp phải nhìn màn hình trống suốt thời gian
  đầu ra còn tiếp diễn.

Không được mang theo: màu gạch chân, hyperlink, màn hình chính phía sau
một alternate screen, và việc con trỏ đã lưu có đang chờ wrap hay không. Trong
các bộ ký tự, host chỉ có ASCII và line drawing, nên chỉ hai bộ đó
được mang theo.

Hạn chế: bộ giả lập của host thấy byte trước khi vẽ chúng. Snapshot
chụp khi chương trình đang ở giữa một synchronized update (DEC mode 2026)
hoặc một escape sequence có thể đại diện cho đầu ra mà màn hình chưa hiển thị. Khi
snapshot được chụp lúc đầu ra đang chảy, hoặc lúc một synchronized update
đang mở, host vì vậy gửi thêm một cái nữa, settle snapshot, để sửa lại:

- Nó được gửi khi đầu ra đã yên lặng 400 ms. Mỗi khối đầu ra mới
  khởi động lại khoảng chờ đó, nhưng không bao giờ vượt quá 2 s sau snapshot mà nó
  sửa.
- Nếu một synchronized update vẫn còn mở khi đến hạn, host kiểm tra lại
  mỗi 50 ms, tối đa 20 lần (khoảng một giây), rồi gửi luôn, vì
  chương trình thoát giữa một update không bao giờ kết thúc nó.
- Settle snapshot không có cái nào theo sau. Snapshot chụp vì lý do khác
  khi một cái đang chờ sẽ thay nó, và chỉ kéo theo lần lặp riêng nếu
  nó cũng được chụp trước màn hình.

Client nên sẵn sàng cho lần reset thứ hai đó và vẽ nó không nhấp nháy nếu
có thể.

### Project server stream

Một Zode điều khiển Zode khác sẽ mở dự án của Zode kia như thể nó nằm trên
đĩa của chính mình. Zode được điều khiển chạy project server cho nó và mang
các thông điệp của server đó trên một stream của kênh mã hoá. Trình duyệt
không dùng cái này.

- **Capability.** Host chỉ liệt kê `ide` trong `hello_ack` `capabilities` khi
  tìm được một project server để chạy. Nếu không, `ide_open` được trả lời bằng
  `error` code `unsupported`.
- **Mở.** Controller gửi `ide_open` với `stream_id` do nó chọn
  (khác không, chưa dùng), `app_version` nó đang chạy và `proto_version` của
  giao thức riêng của project server. `path` là thư mục nó định mở, hoặc
  rỗng nếu chưa chọn; host không giới hạn server trong thư mục đó.
  Yêu cầu thiếu `stream_id`, `app_version` hoặc `proto_version`, hoặc nêu
  stream 0 hay stream từ `0x80000000` trở lên, được trả lời bằng `error` code
  `malformed` kèm `request_id`.
- **Bị từ chối khi các server trước còn đang dừng.** Tối đa bốn project server
  được ở trạng thái đang bị dừng cùng lúc. Thiết bị mở một dự án
  khi bốn cái vẫn đang thoát sẽ được trả lời bằng `error` code
  `rate_limited` kèm `request_id`, với thông điệp "earlier project servers
  are still being stopped; try again shortly". Không có gì được khởi chạy, và
  phiên vẫn tiếp tục.
- **Phản hồi.** `ide_opened` lặp lại `request_id` và `stream_id`, và cho
  controller biết điều nó không thể tự biết từ stream trước khi dựng
  kết nối trên đó: `path_style` (`posix` hoặc `windows`),
  `shell` đăng nhập của host và `default_shell` của nó.
- **Framing.** Trên stream, byte chảy hai chiều cho tới khi stream kết thúc. Chúng
  là framing riêng của project server, mà giao thức này không diễn giải:
  mỗi thông điệp là độ dài 4 byte little-endian theo sau là đúng chừng đó byte.
  Hãy coi stream là byte; một thông điệp có thể bị cắt qua nhiều frame data. Cả
  controller lẫn project server đều không đọc thông điệp dài hơn 256 MiB: độ
  dài vượt mức đó làm stream kết thúc với lỗi. Host có đọc
  phía controller đủ để phân biệt ping với request, để giới hạn
  nhàn rỗi đếm request chứ không đếm heartbeat. Nó không bao giờ hành động theo những gì
  đọc được, và các byte được chuyển tiếp nguyên vẹn tới server.
- **Một stream mỗi phiên.** Một phiên có tối đa một project stream. Cái thứ hai
  `ide_open` khi một cái đang mở được trả lời bằng `error` code `too_large` và
  `request_id`; cái đầu không bị ảnh hưởng. Sau khi nó kết thúc, có thể mở cái khác.
- **Kết thúc.** Một frame data rỗng kết thúc stream từ phía nào cũng được. Khi
  controller kết thúc nó, project server và mọi terminal nó đã mở bị dừng;
  khi host kết thúc, nghĩa là server đã thoát. Không bên nào kết thúc phiên:
  bản mirror vẫn chạy. Kết thúc phiên thì stream cũng kết thúc theo cách đó.
- **Dừng server.** Host yêu cầu server dừng, khiến nó
  đóng các terminal trước, và kill nó nếu sau 5 giây vẫn còn.
  Trên hệ thống có `SIGTERM` thì đó là yêu cầu. Trên Windows không có
  tín hiệu như vậy, nên host ghi một file `server.stop` vào thư mục trạng thái
  của server, cạnh file pid, và server, cứ mỗi
  250 ms lại tìm file đó, sẽ dừng và thoát khi nó xuất hiện. Phiên kết thúc trước khi server
  ghi file pid vẫn được dừng: host chờ tối đa 5 giây để
  có file.
- **Terminal.** Một project server giữ tối đa 16 terminal cho controller của nó
  cùng lúc, và terminal còn đang đóng vẫn được tính cho tới khi tiến trình của nó
  được thu dọn. Cái thứ thêm bị từ chối với thông điệp "too many terminals: at most 16 may
  be open at once".
- **Phiên bản.** Thông điệp của project server chỉ tương thích giữa các
  bản build giống hệt nhau, và ở phiên bản này không có gì được tải lên qua relay. Vì vậy
  `app_version` phải bằng của host và `proto_version` phải bằng phiên bản
  giao thức dự án của host. Nếu một trong hai khác, host trả lời `error` code
  `version` kèm `request_id`, bằng lời nhắn người dùng cập nhật cả hai
  Zode lên cùng một phiên bản, và không khởi chạy gì. Phiên vẫn tiếp tục.
- **Ai được yêu cầu.** Chỉ phiên đã giải mã được một thông điệp hợp lệ
  dưới khoá của nó, và đã nói `hello`, mới được xử lý; project server chạy
  với quyền của người dùng Zode được điều khiển và không hơn.

### File

Duyệt chỉ đọc các thư mục đang mở trên host. Ở đây không có gì ghi, và không
chương trình nào được chạy để tạo diff.

- **Capability.** Host chỉ liệt kê `files` trong `hello_ack` `capabilities` khi
  nó phục vụ được các yêu cầu này. Nếu không, chúng được trả lời bằng `error` code
  `unsupported` và `request_id`.
- **Thư mục.** `files_list` không có `worktree_id` liệt kê các thư mục đang mở: mỗi thư mục
  một mục, với `worktree_id` đặt thành id dùng về sau (một chuỗi
  thập phân). Với `worktree_id`, nó liệt kê con trực tiếp của `path`, là đường dẫn
  tương đối với thư mục đó và dùng `/`; `path` rỗng là chính thư mục đó.
  Các mục xếp thư mục trước, rồi theo tên không phân biệt hoa thường; `kind`
  là `file` hoặc `directory`, `size` chỉ đặt cho file, và mục không có
  `worktree_id` bên dưới các gốc. Liệt kê tối đa 2000 mục, và không quá
  khoảng 1.5 MiB sau khi mã hoá; `truncated` cho biết còn nhiều hơn.
- **Danh sách dài.** Danh sách không vừa một thông điệp control sẽ đến
  dưới dạng nhiều thông điệp `files_list_reply` cùng `request_id`, theo thứ tự.
  Mọi cái trừ cái cuối đều có `more`; `truncated` giống nhau ở tất cả.
  Hãy nối các `entries` của chúng.
- **Đọc.** `file_read` cần `worktree_id` và `path`. Phản hồi cho biết
  `size` của file và một `stream_id`; các byte theo sau trong frame data trên đó, và
  một frame data rỗng kết thúc stream. Chỉ văn bản UTF-8 tới 1 MiB được gửi: file
  không phải văn bản được trả lời bằng `error` code `binary`, file lớn hơn bằng
  `too_large`. File rỗng là một phản hồi và một lần kết thúc stream không có byte
  nào ở giữa.
- **Diff.** `diff_request` cần `worktree_id`. Phản hồi mang một `stream_id`;
  diff dạng unified của working tree so với `HEAD` (thay đổi đã staged và chưa staged,
  cộng các file git chưa theo dõi) theo sau trên đó theo cách tương tự.
  Các file git chưa theo dõi và không bỏ qua mỗi file được hiển thị như file mới,
  tối đa 100 file. Nó chỉ bao gồm các file nằm trong thư mục đó, nên thư mục
  là một phần của repository lớn hơn sẽ không hiện phần còn lại, và bỏ
  những gì thư mục không hiển thị: file private và các file mà
  `file_scan_exclusions` của người dùng ẩn, kể cả file mới. Không chương trình diff bên ngoài và
  không chương trình textconv nào được chạy. Các bộ lọc git riêng của repository (ví dụ bộ lọc `clean`
  nêu trong attributes) vẫn áp dụng, như với bất kỳ
  `git status` hay `git diff` nào Zode chạy. Submodule đã thay đổi hiện ở dạng
  ngắn, commit cũ và mới, bất kể `diff.submodule` của người dùng nói gì. Đường dẫn
  thay đổi không phải UTF-8 hợp lệ bị bỏ. Gửi tối đa 2 MiB, cắt ở cuối một dòng. `truncated` cho biết
  diff chưa phải toàn bộ thay đổi: nó bị cắt, có hơn 100 file mới,
  một file không đọc được, hoặc đến giây thứ 8 của lần đọc vẫn còn file mới
  chưa bắt đầu. Thư mục không nằm trong repository git, hoặc
  repository chưa có commit, được trả lời bằng `not_found`.
- **Thời gian dành cho diff.** Git được cấp 20 giây cho một diff. Nếu lâu
  hơn, git bị dừng và yêu cầu được trả lời bằng `internal`, để
  chừa khoảng trống trước 30 giây mà mọi yêu cầu có. Diff mà không ai
  còn chờ nữa, vì yêu cầu hết hạn hoặc phiên kết thúc, cũng
  bị dừng, và diff đang chờ tới lượt thì bị bỏ qua, để nó không làm chậm
  các công việc git khác của repository.
- **Stream do host mở.** `stream_id` trong phản hồi do host chọn
  và từ `0x80000000` trở lên, nên không bao giờ trùng id trình duyệt chọn cho
  stream của nó; trình duyệt không được chọn id cao như vậy cho `terminal_attach`
  hay `ide_open`, những yêu cầu đó được trả lời bằng `malformed`. Frame data dài tối đa
  một frame; phản hồi được gửi trước frame đầu tiên.
- **Cái gì có thể được gọi tên.** Đường dẫn chỉ được tra trong các file host đã
  quét trong thư mục đó, không bao giờ trên đĩa của nó. Mọi thứ khác -- `..`, đường dẫn
  tuyệt đối, đường dẫn có `..` hoặc dấu phân cách đôi, file mà
  `file_scan_exclusions` của người dùng ẩn, `worktree_id` không rõ -- là `not_found`. File
  người dùng đánh dấu private (cài đặt `private_files`) và
  mọi thứ trong thư mục private cũng vậy: file private không bao giờ được liệt kê, đọc hay
  hiển thị trong diff. Symlink cũng không được phục vụ: symlink không được liệt kê, và
  mọi thứ đi qua nó cũng không, dù nó trỏ ra ngoài thư mục
  hay không. Điều này được kiểm tra lại khi đọc file, theo những gì trên đĩa
  lúc đó: một thư mục trên đường tới file mà đã thành symlink dẫn ra ngoài
  thư mục thì không được đi theo, và mọi thứ không phải file thường, như
  named pipe hay socket, là `not_found`. Chỉ các thư mục người dùng thấy mới được đưa ra.
- **Giới hạn.** Một thiết bị có thể có bốn yêu cầu file đang được xử lý cùng lúc, và
  tối đa 8 MiB phản hồi đang chờ relay. Một yêu cầu chỉ được vào nếu
  các phản hồi đang chờ cộng với phản hồi lớn nhất (2 MiB) mà mỗi yêu cầu
  đang xử lý, kể cả cái này, còn có thể tạo ra vẫn vừa trong 8 MiB; nếu
  không, nó được trả lời bằng `rate_limited`. Những lần từ chối đó, và lỗi cho
  yêu cầu đã được nhận, được xếp hàng sau các thông điệp control đang
  chờ, nên đến theo thứ tự. Thông điệp control nhỏ và đi trước
  frame file và diff: phản hồi không chờ sau một file, và frame của file
  không bao giờ vượt phản hồi nêu tên stream của chúng. Lỗi cho yêu cầu chưa từng được nhận --
  `unsupported`, hoặc `malformed` do thiếu `worktree_id` -- được gửi ngay
  và có thể đến trước các phản hồi còn chờ; `request_id` của nó cho biết nó trả lời
  yêu cầu nào. Thiết bị có phản hồi đang chờ đạt 8 MiB cộng thêm
  64 KiB là đang phớt lờ những gì được báo, và phiên của nó bị kết thúc khi
  phản hồi kế tiếp được xếp hàng cho nó. Chỉ những gì thiết bị yêu cầu mới được tính vào
  đó. Những gì host tự đẩy không ai hỏi, như trạng thái agent hay danh sách
  terminal, không bao giờ kết thúc phiên: bản đẩy mới hơn thay bản cũ hơn về
  cùng một việc còn đang chờ, và tối đa 256 bản đẩy chờ một thiết bị. Kết thúc phiên sẽ huỷ các yêu cầu còn
  đang xử lý. Yêu cầu chưa xong
  sau 30 giây được trả lời bằng `internal` và chỗ của nó được giải phóng.
  Yêu cầu thiếu `worktree_id` ở nơi cần là `malformed`.

### Session

Liệt kê các cuộc hội thoại agent trước đây của các thư mục đang mở trên host, và khởi động lại
một cuộc cho thiết bị. Bản ghi riêng của host về mỗi phiên quyết định nó thuộc về
agent nào, chạy ở đâu và được tiếp tục ra sao; thiết bị gọi tên một
phiên bằng id của nó và không gì khác.

Một danh sách, rồi tiếp tục phiên mà nó cho thấy đang chạy sẵn: host
không mở gì và đặt tên cho tab (`→` là trình duyệt tới Zode, `←` là phản hồi):

```text
→ {"t":"sessions_list","request_id":7}
← {"t":"sessions_list_reply","request_id":7,"sessions":[{"id":"0d6c6a52-9a3f-4a51-9f0e-6c1b2a3d4e5f","agent":"claude","title":"Fix the terminal snapshot","preview":"Carry the scroll region","worktree_id":"3","folder":"zode/crates/terminal","branch":"main","updated_at":1760572800000,"can_fork":true,"open_agent_id":"terminal-42"}],"more":false,"next":{"updated_at":1760572800000,"id":"0d6c6a52-9a3f-4a51-9f0e-6c1b2a3d4e5f"}}
→ {"t":"session_resume","request_id":9,"session_id":"0d6c6a52-9a3f-4a51-9f0e-6c1b2a3d4e5f","fork":false}
← {"t":"session_resumed","request_id":9,"open_agent_id":"terminal-42"}
```

- **Capability.** Host liệt kê `sessions` trong `hello_ack` `capabilities` khi
  nó phục vụ các yêu cầu này. Nếu không, `sessions_list` và `session_resume` được
  trả lời bằng `error` code `unsupported` và `request_id`. Các thông điệp này
  ra đời mà không có `rpc_protocol` mới, nên client chỉ gửi chúng tới host
  có liệt kê capability đó.
- **Phạm vi.** Một phiên chỉ được liệt kê nếu thư mục nó chạy nằm trong
  thư mục hiển thị của một dự án đang mở trong cửa sổ trên host, trên cùng máy
  với phiên, khớp theo từng thành phần đường dẫn đầy đủ. Ngoài máy của chính host,
  đó là mọi bản phân phối WSL, SSH host và dev container mà một cửa sổ đang mở
  trên đó; dự án chia sẻ của cộng tác viên và host kết nối qua relay bị
  loại. Thư mục đi ngược ra ngoài bằng `..` không thuộc thư mục nào, và
  khi các thư mục đang mở lồng nhau, thư mục sâu nhất sở hữu phiên.
  `folder` là tên thư mục đó theo sau là đường dẫn bên trong nó, nối bằng
  `/`; thư mục không có tên riêng được gọi là `root`. Đường dẫn tuyệt đối
  không bao giờ được gửi.
- **Đường dẫn ẩn.** Phiên có thư mục nằm tại hoặc dưới một đường dẫn mà thư mục của nó
  đánh dấu `private_files`, hoặc loại bỏ bằng `file_scan_exclusions`, được coi như
  không có: nó không được liệt kê, và tiếp tục nó là `not_found`. Đây là hai
  phép kiểm tra mà trình duyệt file thực hiện, lấy từ cài đặt của thư mục sở hữu
  thư mục đó.
- **Danh sách.** `sessions_list` trả về các phiên mới nhất trước: theo
  `updated_at` giảm dần, rồi `id` tăng dần. `limit` mặc định 50 và bị giữ
  trong khoảng 1 tới 200. `before` là `{updated_at, id}`, chép từ `next`
  của một phản hồi trước, và trang bắt đầu sau mục đó. Yêu cầu không có
  `before` khiến host đọc lại kho phiên của các agent trước, trừ khi
  một lần đọc đã xong cách đây chưa tới 5 giây, khi đó nó trả lời từ lần đó.
  Các trang sau được đọc mà không quét lại. Kho của host từ xa được yêu cầu
  đọc lại theo cùng điều kiện, nhưng phản hồi không chờ chúng: nó
  có những gì kho của host đang giữ khi được hỏi, và host có kho
  chưa được đọc, hoặc lần đọc cuối thất bại, được nêu trong
  `unavailable_hosts` thay vào đó.
- **Trang dài.** Trang không vừa một thông điệp control sẽ đến dưới dạng
  nhiều thông điệp `sessions_list_reply` cùng `request_id`, theo thứ tự.
  Mọi cái trừ cái cuối đều có `more`, và chỉ cái cuối mang `next`, và
  chỉ khi còn trang khác. Hãy nối các `sessions` của chúng.
- **Host.** Phiên trên host từ xa có `host` được đặt, và `id` cùng
  `worktree_id` của nó có tiền tố là một token nêu tên host và dấu hai chấm:
  `{token}:{id}`. Token là 12 chữ số hex thường đầu tiên của SHA-256 của
  danh tính host, nên nó giữ nguyên qua các lần khởi động lại và không lộ
  người dùng hay cổng. Phiên trên máy của chính host không có `host` và id của nó
  vẫn như xưa, nên bên đọc không biết trường đó vẫn liệt kê mọi
  phiên như trước, và coi id là mờ đục: một id trên hai máy là hai
  mục, và tiếp tục cái nào thì mở nó trên máy của chính nó.
- **Host không khả dụng.** Thông điệp cuối của trang đầu có thể mang
  `unavailable_hosts`, tối đa 16 phần tử `{host, reason}`: `host` như
  `host` của một mục, `reason` là `reading` khi các phiên của nó chưa được đọc và
  danh sách sau sẽ có, hoặc `unavailable` khi không đọc được
  vì host cũ, không với tới được hoặc đã ngắt kết nối. Chuyện gì đã hỏng
  trên host không bao giờ được gửi. Bỏ đi khi mọi host đều được liệt kê.
- **Mục.** Mỗi mục trong `sessions` có các trường này. `title`, `preview`,
  `folder` và `branch` bị cắt còn 200 ký tự, ký tự cuối là `…` khi
  có phần bị cắt.

| Trường          | Ý nghĩa                                                                                                                            |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `id`            | Id riêng của agent, có tiền tố với host từ xa (xem Host). Thứ `session_resume` nhận.                                               |
| `agent`         | `claude`, `codex`, `copilot` hoặc `opencode`.                                                                                      |
| `title`         | Tên của phiên. Khi trống thì là `preview` của nó.                                                                                  |
| `preview`       | Phần đầu cuộc hội thoại. Bỏ đi khi trống.                                                                                          |
| `worktree_id`   | Thư mục đang mở mà nó chạy trong đó: như `files_list` gọi tên cho máy này; với host từ xa nó chỉ để phân biệt các thư mục.         |
| `folder`        | Tên thư mục và đường dẫn bên trong nó.                                                                                             |
| `branch`        | Nhánh mà phiên đã ghi lại, khi nó có ghi.                                                                                          |
| `updated_at`    | Lần cập nhật cuối của phiên, tính bằng mili giây Unix.                                                                             |
| `can_fork`      | Có được đặt `fork` cho nó hay không: true với Claude Code và opencode. Luôn được gửi.                                              |
| `open_agent_id` | `id` trong `agent_list` của một tab đang chạy phiên này. Bỏ đi khi không có.                                                       |
| `host`          | Host từ xa mà nó nằm trên: biệt danh SSH hoặc tên host, một distro, tên container, cắt còn 60 ký tự. Bỏ đi với máy của chính host. |

- **Đã mở sẵn.** `open_agent_id` được đặt khi một tab trong `agent_list` có
  `session_id` bằng `id` của mục và agent của nó đang chạy trong đó. Tab
  mà agent đã thoát thì không tính.
- **Tin cậy.** `session_id` được kiểm tra hình dạng trước khi tra: 1 tới
  128 byte gồm chữ cái ASCII, chữ số, `.`, `_` và `-`, ký tự đầu là chữ cái hoặc
  chữ số, để không bao giờ bị đọc thành cờ hay thành `.` hoặc `..` -- tuỳ chọn sau một
  token 12 chữ số hex thường và dấu hai chấm. `id` bên trong `before` phải
  có cùng hình dạng, và phiên có id không thoả thì không được liệt kê. Id
  không vừa là `not_found`.
- **Tiếp tục.** `session_resume` với `fork` false, cũng là mặc định,
  tiếp tục phiên. Nếu một tab đã chạy nó, host trả lời
  `session_resumed` kèm `open_agent_id` của tab đó và không mở gì. Nếu không,
  nó mở một tab trên phiên đó trong cửa sổ có thư mục chứa thư mục chạy,
  trên máy mà id nêu, và trả lời `session_resumed` không có
  `open_agent_id`. Thư mục còn đó hay không được hỏi máy
  đó. Host từ xa không còn cửa sổ nào mở thì không có gì để tiếp tục,
  kể cả trong danh sách lấy lúc nó còn, và host không bao giờ kết nối lại với nó vì
  một thiết bị. Tab được thêm sau phản hồi và đến như mọi tab khác, dưới dạng
  `agent_update` mà `session_id` là cái được yêu cầu. Tab vẫn đang khởi động agent thì
  chưa tính là đang chạy, nhưng nó vẫn giữ phiên của mình: một
  `session_resume` thứ hai cho phiên đó không mở gì. Phản hồi của nó vẫn là
  `session_resumed` không có `open_agent_id`, và tab đến là tab
  mà yêu cầu đầu đã mở.
- **Fork.** Với `fork` true host luôn mở tab mới, và agent
  rẽ nhánh cuộc hội thoại: Claude Code được khởi chạy với
  `--resume <id> --fork-session` và opencode với `-s <id> --fork`. Agent
  chọn id của nhánh và không báo lại, nên tóm tắt của tab mới
  không có `session_id`. Nếu nguồn đã biến mất khỏi kho của agent khi
  tab khởi động, tab báo phiên không khả dụng và không mở một
  tab trống. `fork` trên phiên có `can_fork` false là `unsupported`.
- **Giới hạn.** `sessions_list` tính vào cùng hạn mức với yêu cầu file:
  bốn yêu cầu xử lý cùng lúc, và 8 MiB phản hồi đang chờ
  relay, như mô tả ở mục File. Một thiết bị có thể có một `session_resume` đang được
  xử lý tại một thời điểm, tính trên mọi kết nối của nó. Sau khi một cái đã mở
  tab được phản hồi, cái kế tiếp từ thiết bị đó được trả lời `rate_limited`
  trong 3 giây, bất kể nó nêu phiên nào và `fork` của nó là gì. Tab
  được thêm sau phản hồi và agent chỉ được tính là đang chạy khi tiến trình của nó
  chạy, nên đến lúc đó `open_agent_id` chưa được đặt cho nó. Phản hồi chỉ
  trỏ tới tab đang chạy, và lỗi, không mở gì và không bắt đầu thời gian chờ. Yêu cầu
  chưa xong sau 30 giây được trả lời bằng `internal`.
- **Thông báo trên host.** Khi một phiên được mở cho một thiết bị, host hiện
  một thông báo, `<device> resumed “<title>”` hoặc `<device> forked “<title>”`, trong
  workspace nhận tab và cả trong workspace đang ở phía trước nếu nó
  khác. Tiêu đề bị cắt còn 60 ký tự và tên thiết bị còn 40.
  Mỗi thông báo tách biệt, nên cái thứ hai không thay cái đầu, và mỗi cái tự
  biến mất sau năm giây. Phản hồi chỉ trỏ tới tab đang chạy không hiện
  thông báo nào.

Lỗi, trong thông điệp `error` kèm `request_id`:

| Yêu cầu          | `code`         | Khi nào                                                                                                                                                                                                                                            |
| ---------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| một trong hai    | `unsupported`  | Host không phục vụ session.                                                                                                                                                                                                                        |
| `sessions_list`  | `not_found`    | `before.id` không thể là id phiên.                                                                                                                                                                                                                 |
| `sessions_list`  | `rate_limited` | Hạn mức file và session đã dùng hết.                                                                                                                                                                                                               |
| `sessions_list`  | `internal`     | Quá hạn.                                                                                                                                                                                                                                           |
| `session_resume` | `not_found`    | Id không thể là id phiên hoặc không nêu phiên nào, phiên nằm ngoài các thư mục đang mở hoặc dưới đường dẫn private hay bị loại, thư mục của nó đã mất, cửa sổ đã đóng, không có cửa sổ nào mở trên host từ xa của nó, hoặc không hỏi được host đó. |
| `session_resume` | `unsupported`  | Agent không tiếp tục được nó, hoặc `fork` được đặt ở nơi `can_fork` là false.                                                                                                                                                                      |
| `session_resume` | `rate_limited` | Một lần tiếp tục cho thiết bị này đang được xử lý, một cái đã được mở cho nó trong 3 giây gần nhất, hoặc phản hồi đang chờ của nó vượt hạn mức.                                                                                                    |
| `session_resume` | `internal`     | Quá hạn, hoặc host hỏng vì lý do khác.                                                                                                                                                                                                             |

Thiết bị gửi `sessions_list_reply` hoặc `session_resumed` sẽ nhận `malformed`:
đó là phản hồi, không phải yêu cầu.

### Lỗi

| `code`         | Ý nghĩa                                                         |
| -------------- | --------------------------------------------------------------- |
| `version`      | Peer nói một phiên bản giao thức mà bản build này không hỗ trợ. |
| `malformed`    | Các byte không parse được thành thông điệp mà chúng tự nhận.    |
| `unauthorized` | Bên gửi không được dùng thiết bị hay tài nguyên mà nó nêu.      |
| `not_found`    | Yêu cầu nêu một thứ không tồn tại.                              |
| `too_large`    | Yêu cầu vượt giới hạn kích thước.                               |
| `binary`       | File được yêu cầu không phải văn bản UTF-8.                     |
| `rate_limited` | Bên gửi đi nhanh hơn mức bên nhận cho phép.                     |
| `internal`     | Bên nhận hỏng vì lý do không phải của bên gửi.                  |
| `unsupported`  | Bên nhận biết yêu cầu nhưng chưa phục vụ.                       |

Bên nhận gặp `code` không biết thì coi đó là từ chối yêu cầu đã gây ra nó.
`version` cũng là phản hồi cho `ide_open` từ một bản build khác; không như
`hello` với `relay_protocol` không rõ, nó không kết thúc phiên.
not end the session.

### Phiên bản

`relay_protocol` là `1`: relay frame, các tham số Noise và inner
frame. `rpc_protocol` là `1`: các thông điệp control và data. Peer
nhận `relay_protocol` mà nó không hỗ trợ sẽ trả lời bằng `error` code
`version` rồi đóng. Nó không đoán: đọc framing mới như framing cũ
nghĩa là diễn giải bản mã như thông điệp control.

## Ghép cặp

Ghép cặp dạy mỗi thiết bị khoá công khai tĩnh của thiết bị kia, và cho hai
người ở hai màn hình một cách kiểm tra rằng không ai ở giữa đã tráo chúng.
So sánh một số ngắn là điểm yếu của mọi cơ chế như vậy, nên nó được dựng
sao cho việc mài giũa cặp khoá cho tới khi số khớp sẽ không có tác dụng.

Vai trò: **requester** là trình duyệt, **acceptor** là Zode. Khoá là khoá công khai
X25519 thô 32 byte; nonce là 32 byte từ CSPRNG. Thông điệp là
JSON trong frame text của relay, với byte được mã hoá bằng base64 chuẩn có padding.

```
requester                                   acceptor
   │  pair_request {public_key, commitment}    │
   │ ────────────────────────────────────────► │   commitment = SHA-256(pk_B ‖ n_B)
   │  pair_accept  {public_key, nonce}         │
   │ ◄──────────────────────────────────────── │   pk_A, n_A
   │  pair_reveal  {nonce}                     │
   │ ────────────────────────────────────────► │   n_B — acceptor checks the commitment
   │                                           │
   both compute SAS and show it
```

Acceptor tính lại `SHA-256(pk_B ‖ n_B)` từ khoá trong request và
nonce được tiết lộ, và huỷ ghép cặp nếu nó khác với commitment. Thiết
bị từ chối ghép cặp với chính khoá của nó.

**Chuỗi xác thực ngắn (SAS)** gồm sáu chữ số:

```
digest = SHA-256("zode-pair/1" ‖ pk_A ‖ pk_B ‖ n_A ‖ n_B)
value  = (first 20 bits of digest, big-endian) mod 1000000
SAS    = value, zero-padded to six digits
```

`A` là acceptor và `B` là requester, bất kể bên nào là bên tính.

**Vì sao cần commitment.** Hash của riêng hai khoá công khai có thể bị mài giũa: một
máy ở giữa sinh cặp khoá cho tới khi hai khoá giả của nó cho ra
cùng sáu chữ số ở cả hai phía, cỡ một triệu lần thử. Khi requester
cam kết nonce của mình trước, nonce và khoá của acceptor được cố định trước khi
của requester được tiết lộ, nên các chữ số không phải thứ kẻ tấn công có thể
lái sau đó. Mỗi lần đoán khi đó cần một request được trả lời, mà số này
bị giới hạn (bên dưới), và một lần khớp sai làm ghép cặp thất bại một cách lộ liễu.

**Các quy tắc acceptor thực thi:**

- Nó giữ **một lượt trao đổi ghép cặp đang chờ tại một thời điểm**. Request thứ hai khi
  một cái đang chờ bị từ chối vì bận.
- Lượt trao đổi đang chờ **hết hạn sau 120 giây** kể từ request. Reveal
  đến muộn hơn bị từ chối, và request mới khi đó có thể thay nó.
- **Sau 3 lần thử thất bại, ghép cặp bị khoá.** Lần thử thất bại là một reveal
  không khớp commitment, hoặc các chữ số mà người dùng nói là không khớp.
  Khoá này không tự hết hạn: chỉ một hành động tường minh của người dùng mới mở khoá, vì một
  khoá tự hết hạn chỉ làm chậm việc đoán mà thôi.
- **Request được trả lời có giới hạn.** Acceptor đếm mỗi `pair_request` mà nó
  trả lời bằng `pair_accept`, dù sau đó có hỏi người dùng hay không:
  tối đa 3 mỗi giờ cho một id thiết bị requester và 12 mỗi giờ cho tất cả
  id. Trả lời là thứ cho phép requester biết các chữ số trước khi tiết lộ,
  nên nếu không có giới hạn, một relay có thể đóng mọi lượt trao đổi mà chữ số nó không
  ưng rồi thử lại mà người dùng không mất gì. Request vượt một trong hai giới hạn bị
  đóng mà không trả lời, và người dùng được báo. Request bị từ chối không
  tính, và số đếm trượt theo giờ. Ngoài ra, người dùng chỉ được hỏi
  tối đa 3 lần mỗi giờ; số đếm đó chỉ gồm các câu hỏi thực sự đã hiện ra.
- Một reveal là một lần thử. Dù kết quả ra sao, lượt trao đổi đang chờ kết thúc.
- Khoá công khai toàn số không, hoặc điểm bậc nhỏ nào khác, bị từ chối trong
  `pair_request` và `pair_accept`.

Khi cả hai người xác nhận các chữ số khớp, mỗi bên lưu khoá công khai của bên kia
làm khoá tĩnh đã ghim cho handshake Noise.

### Điều khiển Zode

Zode điều khiển Zode khác là **requester** khi ghép cặp và là
**initiator** của kênh, theo các vai trò mà trình duyệt có. Nó theo cùng
quy tắc, thêm một điều ở mỗi bước, vì nó là thiết bị giữ khoá mà nó có thể
đối chiếu với danh sách thiết bị của tài khoản:

- Không gì được gửi sau `pair_reveal`, và không gì được ghim, cho tới khi người dùng
  ở màn hình này nói rằng các chữ số khớp. Nếu họ nói chúng khác, lượt
  trao đổi bị bỏ.
- Khoá công khai mà host đưa ra khi ghép cặp phải bằng khoá mà
  tài khoản liệt kê cho thiết bị đó. Nếu không, ghép cặp bị từ chối; các chữ số
  thậm chí không được hiển thị.
- Sau khi xác nhận, một handshake `session` với khoá đã ghim phải
  thành công, và host phải trả lời `hello`, trước khi host được ghim: handshake
  là cổng thứ hai, và khoá không hoàn tất được nó thì không bao giờ được giữ.
- Các host mà một Zode đã ghép cặp được lưu riêng với các thiết bị mà nó
  cho phép điều khiển nó. Được tin để điều khiển một Zode không nói lên điều gì về việc
  Zode đó có được điều khiển lại hay không.

## Khoá thiết bị trong trình duyệt

Khoá tĩnh của trình duyệt được WebCrypto sinh ra là `X25519` với
`extractable: false` và usage `deriveBits`, và lưu dưới dạng `CryptoKeyPair` trong
IndexedDB. Script trên trang, kể cả dependency bị xâm phạm, có thể yêu cầu
khoá thực hiện Diffie–Hellman nhưng không thể đọc nó ra. Chỉ khoá công khai
được xuất ra.

Điều này ngăn việc rò rỉ khoá, không ngăn việc dùng nó. Script trang chạy trong origin — một
lỗ hổng cross-site scripting, một dependency bị xâm phạm — vẫn có thể yêu cầu khoá
thực hiện Diffie–Hellman chừng nào nó còn chạy, và do đó vẫn có thể tham gia
handshake dưới danh nghĩa thiết bị này. Điều mà tính không thể trích xuất mang lại là khoá không thể
bị chép đi và dùng từ nơi khác về sau. Trình duyệt không có WebCrypto
X25519 bị từ chối bằng một lỗi rõ ràng thay vì quay sang dùng thư viện
giữ khoá dưới dạng byte.

WebCrypto không có ChaCha20-Poly1305, nên kênh dùng AES-GCM.
Máy trạng thái Noise được viết tay trong trình duyệt trên nền
X25519, HMAC-SHA-256, SHA-256 và AES-GCM của WebCrypto — các thư viện Noise
JavaScript có sẵn nhận khoá riêng dưới dạng byte, buộc nó phải
trích xuất được. Các vector bên dưới do bản cài đặt Rust sinh ra (crate
`snow`), và trình duyệt phải tái tạo chúng đúng từng byte.

## Relay thấy gì

| Thấy                                 | Không thấy                                     |
| ------------------------------------ | ---------------------------------------------- |
| Tài khoản và thiết bị nào đã kết nối | Cái gì đang được điều khiển                    |
| `session_id` và độ dài của mỗi frame | Loại hay nội dung của bất kỳ thông điệp nào    |
| Thời điểm frame được gửi             | Bất kỳ file, đường dẫn hay đầu ra terminal nào |

## Tự kiểm chứng

Bạn không cần tin trang này.

- Các vector bên dưới được bộ test `remote_relay_protocol` kiểm khẳng định
  với chính file này. Sửa một chữ số ở đây là test của Zode đỏ.
- Trình duyệt giữ một bản sao các vector đó làm fixture, được test của nó kiểm tra, để hai
  bản cài đặt không thể lệch nhau mà không ai biết. `fixture.sha256`
  là SHA-256 của các dòng vector bên dưới, mỗi dòng dạng `name = value` theo sau là
  một dòng mới, không tính dòng `fixture.sha256` chính nó.

## Vector cố định

Các khoá tĩnh là khoá test X25519 trong RFC 7748 §6.1 (Alice là
initiator, Bob là responder). Các khoá ephemeral được cố định để handshake
tái tạo được; handshake thật không bao giờ dùng lại một khoá.

```text zode-remote-vectors
noise.protocol = Noise_KK_25519_AESGCM_SHA256
noise.user_id = user-0001
noise.initiator_device_id = device-browser
noise.responder_device_id = device-desktop
noise.prologue = 7a6f64652d72656d6f74652f311f757365722d303030311f6465766963652d62726f777365721f6465766963652d6465736b746f70
initiator.static.private = 77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a
initiator.static.public = 8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a
responder.static.private = 5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb
responder.static.public = de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f
initiator.ephemeral.private = 1111111111111111111111111111111111111111111111111111111111111111
responder.ephemeral.private = 2222222222222222222222222222222222222222222222222222222222222222
handshake.message1.payload.utf8 = hello
handshake.message1.ciphertext = 7b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13cf78cfc936ebd851b8cad9ffa43cdace95d33248bb
handshake.message2.payload.utf8 = hello-ack
handshake.message2.ciphertext = 0faa684ed28867b97f4a6a2dee5df8ce974e76b7018e3f22a1c4cf2678570f201e9962d59102376d27a100ee30b5ecc30fadd03cbe21e3d26e
handshake.hash = 8dee97edbbb9d3828233563d5eabdda09fece7521c26cfa4b8a8844fe9baacb5
handshake.empty.message1.ciphertext = 7b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13b35770df5cc53091f6c763a6cc845dc0
handshake.empty.message2.ciphertext = 0faa684ed28867b97f4a6a2dee5df8ce974e76b7018e3f22a1c4cf2678570f208f5b6645787da0523e8d34aa2ff36c4b
handshake.empty.hash = d8845252aa8d7524561873ec9d339b45b2af9d6bfb52144998e8e7a4c66d6dbd
transport.initiator_to_responder.0.frame = 00000000007b2274223a2270696e67227d
transport.initiator_to_responder.0.ciphertext = 8991c042126a331d03219a34c05345111fd4728bc029fe0a083f4c6807ff119465
transport.initiator_to_responder.1.frame = 01000000026c73202d6c610a
transport.initiator_to_responder.1.ciphertext = e9c66436322ee26742bf19bb690f0a4c013890c478e7226771609848
transport.responder_to_initiator.0.frame = 00000000007b2274223a22706f6e67227d
transport.responder_to_initiator.0.ciphertext = 71743ac15c1a08c255e6e973e6b6c3bdaaefbd4107820449ee98e61547ab07c00b
transport.initiator_to_responder.300.frame = 0100000002636f756e7465722d333030
transport.initiator_to_responder.300.ciphertext = d01d12be71c55172d88c626eec8ceef3bce397713801630420d5880d90701a73
frame.relay.session_id = 305419896
frame.relay.bytes = 12345678deadbeef
control.hello.json = {"t":"hello","relay_protocol":1,"app_version":"0.1.5","rpc_protocol":1,"capabilities":["terminal","files"]}
pairing.requester.public = b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1
pairing.acceptor.public = a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1
pairing.requester.nonce = b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2
pairing.acceptor.nonce = a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2
pairing.commitment = 59e69044db06938ded9e05949f501ae9ff1f28c0d538758a448b074363214566
pairing.sas = 318937
fixture.sha256 = 7f2b34c66fe4fe4da701db8b1f74c9965e52bcad30f1edbc7d06ead8ea97484a
```

`handshake.empty.*` là cùng handshake với payload rỗng ở cả hai
hướng: payload rỗng vẫn mang một tag, và bản cài đặt nào
bỏ qua việc seal nó sẽ khác ở đây. `transport.initiator_to_responder.300.*` là
thông điệp ở bộ đếm 300, bộ đếm cần hơn một byte, với
các ping ở bộ đếm 2 tới 299 ở giữa.

Đọc `transport.initiator_to_responder.0.frame`: `00` là `kind = control`,
`00000000` là `stream_id = 0`, và phần còn lại là UTF-8 của `{"t":"ping"}`. Phần
`ciphertext` tương ứng là frame đó được seal với bộ đếm 0 — mười bảy byte
frame cộng tag 16 byte.
