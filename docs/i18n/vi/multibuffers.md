---
title: Multibuffers - Edit Multiple Files at Once in Zode
description: Edit multiple files simultaneously in Zode using multibuffers. Combine with multi-cursor for fast cross-file refactoring.
---

# Multibuffer

Một trong những điểm mạnh của Zode là sửa nhiều file cùng lúc. Kết hợp với nhiều con trỏ, các đợt refactor trên diện rộng nhanh hơn hẳn.

## Chỉnh sửa trong multibuffer

<div class="video" style="position: relative; padding-top: 71.71314741035857%;">
  <iframe
    src="https://customer-snccc0j9v3kfzkif.cloudflarestream.com/bda0a6584c19f4b39e58a263c0ae4358/iframe?muted=true&preload=true&loop=true&autoplay=true&poster=https%3A%2F%2Fcustomer-snccc0j9v3kfzkif.cloudflarestream.com%2Fbda0a6584c19f4b39e58a263c0ae4358%2Fthumbnails%2Fthumbnail.jpg%3Ftime%3D%26height%3D600&controls=false"
    style="border: none; position: absolute; top: 0; left: 0; height: 100%; width: 100%;"
    allow="accelerometer; gyroscope; autoplay; encrypted-media; picture-in-picture;"
    allowfullscreen="true"
  ></iframe>
</div>

Sửa multibuffer cũng giống sửa một file bình thường. Thay đổi của bạn hiện luôn ở các bản đang mở của file đó trong phần còn lại của trình soạn thảo, và bạn lưu tất cả file bằng `editor: Save` (phím `cmd-s` trên macOS, `ctrl-s` trên Windows/Linux, hoặc `:w` trong chế độ Vim).

Trong multibuffer, dùng nhiều con trỏ để sửa mọi file cùng lúc thường rất tiện. Nếu chỉ muốn sửa vài chỗ, bạn chọn chúng bằng chuột (`option-click` trên macOS, `alt-click` trên Windows/Linux) hoặc bằng bàn phím. `cmd-d` trên macOS, `ctrl-d` trên Windows/Linux, hoặc `gl` trong chế độ Vim sẽ chọn kết quả khớp tiếp theo của từ dưới con trỏ.

Muốn sửa tất cả kết quả khớp, chạy lệnh `editor: Select All Matches` để chọn hết (`cmd-shift-l` trên macOS, `ctrl-shift-l` trên Windows/Linux, hoặc `g a` trong chế độ Vim).

## Đi tới file nguồn

Sửa file ngay trong multibuffer thì dễ, nhưng nhiều khi nhảy thẳng tới file nguồn lại tiện hơn. Bạn bấm vào một đường kẻ phân cách giữa các đoạn trích, hoặc đặt con trỏ trong một đoạn trích rồi chạy lệnh `editor: open excerpts`. Lưu ý: nếu đang dùng nhiều con trỏ, lệnh sẽ mở file nguồn tại vị trí của từng con trỏ trong multibuffer.

Nếu bạn thích dùng chuột và muốn nhấp đúp vào đoạn trích để mở, hãy bật bằng setting: `"double_click_in_multibuffer": "open"`.

## Tìm trong dự án

Để bắt đầu tìm kiếm, chạy lệnh `pane: Toggle Search` (`cmd-shift-f` trên macOS, `ctrl-shift-f` trên Windows/Linux, hoặc `g/` trong chế độ Vim). Khi tìm xong, kết quả hiện trong một multibuffer mới, mỗi dòng khớp trên toàn dự án là một đoạn trích.

## Chẩn đoán

Nếu đã cài language server, khung chẩn đoán cho bạn xem mọi lỗi trong dự án. Mở bằng cách bấm biểu tượng trên thanh trạng thái, hoặc chạy lệnh `diagnostics: Deploy` (`cmd-shift-m` trên macOS, `ctrl-shift-m` trên Windows/Linux, hoặc `:clist` trong chế độ Vim).

## Tìm tham chiếu

Nếu đã cài language server, bạn tìm được mọi tham chiếu tới symbol dưới con trỏ bằng lệnh `editor: Find References` (`cmd-click` trên macOS, `ctrl-click` trên Windows/Linux, hoặc `g A` trong chế độ Vim.

Tùy language server, các lệnh như `editor: Go To Definition` và `editor: Go To Type Definition` cũng mở một multibuffer khi có nhiều định nghĩa khả dĩ.
