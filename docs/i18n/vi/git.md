---
description: Zode is a text editor that supports lots of Git features
title: Zode Editor Git integration documentation
---

# Git

Zode có hỗ trợ Git tích hợp sẵn, cho bạn quản lý phiên bản mà không cần rời trình soạn thảo. Bảng Git cho thấy trạng thái cây làm việc, vùng staging và thông tin nhánh. Thay đổi bạn làm trên dòng lệnh được phản ánh ngay trong Zode.

Với những thao tác Zode chưa hỗ trợ sẵn, bạn có thể dùng terminal tích hợp.

## Bảng Git

Bảng Git cho thấy trạng thái cây làm việc và vùng staging của Git.

Bạn mở bảng Git bằng {#action git_panel::ToggleFocus}, hoặc bấm biểu tượng Git trên thanh trạng thái.

Trong bảng, bạn thấy ngay trạng thái dự án: repository và nhánh nào đang hoạt động, file nào đã thay đổi và trạng thái staging hiện tại của từng file.

Zode theo dõi repository của bạn nên thay đổi bạn làm trên dòng lệnh được phản ánh tức thì.

### Cấu hình

Mở Settings Editor (`Cmd+,` trên macOS, `Ctrl+,` trên Linux/Windows) để tuỳ chỉnh hành vi Git. Các cài đặt nằm trên hai trang:

- **Panels > Git Panel**: Vị trí bảng, dạng cây hay danh sách phẳng, kiểu hiển thị trạng thái
- **Version Control**: Chỉ báo ở gutter, inline blame, kiểu hunk

#### Di chuyển bảng Git

Theo mặc định, bảng Git gắn ở bên trái. Vào **Panels > Git Panel** và đổi **Git Panel Dock** để chuyển sang phải hoặc xuống dưới.

#### Chuyển sang dạng cây

Theo mặc định, bảng Git hiển thị danh sách phẳng các file đã đổi. Để xem file theo cấu trúc thư mục, bật **Tree View** trong menu ngữ cảnh của bảng, hoặc bật trong **Panels > Git Panel**.

#### Inline blame

Zode hiển thị thông tin Git blame ở dòng hiện tại. Để tắt hoặc thêm độ trễ trước khi nó hiện, vào **Version Control > Inline Git Blame**.

#### Ẩn chỉ báo ở gutter

Các thanh màu ở gutter cho biết dòng được thêm, sửa và xoá có thể ẩn đi. Vào **Version Control > Git Gutter** và đặt **Visibility** thành "Hide".

#### Độ dài dòng của commit message

Zode ngắt dòng commit message ở 72 ký tự (quy ước của Git). Để đổi, tìm "Git Commit" trong Settings và chỉnh **Preferred Line Length**.

## Repository

Mục **Repositories** của bảng Git, nằm trên **Changes**, liệt kê mỗi repository Git của
dự án trên một dòng. Mục này hiện dù dự án có một hay nhiều repository — ví dụ nhiều
worktree mở cùng nhau. Như các mục khác, bạn có thể thu gọn nó từ phần đầu mục.

- Bấm vào dòng của một repository để đặt nó làm repository đang hoạt động; dòng đang hoạt động được tô sáng.
- Tên nhánh trên mỗi dòng là một nút — bấm vào để mở bộ chọn nhánh cho
  repository của dòng đó, không chỉ repository đang hoạt động.
- Nút fetch/pull/push (xem [Fetch, Push và Pull](#fetch-push-and-pull)) chỉ hiện trên
  dòng đang hoạt động, vì {#action git::Fetch}, {#action git::Push} và {#action git::Pull} luôn
  tác động lên repository đang hoạt động.
- Khi không có repository nào mở, mục này hiện "No Git repositories" và, nếu có thư mục đang mở, một
  nút **Initialize Repository**.
- Mục này hiện tối đa năm repository trước khi phải cuộn.
- Nút tải lại (↻) ở đầu mục quét lại mọi thư mục dự án từ đĩa, nhặt
  các repository Git mới tạo mà trình theo dõi file bỏ sót. Xem
  [Tải lại thư mục dự án](./visual-customization.md#project-panel).

Bạn cũng có thể đổi repository đang hoạt động từ bất cứ đâu trong Zode bằng action {#action git::SelectRepo}
trong bảng lệnh (command palette).

## Biểu đồ commit

Mục **Graph** của bảng Git và tab Git Graph (nút **Open Git Graph** ở đầu mục) hiển thị lịch sử commit dưới dạng biểu đồ. Nút **Auto**/**All** của mục chọn nguồn log: **Auto** theo nhánh đang checkout, **All** hiển thị mọi ref.

Nó cũng chạy trong cửa sổ WSL và SSH: commit được lấy từ máy chủ theo trang và chi tiết commit theo lô. Xem [Phát triển từ xa](./remote-development.md#the-git-graph-in-remote-windows).

Thay cho các dòng, mục này có thể hiện:

| Thông báo                                   | Ý nghĩa                                                                                   |
| ------------------------------------------- | ----------------------------------------------------------------------------------------- |
| "Loading graph…"                            | Các commit vẫn đang được lấy.                                                             |
| "No commits"                                | Log không có commit nào.                                                                  |
| "No branch checked out"                     | **Auto** với HEAD tách rời. Chuyển sang **All** để vẫn nạp biểu đồ.                       |
| "Could not load the graph: _error_"         | Log bị lỗi. Nó không tự thử lại; hãy tải lại bảng, hoặc mở lại tab Git Graph, để thử lại. |
| "Git graph is not supported for collab yet" | Khách trong collab không có biểu đồ.                                                      |

Commit không đọc được chi tiết sẽ hiện "Unavailable". Tên nhánh bắt đầu bằng `-` bị từ chối làm nguồn log.

## Project Diff

Bạn xem được mọi thay đổi Git ghi nhận trong Zode bằng cách mở Project Diff ({#kb git::Diff}), qua action {#action git::Diff} trong bảng lệnh hoặc từ bảng Git.

Mọi thay đổi hiển thị trong Project Diff hoạt động y như bất kỳ multibuffer nào khác: chúng đều là các đoạn file có thể sửa được.

Bạn có thể stage hoặc unstage từng hunk cũng như cả file bằng các nút trên thanh tab hoặc phím tắt tương ứng.

### Tô sáng diff theo từ

Theo mặc định, Zode tô sáng các từ đã đổi trong dòng bị sửa để dễ thấy chính xác cái gì đã thay đổi. Để tắt toàn cục, mở Settings Editor và vào **Languages & Tools > Miscellaneous**, rồi tắt **Word Diff Enabled**.

Để chỉ tắt word diff cho một số ngôn ngữ, thêm đoạn này vào settings.json:

```json
{
  "languages": {
    "Markdown": {
      "word_diff_enabled": false
    }
  }
}
```

### Kiểu hiển thị diff

Zode hiển thị diff theo hai chế độ: **split** (so sánh song song) hoặc **unified** (thay đổi nội tuyến). Mặc định là split.

#### Đổi kiểu hiển thị diff

Mở Settings Editor ({#kb zode::OpenSettings}) và tìm "diff view style". Chọn **Split** hoặc **Unified**.

Để đổi mặc định, thêm đoạn này vào `settings.json`:

```json
{
  "diff_view_style": "unified"
}
```

Xem [Cấu hình Zode](./configuring-zode.md) để biết thêm về Settings Editor.

#### Split và unified

- **Split**: Hiển thị bản gốc và bản đã sửa cạnh nhau. Hữu ích để so sánh cấu trúc file hoặc xem lại thay đổi lớn.
- **Unified**: Hiển thị thay đổi nội tuyến với phần thêm và xoá trong một khung nhìn. Hữu ích để tập trung vào các thay đổi ở từng dòng.

Bạn có thể chuyển giữa các chế độ bất cứ lúc nào. Lựa chọn của bạn áp dụng cho [Project Diff](#project-diff), [File History](#file-history) và [Stash Diff View](#stash-diff-view). Các khung nhìn diff này hoạt động như [multibuffer](./multibuffers.md), cho phép bạn sửa nhiều đoạn cùng lúc.

## File History

File History cho thấy lịch sử commit của một file riêng lẻ. Mỗi mục hiển thị tác giả, thời điểm và thông điệp của commit. Chọn một commit sẽ mở khung nhìn diff chỉ lọc ra các thay đổi của file đó trong commit đó.

Để xem File History:

- Bấm chuột phải vào file trong bảng dự án và chọn "View File History"
- Bấm chuột phải vào file trong bảng Git và chọn "View File History"
- Bấm chuột phải vào tab trình soạn thảo và chọn "View File History"
- Dùng bảng lệnh và tìm "file history"

## Fetch, Push và Pull

Fetch, push hoặc pull repository Git trong Zode bằng các nút trên bảng Git hoặc qua bảng lệnh, tìm các action tương ứng: {#action git::Fetch}, {#action git::Push} và {#action git::Pull}.

### Cấu hình push

Zode tôn trọng cấu hình push của Git. Khi push, Zode kiểm tra theo thứ tự:

1. `pushRemote` được cấu hình cho nhánh hiện tại
2. `remote.pushDefault` trong cấu hình Git của bạn
3. Remote theo dõi của nhánh

Điều này khớp với hành vi chuẩn của Git, nên nếu bạn đã cấu hình `pushRemote` hoặc `pushDefault` trong `.gitconfig` hoặc qua `git config`, Zode sẽ dùng các cài đặt đó.

## Remote

Khi repository có nhiều remote, Zode hiển thị bộ chọn remote trong bảng Git. Bấm nút remote cạnh push/pull để chọn remote dùng cho thao tác đó.

## Quy trình staging

Zode có hai quy trình staging chính, dùng Project Diff hoặc dùng trực tiếp bảng.

### Dùng Project Diff

Trong khung nhìn Project Diff, bạn có thể tập trung vào từng hunk và stage riêng từng cái bằng cách bấm nút trên thanh tab hoặc dùng phím tắt {#action git::StageAndNext} ({#kb git::StageAndNext}).

Tương tự, stage mọi hunk cùng lúc bằng phím tắt {#action git::StageAll} ({#kb git::StageAll}) rồi commit ngay bằng {#action git::Commit} ({#kb git::Commit}).

### Dùng bảng Git

Từ bảng, bạn chỉ cần gõ commit message rồi bấm nút commit, hoặc {#action git::Commit}. Thao tác này tự động stage mọi file đang được theo dõi (hiển thị bằng `[·]` trong ô chọn của mục) và commit chúng.

<!-- Show a set of changes with default staged -->

Có thể stage từng mục bằng ô chọn riêng của mục đó. Có thể stage tất cả bằng biểu tượng stage-all hiện ra khi bạn rê chuột lên đầu mục **Changes**, hoặc {#action git::StageAll}.

<!-- Add media -->

## Commit

Zode có hai ô nhập commit:

1. Ô thứ nhất nằm ở đầu bảng Git, trong mục **Changes**, phía trên danh sách file. Nhấn {#kb git::Commit} sẽ commit ngay mọi thay đổi đã stage.
2. Ô thứ hai mở được bằng action {#action git::ExpandCommitEditor} hoặc nhấn {#kb git::ExpandCommitEditor} khi đang focus ở ô commit của bảng Git.

### Hoàn tác một commit

Ngay khi bạn commit trong Zode, bảng Git hiển thị dòng tiêu đề của commit dưới ô commit. Bấm vào đó để mở chi tiết commit, hoặc rê chuột lên để xem nhanh.

Để hoàn tác commit, mở menu mở rộng (**⋯**) ở đầu mục **Changes** và chọn "Uncommit", lệnh này thực hiện `git reset HEAD^ --soft`.

### Cấu hình độ dài dòng commit

Theo mặc định, Zode đặt độ dài dòng commit là `72` nhưng bạn có thể cấu hình trong file `settings.json` cục bộ.

Xem thêm cách đặt `preferred-line-length` ở mục [Cấu hình](#configuration).

## Quản lý nhánh

### Tạo và chuyển nhánh

Tạo nhánh mới bằng {#action git::Branch} hoặc chuyển sang nhánh có sẵn bằng {#action git::Switch} hoặc {#action git::CheckoutBranch}.

### Xoá nhánh

Để xoá nhánh, mở bộ chuyển nhánh bằng {#action git::Switch}, tìm nhánh muốn xoá và dùng tuỳ chọn xoá. Zode sẽ xác nhận trước khi xoá để tránh mất dữ liệu ngoài ý muốn.

> **Lưu ý:** Bạn không thể xoá nhánh đang checkout. Hãy chuyển sang nhánh khác trước.

## Xung đột merge

Khi gặp xung đột merge sau một lần merge, rebase hoặc pull, Zode tô sáng các vùng xung đột trong file và hiển thị các nút giải quyết phía trên mỗi xung đột.

### Xem xung đột

File có xung đột hiện trong bảng Git kèm biểu tượng cảnh báo. Bạn cũng thấy xung đột trong khung nhìn Project Diff, nơi mỗi vùng xung đột được tô sáng:

- Thay đổi từ nhánh hiện tại của bạn được tô màu xanh lá
- Thay đổi từ nhánh đến được tô màu xanh dương

### Giải quyết xung đột

Mỗi xung đột có ba nút:

- **Use [branch-name]**: Giữ thay đổi từ một nhánh (hiện tên nhánh thật, như "main")
- **Use [other-branch]**: Giữ thay đổi từ nhánh còn lại (như "feature-branch")
- **Use Both**: Giữ cả hai bộ thay đổi, thay đổi của nhánh bạn đứng trước

Bấm một nút để giải quyết xung đột đó. Các dấu xung đột bị xoá và thay bằng nội dung bạn chọn. Sau khi giải quyết hết xung đột trong một file, hãy stage và commit để hoàn tất merge.

> **Mẹo:** Với xung đột phức tạp cần sửa tay, bạn có thể sửa thẳng file. Xoá các dấu xung đột (`<<<<<<<`, `=======`, `>>>>>>>`) và giữ nội dung bạn muốn.

## Stash

Git stash cho phép bạn lưu tạm các thay đổi chưa commit và đưa thư mục làm việc về trạng thái sạch. Điều này đặc biệt hữu ích khi bạn cần chuyển nhánh nhanh hoặc pull bản cập nhật mà không commit phần việc dang dở.

### Tạo stash

Để stash toàn bộ thay đổi hiện tại, dùng action {#action git::StashAll}. Thao tác này lưu cả thay đổi đã stage và chưa stage vào một mục stash mới và làm sạch thư mục làm việc.

### Quản lý stash

Zode có bộ chọn stash, mở bằng {#action git::ViewStash} hoặc từ menu mở rộng của bảng Git. Từ bộ chọn stash, bạn có thể:

- **View stash list**: Duyệt mọi stash đã lưu cùng mô tả và thời điểm
- **Open diffs**: Xem chính xác những thay đổi nào được lưu trong từng stash
- **Apply stashes**: Áp dụng thay đổi của stash vào thư mục làm việc và giữ lại mục stash
- **Pop stashes**: Áp dụng thay đổi của stash và xoá mục stash khỏi danh sách
- **Drop stashes**: Xoá các mục stash không cần mà không áp dụng chúng

### Thao tác stash nhanh

Để làm việc nhanh hơn, Zode có các action trực tiếp cho stash gần nhất:

- **Apply latest stash**: Dùng {#action git::StashApply} để áp dụng stash gần nhất mà không xoá nó
- **Pop latest stash**: Dùng {#action git::StashPop} để áp dụng và xoá stash gần nhất

### Stash Diff View

Để xem nội dung stash, chọn nó trong bộ chọn stash và nhấn {#kb stash_picker::ShowStashItem}. Từ khung nhìn diff, bạn có thể dùng các phím tắt sau:

| Action                            | Phím tắt                     |
| --------------------------------- | ---------------------------- |
| Áp dụng stash                     | {#kb git::ApplyCurrentStash} |
| Pop stash (áp dụng và xoá)        | {#kb git::PopCurrentStash}   |
| Drop stash (xoá mà không áp dụng) | {#kb git::DropCurrentStash}  |

## Tích hợp Git

Zode tích hợp với các dịch vụ lưu trữ Git phổ biến để mã băm commit Git và các tham chiếu tới Issue, Pull Request và Merge Request trở thành liên kết bấm được.

Hiện Zode hỗ trợ liên kết tới bản lưu trữ của
[GitHub](https://github.com),
[GitLab](https://gitlab.com),
[Bitbucket](https://bitbucket.org),
[SourceHut](https://sr.ht) and
[Codeberg](https://codeberg.org).

### Instance tự lưu trữ

Zode tự nhận diện nhà cung cấp lưu trữ Git bằng cách tìm từ khoá trong URL remote Git của bạn. Ví dụ, nếu URL tự lưu trữ của bạn chứa `gitlab`, `gitea` hoặc tên nhà cung cấp được nhận diện khác, Zode tự đăng ký nhà cung cấp đó mà không cần cấu hình gì.

Tuy nhiên, nếu URL instance Git tự lưu trữ không chứa từ khoá nhận diện, bạn có thể cấu hình thủ công để Zode tạo liên kết bấm được tới instance của bạn bằng cách thêm cài đặt `git_hosting_providers`, để mã băm commit và permalink trỏ về tên miền của bạn:

```json [settings]
{
  "git_hosting_providers": [
    {
      "provider": "gitlab",
      "name": "Corp GitLab",
      "base_url": "https://git.example.corp"
    }
  ]
}
```

Trường `provider` chỉ định loại dịch vụ lưu trữ bạn dùng. Các giá trị `provider` được hỗ trợ là `github`, `gitlab`, `bitbucket`, `gitea`, `forgejo` và `sourcehut`. `name` là tuỳ chọn, dùng làm tên hiển thị cho instance của bạn, còn `base_url` là URL gốc của server tự lưu trữ.

Bạn có thể cấu hình nhiều nhà cung cấp tuỳ chỉnh nếu làm việc với nhiều instance tự lưu trữ.

### Permalink

Zode cũng có tính năng Copy Permalink để tạo liên kết cố định tới một đoạn code trên dịch vụ lưu trữ Git của bạn.
Các liên kết này hữu ích để chia sẻ một dòng hoặc một khoảng dòng cụ thể trong file tại một commit cụ thể.
Kích hoạt action này qua [bảng lệnh (command palette)](./getting-started.md#command-palette) (tìm `permalink`),
bằng cách tạo [phím tắt tuỳ chỉnh](key-bindings.md#custom-key-bindings) cho các action
`editor::CopyPermalinkToLine` hoặc `editor::OpenPermalinkToLine`
hoặc đơn giản là bấm chuột phải và chọn `Copy Permalink` khi đã chọn dòng trong trình soạn thảo.

## Phím tắt cho diff hunk

Khi xem file có thay đổi, Zode hiển thị các diff hunk có thể mở rộng hoặc thu gọn để xem lại chi tiết:

- **Expand all diff hunks**: {#action editor::ExpandAllDiffHunks} ({#kb editor::ExpandAllDiffHunks})
- **Collapse all diff hunks**: Nhấn `Escape` (gắn với {#action editor::Cancel})
- **Toggle selected diff hunks**: {#action editor::ToggleSelectedDiffHunks} ({#kb editor::ToggleSelectedDiffHunks})
- **Navigate between hunks**: {#action editor::GoToHunk} và {#action editor::GoToPreviousHunk}

> **Mẹo:** Phím `Escape` là cách nhanh nhất để thu gọn mọi diff hunk đã mở rộng và quay về cái nhìn tổng quát các thay đổi.

## Bảng tham chiếu action

| Action                                    | Phím tắt                              |
| ----------------------------------------- | ------------------------------------- |
| {#action git::Add}                        | {#kb git::Add}                        |
| {#action git::StageAll}                   | {#kb git::StageAll}                   |
| {#action git::UnstageAll}                 | {#kb git::UnstageAll}                 |
| {#action git::ToggleStaged}               | {#kb git::ToggleStaged}               |
| {#action git::StageAndNext}               | {#kb git::StageAndNext}               |
| {#action git::UnstageAndNext}             | {#kb git::UnstageAndNext}             |
| {#action git::Commit}                     | {#kb git::Commit}                     |
| {#action git::ExpandCommitEditor}         | {#kb git::ExpandCommitEditor}         |
| {#action git::Push}                       | {#kb git::Push}                       |
| {#action git::ForcePush}                  | {#kb git::ForcePush}                  |
| {#action git::Pull}                       | {#kb git::Pull}                       |
| {#action git::PullRebase}                 | {#kb git::PullRebase}                 |
| {#action git::Fetch}                      | {#kb git::Fetch}                      |
| {#action git::Diff}                       | {#kb git::Diff}                       |
| {#action git::Restore}                    | {#kb git::Restore}                    |
| {#action git::RestoreFile}                | {#kb git::RestoreFile}                |
| {#action git::Branch}                     | {#kb git::Branch}                     |
| {#action git::Switch}                     | {#kb git::Switch}                     |
| {#action git::CheckoutBranch}             | {#kb git::CheckoutBranch}             |
| {#action git::SelectRepo}                 | {#kb git::SelectRepo}                 |
| {#action git::Blame}                      | {#kb git::Blame}                      |
| {#action git::StashAll}                   | {#kb git::StashAll}                   |
| {#action git::StashPop}                   | {#kb git::StashPop}                   |
| {#action git::StashApply}                 | {#kb git::StashApply}                 |
| {#action git::ViewStash}                  | {#kb git::ViewStash}                  |
| {#action editor::ToggleGitBlameInline}    | {#kb editor::ToggleGitBlameInline}    |
| {#action editor::ExpandAllDiffHunks}      | {#kb editor::ExpandAllDiffHunks}      |
| {#action editor::ToggleSelectedDiffHunks} | {#kb editor::ToggleSelectedDiffHunks} |

> Không phải action nào cũng có phím tắt mặc định, nhưng có thể gán bằng cách [tuỳ chỉnh keymap](./key-bindings.md#user-keymaps).

## Cấu hình Git CLI

Nếu bạn muốn dùng Zode làm [trình soạn thảo commit message](https://git-scm.com/book/en/v2/Customizing-Git-Git-Configuration#_core_editor) khi commit từ dòng lệnh, bạn có thể dùng `zode --wait`:

```sh
git config --global core.editor "zode --wait"
```

Hoặc thêm đoạn sau vào môi trường shell (trong `~/.zshrc`, `~/.bashrc`, v.v.):

```sh
export GIT_EDITOR="zode --wait"
```
