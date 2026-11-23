---
title: Dịch
description: Bôi đen chữ, bấm phím tắt, đọc bản dịch ngay cạnh đó. Dùng được trong editor, terminal và các view Markdown, với DeepL, Google Cloud Translation, API kiểu OpenAI hoặc endpoint miễn phí của Google.
---

# Dịch

Bôi đen một đoạn chữ, bấm phím tắt của một cặp ngôn ngữ, bản dịch hiện trong một khung nhỏ ngay cạnh vùng chọn. Mỗi cặp ngôn ngữ có phím tắt riêng, ví dụ một phím cho tiếng Việt sang tiếng Anh và một phím cho tiếng Anh sang tiếng Việt. Zode không tự dịch: nó gửi chữ tới dịch vụ bạn chọn.

## Bắt đầu nhanh

1. Mở Settings ({#kb zode::OpenSettings}) rồi vào **Translate**.
2. Chọn **Provider**, bấm **Set API Key…** rồi dán key. Máy chủ chạy trên máy như Ollama, hoặc Google Translate (free, no key), thì không cần key.
3. Bôi đen chữ trong editor rồi bấm Cmd-Shift-1 trên Mac (Ctrl-Shift-1 trên Linux và Windows) để dịch từ tiếng Việt sang tiếng Anh, hoặc Cmd-Shift-2 (Ctrl-Shift-2) để dịch từ tiếng Anh sang tiếng Việt.

## Phím tắt mặc định

| Cặp ngôn ngữ | macOS   | Linux    | Windows        | Trên bàn phím US                     |
| ------------ | ------- | -------- | -------------- | ------------------------------------ |
| Việt → Anh   | `cmd-!` | `ctrl-!` | `ctrl-shift-1` | Cmd (Mac) hoặc Ctrl (khác), Shift, 1 |
| Anh → Việt   | `cmd-@` | `ctrl-@` | `ctrl-shift-2` | Cmd (Mac) hoặc Ctrl (khác), Shift, 2 |

Trên macOS và Linux, keymap ghi ký tự mà Shift-1 và Shift-2 gõ ra trên bố cục US, tức `!` và `@`. Với bố cục mà Shift-1 hay Shift-2 gõ ra ký tự khác, phím mặc định có thể rơi vào phím khác hoặc không chạy. Hãy chọn phím hợp với bàn phím của bạn bằng **Edit Shortcuts…** (xem [Cặp ngôn ngữ và phím tắt](#language-pairs-and-shortcuts)).

Trên Linux, keymap Emacs đã dùng Ctrl-Shift-2 (đặt mark) trong editor, còn keymap Sublime Text dùng Ctrl-Shift-1 và Ctrl-Shift-2 để chuyển tab sang pane khác. Nếu dùng một trong hai keymap đó, hãy gán lại phím bằng **Edit Shortcuts…**.

## Dùng được ở đâu

Cùng một phím tắt chạy ở ba nơi. Mỗi cặp ngôn ngữ là một phím tắt phủ cả ba.

- **Editor và ô nhập**: file đang mở trong editor (kể cả file code) và các ô nhập chữ như thanh tìm kiếm, hộp thoại và cửa sổ Settings. Chữ được gửi là vùng chọn mới nhất.
- **Terminal**: chữ đang bôi đen, hoặc nếu không bôi đen gì thì là chữ chương trình trong terminal vừa copy, ví dụ chữ bôi đen trong chế độ toàn màn hình của Claude Code. Bản copy đó dùng được tới khi bạn gõ hoặc dán, tối đa hai phút. Ngoài ra phím đi thẳng tới chương trình chạy trong terminal, nên `vim` hay `tmux` vẫn nhận được. Chữ copy từ ứng dụng khác không bao giờ bị dịch.
- **View Markdown**: Markdown preview và các panel hiển thị Markdown đã render, như agent panel.

Bấm phím tắt khi không bôi đen gì thì editor và view Markdown không làm gì cả.

## Khung bản dịch

Trong editor, khung hiện ngay dưới cuối vùng chọn; trong terminal và view Markdown, khung hiện gần con trỏ chuột. Dòng đầu ghi tên dịch vụ và chiều dịch, ví dụ _DeepL · Vietnamese → English_. Trong lúc chờ, khung hiện _Translating…_.

- **Copy** chép bản dịch.
- **Swap direction** dịch lại chính đoạn chữ đó theo chiều ngược lại.
- **Replace** ghi bản dịch đè lên chữ gốc (chỉ trong editor; xem bên dưới).

Khung đóng khi bạn bấm Escape, click ra ngoài, đổi vùng chọn hoặc cuộn editor. Bấm phím dịch lần nữa thì khung mới thay khung cũ, và yêu cầu đang chạy dở bị huỷ.

Nếu dịch vụ chưa cài đặt xong, ví dụ thiếu key hoặc key bị từ chối, khung báo rõ và có nút **Open Translate settings**. Các lỗi khác, như hết thời gian chờ hay lỗi mạng, hiện trong khung đúng như dịch vụ báo về. Zode không bao giờ tự chuyển sang dịch vụ khác.

## Thay thế và hoàn tác

**Replace** chỉ hiện khi chữ lấy từ một editor hoặc ô nhập gõ được, kể cả file code. Nút này không hiện với editor chỉ đọc, terminal hay view Markdown.

Bản dịch chỉ được ghi nếu chữ gốc vẫn y nguyên như lúc bạn bấm phím. Nếu bạn đã sửa đoạn đó trong lúc chờ, nút bị khoá và khung báo _The text changed since it was translated._ Thay thế là một lần sửa duy nhất: hoàn tác một lần ({#kb editor::Undo}) là chữ gốc quay lại.

## Dịch vụ và API key

Chọn dịch vụ ở **Settings → Translate → Provider**. Key của mỗi dịch vụ được giữ riêng, nên đổi dịch vụ không làm mất key nào.

| Dịch vụ                         | Key                  | Ghi chú                                                                                            |
| ------------------------------- | -------------------- | -------------------------------------------------------------------------------------------------- |
| DeepL                           | API key của DeepL    | Key miễn phí có đuôi `:fx` và tự được gửi tới endpoint miễn phí của DeepL.                         |
| Google Cloud Translation        | API key Google Cloud | Dùng Cloud Translation API (bản Basic, v2).                                                        |
| OpenAI-compatible API           | Không bắt buộc       | Mọi dịch vụ có endpoint `/chat/completions` kiểu OpenAI: OpenAI, Ollama, LM Studio.                |
| Google Translate (free, no key) | Không có             | Endpoint miễn phí không chính thức. Có thể bị giới hạn hoặc ngừng chạy. Không bao giờ là mặc định. |

### DeepL

Đăng ký DeepL API (gói API, không phải DeepL Translator). Gói API miễn phí cho 500.000 ký tự mỗi tháng. Chép authentication key trong tài khoản DeepL; key miễn phí có đuôi `:fx`.

DeepL thêm tiếng Việt từ năm 2025, lúc đầu chỉ cho gói Pro API. Nếu gói của bạn chưa có, khung báo _DeepL cannot translate from "vi" to "en"._ Khi đó hãy dùng dịch vụ khác cho tiếng Việt.

### Google Cloud Translation

Trong Google Cloud console, tạo hoặc chọn một project, bật **Cloud Translation API**, rồi tạo API key ở **APIs & Services → Credentials**. Nên giới hạn key chỉ dùng cho Cloud Translation API.

### API kiểu OpenAI

Đặt **Base URL** là địa chỉ API tính tới hết phần phiên bản, và **Model** là model sẽ dịch. Model không có giá trị mặc định, nên chưa đặt thì dịch sẽ lỗi.

- **Ollama**: dùng Base URL mặc định `http://localhost:11434/v1`, với một model bạn đã pull, ví dụ `llama3.1`. Không cần key.
- **LM Studio**: bật local server của nó rồi dùng `http://localhost:1234/v1` với model đã nạp. Không cần key.
- **OpenAI** hay dịch vụ online khác: địa chỉ `https://` của họ, như `https://api.openai.com/v1`, một model như `gpt-4o-mini`, kèm API key.

`http://` trơn chỉ được chấp nhận với `localhost` và `127.0.0.1`; mọi địa chỉ khác phải dùng `https://`.

### Google Translate (free, no key) {#google-translate-free-no-key}

Dịch vụ này không cần key và không mất tiền. Nó gửi chữ tới endpoint đứng sau các tiện ích dịch miễn phí trên web của Google (`translate.googleapis.com/translate_a/single`), không phải Cloud Translation API. Endpoint đó không chính thức: Google không công bố tài liệu và không hỗ trợ nó cho các ứng dụng như Zode.

- Google có thể làm chậm hoặc chặn nó bất cứ lúc nào, hay gặp nhất là sau nhiều yêu cầu dồn trong thời gian ngắn. Khi đó khung báo _Google is limiting free requests right now_. Chờ một lúc, hoặc chuyển sang dịch vụ có key.
- Nó có thể ngừng chạy hẳn nếu Google đổi hoặc đóng endpoint. Zode không tự chuyển sang dịch vụ khác.
- Dùng nó phải tuân theo điều khoản dịch vụ của Google. Nếu dùng đều đặn hoặc nhiều, cách được hỗ trợ là Google Cloud Translation với key của chính bạn.

Nó không bao giờ là mặc định: chỉ chọn ở **Provider** khi bạn muốn dùng.

### Lưu key

Bấm **Set API Key…** ở trang Translate, hoặc chạy `translate: set api key` trong bảng lệnh. Hộp thoại áp dụng cho dịch vụ đang được chọn.

- **Save** lưu key vào keychain của hệ thống. Khoảng trắng và dấu xuống dòng quanh key vừa dán được bỏ đi.
- **Remove** xoá key đã lưu.
- **Test** dịch thử một từ ngắn để kiểm tra key và cài đặt có chạy không.

Với Google Translate (free, no key), hộp thoại không có ô nhập key, **Save** hay **Remove**: nó chỉ báo là không cần key và giữ nút **Test**.

Key không bao giờ nằm trong `settings.json`. Đừng ghi key vào đó.

## Cặp ngôn ngữ và phím tắt {#language-pairs-and-shortcuts}

Bấm **Edit Shortcuts…** ở trang Translate, hoặc chạy `translate: edit shortcuts` trong bảng lệnh. Hộp thoại liệt kê mọi cặp ngôn ngữ trong keymap của bạn cùng phím tắt của chúng.

- **Đổi phím tắt**: click vào phím tắt của cặp, bấm tổ hợp phím mới rồi bấm **Save**.
- **Xoá cặp**: bấm nút thùng rác trên dòng của cặp đó.
- **Thêm cặp**: ở mục **Add Language Pair**, chọn hai ngôn ngữ, ghi phím tắt rồi bấm **Add**. Hai ngôn ngữ phải khác nhau.

Nếu tổ hợp phím đã được việc khác dùng, hộp thoại nêu tên việc đó và chưa lưu. Bấm **Save** hoặc **Add** lần nữa nếu vẫn muốn dùng. Thay đổi được ghi vào file keymap, nên cũng hiện trong Keymap Editor. Mỗi cặp được lưu cho editor, terminal và view Markdown cùng lúc, và hộp thoại sửa các mục đó như một.

Bạn cũng có thể tự ghi cặp vào keymap. Action nhận hai mã ngôn ngữ, như `en`, `vi`, `pt-BR` hay `zh-Hans`:

```json [keymap]
[
  {
    "context": "Editor",
    "bindings": {
      "alt-shift-3": [
        "translate::TranslateSelection",
        { "from": "ja", "to": "en" }
      ]
    }
  }
]
```

Binding trong `Editor` chỉ chạy trong editor. Muốn dùng cả trong terminal và view Markdown, thêm đúng binding đó vào context `Terminal` và `Markdown`, hoặc dùng hộp thoại, nó làm việc này giúp bạn.

## Quyền riêng tư

- Chưa bấm phím dịch thì không có gì được gửi đi. Bấm rồi thì chỉ đoạn chữ đang chọn được gửi, và chỉ tới dịch vụ bạn chọn.
- DeepL, Google và các dịch vụ online kiểu OpenAI nhận đoạn chữ đó theo điều khoản của họ. Muốn chữ không rời máy, hãy dùng máy chủ chạy trên máy như Ollama hoặc LM Studio.
- Google Translate (free, no key) gửi đoạn chữ đang chọn tới Google mà không cần tài khoản hay key, theo điều khoản của Google. Đừng dùng nó cho đoạn chữ mà bạn sẽ không tự dán vào Google Translate.
- API key nằm trong keychain của hệ thống, không bao giờ nằm trong `settings.json`.

## Giới hạn

- Mỗi lần một vùng chọn: khi có nhiều con trỏ, chỉ vùng chọn mới nhất được dịch.
- Phím tắt chỉ chạy bên trong Zode; không có phím tắt toàn hệ thống.
- Mỗi phím tắt có chiều dịch cố định. Zode không tự nhận diện ngôn ngữ của đoạn chữ.
- Vùng chọn dài hơn **Max Characters** (mặc định 2.000) không được gửi và cũng không bị cắt; khung hiện cảnh báo thay vào đó.
- Thay thế chỉ có trong editor, không có trong terminal hay view Markdown.

## Cài đặt

| Cài đặt                                | Mặc định                      | Tác dụng                                                           |
| -------------------------------------- | ----------------------------- | ------------------------------------------------------------------ |
| `translate.provider`                   | `"deepl"`                     | `"deepl"`, `"google"`, `"openai_compatible"` hoặc `"google_free"`. |
| `translate.openai_compatible.base_url` | `"http://localhost:11434/v1"` | Địa chỉ API của dịch vụ kiểu OpenAI.                               |
| `translate.openai_compatible.model`    | `""`                          | Model sẽ dịch. Bắt buộc với dịch vụ kiểu OpenAI.                   |
| `translate.max_characters`             | `2000`                        | Độ dài tối đa của vùng chọn được gửi đi.                           |
| `translate.timeout_seconds`            | `20`                          | Thời gian chờ bản dịch trước khi bỏ cuộc, tính bằng giây.          |

API key và phím tắt không phải cài đặt: key nằm trong keychain, phím tắt nằm trong keymap. Xem [Tất cả cài đặt](./reference/all-settings.md#translate) để biết đầy đủ.
