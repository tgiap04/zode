---
title: Key Bindings and Shortcuts - Zode
description: Customize Zode's keyboard shortcuts. Rebind actions, create key sequences, and set context-specific bindings.
---

# Phím tắt

Hệ thống phím tắt của Zode tuỳ biến hoàn toàn. Bạn có thể gán lại bất kỳ action nào, tạo chuỗi phím, và định nghĩa phím tắt theo ngữ cảnh.

## Keymap định sẵn

Nếu bạn quen với mặc định của một trình soạn thảo cụ thể, bạn có thể đổi `base_keymap` qua cửa sổ cài đặt ({#kb zode::OpenSettings}) hoặc trực tiếp trong file `settings.json` ({#kb zode::OpenSettingsFile}).
Hiện chúng tôi hỗ trợ:

- VS Code (mặc định)
- Atom
- Emacs (Beta)
- JetBrains
- Sublime Text
- TextMate
- Cursor
- None (tắt _mọi_ phím tắt)

Cài đặt này cũng đổi được qua bảng lệnh (command palette) bằng action `zode: toggle base keymap selector`.

Bạn cũng có thể bật `vim_mode` hoặc `helix_mode`, thêm các phím tắt theo chế độ.
Xem thêm tài liệu về [Vim mode](./vim.md) và [Helix mode](./helix.md).

## Keymap Editor

Bạn mở keymap editor bằng action {#kb zode::OpenKeymap} hoặc chạy action {#action zode::OpenKeymap} từ bảng lệnh. Bạn dễ dàng thêm hoặc đổi phím tắt cho một action bằng nút `Change Keybinding` hoặc `Add Keybinding` ở góc dưới bên trái của bảng lệnh.

Ở đó bạn thấy mọi action hiện có trong Zode cùng các phím tắt mặc định gán cho chúng.

Bạn cũng tuỳ chỉnh được ngay tại đó, bằng cách bấm biểu tượng bút chì hiện ra khi rê chuột lên một action, bấm đúp vào dòng của action, hoặc nhấn phím `enter`.

Mọi thứ bạn làm trong keymap editor cũng được phản ánh vào file `keymap.json`.

## Keymap của người dùng

File keymap nằm ở các vị trí sau cho từng nền tảng:

- macOS/Linux: `~/.config/zode/keymap.json`
- Windows: `~\AppData\Roaming\Zode/keymap.json`

Bạn mở keymap bằng action {#action zode::OpenKeymapFile} từ bảng lệnh.

File này chứa một mảng JSON gồm các object có `"bindings"`.
Nếu không đặt `"context"`, các phím tắt luôn có hiệu lực.
Nếu có đặt, phím tắt chỉ có hiệu lực khi [ngữ cảnh khớp](#contexts).

Trong mỗi nhóm phím tắt, một [chuỗi phím](#keybinding-syntax) được ánh xạ tới [một action](#actions).
Nếu phát hiện xung đột, chúng được giải quyết như [mô tả bên dưới](#precedence).

Nếu bạn dùng bàn phím không phải QWERTY, ký tự không phải Latin, bạn có thể muốn đặt `use_key_equivalents` thành `true`. Xem [Bàn phím không phải QWERTY](#non-qwerty-keyboards) để biết thêm.

Ví dụ:

```json [keymap]
[
  {
    "bindings": {
      "ctrl-right": "editor::SelectLargerSyntaxNode",
      "ctrl-left": "editor::SelectSmallerSyntaxNode"
    }
  },
  {
    "context": "ProjectPanel && not_editing",
    "bindings": {
      "o": "project_panel::Open"
    }
  }
]
```

Bạn xem mọi phím tắt mặc định của Zode cho từng nền tảng trong các file keymap mặc định:

- [macOS](https://github.com/tgiap04/zode/blob/develop/assets/keymaps/default-macos.json)
- [Windows](https://github.com/tgiap04/zode/blob/develop/assets/keymaps/default-windows.json)
- [Linux](https://github.com/tgiap04/zode/blob/develop/assets/keymaps/default-linux.json).

Nếu muốn gỡ lỗi keymap tuỳ chỉnh, bạn dùng `dev: Open Key Context View` từ bảng lệnh.
Hãy tạo [một issue](https://github.com/tgiap04/zode/issues) nếu bạn gặp trường hợp tưởng chạy được mà không chạy.

### Cú pháp phím tắt

Zode khớp được không chỉ một lần nhấn phím mà cả một chuỗi phím gõ lần lượt. Mỗi khoá trong map `"bindings"` là một chuỗi các lần nhấn phím cách nhau bằng dấu cách.

Mỗi lần nhấn phím là một chuỗi phím bổ trợ theo sau là một phím. Các phím bổ trợ là:

- `ctrl-` Phím control
- `cmd-`, `win-` hoặc `super-` cho phím bổ trợ của nền tảng (Command trên macOS, phím Windows trên Windows, và phím Super trên Linux).
- `alt-` cho alt (option trên macOS)
- `shift-` Phím shift
- `fn-` Phím chức năng
- `secondary-` Tương đương `cmd` khi Zode chạy trên macOS và `ctrl` trên Windows và Linux

Phím có thể là bất kỳ codepoint Unicode đơn nào mà bàn phím của bạn tạo ra (ví dụ `a`, `0`, `£` hoặc `ç`), hoặc bất kỳ phím có tên nào (`tab`, `f1`, `shift` hoặc `cmd`). Nếu bạn dùng bố cục không phải Latin (ví dụ Cyrillic), bạn có thể gán cho ký tự Cyrillic hoặc cho ký tự Latin mà phím đó tạo ra khi nhấn `cmd`.

Vài ví dụ:

```json [keymap]
{
  "bindings": {
    "cmd-k cmd-s": "zode::OpenKeymap", // matches ⌘-k then ⌘-s
    "space e": "editor::ShowCompletions", // type space then e
    "ç": "editor::ShowCompletions", // matches ⌥-c
    "shift shift": "file_finder::Toggle" // matches pressing and releasing shift twice
  }
}
```

Phím bổ trợ `shift-` chỉ dùng được kết hợp với một chữ cái để chỉ chữ hoa. Ví dụ `shift-g` khớp với việc gõ `G`. Dù trên nhiều bàn phím shift được dùng để gõ dấu câu như `(`, lần nhấn phím đó không được coi là có phím bổ trợ, nên `shift-(` không khớp.

Phím bổ trợ `alt-` có thể tạo ra phím khác trên nhiều bố cục. Ví dụ, trên bàn phím US của macOS, tổ hợp `alt-c` gõ ra `ç`. Bạn khớp với cái nào cũng được trong file keymap, nhưng theo quy ước, Zode viết tổ hợp này là `alt-c`.

Có thể khớp với việc gõ riêng một phím bổ trợ. Ví dụ, `shift shift` có thể dùng để làm phím tắt 'Search Everywhere' của JetBrains. Trường hợp này, phím tắt kích hoạt khi nhả phím chứ không phải khi nhấn.

### Ngữ cảnh (Contexts)

Nếu một nhóm phím tắt có khoá `"context"`, nó được khớp với các ngữ cảnh đang hoạt động trong Zode.

Các ngữ cảnh của Zode tạo thành một cây, gốc là `Workspace`. Workspace chứa Pane và Panel, Pane chứa Editor, v.v. Cách dễ nhất để xem ngữ cảnh nào đang hoạt động tại một thời điểm là key context view, mở bằng lệnh `dev: open key context view` trong bảng lệnh.

Ví dụ:

```
# in an editor, it might look like this:
Workspace os=macos keyboard_layout=com.apple.keylayout.QWERTY
  Pane
    Editor mode=full extension=md vim_mode=insert

# in the project panel
Workspace os=macos
  Dock
    ProjectPanel not_editing
```

Biểu thức ngữ cảnh có thể chứa các cú pháp sau:

- `X && Y`, `X || Y` để kết hợp hai điều kiện bằng và/hoặc
- `!X` để kiểm tra một điều kiện là sai
- `(X)` để nhóm
- `X > Y` để khớp nếu một tổ tiên trong cây khớp X và tầng này khớp Y.

Ví dụ:

- `"context": "Editor"` - khớp mọi editor (kể cả ô nhập inline)
- `"context": "Editor && mode == full"` - khớp các editor chính dùng để sửa mã
- `"context": "!Editor && !Terminal"` - khớp ở mọi nơi trừ khi Editor hoặc Terminal đang được focus
- `"context": "os == macos > Editor"` - khớp mọi editor trên macOS.

Cần lưu ý rằng thuộc tính chỉ có trên node mà nó được định nghĩa. Nghĩa là nếu bạn muốn (ví dụ) chỉ bật một phím tắt khi trình gỡ lỗi đang dừng ở chế độ vim normal, bạn cần viết `debugger_stopped > vim_mode == normal`.

> Lưu ý: Trước Zode v0.197.x, toán tử `!` chỉ xét một node mỗi lần, và `>` nghĩa là "cha" chứ không phải "tổ tiên". Nghĩa là `!Editor` sẽ khớp ngữ cảnh `Workspace > Pane > Editor`, vì (gây khó hiểu) Pane khớp `!Editor`, và `os == macos > Editor` không khớp ngữ cảnh `Workspace > Pane > Editor` vì có node `Pane` ở giữa.

Nếu bạn dùng Vim mode, chúng tôi có thông tin về cách [các chế độ vim ảnh hưởng tới ngữ cảnh](./vim.md#contexts). Helix mode xây trên Vim mode và dùng cùng các ngữ cảnh.

### Actions

Hầu hết mọi chức năng của Zode đều được phơi ra dưới dạng action.
Dù không có danh sách được ghi tài liệu rõ ràng, bạn tìm được phần lớn chúng bằng cách tìm trong bảng lệnh, xem keymap mặc định cho [macOS](https://github.com/tgiap04/zode/blob/develop/assets/keymaps/default-macos.json), [Windows](https://github.com/tgiap04/zode/blob/develop/assets/keymaps/default-windows.json) hoặc [Linux](https://github.com/tgiap04/zode/blob/develop/assets/keymaps/default-linux.json), hoặc dùng autocomplete của Zode trong file keymap.

Hầu hết action không cần đối số, nên bạn gán được dưới dạng chuỗi: `"ctrl-a": "language_selector::Toggle"`. Một số cần một đối số và phải gán dưới dạng mảng: `"cmd-1": ["workspace::ActivatePane", 0]`. Một số action cần nhiều đối số và được gán dưới dạng mảng gồm một chuỗi và một object: `"ctrl-a": ["pane::DeploySearch", { "replace_enabled": true }]`.

### Thứ tự ưu tiên

Khi nhiều phím tắt có cùng tổ hợp phím và cùng hoạt động một lúc, thứ tự ưu tiên được giải quyết bằng ba quy tắc, theo thứ tự:

1. **Phím tắt bạn tự viết thắng trước.** Phím tắt định nghĩa trong `keymap.json` của riêng bạn được ưu tiên hơn mọi phím tắt đi kèm — keymap mặc định, base keymap (VS Code, Atom, v.v.), hay phím tắt của Vim mode — kể cả khi phím tắt đi kèm khớp ở ngữ cảnh sâu hơn. Ví dụ, nếu keymap của bạn gán `ctrl-w` với ngữ cảnh `Workspace` và keymap mặc định gán `ctrl-w` với ngữ cảnh `Editor`, phím tắt của bạn thắng.
2. **Sau đó là độ sâu ngữ cảnh.** Giữa các phím tắt cùng tầng (hai phím tắt của bạn, hoặc hai phím tắt đi kèm không bị ghi đè), phím nào khớp ở node thấp hơn trong cây ngữ cảnh thì thắng. Phím tắt có ngữ cảnh `Editor` được ưu tiên hơn phím có ngữ cảnh `Workspace`. Phím tắt không có ngữ cảnh khớp ở mức thấp nhất trong cây.
3. **Sau đó là thứ tự định nghĩa.** Nếu các phím tắt vẫn hoà ở cùng tầng và độ sâu, cái định nghĩa sau được ưu tiên.

Có một ngoại lệ với quy tắc 1: nó không bao giờ áp dụng cho phím tắt `null` (xem [Tắt một phím tắt](#disabling-a-binding)). Bất cứ khi nào một phím tắt `null` cạnh tranh một tổ hợp phím, toàn bộ tập phím tắt khớp — `null` và mọi thứ nó cạnh tranh — chỉ được sắp theo quy tắc 2 và 3. Điều này giữ cho một `"key": null` nông trong keymap của bạn không vươn quá ngữ cảnh của nó để làm im một phím tắt đi kèm định nghĩa sâu hơn mà bạn chưa hề đụng tới.

Loại xung đột còn lại xảy ra khi bạn có hai phím tắt, một cái là tiền tố của cái kia. Ví dụ, nếu bạn có `"ctrl-w":"editor::DeleteToNextWordEnd"` và `"ctrl-w left":"editor::DeleteToEndOfLine"`.

Khi đó, và cả hai phím tắt đều hoạt động trong ngữ cảnh hiện tại, Zode sẽ chờ 1 giây sau khi bạn gõ `ctrl-w` để xem bạn có định gõ `left` không. Nếu bạn không gõ gì, hoặc gõ phím khác, `DeleteToNextWordEnd` sẽ được kích hoạt. Nếu bạn gõ, `DeleteToEndOfLine` sẽ được kích hoạt.

Sự chờ này được bỏ qua khi phím tắt từ keymap của chính bạn là cái thắng chuỗi ngắn hơn. Nếu bạn tự gán lại `ctrl-w`, quy tắc 1 ở trên nghĩa là phím tắt của bạn đã vượt mọi `ctrl-w left` đi kèm, nên nó kích hoạt ngay khi bạn gõ `ctrl-w` — Zode không chờ xem một chuỗi dài hơn đi kèm có sắp theo sau không.

### Bàn phím không phải QWERTY

Hỗ trợ bàn phím không phải QWERTY của Zode vẫn đang hoàn thiện.

Nếu bàn phím của bạn gõ được toàn bộ dải ASCII (DVORAK, COLEMAK, v.v.), phím tắt sẽ hoạt động như bạn mong đợi.

Nếu không, đọc tiếp...

#### macOS

Trên bàn phím Cyrillic, Hebrew, Armenian và các bàn phím chủ yếu không phải ASCII, macOS tự ánh xạ phím về dải ASCII khi giữ `cmd`. Zode đi xa hơn một bước: nó luôn khớp được lần nhấn phím với bố cục ASCII hoặc bố cục thật, bất kể phím bổ trợ và setting `use_key_equivalents`. Ví dụ, trong tiếng Thái, nhấn `ctrl-ๆ` sẽ khớp các phím tắt gắn với `ctrl-q` hoặc `ctrl-ๆ`.

Trên bàn phím hỗ trợ bảng chữ Latin mở rộng (AZERTY của Pháp, QWERTZ của Đức, v.v.), thường không gõ được toàn bộ dải ASCII mà không có `option`. Điều này tạo ra sự mơ hồ: `option-2` tạo ra `@`. Để đảm bảo mọi phím tắt dựng sẵn vẫn gõ được trên các bàn phím này, chúng tôi dời các phím tắt đi. Ví dụ, phím tắt gán vào `@` trên QWERTY được dời sang `"` trên bố cục Tây Ban Nha. Ánh xạ này dựa trên mặc định của hệ thống macOS và xem được bằng cách chạy `dev: open key context view` từ bảng lệnh.

Nếu bạn định nghĩa phím tắt trong keymap cá nhân, bạn có thể chọn dùng ánh xạ key equivalent bằng cách đặt `use_key_equivalents` thành `true` trong keymap:

```json [keymap]
[
  {
    "use_key_equivalents": true,
    "bindings": {
      "ctrl->": "editor::Indent" // parsed as ctrl-: when a German QWERTZ keyboard is active
    }
  }
]
```

### Linux

Từ v0.196.0, trên Linux, nếu phím bạn gõ không tạo ra ký tự ASCII, chúng tôi dùng phím tương đương theo bố cục QWERTY cho phím tắt. Nghĩa là nhiều phím tắt gõ được trên nhiều bố cục.

Chúng tôi chưa ánh xạ lại phím tắt để mọi phím tắt dựng sẵn gõ được trên mọi bố cục. Nếu bố cục của bạn không gõ được một số ký tự ASCII, bạn có thể cần phím tắt tuỳ chỉnh. Chúng tôi dự định cải thiện điều này.

## Mẹo và thủ thuật

### Tắt một phím tắt

Nếu bạn muốn một phím tắt không làm gì trong một ngữ cảnh nhất định, bạn có thể dùng
`null` làm action. Cách này hữu ích nếu bạn lỡ nhấn phím tắt và
muốn tắt nó, hoặc muốn gõ ký tự mà chuỗi phím sẽ gõ ra,
hoặc muốn tắt các phím tắt nhiều phím bắt đầu bằng phím đó.

```json [keymap]
[
  {
    "context": "Workspace",
    "bindings": {
      "cmd-r": null // cmd-r will do nothing when the Workspace context is active
    }
  }
]
```

Phím tắt `null` theo cùng quy tắc ưu tiên như action thông thường, nên nó cũng tắt mọi phím tắt khớp ở cao hơn trong cây. Nếu bạn muốn một phím tắt khớp ở cao hơn trong cây được ưu tiên hơn một phím tắt thấp hơn, bạn cần gán lại nó cho action bạn muốn trong ngữ cảnh bạn muốn.

Điều này hữu ích để ngăn Zode rơi về phím tắt mặc định khi action bạn chỉ định có điều kiện và lan truyền. Ví dụ, `buffer_search::DeployReplace` chỉ kích hoạt khi thanh tìm kiếm không hiện. Nếu thanh tìm kiếm đang hiện, nó sẽ lan truyền và kích hoạt action mặc định của phím tắt đó, như mở dock bên phải. Để ngăn điều này:

```json [keymap]
[
  {
    "context": "Workspace",
    "bindings": {
      "cmd-r": null // cmd-r will do nothing when the search bar is in view
    }
  },
  {
    "context": "Workspace",
    "bindings": {
      "cmd-r": "buffer_search::DeployReplace" // cmd-r will deploy replace when the search bar is not in view
    }
  }
]
```

### Gán lại phím

Một yêu cầu phổ biến là ánh xạ từ một lần nhấn phím sang một chuỗi. Bạn làm được bằng action `workspace::SendKeystrokes`.

```json [keymap]
[
  {
    "bindings": {
      // Move down four times
      "alt-down": ["workspace::SendKeystrokes", "down down down down"],
      // Expand the selection (editor::SelectLargerSyntaxNode);
      // copy to the clipboard; and then undo the selection expansion.
      "cmd-alt-c": [
        "workspace::SendKeystrokes",
        "ctrl-shift-right ctrl-shift-right ctrl-shift-right cmd-c ctrl-shift-left ctrl-shift-left ctrl-shift-left"
      ]
    }
  },
  {
    "context": "Editor && vim_mode == insert",
    "bindings": {
      "j k": ["workspace::SendKeystrokes", "escape"]
    }
  }
]
```

Cách này có một số hạn chế, đáng chú ý là:

- Mọi thao tác bất đồng bộ sẽ không xảy ra cho đến khi tất cả phím tắt của bạn đã được gửi đi. Ví dụ, nghĩa là bạn dùng được một phím tắt để mở file (như ví dụ `cmd-alt-r`), nhưng không gửi tiếp được các phím rồi mong view mới diễn giải chúng.
- Các ví dụ khác về thao tác bất đồng bộ: mở bảng lệnh, giao tiếp với language server, đổi ngôn ngữ của buffer, bất cứ thứ gì chạm tới mạng.
- Có giới hạn 100 phím mô phỏng mỗi lần.

Đối số của `SendKeystrokes` là danh sách các lần nhấn phím cách nhau bằng dấu cách (dùng cùng cú pháp như trên). Do cách các lần nhấn phím được phân tích, bất kỳ đoạn nào không nhận ra là một lần nhấn phím sẽ được gửi nguyên văn tới ô nhập đang được focus.

Nếu đối số của `SendKeystrokes` chứa chính phím tắt dùng để kích hoạt nó, nó sẽ dùng định nghĩa có độ ưu tiên cao kế tiếp của phím tắt đó. Nhờ vậy bạn mở rộng được hành vi mặc định của một phím tắt.

### Chuyển phím vào terminal

Nếu bạn dùng Linux hoặc Windows, có thể bạn muốn chuyển các tổ hợp phím vào terminal tích hợp thay vì để Zode xử lý.

Ví dụ, `ctrl-n` tạo tab mới trong Zode trên Linux. Nếu muốn gửi `ctrl-n` tới terminal tích hợp khi nó đang được focus, thêm đoạn sau vào keymap:

```json [keymap]
{
  "context": "Terminal",
  "bindings": {
    "ctrl-n": ["terminal::SendKeystroke", "ctrl-n"]
  }
}
```

### Phím tắt cho Task

Bạn cũng gán phím để chạy các Task của Zode định nghĩa trong `tasks.json`.
Xem [tài liệu về task](tasks.md#custom-keybindings-for-tasks) để biết thêm.
