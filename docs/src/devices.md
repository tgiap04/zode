---
title: Devices
description: Start iOS simulators and Android emulators, see connected phones, pair an Android phone over Wi-Fi, and run your app on any of them, from a panel in the right dock.
---

# Devices

The Devices panel lists the iOS simulators, Android emulators and physical phones of your computer, and starts and stops the virtual ones. The phone icon in the right dock opens it. The devices open in their own windows, the way the system shows them; Zode does not draw their screens.

## What you need

The panel only reads what is already installed. It never installs an SDK, accepts a licence or changes your Xcode or Android setup.

| Group             | You need                                                                                                           |
| ----------------- | ------------------------------------------------------------------------------------------------------------------ |
| iOS Simulators    | A Mac with Xcode 15 or newer selected as the active developer folder; the Command Line Tools alone are not enough. |
| Android Emulators | The Android SDK with the **Android Emulator** package, and at least one virtual device (AVD).                      |
| Physical Devices  | Android: the SDK's `adb` (in the `platform-tools` package). iPhone: a Mac with Xcode 15 or newer.                  |

Zode looks for the Android SDK in `ANDROID_HOME`, then `ANDROID_SDK_ROOT`, then the default install folder of Android Studio, and last at an `adb` on the `PATH` of your login shell. It never reads a project's `local.properties` or `.envrc` to find it. When something is missing, the group says what and offers **Open setup guide**, which opens this page.

The panel works with projects on this computer only. In a remote or shared project it says so and runs nothing.

## The list does not update itself

Zode reads the devices once, the first time the panel is shown, and again when you press **Reload** (the circular arrow in the panel header, or `devices panel: reload` in the command palette). Nothing polls in the background. After you plug in a phone, start an emulator from Android Studio or close a simulator, press **Reload**.

The three exceptions are things you just did in the panel. After **Boot** or **Shutdown**, Zode watches that one device until it has finished or timed out; after a successful wireless connect it reads the list once; and **Run** reads it once more before it starts, so the app goes to the device as it is now. Then it stops.

## Starting and stopping

- **Boot** starts a simulator or emulator, and its row shows _Starting…_ until the system reports it as running. Zode waits up to two minutes for a simulator and three for an emulator.
- **Show window** (simulators) brings the Simulator window back if you closed it. An emulator opens its own window when it boots.
- **Shutdown** stops the device.

Quitting Zode does not shut down the devices you started; they keep running until you stop them. You can boot as many as you like at once. If an emulator fails to start, the message under its row says why when Zode recognises the cause, and **Open log** opens the emulator's own log. Zode keeps the last five logs of each AVD in the `devices` folder inside its log folder.

## Memory

The line under the header counts the running simulators and emulators and the free memory, for example _2 running · 9.5 GB free_. Each one takes about 3 GB. Below 4 GB free the line turns to a warning colour. It is only a warning: Zode never stops you from starting another.

## Physical devices

Physical iPhones and Android phones are listed together in the **Physical Devices** group. Zode cannot turn a phone on or off, so these rows have no Boot or Shutdown.

### Android over USB

Turn on Developer options and **USB debugging** on the phone, connect it and press **Reload**. A phone that is listed but not ready has a line under its row:

| State                 | What it means and what to do                                                                                        |
| --------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Allow USB debugging   | Unlock the phone and accept the "Allow USB debugging?" prompt, then **Reload**. Zode never accepts the key for you. |
| Offline               | Replug the cable, or turn USB debugging off and on, then **Reload**.                                                |
| No permission (Linux) | `adb` cannot open the device. Add a udev rule for the phone's vendor, replug it and **Reload**.                     |

### Android over Wi-Fi

Press **Connect wirelessly…** under the Physical Devices group. You need Android 11 or newer, and the phone and computer on the same network.

1. On the phone, open Developer options, then **Wireless debugging**, then **Pair device with pairing code**.
2. In the dialog, enter the address and port shown there and the six-digit code, then press **Pair**.
3. Enter the address shown on the main Wireless debugging screen. Its port is not the pairing port. Press **Connect**.

If you have already paired this computer, press **Already paired** to go straight to connecting. When the connection succeeds the dialog closes and the phone appears in the list.

Only private and link-local addresses are accepted (`192.168.x.x`, `10.x.x.x`, `172.16.x.x` to `172.31.x.x`, `169.254.x.x` and `fe80::`). Anything else is refused before adb runs, and Zode does not scan the network for phones. The pairing code is sent to adb on its standard input, so it does not appear in a process list, and Zode does not log it or keep it after you press **Pair**. If pairing or connecting fails, the dialog shows adb's own message and stays open. On a network that blocks device-to-device traffic, run `adb connect` yourself in a terminal and press **Reload**.

### iPhone

iPhones are read through Xcode's `devicectl`, whose output Apple does not document. The rows carry an **Experimental** label because this has not been verified on a real iPhone yet. A phone that is paired but not connected is listed as _Not connected_.

## Run app

**Run** on a device row builds your app and runs it on that device, in a new tab of the terminal panel. Zode runs the framework's own command line; it does not build anything itself.

