---
title: Agent Usage Indicator
description: What the status bar shows about your Claude Code and Codex subscription quota, the panel and menu it opens, where the numbers come from, when it goes blank, and why Antigravity, Copilot and opencode are absent from it.
---

# Chỉ báo mức dùng agent

Một chỉ báo nhỏ ở bên trái thanh trạng thái, cạnh chỉ báo hoạt động, cho biết bạn đã dùng bao nhiêu hạn mức đăng ký Claude Code và/hoặc Codex:

```
53% used 1h 17m · 10% used 6d 12h · 0% used Fable
```

Mỗi nhóm cách nhau bằng `·` là một cửa sổ hạn mức — gồm một con số phần trăm, và hoặc là đồng hồ đếm ngược tới lúc nó đặt lại, hoặc, với cửa sổ chỉ áp dụng cho một model, là tên model đó.

**Bấm trái** để mở bảng mức dùng. **Bấm phải** để mở menu chọn mục nào của thanh trạng thái được hiện. Cả hai được mô tả bên dưới. Ba setting điều khiển chính chỉ báo này — xem [Cài đặt thanh trạng thái](./visual-customization.md#status-bar).

## Cửa sổ remote

Trong cửa sổ WSL, SSH hoặc dev container, chỉ báo hiện mức dùng của tài khoản đăng nhập **trên host**, vì đó là tài khoản mà các agent của nó tiêu. Nó không bao giờ quay về tài khoản trên máy này. Khi không đọc được tài khoản của host, chỉ báo hiện một dấu mờ, và bảng của nó nói lý do: ví dụ chưa ai đăng nhập trên host đó, thiếu CLI `codex`, hoặc kết nối không khả dụng. Cửa sổ mở trên dự án chia sẻ của cộng tác viên thì giữ tài khoản của máy này.

## Bảng mức dùng

Bấm trái vào chỉ báo sẽ mở một bảng. Bảng đóng ngay khi bạn bấm ra ngoài.

- Một tiêu đề ghi "Usage", một nhãn phạm vi (`all agents`, `Claude only`, `Codex only` hoặc `none on the bar`, tuỳ agent nào đang bật), và một biểu tượng làm mới đọc lại cả hai agent ngay lập tức.
- Một nút chuyển **Detailed** / **Compact**. Nút này ghi setting `agent_usage_display` — chính setting mô tả trong [Cài đặt thanh trạng thái](./visual-customization.md#status-bar) — nên lựa chọn được giữ qua các lần khởi động lại thay vì trở về như cũ khi bảng đóng.
- Một dòng cho mỗi agent: biểu tượng, tên, trạng thái một dòng (đồng hồ đếm ngược tới cửa sổ sắp hết nhất, hoặc lý do không có gì để hiện), và một thanh phần trăm gọn cho mỗi cửa sổ hạn mức.

Bấm mũi tên của một dòng để mở rộng ngay tại chỗ. Dòng mở rộng hiện, cho từng cửa sổ, tên đầy đủ, phần trăm chính xác và thời điểm đặt lại **tuyệt đối** theo múi giờ của bạn — dòng thu gọn hiện đồng hồ đếm ngược, dễ đọc thoáng qua nhưng không đối chiếu được với đồng hồ.

Mở bảng, mở rộng một dòng và chuyển Detailed/Compact đều chỉ đọc trạng thái có sẵn hoặc ghi một setting — không cái nào gửi request mạng.

## Menu bấm phải

Bấm phải vào chỉ báo mở một menu có hai ô tick: **Claude Usage** và **Codex Usage**. Mỗi ô ghi thẳng vào setting `status_bar.*` tương ứng (xem [Cài đặt thanh trạng thái](./visual-customization.md#status-bar)).

Menu này chỉ nói về hai chỉ báo mức dùng — không bao gồm các mục `status_bar.*` khác (tên file đang mở, ngôn ngữ đang dùng, vị trí con trỏ, kiểu xuống dòng, encoding đang dùng, v.v.). Những mục đó chỉnh được từ Settings Editor, file settings, hoặc từ một menu riêng thứ hai: bấm phải vào phần _trống_ của thanh trạng thái (không phải chính chỉ báo) sẽ mở menu gồm mọi mục bật tắt được của thanh trạng thái, nhóm theo bên — xem [All Settings § Status Bar](./reference/all-settings.md#status-bar). Menu đó chỉ có một dòng **Agent Usage** bật tắt Claude Usage và Codex Usage cùng lúc, thay vì lặp lại hai dòng của menu này.

**Tắt cả Claude Usage và Codex Usage sẽ gỡ hẳn chỉ báo khỏi thanh trạng thái** — kéo theo mất luôn đích bấm phải duy nhất của menu này. Không còn là ngõ cụt nữa: bấm phải vào phần trống của thanh trạng thái và bật lại **Agent Usage** ở đó, hoặc đặt lại `true` từ Settings Editor ({#kb zode::OpenSettings}), hoặc sửa `claude_usage_button` / `codex_usage_button` về `true` trong file settings — xem [Settings Files](./configuring-zode.md#settings-files).

## Số liệu đến từ đâu

**Claude** — một request tới `https://api.anthropic.com/api/oauth/usage`, được uỷ quyền bằng OAuth access token mà Claude Code đã giữ: trên macOS, lấy từ keychain (`security find-generic-password -s "Claude Code-credentials"`); còn lại lấy từ `~/.claude/.credentials.json`. Token được đọc mới cho mỗi request — bản build này không bao giờ lưu, ghi log hay ghi nó ra đâu cả.

Đây **không phải API công khai có tài liệu**. Nó là chính endpoint mà CLI của Claude Code dùng, không công bố và không đánh phiên bản. Nó có thể ngừng chạy mà không có gì trong trình soạn thảo này đổi — nếu vậy, chỉ báo đơn giản là ngừng báo số của Claude.

**Codex** — một tiến trình con `codex app-server` sống ngắn, được hỏi qua JSON-RPC (`account/rateLimits/read`, đi sau bước bắt tay `initialize` mà server đòi trước khi trả lời bất cứ gì). Trình soạn thảo này không bao giờ đụng tới credential của Codex; tiến trình con tự giữ và tự làm mới phiên của nó.

Đã kiểm chứng với `codex-cli 0.149.0` và đối chiếu với schema giao thức mà chính CLI phát ra (`codex app-server generate-json-schema`). Schema đó được đánh dấu thử nghiệm và còn thay đổi, nên nếu nó đổi, số của Codex sẽ ngừng hiện và một dòng log nêu tên các khoá JSON nhận về — không bao giờ nêu giá trị. Có một test bị bỏ qua, `reading_a_live_codex_app_server`, để kiểm lại với CLI mới nâng cấp.

Tiền cố ý không được đọc: phản hồi của Claude còn mang `spend` và `extra_usage`, và không có gì trong bản build này đọc chúng.

## Khi nó không hiện gì

Chỉ báo không vẽ biểu tượng và không giữ chỗ cho nguồn không có gì để nói. Lý do, theo từng agent:

**Claude** im lặng khi:

- `ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN` hoặc `ANTHROPIC_API_KEY` được đặt — CLI đang trỏ tới nơi khác ngoài gói đăng ký của bạn, nên hạn mức đăng ký không còn mô tả nó
- không có đăng nhập Claude Code nào trên máy này
- gói đang đăng nhập không báo cửa sổ hạn mức nào cả (ví dụ gói miễn phí)

**Codex** im lặng khi:

- CLI `codex` chưa được cài
- Codex có trả lời, nhưng bản build này không tìm thấy các trường mong đợi trong phản hồi

Mở bảng để biết lý do cụ thể — nó giữ một dòng cho agent im lặng và nói vì sao, là điều duy nhất thanh trạng thái không tự nói được. Request đơn giản là thất bại thì tiếp tục hiện những số lần trước thay vì xoá — chỉ xoá vì các lý do trên, khi số cũ không còn là câu trả lời đúng.

Điều này gồm cả **Claude trả lời `429`**: sau khi hết các lần thử lại mô tả ở [Polling](#polling), chỉ báo giữ những số nó đã có thay vì im lặng, và bảng báo rằng tài khoản đang bị giới hạn tốc độ thay vì một trong các lý do trên. Bị hỏi quá nhiều không nói gì về việc những số đó còn đúng hay không.

## Vì sao Antigravity, Copilot và opencode không có số

Thanh bên đưa ra năm agent; chỉ báo này báo cho hai. Antigravity và
Copilot vắng mặt là kết quả của việc dò CLI của chúng. Với opencode đơn giản hơn —
nó vốn không có cửa sổ đăng ký nào để dò.

**Copilot** không công bố đường nào tới hạn mức đăng ký mà trình soạn thảo này đọc được:

- `copilot --help` không có cờ usage hay quota, và không có lệnh con nào cho việc đó.
- Slash command `/usage` của nó được chính CLI mô tả là "Display session usage
  metrics and statistics" — tức cuộc _hội thoại_ này tiêu bao nhiêu, chứ không phải
  gói đăng ký còn bao nhiêu. Đó là con số khác với con số chỉ báo này hiện, và
  nó chỉ tồn tại bên trong TUI tương tác.
- ACP server của nó (`copilot --acp`) công bố `loadSession`, các khả năng prompt và
  liệt kê phiên, và không có gì về tài khoản. `account/rateLimits/read` — phương thức
  mà đường Codex dùng — trả về `-32601 Method not found`, cũng như
  `account/usage/read`, `usage/read`, `account/read`, `rateLimits/read` và
  `account/subscription/read`.
- Log của chính nó chỉ nêu một host, `api.individual.githubcopilot.com`, và hoàn toàn không
  có từ vựng nào về quota, premium-request, entitlement hay rate-limit.

GitHub có tính premium request, nên rất có thể tồn tại một đường REST. Chưa theo đuổi:
để tới nó, trình soạn thảo này phải giữ một credential GitHub riêng, và đường Codex
được chọn có chủ ý thay cho đúng sự đánh đổi đó với "người anh em" Claude. Nếu
có lúc muốn một đường như vậy ở đây, đó trước hết là quyết định về phạm vi credential, rồi mới là
triển khai.

**Antigravity** không lưu gì trên máy này để đọc. `agy` 1.1.20 được cài, đăng nhập
và chạy hai lần; nó ghi **không byte nào** ở bất kỳ đâu dưới thư mục home hay thư mục
làm việc, vậy mà `agy -c` vẫn nhớ lượt trước. Các cuộc hội thoại của nó — và do đó
mọi thứ server biết về quota — nằm sau `/v1/conversations/{conversation_id}`,
chỉ truy cập được bằng credential mà CLI giữ.

Đó là cùng một bức tường Copilot gặp, vì lý do khác: không phải thiếu phương thức, mà là
thiếu kho cục bộ. Đọc nó nghĩa là trình soạn thảo này phải mượn credential Google, mà đó
chính là sự đánh đổi đường Codex sinh ra để tránh.

**opencode** là tự mang key: người dùng cung cấp credential nhà cung cấp của riêng họ
(Anthropic, OpenAI, hay model nào họ đã cấu hình) trực tiếp cho opencode, nên
không có gói đăng ký opencode và không có cửa sổ đăng ký nào đằng sau để công bố. Đó là một sự
vắng mặt khác với Copilot và Antigravity ở trên — chúng đều có gói đăng ký mà trình soạn thảo này
không tìm được cách đọc; opencode vốn không có gói đăng ký nào để đọc. Bản
build này không giữ hay đọc credential nhà cung cấp của người dùng thay cho opencode, và sẽ
không bao giờ làm vậy.

## Polling

Cả hai agent được đọc cùng nhau mỗi 60 giây, nhưng chỉ khi cửa sổ đang được focus. Lấy lại focus thì đọc lại cả hai ngay — trừ khi lần thử gần nhất cách đó chưa tới 30 giây, khi đó nó tin những gì đang hiện và chỉ khởi động lại khoảng chờ. Bấm biểu tượng làm mới — trên thanh trạng thái hay trong bảng — luôn đọc, bất kể giới hạn tần suất: mục đích của việc bấm là không tin những gì đang hiện. Mở bảng hoặc menu chỉ đọc trạng thái có sẵn và không kích hoạt request thêm.

Agent bị tắt — `claude_usage_button: false` hoặc `codex_usage_button: false` — bị bỏ qua ở mọi nhịp thay vì lấy về rồi giấu đi: không có request HTTP cho Claude, không có tiến trình con `codex app-server` cho Codex. Bật lại (từ một trong hai menu bấm phải, Settings Editor, hoặc file settings sửa tay) thì nó được nhặt lại ở nhịp 60 giây kế tiếp hoặc lần làm mới thủ công kế tiếp.

Mức sàn 30 giây đó tồn tại vì endpoint của Claude dùng chung với CLI Claude Code trên một token, nên trước đây alt-tab ra vào là một request cho mỗi lần chuyển tab tới một endpoint trả `429` khi bị hỏi quá nhiều.

**Khi Claude trả lời `429`**, request được thử lại tối đa ba lần với backoff ngắn, tôn trọng header `Retry-After` nếu có và kẹp nó ở năm giây để một lần thử lại không bao giờ sống lâu hơn lượt poll sẽ thay thế nó. `408`, `425` và mọi `5xx` được thử lại theo cách tương tự. Mọi `4xx` khác thì không: `401` lần sau cũng trả lời y như vậy, và hỏi lại chỉ tốn thêm hạn mức. Nếu hết các lần thử, chỉ báo giữ những số nó đã có và báo là đang bị giới hạn tốc độ — bị hỏi quá nhiều không nói gì về việc những số đó còn đúng hay không.

Xem thêm: [Telemetry](./telemetry.md#agent-subscription-quota) để biết nó nằm đâu trong những gì bản build này gửi qua mạng.
