//! Pure logic for the per-project CPU/RAM footer badge: the setting, the
//! footprint types, and the aggregation rules that turn scattered per-process
//! samples into one number per tracked project.
//!
//! The OS sits behind [`ProcessSampler`], mirroring the convention already used
//! by `MemoryPressureReader` (`crates/workspace/src/multi_workspace.rs`): the
//! interesting bugs live in attribution and normalization, not in reading
//! `sysinfo`, so those rules are what has to be exercisable with a scripted
//! fake rather than a real process tree. This crate never reads the OS itself
//! -- `sysinfo_process_sampler.rs` (phase 03) is the only place that does, and
//! it is not built yet.

use gpui::{App, EntityId, SharedString};
use settings::{RegisterSetting, Settings, SettingsContent};

/// A process identifier.
///
/// Not `sysinfo::Pid`: this crate does not depend on `sysinfo` at all, so the
/// aggregation and attribution rules below can be unit tested with no OS
/// access. The sampler that implements [`ProcessSampler`] over real
/// `sysinfo::Pid` values converts with `Pid::as_u32()`.
pub type Pid = u32;

/// Whether the footer badge is shown at all. Read through `try_get` with a
/// `true` fallback, exactly as `KeepDisplayAwakeSetting::is_enabled` does, so
/// a context without a settings store behaves like the default instead of
/// silently switching the indicator off.
#[derive(RegisterSetting)]
pub struct ProjectFootprintSetting(pub bool);

impl Settings for ProjectFootprintSetting {
    /// Panics if `default.json` is missing `project_footprint_indicator`, per
    /// `Settings::from_settings`' convention: `#[derive(RegisterSetting)]`
    /// registers this type at link time, so every `SettingsStore` in the
    /// process evaluates it and a forgotten default fails loudly and
    /// immediately rather than silently taking a fallback.
    fn from_settings(content: &SettingsContent) -> Self {
        Self(
            content
                .project_footprint_indicator
                .expect("project_footprint_indicator missing from default.json"),
        )
    }
}

impl ProjectFootprintSetting {
    pub fn is_enabled(cx: &App) -> bool {
        Self::try_get(cx).map(|setting| setting.0).unwrap_or(true)
    }
}

/// The combined CPU and RAM of one project's tracked processes.
///
/// Both fields are `Option` rather than defaulting to zero: a project with
/// nothing measured (a remote/guest project with no local PIDs, or a freshly
/// built sampler with no CPU baseline yet) is materially different from a
/// project genuinely using zero -- the former renders as "not measured".
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProjectFootprint {
    /// What this platform's own system monitor calls these processes' memory,
    /// summed. Named for that and not for RSS, because it is no longer RSS on
    /// any of the three platforms -- see `footprint_memory`.
    pub memory_bytes: Option<u64>,
    /// A share of the CPUs these processes may run on, clamped to 100.
    pub cpu_percent: Option<f32>,
}

/// The PIDs a `collect` pass should attribute to one project. `roots` are the
/// project's own child processes (language servers, terminals), gathered on
/// the foreground with no syscalls; their descendants are found by
/// [`ProcessSampler::descendants`].
pub struct FootprintRoots {
    pub key: EntityId,
    pub label: SharedString,
    pub roots: Vec<Pid>,
}

/// The per-project results of a `collect` pass, in the order `collect` was
/// given the roots. A `Vec`, not a map: the popover renders projects in a
/// stable order, and the collection is bounded by the number of tracked
/// projects, so there is no lookup cost worth a `HashMap` here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Footprints(pub Vec<(EntityId, SharedString, ProjectFootprint)>);

impl Footprints {
    /// The badge's number: the sum of every project's memory and CPU. A `None`
    /// contributor is skipped rather than treated as zero; the total is
    /// `None` only when *every* contributor is `None` -- otherwise one
    /// remote project among several real ones would blank the whole badge.
    ///
    /// CPU is clamped and memory is not, and the asymmetry is the point. Each
    /// contributor has already been divided by the CPU count, so it is a share
    /// of the whole machine and their sum can claim more machine than exists --
    /// two pegged projects added to 200%. Summed memory has no such ceiling:
    /// six gigabytes across two projects is simply six gigabytes.
    ///
    /// Clamped rather than re-derived, because re-normalizing here would divide
    /// by the CPU count a second time.
    pub fn combined(&self) -> ProjectFootprint {
        let mut memory_bytes: Option<u64> = None;
        let mut cpu_percent: Option<f32> = None;
        for (_, _, footprint) in &self.0 {
            if let Some(memory) = footprint.memory_bytes {
                memory_bytes = Some(memory_bytes.unwrap_or(0).saturating_add(memory));
            }
            if let Some(cpu) = footprint.cpu_percent {
                cpu_percent = Some(cpu_percent.unwrap_or(0.0) + cpu);
            }
        }
        ProjectFootprint {
            memory_bytes,
            cpu_percent: cpu_percent.map(|total| total.clamp(0.0, 100.0)),
        }
    }
}

/// The OS hides behind this trait so the attribution and summing rules in
/// [`collect`] -- the part that can double-count -- are testable with a
/// scripted fake process tree and no real syscalls.
pub trait ProcessSampler: Send {
    /// Full enumeration, used only to find which PIDs descend from `roots`.
    /// Returns `(pid, its root)` pairs: the parent-chain walk that resolves
    /// each PID to *a* root stays inside the sampler, while `collect` decides
    /// what happens when more than one root claims the same PID.
    fn descendants(&mut self, roots: &[Pid]) -> Vec<(Pid, Pid)>;

    /// A narrow refresh of exactly `pids`. `None` CPU means no baseline yet --
    /// `sysinfo` needs two refreshes at least `MINIMUM_CPU_UPDATE_INTERVAL`
    /// apart, so a freshly sampled PID has a real memory reading but no CPU one.
    ///
    /// The `u64` is what the *platform's own system monitor* calls this
    /// process's memory -- proportional set size on Linux, physical footprint
    /// on macOS, private commit on Windows -- falling back to resident set size
    /// wherever that reading cannot be taken. Not plain RSS: summing RSS over a
    /// tree counts every shared page once per process mapping it. See
    /// `footprint_memory`.
    fn sample(&mut self, pids: &[Pid]) -> Vec<(Pid, u64, Option<f32>)>;

    /// How many logical CPUs the measured processes may run on, used to
    /// normalize `sysinfo`'s per-CPU percentages down to a share of the
    /// machine. Logical and not physical: `Process::cpu_usage()`'s own doc says
    /// to divide by "the number of CPUs", and the two differ by the SMT factor
    /// on every platform except the one this feature was measured on.
    fn cpu_count(&self) -> usize;
}

mod footprint_memory;

mod sysinfo_process_sampler;
pub use sysinfo_process_sampler::SysinfoProcessSampler;

mod footprint_collect;
pub use footprint_collect::collect;

mod footprint_format;
pub use footprint_format::{format_cpu, format_memory};

mod footprint_popover;

mod footprint_indicator;
pub use footprint_indicator::ProjectFootprintIndicator;

#[cfg(test)]
mod footprint_memory_tests;

#[cfg(test)]
mod project_footprint_tests;

#[cfg(test)]
mod sysinfo_process_sampler_tests;

#[cfg(test)]
mod footprint_indicator_tests;