| Framework        | Found by                                                                                                           | What runs                                                                                                                                   |
| ---------------- | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Flutter          | `pubspec.yaml` that depends on Flutter, with `lib/main.dart`                                                       | `flutter run -d <device>`                                                                                                                   |
| Expo             | `expo` in `package.json`, and `app.json` or `app.config.*`                                                         | `npx --no-install expo run:ios -d <device>`, or `run:android`                                                                               |
| React Native     | `react-native` in `package.json`, with an `ios` or `android` folder                                                | `npx --no-install react-native run-ios --udid <device> --no-packager`, or `run-android --device`, and Metro in its own tab                  |
| Android (Gradle) | `gradlew` and `settings.gradle` or `settings.gradle.kts`, and a module that applies the Android application plugin | `./gradlew :app:installDebug` (`.\gradlew.bat` on Windows) with `ANDROID_SERIAL` set to the device, then `adb shell monkey` to open the app |
| Xcode (Mac only) | an `.xcworkspace`, or else an `.xcodeproj`; CocoaPods' own `Pods` files are skipped                                | `xcodebuild … -destination id=<simulator> build`, then `xcrun simctl install` and `xcrun simctl launch`                                     |

Zode looks for apps at the top of each open folder and one level below it. The `ios` and `android` folders of a Flutter, Expo or React Native app belong to that app and are not listed again. Folders that are symbolic links are not followed.

### Trust comes first

A run runs the project's own code: the Gradle wrapper (which can download Gradle from an address the project names), the scripts under `node_modules/.bin`, Xcode build phases and Swift packages. So Run is offered only in folders you have trusted, and Zode checks the trust again right before it reads the project or starts anything. In a folder you have not trusted, the panel lists no app and says why. For an Xcode project, even listing the schemes runs `xcodebuild`, so that too waits for the trust check.

### Starting a run

- On a stopped simulator or emulator, **Run** boots it first and runs the app once it is up.
- When several apps fit the device, or an Xcode project has several schemes, the row asks which one. It asks again on every click; nothing is remembered.
- Each run opens its own terminal tab. The tools come from the `PATH` of your login shell: if the tab says `flutter` or `npx` was not found, fix `PATH` in your shell profile and restart Zode.

### Expo, React Native and Flutter

- **Expo prebuild**: when the project has no `ios` or `android` folder for the device, Zode asks first. Expo then runs prebuild, which creates the folder and edits `app.json` and `package.json`.
- **One device per project**: Expo and React Native share Metro's port, so one of their apps runs on one device at a time. Zode checks this inside one window only: two windows on the same project can still collide.
- **Metro**: for React Native, Zode starts Metro (`npx react-native start`) in its own tab, once per project.
- **Flutter** runs on several devices at once. Pressing Ctrl-C in its tab stops `flutter run` and closes the app on the device; press `d` instead to detach and leave the app running.

### Android (Gradle) and Xcode

- **Gradle** runs `installDebug` on the app module (`app` when there is one). Projects with product flavors are not supported yet; Gradle reports the missing task in the tab.
- After the install, Zode reads the application id from `app/build/outputs/apk/debug/output-metadata.json`, the file the build itself wrote, so an `applicationIdSuffix` is taken into account. If it cannot read the id, the tab says the app was installed and you open it on the device yourself.
- **Xcode** projects run on iOS simulators only. A physical iPhone needs the app signed for it, which Zode does not set up, so Run on an iPhone is refused with that reason; run it from Xcode. The first Run of a project with Swift packages resolves them before anything builds, which can take a few minutes.
- The build, install and launch steps are one command: when the build fails, nothing is installed or opened, and the error stays in the tab. This command always runs in `/bin/sh` (`cmd.exe` on Windows), not in your own shell, so a failed step stops the rest even if your shell is PowerShell 5, Nushell or Elvish.

### Stopping

Stopping the task or closing its tab stops the command line tool, not always the app. To close the app itself, run `adb -s <serial> shell am force-stop <application id>` for Android, or `xcrun simctl terminate <simulator> <bundle id>` for a simulator.

## Limits

- The devices are not embedded in Zode; they open in their own windows.
- iOS simulators and iPhones need macOS. On Windows and Linux the iOS groups are hidden.
- Only projects on this computer are supported.
- Zode does not install SDKs, create AVDs or accept licences.
- Run app: Xcode projects run on simulators only, Gradle projects with product flavors are not supported, and apps are found only at the top of an open folder and one level below.

## Settings

| Setting                       | Default | What it does                                |
| ----------------------------- | ------- | ------------------------------------------- |
| `devices_panel.button`        | `true`  | Show the phone icon in the right dock.      |
| `devices_panel.default_width` | `320`   | The width of the panel when it first opens. |

See [All Settings](./reference/all-settings.md) for how to change them.

## Troubleshooting

- **"... is not set up"**: install what the message names (Xcode, or Android Studio with the SDK), open it once, then press **Reload**. If your SDK is somewhere unusual, set `ANDROID_HOME` before starting Zode.
- **Two versions of adb**: adb says its server and client differ. Use a single Android SDK: point `ANDROID_HOME` at it and remove other copies of `adb` from your `PATH`. Zode never runs `adb kill-server` itself.
- **The AVD is already running**: an AVD can only run once. Close its emulator window, then press **Reload**.
- **Hardware acceleration**: emulators need virtualization. On Linux install KVM and add your user to the `kvm` group; on Windows turn on Windows Hypervisor Platform or install the emulator hypervisor driver; on every system make sure VT-x or AMD-V is enabled in the BIOS.
- **A phone is missing**: press **Reload**, check the cable and the USB debugging prompt on the phone, and run `adb devices` in a terminal to see what adb itself reports.
