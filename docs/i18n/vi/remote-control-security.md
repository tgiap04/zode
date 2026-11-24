# Remote control — điều gì có thể và không thể xảy ra

Remote control cho một thiết bị khác điều khiển Zode này qua một relay tại
zodekit.com. Relay không phải mã nguồn mở, và nó nằm giữa mọi phiên. Trang này
tồn tại vì lo ngại điều đó là hợp lý. Mọi điều ở đây hoặc bạn tự kiểm được, hoặc
được giữ bằng một test trong các repository nêu tên bên dưới; chỗ nào một
khẳng định yếu hơn vẻ ngoài của nó, trang sẽ nói rõ.

Cách dùng tính năng này nằm ở [Remote control](./remote-control.md). Định dạng
truyền nằm ở [Giao thức remote control](./remote-control-protocol.md).

## Tóm tắt

|                                                         |                                                                                |
| ------------------------------------------------------- | ------------------------------------------------------------------------------ |
| Nó có chạy nếu tôi không bật không?                     | Không. Mặc định tắt; khi tắt, không có kết nối nào được mở.                    |
| Relay có đọc được tôi làm gì trên thiết bị từ xa không? | Không. Nội dung được mã hoá giữa hai thiết bị; relay không có khoá.            |
| Relay có giả mạo được đầu vào gửi tới máy tôi không?    | Không. Mỗi bên chỉ nhận các khoá đã ghim lúc ghép đôi.                         |
| Relay biết được gì?                                     | Ai kết nối tới ai, khi nào, và bao nhiêu byte.                                 |
| Một thiết bị đã ghép đôi có gây hại được không?         | **Có.** Nó có một shell trên máy bạn. Thu hồi thiết bị bị mất ngay lập tức.    |
| Điều gì ngăn người khác ghép đôi mà không có tôi?       | Một mã sáu chữ số bạn tự so, và việc bạn phải xác nhận trên máy bị điều khiển. |

## Mô hình mối đe doạ, nói thẳng

**Giả định là thù địch:** relay, cơ sở dữ liệu và bên vận hành của nó, và bất kỳ ai
trên mạng giữa thiết bị của bạn và nó.

**Giả định là đáng tin:** hai thiết bị của bạn, hệ điều hành của chúng, bản Zode
bạn chạy, và trình duyệt bạn ghép đôi. Nếu một thiết bị đã ghép đôi bị xâm phạm,
không gì ở đây giúp được: nó được phép điều khiển Zode của bạn.

**Ngoài phạm vi:** người đã có máy đang mở khoá của bạn.

## Relay làm được và không làm được gì

Relay là zero-knowledge. Nó chuyển tiếp các frame giữa hai đầu của một
phiên và không phân tích bên trong chúng có gì.

| Nó thấy được                             | Nó không làm được                                                    |
| ---------------------------------------- | -------------------------------------------------------------------- |
| Tài khoản và thiết bị nào đã kết nối     | Đọc output terminal, file, đường dẫn hay phím gõ                     |
| Client đã yêu cầu host nào               | Giả mạo hoặc sửa một message: mọi can thiệp đều đóng phiên vĩnh viễn |
| Độ dài frame và thời điểm gửi frame      | Phát lại một message vào phiên đang chạy: nó bị từ chối              |
| Thiết bị đã bị thu hồi hoặc đang offline | Mở phiên tới thiết bị không giữ khoá đã ghim; bắt tay thất bại       |

Kênh dùng Noise KK: cả hai khoá tĩnh được mỗi bên biết trước khi
bắt tay, vì việc ghép đôi đã ghim chúng. Relay không giữ chúng thì không thể
hoàn tất, và một lần bắt tay làm cho một tài khoản và một cặp thiết bị sẽ không
mở được dưới tài khoản khác.

**Một điều relay thù địch có thể ép.** Thiết bị được báo là đã bị thu hồi
sẽ xoá các lần ghép đôi và khoá của nó, và thứ báo cho nó là relay hoặc dịch vụ
tài khoản. Một relay thù địch có thể báo sai. Zode chỉ tin hai câu trả lời chính xác:
một `403` từ dịch vụ tài khoản có `error` trong JSON là `device_revoked`, hoặc
relay đóng kết nối với mã 4403 và lý do `device_revoked`.
Mọi thứ khác không xoá gì, kể cả nội dung lỗi khác và 4403 với
lý do khác. Cái giá là phải ghép đôi lại: thiết bị còn lại không có thiết bị
tin cậy và không có khoá. Nó không cho ai quyền truy cập.

