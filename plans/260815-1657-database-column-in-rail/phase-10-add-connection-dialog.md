# Phase 10 — Thêm kết nối bằng hộp thoại, không phải bằng file settings

**Context:** [plan.md](plan.md)
**Priority:** P1 · **Status:** ✅ done (2026-08-15) · **Blocked by:** 09

Phản hồi của người dùng, và họ đúng: bắt sửa JSON bằng tay để thêm một kết nối là bắt người dùng làm
việc của máy. Nút `+` ở header cột, bấm ra hộp thoại.

## Chỗ khó, và nó là chỗ duy nhất khó

`database_ui` **không được biết engine nào tồn tại** — bất biến từ phase 07, có test canh. Nên không
được viết "SQLite thì hỏi đường dẫn file, Postgres thì hỏi host/port/user/database".

Ba đường đã cân:

| Cách | Vì sao không / có |
|---|---|
| Hardcode form theo `driver.id` trong UI | Phá thẳng bất biến. Engine thứ tư từ extension sẽ không có form |
| Thêm method `connection_form` vào protocol | Method mới → driver cũ trả `unsupported`; shape lớn hơn thứ nhận lại |
| ✅ `Capabilities.connection_form` — trường optional | **Đúng tiền lệ `identifier_quote` ở phase 08.** Không phá vỡ, **không tăng version** |

**Driver tự khai các ô của nó.** Zode khởi động driver, đọc `connection_form` từ `initialize`, dựng
form từ đó. Driver không khai gì → hỏi một ô `URL`, đúng thứ mọi driver viết trước khi trường này tồn
tại vẫn nhận được.

## Ba luật đi kèm, và lý do

1. **`secret` không bao giờ vào URL.** Nó đi keychain, khoá bằng chính URL. URL thì được ghi vào file
   settings — file người ta chia sẻ và backup.
2. **`url_encoded` theo từng ô, mặc định tắt.** Đường dẫn file **chính là** template; encode nó thì
   mọi dấu phân cách thành `%2F`. Driver có template là URL thật thì bật cho phần nằm trong authority
   — chỗ mà một `@` trong tên user quyết định host bắt đầu từ đâu.
3. **Template là nguyên khối, không phải ghép.** Chỉ driver biết engine của nó muốn scheme, socket hay
   đường dẫn trần. Một client biết ghép DSN là một client biết engine.

Ô không phải `secret` thì **bắt buộc**. Server không cần mật khẩu thì để trống ô mật khẩu; để trống
thứ khác là một địa chỉ có lỗ. Driver cần ô tuỳ chọn thì cho nó `default` — cái người dùng thấy luôn
là một câu trả lời đầy đủ có thể sửa.

## Đã làm

- [x] `Capabilities.connection_form` + `ConnectionForm` / `ConnectionField` + `build_url()`
- [x] `percent_encode` — cố tình chặt tay: chạy trên tên user sắp nằm trong authority của URL, encode
      thừa một ký tự không tốn gì so với để một `@` quyết định host
- [x] Ba driver tự khai form (SQLite: 1 ô đường dẫn, không encode · Postgres/MySQL: host/port/user/
      database + password, có encode)
- [x] `ConnectionModal` — chọn engine → hỏi driver → điền. Một driver duy nhất thì bỏ luôn bước chọn:
      danh sách một dòng là một cú bấm không quyết định gì
- [x] Tên gợi ý sẵn theo tên engine, sửa được
- [x] Trùng tên bị từ chối — project ghim kết nối **theo tên**, hai dòng cùng tên sẽ ghim của nhau
- [x] Ghi vào **settings**, không phải kho riêng của hộp thoại: kết nối thêm ở đây và kết nối gõ tay
      phải là cùng một thứ, nếu không hai bên trôi và chỉ một bên nằm trong backup của ai đó
- [x] Nút `+` ở header + action `database::AddConnection`; màn hình rỗng đổi từ "sửa trong settings"
      thành nút bấm được
