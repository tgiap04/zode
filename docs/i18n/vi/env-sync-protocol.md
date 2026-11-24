# Đồng bộ môi trường — giao thức

Trang này nói đầy đủ mọi thứ. Mọi dữ liệu Zode gửi đi khi bạn đồng bộ một file `.env`
đều được mô tả ở đây, đủ chi tiết để viết một client tương thích mà không cần
đọc mã nguồn của Zode — và các vector cố định ở cuối trang được bộ test của
Zode kiểm tra, nên nếu trang này lệch với code thì build sẽ hỏng chứ không
đánh lừa bạn.

Lý do viết ra khá hẹp nhưng đáng nói thẳng: server lưu các file môi trường của bạn
là mã nguồn đóng, và bạn không nên phải
tin lời ai về những gì nó đọc được. Nó đọc được độ dài của một blob
làm tròn lên 4 KiB, và thời điểm bạn ghi blob gần nhất. Chỉ có vậy.

## Khóa

```
recovery key "ZODE-…"  ──►  DEK                      32 bytes, from the OS CSPRNG
                              │
                              └─► wraps envDEK       32 bytes, from the OS CSPRNG
                                        │
                                        ├─► env-manifest
                                        └─► every env entry
```

**Không có mật khẩu và không có bước dẫn xuất khóa nào trong thiết kế này.** Đó là
chủ ý, và là điều quan trọng nhất trên trang này. Server
giữ bản mã của bạn; nếu khóa mở được nó đến từ thứ con người
nhớ được, server có thể thong thả dò ngoại tuyến. Một khóa 256-bit
từ CSPRNG thì không dò được. Cái giá bạn trả là recovery key
không thể khôi phục — xem [Tài khoản Zode của bạn](./account.md).

`envDEK` tách riêng khỏi `DEK` để việc thu hồi quyền truy cập môi trường sau khi
mất laptop không buộc mọi máy khác phải nhập lại recovery key
cho cả settings. Bạn vẫn chỉ phải ghi lại đúng một recovery key, vì
`envDEK` được lưu **bọc bằng `DEK`**, trong slot `env-key` trên
server.

`kid` — dấu vân tay khóa đóng trên mọi envelope — là `SHA-256(key)[..8]`.
Việc duy nhất của nó là phân biệt "sai khóa" với "đã xoay khóa ở nơi khác"; ngoài ra cả hai
đều là một lỗi xác thực giống hệt nhau, và người dùng đọc nhầm một trong
hai thông báo đó sẽ làm sai.

## Envelope

Cùng hình dạng với envelope của settings. JSON, rồi base64, và chuỗi base64
đó chính là toàn bộ trường trong body request mà server lưu.

```json
{
  "v": 1,
  "alg": "AES-256-GCM",
  "kid": "<base64 of 8 bytes>",
  "nonce": "<base64 of 12 bytes>",
  "ct": "<base64 of ciphertext ‖ 16-byte tag>"
}
```

Nonce được sinh bên trong hàm niêm phong và caller không thể cung cấp.
AES-GCM hỏng nghiêm trọng nếu nonce lặp lại dưới cùng một khóa, và
cách chắc nhất để ngăn điều đó là khiến việc yêu cầu nó trở nên bất khả thi.

## Dữ liệu xác thực bổ sung (AAD)

Các trường nối bằng `0x1F` (ký tự phân tách đơn vị ASCII). Dùng `0x1F` thay vì một
ký tự in được vì không user id, nhãn tài nguyên hay định danh nào chứa được nó, nên
không có hai đầu vào khác nhau nào ghép ra cùng một AAD.

| Tài nguyên          | AAD                                                                        |
| ------------------- | -------------------------------------------------------------------------- |
| `env/<entry_id>`    | `user_id ‖ 0x1F ‖ "env" ‖ 0x1F ‖ v ‖ 0x1F ‖ kid(envDEK) ‖ 0x1F ‖ entry_id` |
| `sync/env-manifest` | `user_id ‖ 0x1F ‖ "env-manifest" ‖ 0x1F ‖ v ‖ 0x1F ‖ kid(envDEK)`          |
| `sync/env-key`      | `user_id ‖ 0x1F ‖ "env-key" ‖ 0x1F ‖ v ‖ 0x1F ‖ **kid(DEK)**               |

`kid` là 8 byte thô, không phải dạng base64. `v` là số phiên bản thập phân
dưới dạng ASCII, nên `1` là một byte.

Có hai điểm trong bảng đó đáng dừng lại xem.

**`entry_id` ở cuối.** Không có nó, một server giữ nhiều blob của một
người dùng có thể trả bản mã của entry B từ slot của entry A và client sẽ
giải mã bình thường — tag GCM vẫn xác minh được, vì tag chỉ bao
bản mã. Envelope settings không có trường tương đương vì
chỉ có đúng ba slot settings và nhãn đã phân biệt được
chúng.

**`env-key` được đóng dấu bằng khóa NGOÀI.** Đây là envelope duy nhất trong
hệ thống có định danh là `DEK` chứ không phải `envDEK`, vì nó là envelope
trao `envDEK` đi. Nếu bạn đang viết client tương thích và thấy điều này giống
một sai sót, thì không phải vậy.

## Đóng khung plaintext

Trước khi niêm phong, payload được đóng khung và độn thêm. **Toàn bộ** khối đã đóng khung —
gồm cả header — được niêm phong, và toàn bộ khối đó là bội số của 4096
byte.

```
offset  0   seq           u64, little-endian
offset  8   payload_len   u32, little-endian
offset 12   payload       payload_len bytes
            padding       zero bytes, to the next multiple of 4096
