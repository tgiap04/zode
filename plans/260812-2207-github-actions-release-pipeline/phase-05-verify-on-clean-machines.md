# Phase 05 — Kiểm cài trên máy sạch từng OS

**Priority:** P1 · **Status:** ⬜ pending · **Blocked by:** 04
**Context:** [plan.md](plan.md) · [brainstorm 260819](./reports/brainstorm-260819-release-pipeline-public-repo.md)

## Mục tiêu

Chứng minh 6 asset **thật sự cài và chạy được** trên máy chưa từng có Zode, và README đủ để một người lạ làm theo mà không hỏi lại.

Phase này là phase duy nhất không sinh code. Nó tồn tại vì "CI xanh" và "người dùng cài được" là hai việc khác nhau — và đường ngăn cách chúng là binary không ký số.

## Key insight

Ba giới hạn đã biết của bản build này đều **chỉ lộ ra trên máy sạch**, không lộ ra trong CI:

1. **Không ký số** → macOS Gatekeeper chặn thẳng, Windows SmartScreen cảnh báo. Máy dev của bạn đã từng chạy binary tự build nên có thể **không** thấy chặn — đó chính là lý do phải kiểm trên máy sạch.
2. **Nền glibc dâng lên 2.35** (`ubuntu-22.04`) so với 2.31 mà upstream nhắm. Chỉ distro cũ mới lộ.
3. **Không tự cập nhật** → không có thông báo bản mới. Người dùng phải biết trước.

## Related files

**Modify:** `README.md` (nếu bước kiểm cho thấy hướng dẫn thiếu)
**Không sửa code.** Nếu phase này tìm ra lỗi build, nó quay về Phase 03 hoặc 04 — không tự sửa tại đây.

## Implementation steps

1. **macOS aarch64** — máy Mac sạch (hoặc user account mới):
   - Tải `Zode-aarch64.dmg` từ draft release, mount, kéo vào `/Applications`.
   - Mở lần đầu → **kỳ vọng bị Gatekeeper chặn.** Làm đúng theo README (`xattr -d com.apple.quarantine /Applications/Zode.app` hoặc System Settings → Open Anyway).
   - App mở được, mở được một project, gõ vào editor.
   - Kiểm cài **song song** với Zed thật nếu máy có Zed: hai app không giẫm config nhau (`~/.config/zode/` vs `~/.config/zed/`).

2. **macOS x86_64** — nếu không có máy Intel, chạy `Zode-x86_64.dmg` qua Rosetta trên máy arm. Nói rõ trong bảng kết quả rằng đây là kiểm **gián tiếp**, không phải kiểm trên Intel thật.

3. **Linux x86_64** — container hoặc VM sạch, thử **hai** distro để định vị nền glibc:
   - Ubuntu 22.04 (glibc 2.35) → kỳ vọng chạy.
   - Ubuntu 20.04 hoặc Debian 11 (glibc 2.31) → kỳ vọng **fail** với lỗi `GLIBC_2.35 not found`. Xác nhận nền đã dâng, và ghi con số đó vào README.
   - Giải nén `zode-linux-x86_64.tar.gz`, chạy `bin/zed`.

4. **Linux aarch64** — VM arm hoặc máy arm. Cùng các bước.

5. **Windows x86_64** — VM Windows sạch:
   - Chạy `Zode-x86_64.exe` → **kỳ vọng SmartScreen cảnh báo.** Làm theo README (More info → Run anyway).
   - Installer chạy xong, app mở được, mở project được.

6. **Windows aarch64** — VM Windows on ARM nếu có; nếu không, ghi là **chưa kiểm** trong bảng. Đừng đánh dấu xanh cái chưa chạy.

7. **Đối chiếu README với thực tế.** Mỗi bước ở trên mà bạn phải tự đoán ra cách làm là một chỗ README thiếu. Sửa README, không sửa ký ức.

8. **Cập nhật `docs/project-changelog.md`** với mục pipeline release, và `docs/development-roadmap.md` nếu nó có mục phát hành.

## Bảng kết quả (điền khi kiểm)

| Nền tảng | Asset | Cài được | App mở được | Ghi chú |
|---|---|---|---|---|
| macOS aarch64 | `Zode-aarch64.dmg` | | | Gatekeeper chặn đúng như dự kiến? |
| macOS x86_64 | `Zode-x86_64.dmg` | | | kiểm trực tiếp trên Intel hay qua Rosetta? |
| Linux x86_64 (22.04) | `zode-linux-x86_64.tar.gz` | | | |
| Linux x86_64 (20.04) | cùng file | | | kỳ vọng FAIL — xác nhận nền glibc |
| Linux aarch64 | `zode-linux-aarch64.tar.gz` | | | |
| Windows x86_64 | `Zode-x86_64.exe` | | | SmartScreen cảnh báo đúng như dự kiến? |
| Windows aarch64 | `Zode-aarch64.exe` | | | có VM arm không? nếu không → "chưa kiểm" |

## Success criteria

- Bảng trên được điền hết, mỗi ô là kết quả **đã chạy thật** — ô nào không kiểm được thì ghi "chưa kiểm", không ghi xanh.
- Ít nhất 4 trong 6 asset mở được app trên máy sạch.
- Nền glibc thật được xác nhận bằng một lần fail có chủ đích, và con số đó có mặt trong README.
- Một người chưa từng dùng Zode làm theo README mà không cần hỏi thêm.

## Risk assessment

| Rủi ro | Đối phó |
|---|---|
| Không có đủ máy/VM cho 6 nền tảng | Ghi "chưa kiểm" và nói rõ trong README nền tảng nào chưa được xác minh. **Đừng suy ra từ nền tảng khác.** |
| Kiểm trên máy dev thay vì máy sạch → không thấy Gatekeeper chặn | Dùng user account mới, hoặc VM. Máy đã từng chạy binary tự build không phản ánh trải nghiệm người dùng. |
| App mở được nhưng hỏng ở tính năng sâu (LSP, terminal, git) | Ngoài phạm vi phase này. Phase này trả lời "cài và mở được", không phải "mọi tính năng đúng". Nói rõ giới hạn đó. |
| Bỏ `remote_server` làm hỏng gì đó ở đường khởi động chứ không chỉ mất SSH | Đây là chỗ nó lộ ra. Nếu app không mở được → về Phase 02 xem lại vết cắt. |

## Security considerations

- Binary không ký là **rủi ro thật cho người tải**, không phải chi tiết kỹ thuật. README phải nói thẳng rằng app không được ký và người dùng đang tự nhận rủi ro đó, chứ không chỉ hướng dẫn cách bỏ qua cảnh báo.
- Đừng bao giờ hướng dẫn người dùng tắt Gatekeeper hoặc SmartScreen **toàn hệ thống**. Chỉ hướng dẫn mở ngoại lệ cho đúng một file.

## Next steps

Xong phase này là plan hoàn tất. Việc còn mở, ngoài phạm vi plan:
- Ký số macOS/Windows khi có cert — thêm secret là script tự nhận, không phải sửa thiết kế.
- `auto_update` để app tự cập nhật — quyết định riêng, không thuộc plan này.
- Hạ nền glibc bằng cách build trong container (`Dockerfile-distros` đã có sẵn trong repo).