- [x] Nút Settings **giữ nguyên** — DSN lạ vẫn phải gõ tay được

## Test

`database`: 14 → **17** · `database_ui`: 31 → **35**

| Test | Canh điều gì |
|---|---|
| `a_secret_never_reaches_the_url` | Luật 1, ở tầng protocol |
| `a_value_that_would_change_the_url_is_encoded` | `a@b` thành `a%40b`, không lặng lẽ trỏ chỗ khác |
| `a_path_field_is_left_exactly_as_typed` | Lý do encode là per-field |
| `a_driver_that_declares_no_form_is_asked_for_a_url` | Driver bên thứ ba viết trước vẫn thêm được |
| `a_declared_default_arrives_already_typed` | `localhost` do driver nói, không phải UI biết |
| `a_blank_required_field_is_refused_and_a_blank_password_is_not` | Ranh giới "trống là hợp lệ" |
| `a_typed_password_is_kept_out_of_the_url` | Luật 1, ở tầng UI |

`no_source_file_in_this_crate_names_an_engine` **vẫn xanh** với hai file mới — đó là điều kiện nghiệm
thu thật của phase này.

`show_form()` tách khỏi `choose()` để test được mà không phải spawn process: thứ đáng test ở đây là
cái form, không phải cú spawn.

`Filled` **cố tình không có `Debug`** — nó giữ mật khẩu. Test dùng `match` thay `expect_err`.

## Lỗi phase này tự gây ra, và cách nó bị bắt

Bấm "Add a connection" ở màn hình rỗng → **abort tiến trình**:
`cannot update workspace::Workspace while it is already being updated`.

`Workspace::register_action` (`workspace.rs:7677`) bọc callback trong `cx.listener`, nên **workspace
đang bị mượn suốt handler**. Handler tôi viết lại gọi ngược qua handle:

```
database_ui.rs action handler   (workspace đang bị mượn)
  → panel.add_connection()
    → panel_credentials.rs:121  self.workspace.update(…)   ← abort
```

Nút `+` ở header **không** dính, vì nó gọi thẳng qua `cx.listener` của panel — workspace không bị mượn
lúc đó. Hai nút cùng nghĩa, một cái chạy một cái giết app. Đó là hình dạng tệ nhất của lỗi này.

Sửa: **một đường duy nhất**. Action handler mở modal bằng chính `&mut Workspace` nó đang cầm; cả hai
nút dispatch action đó. `DatabasePanel::add_connection` bị xoá — không còn đường ngắn nào để đi nhầm.

Test `dispatching_add_connection_opens_the_dialog_rather_than_aborting` **panic trước khi sửa, xanh
sau khi sửa**, và dispatch cả hai action crate này đăng ký: thứ đang được canh là "không handler nào
gọi ngược qua workspace handle", và đó là tính chất của cả tập, không phải của một entry.

Đây là lần thứ ba cái bẫy này cắn trong plan này — hai lần trước ở `Panel::position` (`ccd151f`) và ở
`bounding_box_for_pane`. Ghi chú tại chỗ đăng ký action nói thẳng luật.

## Đánh đổi đã nhận

- Chọn engine sẽ **khởi động driver một lần** chỉ để hỏi form, rồi giết. Một process cho một câu hỏi
  không miễn phí, nhưng đó là cách duy nhất UI giữ được việc không biết engine nào tồn tại — và là câu
  hỏi hỏi một lần, lúc người ta đang gõ.
- **Chưa sửa/xoá được kết nối từ hộp thoại.** Chỉ thêm. Sửa vẫn qua settings.
- **Chưa có nút "Test connection"** trước khi lưu. Bấm vào kết nối trong cây là biết ngay, nên chưa
  cần đường thứ hai.

`PROTOCOL_VERSION` vẫn là **1**. `PROTOCOL.md` có mục mô tả `connection_form` và ba luật trên.
