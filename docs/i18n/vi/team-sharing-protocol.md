# Chia sẻ trong nhóm — giao thức

Trang này mô tả mọi thứ Zode gửi đi khi một nhóm chia sẻ file `.env` và
kết nối cơ sở dữ liệu, đủ chi tiết để viết một client tương thích mà không cần
đọc mã nguồn của Zode. Các vector cố định ở cuối trang được bộ test của chính Zode
kiểm tra (`crates/zode_team_vault/tests/vectors.rs`), nên nếu bản
trang này lệch với code thì build sẽ lỗi.

Server lưu dữ liệu nhóm là mã nguồn đóng, và bạn không cần phải tin
lời ai về việc nó đọc được gì. Nó lưu ba thứ và không mở thứ nào trong
đó: khóa chia sẻ công khai của từng thành viên, một bản team key đã niêm phong cho mỗi
thành viên và mỗi phiên bản khóa, và các mục vault niêm phong dưới team key.

## Khóa

```
recovery key "ZODE-…"  ──►  DEK                         your own, 32 bytes
                              │
                              └─► seals sharing key      X25519, one per user
                                        │
              team key v1, v2, …  ◄─────┘ opened from envelopes (Noise X)
              32 bytes each                  sealed to you by an admin
                    │
                    └─► every vault entry, and the team manifest
```

Thiết kế này **không có mật khẩu và không có bước dẫn xuất khóa ở bất kỳ đâu**, vì
cùng lý do như [đồng bộ môi trường](./env-sync-protocol.md): mọi khóa đều là 32
byte từ bộ sinh số ngẫu nhiên của hệ điều hành.

**Khóa chia sẻ (sharing key)** là một cặp khóa X25519, mỗi người dùng một cặp chứ không phải mỗi
máy. Nửa riêng tư của nó được niêm phong dưới recovery key của bạn (`DEK`) và lưu trong
kho đồng bộ của bạn dưới dạng singleton `sharing-key`, nên mọi máy mà
bạn đã nhập recovery key đều mở được cùng các envelope của nhóm. Nửa công khai
được đăng ký với server.

**Team key** là 32 byte ngẫu nhiên cho mỗi phiên bản khóa. Admin tạo phiên bản 1;
mỗi lần xoay khóa tạo phiên bản kế tiếp. Các phiên bản cũ vẫn được giữ, niêm phong cho các thành viên
còn lại, để các mục chưa niêm phong lại vẫn đọc được. Team key chỉ nằm
trong bộ nhớ và không bao giờ ghi ra đĩa.

**Fingerprint** là 16 byte đầu của `SHA-256(public key)`: gửi dưới dạng 32
chữ số hex viết thường, hiển thị thành tám nhóm bốn ký tự (`2397 429a 87d7 …`). Server
tính cùng giá trị đó; Zode tính lại từ khóa và từ chối
câu trả lời có fingerprint không khớp với khóa của nó.

Khóa công khai được truyền dưới dạng base64url không đệm (43 ký tự).

## Ô chứa khóa chia sẻ

`sync/sharing-key` chứa nửa riêng tư được niêm phong y hệt `env-key`: settings
envelope (`AES-256-GCM`, nonce 96-bit ngẫu nhiên sinh bên trong hàm
niêm phong), AAD

```
user_id ‖ 0x1F ‖ "sharing-key" ‖ 0x1F ‖ v ‖ 0x1F ‖ kid(DEK)
```

và plaintext là khóa riêng tư 32 byte. Giống `env-key`, nó được đóng dấu
bằng khóa **ngoài**, vì đó là envelope giao khóa đi.

Một ô không mở được dưới recovery key trên máy này sẽ không bao giờ tự bị
thay thế. Người dùng có thể thay thế nó một cách tường minh: một cặp khóa mới được
niêm phong đè lên ô (`If-Match` với revision vừa đọc, sau khi kiểm tra thêm lần nữa
rằng nó không mở được), rồi đăng ký bằng `PUT /api/account/sharing-key`
và `If-Match` fingerprint cũ. Server xóa mọi envelope niêm phong
cho khóa cũ, ở mọi nhóm, và admin cấp quyền lại cho thành viên.

Việc xoay recovery key niêm phong lại ô này và `env-key` trước mọi thứ khác,
cả hai hoặc không cái nào, và chỉ nhận recovery key mới khi cả hai đã nằm dưới nó.

## Envelope của team key

Mỗi envelope là một thông điệp Noise một chiều,
`Noise_X_25519_AESGCM_SHA256`, từ admin (initiator, static key = sharing key
của họ) tới thành viên (responder, static key = sharing key của họ). Payload
là team key 32 byte. Thông điệp dài đúng 128 byte:

```
offset   0   e          32   the admin's ephemeral public key
offset  32   s          48   the admin's static public key, sealed (32 + 16-byte tag)
offset  80   payload    48   the team key, sealed (32 + 16-byte tag)
```

Nó được truyền dưới dạng base64 chuẩn trong trường `wrappedKey`.

**Prologue** gắn envelope với người nhận nó:

```
"zode-team-key-v1" ‖ 0x1F ‖ teamId ‖ 0x1F ‖ recipientUserId ‖ 0x1F ‖ keyVersion
```

`keyVersion` là phiên bản ở dạng thập phân ASCII. Một envelope bị phát lại vào nhóm khác,
cho thành viên khác, hoặc với vai trò phiên bản khóa khác sẽ cho ra một
handshake hash khác và không mở được. Phát lại một envelope vào chính ô của nó
chỉ cho ra khóa mà ô đó đã giữ, vô hại.

**Khi mở** một envelope, thành viên kiểm tra theo thứ tự:

1. thông điệp dài đúng 128 byte;
2. ephemeral key không phải điểm bậc thấp (snow không kiểm tra kết quả
   Diffie–Hellman của nó, nên điều này bị từ chối trước khi tính bất kỳ kết quả nào);
3. thông điệp Noise xác thực được dưới prologue ở trên;
4. static key của bên niêm phong, lúc này đã biết, không phải điểm bậc thấp; và
5. fingerprint của nó bằng fingerprint **đã ghim (pinned)** của thành viên mà server
   nói đã niêm phong nó (`wrappedByUserId`), hoặc khóa của chính thành viên đó với các envelope
   do họ tự niêm phong. Bên niêm phong chưa có pin chỉ được chấp nhận trên máy
   chưa từng mở nhóm này (chưa thấy phiên bản khóa nào, chưa ghim gì): khóa
   được liệt kê của nó khi đó được ghim ở trạng thái _seen_, không phải verified. Ở mọi lần mở sau, bên niêm phong
   chưa ghim bị từ chối cho tới khi người dùng xác minh fingerprint của nó.

Trước tất cả những bước đó, câu trả lời của server chỉ được chứa tối đa một envelope cho mỗi phiên bản
khóa; hai envelope cho một phiên bản thì cả hai đều bị từ chối. Envelope trượt bất kỳ kiểm tra nào
đều không được dùng. Kiểm tra 5 là thứ ngăn server niêm phong một khóa do chính nó
chọn cho thành viên: nó có thể tạo envelope xác thực được, nhưng không thể tạo envelope
do một khóa mà máy này đã biết niêm phong, và nó không thể giới thiệu một bên niêm phong mới cho máy
đã từng mở nhóm.