```

`payload_len` tối đa là 262144 (256 KiB).

**Padding.** Một `.env` có bốn biến và một file có bốn mươi biến khác nhau về
kích thước lưu, và theo dõi vài tuần thì sự khác biệt đó là bản ghi dễ đọc về
những gì bạn đang xây. Làm tròn lên 4 KiB loại bỏ điều này. Padding nằm trong
bản mã, nên server lưu không tốn gì và cũng không biết thêm gì.

**`seq`.** Một bộ đếm chỉ tăng, ghi _bên trong_ bản mã. Client
ghi lại `seq` cao nhất đã áp dụng cho từng entry và từ chối
mọi giá trị thấp hơn. Nhờ vậy phát hiện được replay: server có thể trả lại
một blob cũ, nhưng không thể tạo ra blob mới hơn, vì tạo ra một blob
nghĩa là phải mã hóa, mà nó không có khóa. Với settings, rollback chỉ gây
bất tiện. Với file môi trường, đó là khôi phục lại một credential
bạn đã thu hồi.

`seq` **không** nằm trong AAD. Đưa nó vào đó sẽ buộc client tin lời
server về việc nó đang được đưa phiên bản nào trước khi giải mã được
bất cứ thứ gì — và một server nói dối sẽ gây ra một lỗi
xác thực không giải thích được, thay vì thông báo rõ ràng "cái này cũ hơn cái bạn đang có".

## Manifest

Slot `sync/env-manifest` chứa dữ liệu này, được đóng khung và niêm phong y hệt
một entry:

```json
{
  "v": 1,
  "projects": {
    "<project_id>": {
      "name": "acme-api",
      "entries": {
        "<entry_id>": { "path": "services/api/.env.production", "seq": 7 }
      }
    }
  }
}
```

`project_id` và `entry_id` là 128 bit ngẫu nhiên, hex viết thường, 32 ký tự.
**Ngẫu nhiên, không dẫn xuất từ bất cứ thứ gì** — không từ đường dẫn, không từ git remote.
Gắn một checkout vào một dự án là bước thủ công trong Zode, nên không bao giờ cần
dẫn xuất lại, và một định danh không có đầu vào dẫn xuất là một định danh server
không thể suy ngược.

`path` là tương đối so với gốc worktree đã gắn và không bao giờ là tuyệt đối. Đường dẫn
tuyệt đối là trường duy nhất mô tả đĩa của bạn được bố trí ra sao, và
dù sao nó cũng sẽ sai trên máy tiếp theo.

## Server thấy gì

| Thấy                                  | Không thấy                 |
| ------------------------------------- | -------------------------- |
| User id của bạn                       | Tên dự án nào              |
| Định danh hex 32 ký tự mờ đục         | Tên file hay đường dẫn nào |
| Độ dài blob, làm tròn lên 4 KiB       | Tên biến nào               |
| Thời điểm blob được ghi               | Giá trị nào                |
| Chuỗi revision ngẫu nhiên mỗi lần ghi | File chứa bao nhiêu biến   |

## Tự kiểm chứng

Bạn không cần tin trang này.

- **Trước mỗi lần push**, Zode cho bạn xem chính xác các byte nó sắp gửi —
  đúng giá trị request mang theo, dựng một lần rồi gửi, không phải một lần
  dựng thứ hai mà đáng ra phải khớp. Không thể là lần dựng thứ hai: nonce
  mới cho mỗi lần mã hóa, nên mã hóa lại để hiển thị sẽ cho ra thứ
  khác mỗi lần.
- Trỏ một proxy như `mitmproxy` vào trình soạn thảo rồi so sánh. Xem
  [Bảo mật đồng bộ môi trường](./env-sync-security.md) để biết việc đó chứng minh được
  và không chứng minh được điều gì.
- Các vector bên dưới được kiểm tra với file này bởi
  `crates/zode_env_sync/tests/spec_vectors_match_docs.rs`. Sửa một chữ số ở đây
  là bộ test của Zode đỏ.

## Vector cố định

```text zode-env-vectors
envdek.bytes        = 33 repeated 32 times
envdek.kid          = deb0e38ced1e41de
dek.bytes           = 11 repeated 32 times
dek.kid             = 02d449a31fbb267c
pack.seq            = 3
pack.payload.utf8   = A=1
pack.block.len      = 4096
pack.block.head     = 030000000000000003000000413d3100
pack.block.sha256   = 798c5f228598020d9aa13385dca724fd7c3d86dda6b57df0726b7a8a51bc2f16
max.payload.bytes   = 262144
pad.block.bytes     = 4096
```

Đọc `pack.block.head`: `0300000000000000` là `seq = 3` dạng u64 little-endian,
`03000000` là `payload_len = 3`, `413d31` là `A=1`, và `00` ngay sau đó
là byte đầu tiên trong 4081 byte padding.

Hai blob đã niêm phong cũng được commit cạnh test —
`tests/fixtures/env-entry-v1.b64` và `tests/fixtures/env-key-v1.b64`. Chúng
được tạo một lần bằng code này và phải tiếp tục mở được. Một test round-trip
vẫn qua khi cả hai phía của định dạng cùng thay đổi; hai blob này thì không.
