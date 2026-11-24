# Fixtures

Captured on a Mac on 2026-11-23, then scrubbed by hand: host names, home
directories, the paired iPhone's identifiers and every UDID were replaced with
placeholders (`testuser`, `00000000-0000-4000-8000-0000000000NN`).

Tool versions: Xcode 26.4 (17E192) with `devicectl` 518.27 and one iOS 26.4
runtime; Android SDK platform-tools 37.0.0 (adb 1.0.41), emulator 36.5.11, one
AVD `Medium_Phone` (API 37, arm64).

- `simctl-list-devices-*.json`: `xcrun simctl list devices available -j`, sampled
  while one simulator moved through `Shutdown`, `Booting`, `Booted` and
  `Shutting Down`.
- `simctl-list-runtimes.json`: `xcrun simctl list runtimes -j`.
- `simctl-boot-already-booted.txt`, `simctl-shutdown-already-shutdown.txt`: the
  stderr of a redundant boot or shutdown, which exits 149.
- `devicectl-list-devices-paired-unreachable.json`: the `--json-output` file of
  `xcrun devicectl list devices` with an iPhone that is paired but unplugged.
  Not observed: a connected iPhone, an empty list, `outcome: failed`.
- `devicectl-list-devices-stderr.txt`: the harmless stderr of the same call.
- `adb-*.txt`: `adb devices -l`, `emu avd name`, `emu kill` and `pair` output.
  `adb-emulator-boot-poll.txt` is a hand-written log of `adb devices -l` and
  `getprop sys.boot_completed` sampled every 2 s while an emulator booted. Not
  observed: `unauthorized`, `no permissions`, an adb client/server mismatch.
- `emulator-*.txt|log`: `emulator -list-avds`, `-accel-check`, the head of a
  launch log, and a second launch of an AVD that is already running.
- `gradle-output-metadata-debug.json`: the `output-metadata.json` an Android
  Gradle Plugin 9.3 `installDebug` wrote for a project with
  `applicationIdSuffix ".debug"`.
- `xcodebuild-list-*.json`, `xcodebuild-showbuildsettings-native-prebuild.json`:
  `xcodebuild -list -json` for a project and a workspace, and
  `-showBuildSettings -json` read before the first build.

Lines starting with `# ` in a text fixture describe how it was captured and are
not tool output; tests strip them.

Run app (Flutter 3.41.6, Expo SDK 58, React Native 0.88.1 with CLI 20.2.0):

- `expo-run-*-help.txt`, `rn-run-*-help.txt`: `--help` of the run commands; tests
  check that every option Zode passes is listed there.
- `*-run-device-*behaviour.txt`: what each CLI printed when given a device id,
  including React Native's exit code 0 for a device that is not connected.
- `projects/`: minimal hand-written `pubspec.yaml`, `package.json` and `app.json`
  files for each kind of project the detector must accept or reject.
