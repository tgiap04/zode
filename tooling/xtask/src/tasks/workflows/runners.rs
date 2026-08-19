// GitHub-hosted standard runners: free without a minute cap on public repositories.
// The size tiers below all resolve to the same label because only one size exists at
// that price (4 vCPU / 16 GB); the distinct names are kept so the ~60 call sites don't
// have to change.
pub const LINUX_SMALL: Runner = Runner("ubuntu-24.04");
pub const LINUX_DEFAULT: Runner = LINUX_XL;
pub const LINUX_XL: Runner = Runner("ubuntu-24.04");
pub const LINUX_LARGE: Runner = Runner("ubuntu-24.04");
pub const LINUX_MEDIUM: Runner = Runner("ubuntu-24.04");

// 24.04, which sets the glibc floor at 2.39. Upstream targeted 20.04 (glibc 2.31) for a
// lower floor, but GitHub retired that image, and 22.04 does not work here: `script/linux`
// only installs clang-18 on its 20.04 branch, webrtc-sys needs clang 17+, and while the
// x86 22.04 image happens to carry clang-18 the arm one does not. 24.04 ships clang-18 as
// the default `clang` on both architectures.
//
// Lowering the floor again means building inside a container -- `Dockerfile-distros` is
// already in the repository for that.
pub const LINUX_X86_BUNDLER: Runner = Runner("ubuntu-24.04");
pub const LINUX_ARM_BUNDLER: Runner = Runner("ubuntu-24.04-arm");

pub const LINUX_LARGE_RAM: Runner = Runner("ubuntu-24.04");

// The arm64 mac runner carries only 7 GB of RAM against the Intel one's 14 GB, so the
// aarch64 bundle is the tightest of the six targets rather than the roomiest.
pub const MAC_DEFAULT: Runner = Runner("macos-15");
pub const MAC_INTEL: Runner = Runner("macos-15-intel");
pub const WINDOWS_DEFAULT: Runner = Runner("windows-2025");
pub const WINDOWS_ARM: Runner = Runner("windows-11-arm");

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

    pub fn windows_bundler(&self) -> Runner {
        match self {
            Arch::X86_64 => WINDOWS_DEFAULT,
            Arch::AARCH64 => WINDOWS_ARM,
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
}
