---
title: Thiết bị
description: Bật simulator iOS và emulator Android, xem điện thoại đang cắm, ghép đôi điện thoại Android qua Wi-Fi, và chạy app của bạn trên các máy đó, từ một bảng ở dock bên phải.
---

# Thiết bị

Bảng Devices liệt kê simulator iOS, emulator Android và điện thoại thật của máy bạn, đồng thời bật và tắt các máy ảo. Bấm icon điện thoại ở dock bên phải để mở. Máy hiện trong cửa sổ riêng do hệ điều hành vẽ; Zode không tự vẽ màn hình của chúng.

## Cần có gì

Bảng chỉ đọc những gì đã cài sẵn. Nó không cài SDK, không chấp nhận giấy phép và không đổi cấu hình Xcode hay Android của bạn.

| Nhóm              | Bạn cần                                                                                                 |
| ----------------- | ------------------------------------------------------------------------------------------------------- |
| iOS Simulators    | Máy Mac có Xcode 15 trở lên đang được chọn làm thư mục developer; chỉ có Command Line Tools là chưa đủ. |
| Android Emulators | Android SDK có gói **Android Emulator** và ít nhất một máy ảo (AVD).                                    |
| Physical Devices  | Android: `adb` của SDK (trong gói `platform-tools`). iPhone: máy Mac có Xcode 15 trở lên.               |

Zode tìm Android SDK ở `ANDROID_HOME`, rồi `ANDROID_SDK_ROOT`, rồi thư mục cài mặc định của Android Studio, cuối cùng là `adb` trong `PATH` của login shell. Nó không bao giờ đọc `local.properties` hay `.envrc` của dự án để tìm SDK. Thiếu gì thì nhóm đó nói rõ và có nút **Open setup guide**, mở đúng trang này.

Bảng chỉ làm việc với dự án nằm trên máy này. Với dự án từ xa hay dự án đang chia sẻ, bảng báo như vậy và không chạy lệnh nào.

## Danh sách không tự cập nhật

Zode đọc danh sách thiết bị một lần khi bảng hiện ra lần đầu, và đọc lại khi bạn bấm **Reload** (mũi tên tròn trên đầu bảng, hoặc `devices panel: reload` trong bảng lệnh). Không có gì chạy ngầm để dò. Cắm điện thoại, bật emulator từ Android Studio hay đóng simulator xong thì bấm **Reload**.

Ba ngoại lệ là những việc bạn vừa làm ngay trong bảng. Sau **Boot** hoặc **Shutdown**, Zode theo dõi riêng máy đó cho tới khi xong hoặc quá hạn; sau khi kết nối không dây thành công, Zode đọc lại danh sách đúng một lần; còn **Run** đọc thêm một lần trước khi chạy, để app tới đúng máy như nó đang là. Rồi dừng.

## Bật và tắt

- **Boot** bật simulator hoặc emulator; dòng của nó hiện _Starting…_ cho tới khi hệ thống báo máy đã chạy. Zode chờ tối đa hai phút với simulator và ba phút với emulator.
- **Show window** (simulator) đưa cửa sổ Simulator trở lại nếu bạn đã đóng. Emulator tự mở cửa sổ riêng khi khởi động.
- **Shutdown** tắt máy.

Thoát Zode không tắt các máy bạn đã bật; chúng chạy tiếp cho tới khi bạn tắt. Bạn bật được bao nhiêu máy cùng lúc cũng được. Nếu emulator không khởi động được, thông báo dưới dòng của nó nêu nguyên nhân khi Zode nhận ra, và **Open log** mở log của chính emulator. Zode giữ năm log gần nhất của mỗi AVD trong thư mục `devices` nằm trong thư mục log của Zode.

## Bộ nhớ

Dòng dưới đầu bảng đếm số simulator và emulator đang chạy cùng bộ nhớ còn trống, ví dụ _2 running · 9.5 GB free_. Mỗi máy ngốn khoảng 3 GB. Còn dưới 4 GB thì dòng này chuyển sang màu cảnh báo. Chỉ là cảnh báo: Zode không bao giờ cản bạn bật thêm máy.

## Máy thật

iPhone và điện thoại Android thật nằm chung trong nhóm **Physical Devices**. Zode không bật hay tắt được điện thoại, nên các dòng này không có Boot hay Shutdown.

### Android qua USB

Bật Developer options và **USB debugging** trên điện thoại, cắm máy rồi bấm **Reload**. Điện thoại có trong danh sách nhưng chưa dùng được sẽ có một dòng hướng dẫn dưới tên máy:

| Trạng thái            | Ý nghĩa và cách xử lý                                                                                                   |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Allow USB debugging   | Mở khóa điện thoại và chấp nhận hộp thoại "Allow USB debugging?", rồi **Reload**. Zode không bao giờ tự chấp nhận khóa. |
| Offline               | Cắm lại cáp, hoặc tắt rồi bật lại USB debugging, rồi **Reload**.                                                        |
| No permission (Linux) | `adb` không mở được thiết bị. Thêm luật udev cho hãng sản xuất điện thoại, cắm lại máy rồi **Reload**.                  |