**Các test giữ điều này.**

| Khẳng định                                                             | Test                                                                                                                                                                                                                                 |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Người lạ không có khoá đã ghim không hoàn tất được bắt tay             | `a_stranger_who_knows_no_pinned_key_cannot_complete_the_handshake` (`crates/remote_relay_protocol`)                                                                                                                                  |
| Message bị sửa sẽ kết thúc phiên                                       | `a_tampered_message_closes_the_session` (`crates/remote_relay_protocol`); `a_tampered_message_closes_the_channel_for_good` (`crates/remote_relay_client`)                                                                            |
| Message bị phát lại sẽ bị từ chối                                      | `a_replayed_message_is_refused` (`crates/remote_relay_protocol`)                                                                                                                                                                     |
| Khoá yếu (bậc thấp) bị từ chối                                         | `a_low_order_key_is_refused_as_a_pinned_key_or_an_ephemeral_key` (`crates/remote_relay_protocol`)                                                                                                                                    |
| Host không làm gì cho tới khi đầu kia chứng minh có khoá               | `a_session_that_never_proves_the_keys_ends_even_when_the_idle_limit_is_off` (`crates/remote_control`)                                                                                                                                |
| Relay chuyển byte nguyên vẹn và chỉ log kích thước, không log nội dung | `delivers 1 MiB of random bytes byte for byte, both ways, and logs sizes only` (`web/backend/test/remote-relay.e2e-spec.ts`); `delivers binary frames byte for byte in both directions` (`web/backend/src/remote/relay-hub.spec.ts`) |
| Message ghép đôi được chuyển nguyên vẹn                                | `forwards pairing messages untouched` (`web/backend/test/remote-relay.e2e-spec.ts`)                                                                                                                                                  |
| Một tài khoản không vươn tới host của tài khoản khác                   | `never lets one account reach another account's host` (`web/backend/test/remote-relay.e2e-spec.ts`)                                                                                                                                  |
| Chỉ đúng câu trả lời thu hồi mới xoá một thiết bị                      | `only_the_exact_code_and_reason_say_the_device_was_revoked`, `only_a_body_whose_error_is_device_revoked_means_revoked` (`crates/remote_relay_client`)                                                                                |

## Nếu có sự cố

