---
title: Install Zode - macOS, Linux, Windows
description: Download and install Zode on macOS, Linux, or Windows. Includes direct download and the Linux installation script.
---

# Installing Zode

## Download Zode

### macOS

Download the latest `Zode-<arch>.dmg` from the [releases page](https://github.com/tgiap04/zode/releases), open it, and drag Zode to your Applications folder. After the first manual installation, Zode will periodically check for install updates.

### Windows

Download the latest `Zode-<arch>.exe` installer from the [releases page](https://github.com/tgiap04/zode/releases) and run it. After the first manual installation, Zode will periodically check for install updates.

### Linux

For most Linux users, the easiest way to install Zode is through our installation script:

```sh
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | sh
```

You can optionally specify a **version** of Zode to install using the `ZODE_VERSION` environment variable:

```sh
# Install the latest stable version (default)
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | sh

# Install a specific version
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | ZODE_VERSION=0.1.6 sh
```

To install the nightly build:

```sh
curl -f https://raw.githubusercontent.com/tgiap04/zode/develop/script/install.sh | ZODE_CHANNEL=nightly sh
```

Preview builds are not published. The older `ZED_VERSION` and `ZED_CHANNEL` variable names are still accepted.

If you installed Zode 0.1.5 on Linux, it cannot update itself to 0.1.6: run the installation script once more. See [Upgrading from Zode 0.1.5](./linux.md#upgrading-from-zode-015).

This script supports `x86_64` and `AArch64`, as well as common Linux distributions: Ubuntu, Arch, Debian, RedHat, CentOS, Fedora, and more.

If Zode is installed using this installation script, it can be uninstalled at any time by running the shell command `zode --uninstall`. The shell will then prompt you whether you'd like to keep your preferences or delete them. After making a choice, you should see a message that Zode was successfully uninstalled.

If this script is insufficient for your use case, you run into problems running Zode, or there are errors in uninstalling Zode, please see our [Linux-specific documentation](./linux.md).

## System Requirements

### macOS

Zode supports the follow macOS releases:

| Version       | Codename | Apple Status   | Zode Status         |
| ------------- | -------- | -------------- | ------------------- |
| macOS 26.x    | Tahoe    | Supported      | Supported           |
| macOS 15.x    | Sequoia  | Supported      | Supported           |
| macOS 14.x    | Sonoma   | Supported      | Supported           |
| macOS 13.x    | Ventura  | Supported      | Supported           |
| macOS 12.x    | Monterey | EOL 2024-09-16 | Supported           |
| macOS 11.x    | Big Sur  | EOL 2023-09-26 | Partially Supported |
| macOS 10.15.x | Catalina | EOL 2022-09-12 | Partially Supported |

The macOS releases labelled "Partially Supported" (Big Sur and Catalina) do not support screen sharing via Zode Collaboration. These features use the [LiveKit SDK](https://livekit.io) which relies upon [ScreenCaptureKit.framework](https://developer.apple.com/documentation/screencapturekit/) only available on macOS 12 (Monterey) and newer.

#### Mac Hardware

Zode supports machines with Intel (x86_64) or Apple (aarch64) processors that meet the above macOS requirements:

- MacBook Pro (Early 2015 and newer)
- MacBook Air (Early 2015 and newer)
- MacBook (Early 2016 and newer)
- Mac Mini (Late 2014 and newer)
- Mac Pro (Late 2013 or newer)
- iMac (Late 2015 and newer)
- iMac Pro (all models)
- Mac Studio (all models)

### Linux

Zode supports 64-bit Intel/AMD (x86_64) and 64-bit Arm (aarch64) processors.

Zode requires a Vulkan 1.3 driver and the following desktop portals:

- `org.freedesktop.portal.FileChooser`
- `org.freedesktop.portal.OpenURI`
- `org.freedesktop.portal.Secret` or `org.freedesktop.Secrets`

### Windows

Zode supports the following Windows releases:

| Version                            | Zode Status |
| ---------------------------------- | ----------- |
| Windows 11, version 22H2 and later | Supported   |
| Windows 10, version 1903 and later | Supported   |

A 64-bit operating system is required to run Zode.

#### Windows Hardware

Zode supports machines with x64 (Intel, AMD) or Arm64 (Qualcomm) processors that meet the following requirements:

- Graphics: A GPU that supports DirectX 11 (most PCs from 2012+).
- Driver: Current NVIDIA/AMD/Intel/Qualcomm driver (not the Microsoft Basic Display Adapter).

### FreeBSD

Not yet available as an official download. Can be built [from source](./development/freebsd.md).

### Web

Not supported at this time. See our [Platform Support issue](https://github.com/zed-industries/zed/issues/5391).