### Android qua Wi-Fi

Bấm **Connect wirelessly…** dưới nhóm Physical Devices. Bạn cần Android 11 trở lên, điện thoại và máy tính cùng một mạng.

1. Trên điện thoại, mở Developer options, chọn **Wireless debugging**, rồi **Pair device with pairing code**.
2. Trong hộp thoại, nhập địa chỉ và cổng hiện ở đó cùng mã sáu số, rồi bấm **Pair**.
3. Nhập địa chỉ hiện ở màn hình Wireless debugging chính. Cổng này khác cổng ghép đôi. Bấm **Connect**.

Nếu máy tính này đã ghép đôi rồi, bấm **Already paired** để sang thẳng bước kết nối. Kết nối xong thì hộp thoại đóng và điện thoại hiện trong danh sách.

Chỉ nhận địa chỉ mạng riêng và link-local (`192.168.x.x`, `10.x.x.x`, `172.16.x.x` đến `172.31.x.x`, `169.254.x.x` và `fe80::`). Địa chỉ nào khác bị từ chối trước khi adb chạy, và Zode không quét mạng để tìm điện thoại. Mã ghép đôi được gửi cho adb qua đầu vào chuẩn nên không lộ trong danh sách tiến trình; Zode không ghi log mã này và không giữ lại sau khi bạn bấm **Pair**. Ghép đôi hay kết nối thất bại thì hộp thoại hiện đúng thông báo của adb và vẫn mở. Nếu mạng chặn lưu lượng giữa các máy, tự chạy `adb connect` trong terminal rồi bấm **Reload**.

### iPhone

iPhone được đọc qua `devicectl` của Xcode, và Apple không công bố định dạng đầu ra của lệnh này. Các dòng mang nhãn **Experimental** vì tính năng chưa được kiểm chứng trên iPhone thật. Điện thoại đã ghép đôi nhưng chưa kết nối thì hiện là _Not connected_.

## Chạy app

Nút **Run** trên dòng của một máy sẽ build app và chạy nó trên máy đó, trong một tab mới của bảng terminal. Zode gọi chính công cụ dòng lệnh của framework; nó không tự build gì cả.

| Framework        | Nhận ra nhờ                                                                                                | Lệnh được chạy                                                                                                                       |
| ---------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Flutter          | `pubspec.yaml` phụ thuộc Flutter, kèm `lib/main.dart`                                                      | `flutter run -d <máy>`                                                                                                               |
| Expo             | `expo` trong `package.json`, cùng `app.json` hoặc `app.config.*`                                           | `npx --no-install expo run:ios -d <máy>`, hoặc `run:android`                                                                         |
| React Native     | `react-native` trong `package.json`, có thư mục `ios` hoặc `android`                                       | `npx --no-install react-native run-ios --udid <máy> --no-packager`, hoặc `run-android --device`, và Metro ở tab riêng                |
| Android (Gradle) | `gradlew` và `settings.gradle` hoặc `settings.gradle.kts`, cùng một module dùng plugin Android application | `./gradlew :app:installDebug` (`.\gradlew.bat` trên Windows) với `ANDROID_SERIAL` là máy được chọn, rồi `adb shell monkey` để mở app |
| Xcode (chỉ Mac)  | một `.xcworkspace`, không có thì `.xcodeproj`; bỏ qua các file `Pods` của CocoaPods                        | `xcodebuild … -destination id=<simulator> build`, rồi `xcrun simctl install` và `xcrun simctl launch`                                |

Zode tìm app ở gốc của mỗi thư mục đang mở và một cấp bên dưới. Thư mục `ios` và `android` của app Flutter, Expo hay React Native thuộc về app đó, không bị liệt kê lần nữa. Thư mục là liên kết tượng trưng (symlink) thì bỏ qua.

### Phải tin cậy thư mục trước

Chạy app là chạy mã của chính project: Gradle wrapper (có thể tải Gradle từ địa chỉ do project chỉ định), các script trong `node_modules/.bin`, build phase của Xcode và Swift package. Vì vậy nút Run chỉ có ở thư mục bạn đã tin cậy, và Zode kiểm lại sự tin cậy ngay trước khi đọc project hay chạy bất cứ thứ gì. Ở thư mục chưa tin cậy, bảng không liệt kê app nào và nói rõ lý do. Với project Xcode, chỉ riêng việc liệt kê scheme cũng phải chạy `xcodebuild`, nên bước đó cũng chờ qua kiểm tra tin cậy.

### Bắt đầu chạy

- Simulator hay emulator đang tắt thì **Run** bật máy trước, máy lên xong mới chạy app.
- Khi có nhiều app hợp với máy, hoặc project Xcode có nhiều scheme, dòng của máy sẽ hỏi chọn cái nào. Lần bấm nào cũng hỏi lại; Zode không nhớ lựa chọn.
- Mỗi lần chạy mở một tab terminal riêng. Công cụ được tìm theo `PATH` của shell đăng nhập: nếu tab báo không tìm thấy `flutter` hay `npx`, sửa `PATH` trong file cấu hình shell rồi mở lại Zode.