**Khi niêm phong**, admin từ chối khóa người nhận bậc thấp, và chỉ niêm phong cho thành viên
mà khóa đã được người dùng xác minh trên máy này (xem [Pin](#pins)). Envelope lặp lại
fingerprint của người nhận (`recipientFingerprint`) để server có thể từ chối một lần bọc
nhắm tới khóa mà nó không còn giữ cho họ.

## Mục vault

Mỗi mục, và manifest, được niêm phong bằng settings envelope dưới team
key của một phiên bản. AAD:

| Tài nguyên                      | AAD                                                                                                  |
| ------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `teams/<team>/vault/<entry_id>` | `team_id ‖ 0x1F ‖ "team-entry" ‖ 0x1F ‖ v ‖ 0x1F ‖ kid(team key) ‖ 0x1F ‖ team_id ‖ 0x1F ‖ entry_id` |

Trường owner là nhóm chứ không phải người dùng, vì nhiều thành viên ghi cùng
các mục. Nhóm xuất hiện thêm lần nữa bên cạnh entry id để ràng buộc không
phụ thuộc vào trường owner: một mục bị chuyển sang ô của nhóm khác, hoặc ô của mục khác,
sẽ không mở được.

**Khung dữ liệu (framing).** Framing của `.env`, với phiên bản khóa là bốn byte đầu
của payload đã đóng khung:

```
offset  0   seq           u64, little-endian
offset  8   payload_len   u32, little-endian   (4 + entry length)
offset 12   key_version   u32, little-endian
offset 16   entry         the shared bytes
            padding       zero bytes, to the next multiple of 4096
```

Mục lớn nhất là 262140 byte. `seq` và phiên bản khóa nằm bên trong
ciphertext, nơi server không thể sửa chúng. Bên đọc từ chối một mục
có phiên bản khóa bên trong không phải phiên bản của khóa đã mở nó, và một mục
có `seq` thấp hơn giá trị nó đã thấy cho mục đó — server có thể
trả lại một blob cũ, nhưng không thể tạo ra blob mới hơn.

**Manifest** nằm ở entry id dành riêng `00000000000000000000000000000000`
và được niêm phong như mọi mục. Nó liệt kê tên các mục; server chỉ thấy id:

```json
{
  "v": 1,
  "entries": [
    {
      "entryId": "<32 hex>",
      "kind": "env",
      "label": ".env.production",
      "projectHint": "acme-api"
    },
    { "entryId": "<32 hex>", "kind": "db_connection", "label": "staging" }
  ]
}
```

**Một mục kết nối cơ sở dữ liệu** là `{ "name", "driver", "url", "password"? }`.
`password` chỉ có khi người chia sẻ đã tích "Include password".

## Pin

Server phát khóa công khai, và điều duy nhất thiết kế này không thể chặn chỉ bằng
mật mã là việc server **thay** khóa công khai của một thành viên bằng khóa
nó nắm giữ: với vai trò người nhận, để lần cấp quyền kế tiếp được niêm phong cho server; hoặc với vai trò
bên niêm phong, để một team key do nó chọn được mở. Câu trả lời của Zode là các fingerprint mà người dùng đối chiếu, và nó được nói rõ ở đây
chứ không giấu đi. Mỗi máy ghi nhớ, theo từng (nhóm, thành viên), một fingerprint
ở một trong hai trạng thái:

- **verified**: người dùng đã đối chiếu ngoài kênh (out of band) và nhấn Trust Key (hoặc
  Accept New Key, khi có thay đổi). Đây là trạng thái duy nhất mà thứ gì đó được niêm phong cho: cấp quyền
  và xoay khóa đều từ chối thành viên có khóa chưa verified, và cả hai
  đều không tự ghim gì.
- **seen**: được ghi nhớ mà không đối chiếu, chỉ do máy mở nhóm
  lần đầu, cho thành viên đã niêm phong các envelope của nó. Nó mở các
  envelope đó; nó được hiển thị là chưa verified, và không có gì được niêm phong cho nó.

Một fingerprint khác xuất hiện sau đó được báo là **changed**. Không có gì được niêm phong
cho nó và không có gì do nó niêm phong được mở cho tới khi người dùng chấp nhận thay đổi,
từng thành viên một, sau khi đối chiếu fingerprint với chính người đó. Một bản ghi
được viết từ trước khi pin có hai trạng thái sẽ coi mọi pin là seen.

Hãy đối chiếu fingerprint ngoài kênh — trực tiếp, hoặc qua một kênh server
không kiểm soát. Việc thay khóa xảy ra trước lần mở đầu tiên của máy với một nhóm
là rủi ro còn lại; mọi việc thay sau đó đều bị từ chối.

## Phiên bản khóa và xoay khóa

Máy ghi nhớ phiên bản khóa cao nhất nó đã mở cho mỗi nhóm, và
từ chối server nêu phiên bản thấp hơn là hiện hành. Nếu không, server
có thể lặng lẽ đưa nhóm quay về một khóa mà thành viên đã bị gỡ vẫn nắm giữ, và
mục kế tiếp sẽ bị niêm phong dưới khóa đó.

Xoay khóa, do admin chạy sau khi có người rời đi:

1. tạo khóa kế tiếp và niêm phong cho mọi thành viên có sharing key — server
   từ chối lần xoay bỏ sót một người — sau khi kiểm tra rằng từng
   khóa đó đã verified trên máy này; nếu có khóa chưa verified thì dừng xoay
   trước khi gửi bất cứ gì;
2. `POST /teams/:id/key/rotate` với `fromVersion`; server chuyển
   phiên bản bằng compare-and-swap, nên trong hai admin xoay cùng lúc chỉ đúng một
   người thắng, người kia đọc lại các khóa và tiếp tục dưới khóa của người thắng.
   Dù thế nào, client sau đó đọc lại `rotationRecommended` từ server
   thay vì xóa nó: có thể có người đã rời đi sau lần xoay
   vừa hoàn tất;
3. niêm phong lại mọi mục còn dưới phiên bản cũ, mỗi lần ghi có điều kiện theo
   revision vừa đọc, với `seq` tăng thêm một.

Thành viên bị gỡ giữ những gì họ đã giải mã, và các khóa cũ nếu
trình soạn thảo của họ còn giữ trong bộ nhớ. Chỉ xoay khóa và niêm phong lại mới ngăn
các mục mới đọc được bằng những khóa đó.

## Những gì rời khỏi máy bạn

Tất cả chỉ xảy ra khi bạn dùng chia sẻ nhóm — mở tab nhóm, pull,
chia sẻ, cấp quyền hoặc xoay khóa. Không có gì lúc khởi động và không có gì theo hẹn giờ.

| Request                                                              | Gửi                                                                            |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| `GET`/`PUT /api/sync/sharing-key`                                    | Khóa riêng tư chia sẻ của bạn, niêm phong dưới recovery key (chỉ lần đầu dùng) |
| `GET`/`PUT /api/account/sharing-key`                                 | Khóa **công khai** chia sẻ của bạn                                             |
| `GET /api/teams/:id/key`                                             | Không gì cả; nhận các envelope team key đã niêm phong của bạn                  |
| `GET /api/teams/:id/members/keys`                                    | Không gì cả; nhận khóa công khai và fingerprint của các thành viên             |
| `PUT /api/teams/:id/key/envelopes`, `POST /api/teams/:id/key/rotate` | Team key niêm phong cho các thành viên (admin)                                 |
| `GET`/`PUT`/`DELETE /api/teams/:id/vault[/:entryId]`                 | Các mục niêm phong dưới team key                                               |

### Server thấy gì

| Thấy                                                      | Không thấy                                   |
| --------------------------------------------------------- | -------------------------------------------- |
| User id của bạn, team id của bạn                          | Bất kỳ team key hay khóa riêng tư nào        |
| Khóa công khai và fingerprint của các thành viên          | Bất kỳ tên dự án, tên file hay đường dẫn nào |
| Entry id 32 ký tự không có ý nghĩa                        | Bất kỳ tên hay giá trị biến nào              |
| Độ dài mục, làm tròn lên 4 KiB                            | Bất kỳ chuỗi kết nối hay mật khẩu nào        |
| Phiên bản khóa nào niêm phong mỗi mục, và ai ghi, lúc nào | Mục nào là `.env` và mục nào là kết nối      |

## Vector cố định

Mọi khóa dưới đây là byte cố định, chỉ để tái lập kết quả.

```text zode-team-vectors
wrapper.private          = 41 repeated 32 times
wrapper.public           = 7a1a4e709bf085ac494aba0469b9b1eda0ab1f78b16aabb79ffeda90623e8522
wrapper.fingerprint      = 2397429a87d7d9ba0bbcf46f399ec017
wrapper.fingerprint.shown = 2397 429a 87d7 d9ba 0bbc f46f 399e c017
recipient.private        = 42 repeated 32 times
recipient.public         = 132c442be010fbd57e72603328aa76e71fccc1503aae219327d14d9c9993f472
recipient.fingerprint    = 82dab32a9aedffb4925a762f879f6fdd
team.id                  = 68b1f0c2a4d3e5f607182900
recipient.user           = 68b1f0c2a4d3e5f60718290b
key.version              = 1
prologue                 = 7a6f64652d7465616d2d6b65792d76311f3638623166306332613464336535663630373138323930301f3638623166306332613464336535663630373138323930621f31
teamkey.bytes            = 77 repeated 32 times
teamkey.kid              = e29442e61ad354e5
ephemeral.private        = 55 repeated 32 times
wrap.len                 = 128
wrap                     = 38ab664bd86f77d7e66bdd9ae0792913a94fd8b33a1260027e4b46c1f4884c675e539169258c9e71b1cf5a7ac0c30843c2aa259bdafeec00ff12ab45026ebd19cd8801bb288ed98411b597a508d03e6095026ff0050221b41be909589c6620f3c658f444de2ae230a8e64a660fdad3a00d130db6f103e6109419a532546607a4
frame.seq                = 3
frame.key.version        = 1
frame.entry.utf8         = A=1
frame.head               = 03000000000000000700000001000000413d3100
frame.sha256             = 93e50c83513b12ecd19959d16d7abe08e2e9cdb46ee08d70e069e69ef1b89a36
max.entry.bytes          = 262140
```

Đọc `frame.head`: `0300000000000000` là `seq = 3`, `07000000` là
`payload_len = 7` (bốn byte phiên bản khóa và ba byte mục),
`01000000` là phiên bản khóa 1, `413d31` là `A=1`, và `00` theo sau là
byte đệm đầu tiên.

Các khóa công khai và fingerprint ở trên cũng đã được đối chiếu với X25519 của OpenSSL.
Hai blob niêm phong cũng được commit bên cạnh test —
`tests/fixtures/team-entry-v1.b64` (mục `a1a1…a1`, team key `77…`, seq 3,
phiên bản 1, `A=1`) và `tests/fixtures/sharing-key-v1.b64` (khóa
riêng tư của người nhận niêm phong dưới `DEK` là `11…` cho `recipient.user`). Chúng được
tạo một lần bằng code này và phải tiếp tục mở được.
