// GitHub-hosted standard runners: free without a minute cap on public repositories.
// The size tiers below all resolve to the same label because only one size exists at
// that price (4 vCPU / 16 GB); the distinct names are kept so the ~60 call sites don't
// have to change.
pub const LINUX_SMALL: Runner = Runner("ubuntu-24.04");
pub const LINUX_DEFAULT: Runner = LINUX_XL;
pub const LINUX_XL: Runner = Runner("ubuntu-24.04");
pub const LINUX_LARGE: Runner = Runner("ubuntu-24.04");
pub const LINUX_MEDIUM: Runner = Runner("ubuntu-24.04");

// Docker hosts, nothing more. The glibc floor no longer comes from these labels: the
// bundle jobs run inside `container: ubuntu:22.04` (see `run_bundling::bundle_linux`), so
// the floor is 2.35 and `script/check-glibc-floor` fails the build if anything exceeds it.
//
// `runs-on: ubuntu-22.04` would have been the smaller change and was rejected: that label's
// deprecation begins 2026-09-17 and it is unsupported from 2027-04-17
// (actions/runner-images#14254). Putting the userland in a container decouples the floor
// from whatever GitHub calls its hosted image next, so these two labels can be bumped to
// 26.04 and beyond without touching what we link against. Upstream instead holds a 20.04
// floor by paying for Namespace.so runner profiles, which this fork has no access to.
//
// An earlier version of this comment said 22.04 was impossible because webrtc-sys needs
// clang 17+ and `script/linux` only installs clang-18 on its 20.04 branch. That is dead
// upstream baggage: this fork compiles no C++ at all (zero `webrtc`/`livekit` rows in
// `Cargo.lock`, zero `.cpp`/`.cc` under `crates/` or `tooling/`), so jammy's default
// clang-14 is enough. The same comment claimed the x86 22.04 image carries clang-18; it
// ships 13/14/15. Both claims were inherited rather than measured.
//
// `Dockerfile-distros` is the local reproduction harness for this container build.
pub const LINUX_X86_BUNDLER: Runner = Runner("ubuntu-24.04");
pub const LINUX_ARM_BUNDLER: Runner = Runner("ubuntu-24.04-arm");

pub const LINUX_LARGE_RAM: Runner = Runner("ubuntu-24.04");

// The arm64 mac runner carries only 7 GB of RAM against the Intel one's 14 GB, so the
// aarch64 bundle is the tightest of the six targets rather than the roomiest.
pub const MAC_DEFAULT: Runner = Runner("macos-15");
pub const MAC_INTEL: Runner = Runner("macos-15-intel");

// Both Windows architectures build here, on an x64 host. There is a `windows-11-arm`
// runner, but `Launch-VsDevShell.ps1` only accepts `x86` or `amd64` for `-HostArch`, so a
// native arm64 host is rejected outright. Cross-compiling from x64 is what upstream does
// and what `bundle-windows.ps1` is written for -- it takes the target architecture as an
// argument, unlike `bundle-linux`, which builds for whatever `uname -m` reports and
// therefore does need a native arm runner.
pub const WINDOWS_DEFAULT: Runner = Runner("windows-2025");

pub struct Runner(&'static str);

impl Into<gh_workflow::RunsOn> for Runner {
    fn into(self) -> gh_workflow::RunsOn {
        self.0.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    X86_64,
    AARCH64,
}

impl std::fmt::Display for Arch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Arch::X86_64 => write!(f, "x86_64"),
            Arch::AARCH64 => write!(f, "aarch64"),
        }
    }
}

impl Arch {
    pub fn linux_bundler(&self) -> Runner {
        match self {
            Arch::X86_64 => LINUX_X86_BUNDLER,
            Arch::AARCH64 => LINUX_ARM_BUNDLER,
        }
    }

    pub fn mac_bundler(&self) -> Runner {
        match self {
            Arch::X86_64 => MAC_INTEL,
            Arch::AARCH64 => MAC_DEFAULT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Windows,
    Linux,
    Mac,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Windows => write!(f, "windows"),
            Platform::Linux => write!(f, "linux"),
            Platform::Mac => write!(f, "mac"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseChannel {
    Nightly,
    /// Whatever the release tag says -- `stable` for `v0.1.0`, `preview` for
    /// `v0.1.0-beta.1`. Resolved inside the job, because the bundling scripts read
    /// `crates/zed/RELEASE_CHANNEL` and a job that never writes it bundles the
    /// checked-in channel instead of the one being released.
    FromTag,
}