### Expo, React Native và Flutter

- **Expo prebuild**: project chưa có thư mục `ios` hoặc `android` cho máy đó thì Zode hỏi trước. Sau đó Expo chạy prebuild, tạo thư mục ấy và sửa `app.json`, `package.json`.
- **Mỗi project một máy**: Expo và React Native dùng chung cổng của Metro, nên app của chúng chỉ chạy trên một máy mỗi lúc. Zode chỉ kiểm điều này trong một cửa sổ: hai cửa sổ cùng mở một project vẫn có thể giành nhau.
- **Metro**: với React Native, Zode tự bật Metro (`npx react-native start`) ở tab riêng, mỗi project một lần.
- **Flutter** chạy được trên nhiều máy cùng lúc. Bấm Ctrl-C trong tab sẽ dừng `flutter run` và tắt luôn app trên máy; muốn giữ app chạy thì bấm `d` để tách ra.

### Android (Gradle) và Xcode

- **Gradle** chạy `installDebug` trên module app (ưu tiên `app` nếu có). Project có product flavor chưa được hỗ trợ; Gradle sẽ báo thiếu task ngay trong tab.
- Cài xong, Zode đọc application id từ `app/build/outputs/apk/debug/output-metadata.json`, file do chính lần build ghi ra, nên `applicationIdSuffix` cũng được tính. Không đọc được id thì tab báo app đã cài, bạn tự mở app trên máy.
- Project **Xcode** chỉ chạy trên simulator iOS. iPhone thật cần ký app cho máy đó, việc Zode không lo, nên Run trên iPhone bị từ chối kèm lý do này; hãy chạy từ Xcode. Lần Run đầu của project có Swift package sẽ tải package về trước khi build, có thể mất vài phút.
- Build, cài và mở app là một lệnh duy nhất: build lỗi thì không cài, không mở gì, và lỗi nằm lại trong tab. Lệnh này luôn chạy bằng `/bin/sh` (`cmd.exe` trên Windows) chứ không bằng shell của bạn, nên một bước lỗi sẽ chặn các bước sau kể cả khi shell của bạn là PowerShell 5, Nushell hay Elvish.

### Dừng

Dừng task hay đóng tab chỉ dừng công cụ dòng lệnh, không phải lúc nào cũng tắt app. Muốn tắt hẳn app, chạy `adb -s <serial> shell am force-stop <application id>` với Android, hoặc `xcrun simctl terminate <simulator> <bundle id>` với simulator.

## Giới hạn

- Máy không được nhúng vào Zode; chúng mở trong cửa sổ riêng.
- Simulator iOS và iPhone cần macOS. Trên Windows và Linux, các nhóm iOS bị ẩn.
- Chỉ hỗ trợ dự án nằm trên máy này.
- Zode không cài SDK, không tạo AVD và không chấp nhận giấy phép.
- Chạy app: project Xcode chỉ chạy trên simulator, project Gradle có product flavor chưa được hỗ trợ, và app chỉ được tìm ở gốc thư mục đang mở và một cấp bên dưới.

## Cài đặt

| Cài đặt                       | Mặc định | Tác dụng                              |
| ----------------------------- | -------- | ------------------------------------- |
| `devices_panel.button`        | `true`   | Hiện icon điện thoại ở dock bên phải. |
| `devices_panel.default_width` | `320`    | Chiều rộng của bảng khi mở lần đầu.   |

Xem [Tất cả cài đặt](./reference/all-settings.md) để biết cách đổi.

## Xử lý sự cố

- **"... is not set up"**: cài thứ mà thông báo nêu tên (Xcode, hoặc Android Studio kèm SDK), mở nó một lần, rồi bấm **Reload**. Nếu SDK của bạn nằm ở chỗ lạ, đặt `ANDROID_HOME` trước khi khởi động Zode.
- **Hai bản adb**: adb báo server và client khác phiên bản. Dùng một Android SDK duy nhất: trỏ `ANDROID_HOME` vào nó và xóa các bản `adb` khác khỏi `PATH`. Zode không bao giờ tự chạy `adb kill-server`.
- **AVD đang chạy rồi**: một AVD chỉ chạy được một lần. Đóng cửa sổ emulator của nó rồi bấm **Reload**.
- **Tăng tốc phần cứng**: emulator cần ảo hóa. Trên Linux cài KVM và thêm người dùng vào nhóm `kvm`; trên Windows bật Windows Hypervisor Platform hoặc cài driver hypervisor của emulator; hệ nào cũng cần bật VT-x hoặc AMD-V trong BIOS.
- **Không thấy điện thoại**: bấm **Reload**, kiểm tra cáp và hộp thoại USB debugging trên điện thoại, rồi chạy `adb devices` trong terminal xem chính adb báo gì.