| Tình huống                       | Điều xảy ra                                                                                                                                                                                                   |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Relay độc hại                    | Nó chỉ thấy metadata. Nó có thể từ chối dịch vụ, bỏ hoặc làm trễ frame, hay đóng phiên. Nó không đọc hay giả mạo được nội dung, và không ghép đôi được thiết bị mới nếu không có người xác nhận trên máy bạn. |
| Relay bị man-in-the-middle       | Kẻ tấn công ở vị trí của relay, và nhận được đúng cái không có gì như vậy. Khoá đã được ghim lúc ghép đôi; bắt tay không có chúng sẽ thất bại.                                                                |
| Thiết bị đã ghép đôi bị đánh cắp | Kẻ trộm dùng được nó như bạn. Hãy thu hồi nó ([bên dưới](#if-a-device-is-lost)). Cho tới khi bạn làm vậy, hãy coi như có một shell trên máy bạn.                                                              |
| Tài khoản của bạn bị xâm phạm    | Kẻ tấn công liệt kê được thiết bị của bạn và mở được relay, nhưng không điều khiển được Zode nếu không ghép đôi, và ghép đôi cần bạn xác nhận trên Zode đó.                                                   |

Relay hoặc dịch vụ tài khoản độc hại cũng có thể báo cho một thiết bị rằng nó đã
bị thu hồi, khiến thiết bị xoá các lần ghép đôi và khoá. Xem
[phần trên](#what-the-relay-can-and-cannot-do) về những câu trả lời duy nhất được tính.

Zode điều khiển cũng kiểm tra khoá mà host đưa ra có khớp với khoá
tài khoản của bạn liệt kê cho nó không, và từ chối ghép đôi nếu không khớp.

## Bảo mật khi ghép đôi

Ghép đôi là lúc một thiết bị mới trở nên đáng tin, nên nó được xây để chống
kẻ tấn công muốn được ghép đôi.

- **Cam kết trước, tiết lộ sau.** Bên yêu cầu cam kết một nonce trước khi nonce của
  bên chấp nhận được biết. Một máy ở giữa không thể dò cặp khoá cho tới khi
  sáu chữ số khớp, vì các chữ số được cố định trước khi nó chọn được. Chi tiết nằm
  ở [Ghép đôi](./remote-control-protocol.md#pairing).
- **Sáu chữ số là ít.** Việc so sánh là ranh giới bảo mật. Nếu bạn
  xác nhận trên A mà không nhìn cả hai màn hình, bạn ghép đôi với bất kỳ ai đã yêu cầu. Đừng bao giờ
  xác nhận một lời nhắc bạn không khởi tạo.
- **Không có gì được ghim cho tới khi bạn nói các chữ số khớp**, và host từ chối
  thì không ghim gì.
- **A ghim khi bạn xác nhận trên A.** Việc nói các chữ số khác nhau trên thiết bị
  kia sau đó không hoàn tác được: relay có thể giữ message đó lại,
  nên A không chờ nó. Nếu bạn xác nhận nhầm trên A, hãy xoá thiết bị
  trong **Manage Trusted Devices…**.
- **Khoá tạm thời.** A nhận một lần ghép đôi mỗi lúc và một yêu cầu hết hạn sau 120
  giây. Nó hỏi bạn tối đa ba lần mỗi giờ, và hạn mức tính các
  câu hỏi thực sự đã hiện cho bạn, không tính yêu cầu. Yêu cầu bị bỏ
  trước khi bạn được hỏi không giữ chỗ ghép đôi và không tốn lần nào trong ba lần. Ba
  lần thử thất bại sẽ khoá ghép đôi, và khoá không hết hạn: bạn mở khoá
  rõ ràng trong **Manage Trusted Devices…**, bằng **Unlock Pairing**.
- **Yêu cầu đã trả lời cũng bị giới hạn.** Bỏ một yêu cầu không tốn gì của bạn,
  nhưng trả lời nó cho bên yêu cầu biết các chữ số trước khi tiết lộ, nên một
  relay có thể thử mã không giới hạn bằng cách đóng mọi lần trao đổi mà
  các chữ số nó không ưng. Vì vậy Zode cũng đếm các yêu cầu nó đã trả lời,
  dù sau đó bạn có được hỏi hay không: tối đa ba mỗi giờ cho một
  thiết bị yêu cầu và mười hai mỗi giờ cho tất cả. Relay có thể bịa
  ID thiết bị, nên giới hạn tổng mới là cái chặn việc đoán mã; giới hạn
  theo thiết bị dành cho thiết bị chỉ đơn giản là bị hỏng. Yêu cầu vượt một trong hai
  giới hạn sẽ bị từ chối, và thông báo nói khoảng bao lâu nữa nên chờ.
- **Tắt rồi bật lại remote control không đặt lại gì.** Khoá tạm thời và các
  số đếm theo giờ được giữ cho tài khoản đang đăng nhập khi remote control tắt;
  chỉ đăng nhập bằng tài khoản khác mới bắt đầu lại từ đầu.
- **Điều gì tính là một lần thử thất bại.** Một lần tiết lộ không khớp với
  cam kết của nó, và **Reject**. Một câu hỏi biến mất không có câu trả lời, như
  khi một hộp thoại khác thay thế nó, làm thiết bị bị từ chối nhưng không phải một lần thử
  thất bại: các chữ số không ai so thì không thể không khớp.
- **Bạn được báo khi một yêu cầu bị từ chối.** Zode hiện thông báo khi nó
  từ chối một yêu cầu vì hạn mức theo giờ đã dùng hết, vì có quá nhiều
  yêu cầu được trả lời trong giờ này (thông báo nói có thể ai đó đang cố
  đoán mã ghép đôi), vì ghép đôi đang bị khoá, hoặc vì danh sách
  thiết bị tin cậy đã đầy (32 thiết bị) hoặc không đọc được. Lần kiểm cuối đó diễn ra trước khi bạn thấy bất kỳ chữ số nào, nên bạn không bao giờ
  so các chữ số cho một thiết bị không lưu được.
- **Ghép đôi lại một thiết bị sẽ thay thế nó.** Nếu thiết bị đã được tin cậy,
  lời nhắc nói vậy và nói rằng tin cậy nó sẽ thay thế những gì đã lưu cho nó. Khi
  khoá là mới, việc xác nhận cũng kết thúc mọi phiên thiết bị đó có dưới
  khoá cũ.
- **Tên được làm sạch.** Tên thiết bị đến từ dịch vụ tài khoản, nơi
  mọi thiết bị trên tài khoản của bạn đều ghi được. Trước khi Zode hiện một tên, nó bỏ
  các ký tự điều khiển và ký tự đảo thứ tự chữ, và giữ tối đa 64
  ký tự. Tên không còn gì sẽ thành một tên chung chung.

| Khẳng định                                                    | Test                                                                                                                                                                                  |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Lần tiết lộ không khớp cam kết bị từ chối                     | `a_reveal_that_does_not_match_the_commitment_is_refused` (`crates/remote_relay_protocol`)                                                                                             |
| Khoá bên yêu cầu bị tráo sẽ trượt cam kết                     | `a_swapped_requester_key_fails_the_commitment` (`crates/remote_relay_protocol`)                                                                                                       |
| Thất bại lặp lại khoá ghép đôi cho tới khi người dùng đặt lại | `repeated_failures_lock_pairing_until_the_user_resets_it` (`crates/remote_relay_protocol`)                                                                                            |
| Ba lần không khớp khoá ghép đôi trên host                     | `a_mismatched_commitment_ends_pairing_and_three_of_them_lock_it` (`crates/remote_control`)                                                                                            |
| Khoá tạm thời không hết hạn; chỉ mở khoá mới kết thúc nó      | `a_lockout_does_not_expire_and_only_unlock_ends_it` (`crates/remote_control`)                                                                                                         |
| Khoá tạm thời còn nguyên khi kết nối rớt                      | `a_lockout_survives_the_connection_dropping_and_coming_back` (`crates/remote_control`)                                                                                                |
| Không có gì được ghim và không phiên nào mở khi chưa xác nhận | `nothing_is_pinned_and_no_session_opened_without_confirmation` (`crates/remote_relay_client`)                                                                                         |
| Man in the middle hoàn tất được bắt tay vẫn không được ghim   | `a_machine_in_the_middle_that_could_finish_the_handshake_is_still_not_pinned` (`crates/remote_relay_client`)                                                                          |
| Yêu cầu bị bỏ trước khi bạn được hỏi không tốn gì             | `a_request_abandoned_before_anyone_was_asked_neither_holds_pairing_nor_spends_the_allowance` (`crates/remote_control`)                                                                |
| Câu hỏi bị đóng không trả lời không phải một lần thử thất bại | `closing_the_question_unanswered_is_not_a_failed_guess` (`crates/remote_control`); `a_question_replaced_by_another_modal_does_not_count_toward_the_lockout` (`crates/remote_control`) |
| Bạn được báo khi hạn mức đã hết                               | `once_the_allowance_is_spent_the_person_is_told_why_nothing_was_asked` (`crates/remote_control`)                                                                                      |
| Kho tin cậy đầy được báo trước khi bạn so các chữ số          | `a_full_trust_store_is_said_before_the_person_compares_digits` (`crates/remote_control`)                                                                                              |
| Ghép đôi lại với khoá mới kết thúc các phiên tạo bằng khoá cũ | `re_pairing_a_device_with_a_new_key_ends_the_sessions_made_with_the_old_one` (`crates/remote_control`)                                                                                |
| Tên thiết bị đến nơi đã sạch và ngắn                          | `names_from_the_directory_arrive_clean_and_short` (`crates/remote_relay_client`)                                                                                                      |

## Thiết bị đã ghép đôi có một shell trên A

Hãy hiểu rõ bạn đang cấp gì. Một controller đã ghép đôi gõ được vào các agent
và terminal của bạn, nên nó chạy được mọi lệnh bạn chạy được. Một Zode điều khiển
Zode khác cũng mở được các dự án của nó, và khi đó host chạy một project server dưới
tư cách người dùng của bạn: bạn làm được gì trên máy đó thì nó làm được. Cả giới hạn
viewer lẫn `private_files` đều không đổi điều này: chúng hạn chế những gì trình duyệt file web hiện,
không phải những gì một shell đọc được.

Trình duyệt đã ghép đôi cũng resume và fork được các phiên agent trước đây của bạn. Nó có thể liệt kê
các phiên của những thư mục đang mở trên Zode này, kèm tiêu đề và phần đầu
mỗi cuộc hội thoại, và khởi động lại một phiên. Việc đó khởi chạy một agent khác trên máy
này, trong thư mục mà phiên đã chạy, dưới thiết lập quyền mà máy
này có cho nó. Thiết bị không đổi được thiết lập đó, nên nếu bạn chọn **Run
Agents Without Permission Prompts** cho checkout mà một phiên đã chạy trong đó, phiên
mà thiết bị khởi động ở đó cũng chạy không hỏi quyền. Thiết bị vốn đã có shell,
nên việc này không mở rộng những gì nó làm được, nhưng nó có nghĩa là lịch sử phiên của bạn
hiện ra với nó.

Phạm vi bị giới hạn ở các dự án đang mở. Các phiên ngoài các thư mục đang mở trên
Zode này, và các phiên dưới đường dẫn `private_files` hoặc đường dẫn bị ẩn bởi
`file_scan_exclusions`, không được liệt kê và không khởi động được. Thiết bị gọi tên một
phiên chỉ bằng id của nó; host tự tra mọi thứ còn lại. Thiết bị chỉ được khởi động
một phiên mỗi lúc và không quá một lần mỗi 3 giây, và mỗi lần
một toast trên màn hình này nêu tên thiết bị và phiên.

| Khẳng định                                                                              | Test                                                                                                          |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| Phiên dưới đường dẫn riêng tư hoặc bị loại không được liệt kê cũng không được khởi động | `a_session_under_a_private_or_excluded_path_is_neither_listed_nor_opened` (`crates/remote_control`)           |
| Phiên ngoài các thư mục đang mở, hoặc không biết, không khởi động gì                    | `a_session_the_host_does_not_hold_or_may_not_open_is_not_found_and_opens_nothing` (`crates/remote_control`)   |
| Tên không thể là id phiên bị từ chối trước mọi lần tra                                  | `a_resume_naming_something_that_cannot_be_an_id_is_not_found_and_the_session_lives` (`crates/remote_control`) |
| Mỗi lúc chỉ mở một phiên cho một thiết bị, dù có nhiều kết nối                          | `one_device_has_one_session_opened_at_a_time_however_many_connections_it_has` (`crates/remote_control`)       |
| Phiên vừa mở chặn phiên kế tiếp cho tới khi hết thời gian chờ                           | `a_session_opened_a_moment_ago_cannot_be_opened_again_until_the_cooldown_passes` (`crates/remote_control`)    |

### Nếu mất thiết bị

1. Trên A, mở **Manage Trusted Devices…** (menu tài khoản, chỉ báo thanh trạng thái,
   hoặc action `remote_control::ManageDevices`) và chọn **Forget** cho thiết bị
   đó. Việc này kết thúc phiên của nó ngay; nó không quay lại được nếu không ghép đôi lại.
   Nó chạy cả khi remote control đang tắt, nên bạn quên được một thiết bị trước khi
   bật lại remote control.
2. Với trình duyệt, đăng xuất nó thêm ở trang thiết bị trên web,
   `zodekit.com/account/devices`.
3. Nếu còn phân vân, dùng **Disconnect All Devices** hoặc **Turn Off Remote Control**.

Thu hồi cũng xoá thông tin xác thực trên chính thiết bị bị thu hồi: thiết bị
được báo đã bị thu hồi sẽ xoá khoá và mọi lần ghép đôi của nó. Chỉ hai
câu trả lời chính xác [mô tả ở trên](#what-the-relay-can-and-cannot-do) mới được tính. Lỗi
mạng thông thường thì không: kết nối rớt được thử lại với
backoff, và một phiên đăng nhập kết thúc vì lý do khác sẽ dừng remote control cho tới khi
bạn đăng nhập lại nhưng không xoá gì.

**Forget**, một thiết bị mà relay báo là bị thu hồi, và việc xoá trên thiết bị
bị thu hồi đều hiện thông báo nếu thay đổi không lưu được, thay vì trông như
đã xong. Một lần forget không được lưu có thể khiến thiết bị lại được tin cậy sau khi Zode
khởi động lại.

Quên một host từ Zode điều khiển cũng kết thúc mọi phiên đang chạy với
host đó.

Khoá thiết bị nằm trong keychain của bạn, mỗi kênh phát hành một khoá. Stable giữ
`zode://remote-device-key/{user_id}`; các kênh khác dùng
`zode://remote-device-key-{channel}/{user_id}`, trong đó `{channel}` là `dev`,
`nightly` hoặc `preview`. Vì vậy một bản build phát triển bị thu hồi không bao giờ
xoá khoá của ứng dụng bạn đã cài.

| Khẳng định                                                            | Test                                                                                                                                 |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Quên một thiết bị kết thúc phiên của nó và nó không quay lại được     | `forgetting_a_device_ends_its_session_and_it_cannot_come_back` (`crates/remote_control`)                                             |
| Thu hồi gỡ ghim và phiên                                              | `the_relay_announcing_a_revoked_device_removes_its_pin_and_its_session` (`crates/remote_control`)                                    |
| Chỉ thu hồi tường minh mới xoá ghim và khoá                           | `only_an_explicit_device_revocation_wipes_the_pins_and_key` (`crates/remote_relay_client`)                                           |
| Thu hồi khi đang mở dự án dừng project server                         | `revoking_the_device_during_an_open_project_stops_the_project_server` (`crates/remote_control`)                                      |
| Relay đóng socket của thiết bị bị thu hồi                             | `closes an editor's live socket when the device is revoked, within a second` (`web/backend/test/remote-relay.e2e-spec.ts`)           |
| Tin cậy một host để điều khiển không nói gì về việc nó điều khiển bạn | `hosts_pinned_for_control_never_become_devices_allowed_to_control` (`crates/remote_relay_client`)                                    |
| Phiên đăng nhập kết thúc vì lý do khác không xoá gì                   | `a_revoked_sign_in_halts_remote_control_but_keeps_the_key_and_the_pins` (`crates/remote_control`)                                    |
| Liệt kê và quên thiết bị được khi remote control đang tắt             | `trusted_devices_are_listed_and_forgotten_while_remote_control_is_off` (`crates/remote_control`)                                     |
| Một lần forget không lưu được sẽ nói rõ                               | `a_forget_that_cannot_be_saved_says_so_instead_of_looking_done` (`crates/remote_control`)                                            |
| Quên một host kết thúc các phiên đang mở tới nó                       | `forgetting_a_host_ends_the_sessions_open_to_it` (`crates/remote_relay_client`)                                                      |
| Mỗi kênh phát hành có mục khoá riêng                                  | `no_two_channels_share_a_keychain_entry`, `removing_one_channels_key_leaves_the_other_channels_alone` (`crates/remote_relay_client`) |

## Tắt là tắt hẳn

Khi `remote_control.enabled` là false, Zode không mở kết nối nào cho remote
control và không bắt terminal nào. Tắt nó khi đang có phiên mở sẽ gỡ
mọi kết nối, tap và project server. Thoát Zode cũng vậy: các phiên
kết thúc và project server dừng, nên cả project server lẫn các terminal mà một
thiết bị khởi động trong đó đều không sống lâu hơn Zode. Nó không đổi setting `enabled`.

| Khẳng định                                     | Test                                                                                                                        |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Mặc định tắt, với giới hạn rảnh 30 phút        | `remote_control_ships_off_with_a_thirty_minute_idle_limit` (`crates/remote_control`)                                        |
| Khi tắt, không mở socket và không bắt terminal | `while_disabled_no_socket_is_opened_and_no_terminal_is_tapped` (`crates/remote_control`)                                    |
| Tắt thì ngắt kết nối ngay và giữ nguyên tắt    | `turning_it_off_from_the_action_disconnects_at_once_and_stays_off` (`crates/remote_control`)                                |
| Tắt nó dừng project server                     | `turning_remote_control_off_stops_the_project_server` (`crates/remote_control`)                                             |
| Thoát thì tắt nó và dừng project server        | `quitting_zode_turns_remote_control_off`, `quitting_zode_stops_the_project_server_and_its_daemon` (`crates/remote_control`) |

## Chỗ này yếu hơn bạn tưởng

- **Metadata nhìn thấy được.** Relay biết khi nào bạn kết nối, từ thiết bị nào,
  tới host nào, và bạn gửi bao nhiêu.
- **Thiết bị đã ghép đôi là một shell**, như trên. Lời nhắc ghép đôi nói vậy; mục
  đích của trang này là không giấu điều đó.
- **Script trên trang có thể dùng khoá của trình duyệt.** Nó được lưu
  dạng không trích xuất được, nên không chép đi được, nhưng một script chạy trong
  origin đó vẫn dùng được nó trong lúc chạy. Chỉ ghép đôi trình duyệt bạn tin.
- **Tính sẵn sàng thuộc về relay.** Một relay thù địch có thể từ chối dịch vụ.
- **Agent là các chương trình riêng.** Remote control không đưa chúng vào sandbox.
