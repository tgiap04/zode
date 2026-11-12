//! Where this crate reads the process table. `footprint_memory` is the only
//! other file touching `sysinfo`, and it is split out because it is also the
//! only one carrying `#[cfg(target_os)]`, `unsafe` and file I/O -- concerns
//! worth keeping out of the rhythm logic below.
//!
//! Two nested rhythms. The figures below were **measured on a 10-core macOS
//! box** carrying 817 live processes and have never been re-measured elsewhere;
//! read them as the shape of the difference, not as numbers that hold on Linux
//! or Windows:
//!
//! - `descendants` performs a full enumeration -- **12-15 ms**, touching ~1300
//!   records -- and exists only to learn which PIDs descend from a project's
//!   roots. It runs on the slow (~30 s) cadence.
//! - `sample` performs a narrow refresh of exactly the known PIDs -- **~165 us**,
//!   ~80x cheaper -- on the fast (~3 s) cadence.
//!
//! Linux was doing strictly more than this until `without_tasks()` landed on
//! both calls: it was also enumerating every thread of every process, and
//! `read_dir`-ing `/proc/<pid>/task` to do it. How much more has not been
//! measured, so no Linux figure is quoted here.
//!
//! Two `System` values rather than one, which is the part that keeps the RAM
//! cost honest. A `System` that has only ever been handed
//! `ProcessesToUpdate::Some(&pids)` retains *only* those PIDs (measured: 10
//! records, not 819), so the long-lived `narrow` costs a handful of process
//! structs. Hand it `ProcessesToUpdate::All` even once and it retains all ~1300
//! for the life of the process -- hence the throwaway `System` inside
//! `descendants`, created and dropped inside that call.
//!
//! **The eviction trap, verified by experiment rather than read from the docs:**
//! `remove_dead_processes = true` combined with `ProcessesToUpdate::Some(subset)`
//! does *not* evict PIDs outside `subset` -- refreshing 2 of 10 tracked PIDs
//! retained all 10 -- and `sysinfo` 0.37 exposes `processes()` as an immutable
//! map with no per-PID removal. Terminals open and close constantly, so without
//! the rebuild below this map only ever grows: an unbounded cache, which the
//! repo's rules forbid outright. The rebuild is not redundant. Do not delete it.

use std::collections::{HashMap, HashSet};

use sysinfo::{CpuRefreshKind, ProcessRefreshKind, ProcessesToUpdate, System};

use super::{Pid, ProcessSampler};

/// The denominator that turns summed `Process::cpu_usage()` into a share of the
/// machine.
///
/// `cpu_usage()`'s own doc is explicit -- "If you want a value between 0% and
/// 100%, divide the returned value by the number of CPUs" -- and that is
/// *logical* CPUs. This used to read `System::physical_core_count()`, a
/// different quantity that happens to be equal only where there is no SMT.
/// Apple Silicon is such a machine, and is the one this feature was measured
/// on; on SMT x86 the denominator came out at half the truth and the badge read
/// double.
///
/// `available_parallelism` leads because it is the only candidate that respects
/// cgroup quota and CPU affinity: in a `--cpuset-cpus=0-1` container it
/// answered 2 where sysinfo still answered 10. So the badge reports how busy
/// the CPU *this app may actually use* is, which is the question being asked
/// whenever it is held up against the system's own monitor.
///
/// The sysinfo fallback is deliberately kept rather than collapsing to `1`.
/// `System::cpus()` is **empty** on a `System` that has only refreshed
/// processes (measured: 0), which is why `self.narrow` cannot be asked -- but a
/// `System` built for the CPU list answers properly (measured: 10). Falling
/// straight to `1` on a ten-CPU box would divide by one and peg the badge at
/// 100% for the life of the process: a louder wrong answer than the defect
/// being fixed here.
fn logical_cpu_count() -> usize {
    if let Ok(parallelism) = std::thread::available_parallelism() {
        return parallelism.get();
    }
    // `System::new()` reports no CPUs at all until asked (measured: 0 on both
    // macOS and Linux); one `refresh_cpu_list` answers properly (measured: 10
    // on both). Deliberately not `new_with_specifics(..with_cpu(..))`, which
    // populates the list and then has it thrown away and rebuilt by the very
    // next line -- and on Windows also builds a PDH query for nothing.
    let mut system = System::new();
    system.refresh_cpu_list(CpuRefreshKind::nothing());
    system.cpus().len().max(1)
}

/// A parent chain longer than this is treated as unresolvable rather than
/// walked further, so a cyclic or self-parenting record cannot hang the
/// background thread. Real chains are single digits (a probe resolved
/// `probe -> zsh -> claude -> zode -> launchd` in four hops).
const MAX_PARENT_HOPS: usize = 64;

pub struct SysinfoProcessSampler {
    /// Only ever fed `ProcessesToUpdate::Some`. See the module docs.
    narrow: System,
    /// What `narrow` may currently be holding -- the ledger that makes the
    /// eviction decision possible at all, given there is no per-PID removal.
    tracked: HashSet<Pid>,
    /// False for exactly one `sample` after a rebuild, because a fresh `System`
    /// has no earlier reading to difference against and `cpu_usage()` would
    /// report a meaningless near-zero rather than "not known yet".
    primed: bool,
    cpu_count: usize,
    #[cfg(test)]
    enumerations: usize,
}

impl SysinfoProcessSampler {
    pub fn new() -> Self {
        Self {
            narrow: System::new(),
            tracked: HashSet::new(),
            primed: false,
            cpu_count: logical_cpu_count(),
            #[cfg(test)]
            enumerations: 0,
        }
    }

    /// Resolves which of `roots`, if any, `pid` descends from, by walking the
    /// parent chain. `pid` itself being a root resolves at hop zero.
    fn root_of(
        pid: Pid,
        roots: &HashSet<Pid>,
        parent_of: &HashMap<Pid, Pid>,
        seen: &mut HashSet<Pid>,
    ) -> Option<Pid> {
        seen.clear();
        let mut cursor = pid;
        for _ in 0..MAX_PARENT_HOPS {
            if roots.contains(&cursor) {
                return Some(cursor);
            }
            if !seen.insert(cursor) {
                return None; // Cycle; a chain that never reaches a root.
            }
            cursor = *parent_of.get(&cursor)?;
        }
        None
    }
}

impl Default for SysinfoProcessSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessSampler for SysinfoProcessSampler {
    fn descendants(&mut self, roots: &[Pid]) -> Vec<(Pid, Pid)> {
        if roots.is_empty() {
            // No project owns a child process, so there is nothing a 12-15 ms
            // enumeration could discover. This early return is what makes an
            // idle window genuinely free.
            return Vec::new();
        }

        // Re-read on the slow cadence rather than frozen at construction.
        // `available_parallelism` reads cgroup quota and the affinity mask, and
        // both move under a live process -- `docker update --cpus`, `taskset
        // -p`, CPU hotplug. A value taken once would leave a container whose
        // quota was raised from 2 to 8 reading four times high for the life of
        // the app, while the user documentation promises those limits are
        // respected. One syscall against the 12-15 ms enumeration below.
        self.cpu_count = logical_cpu_count();

        let root_set: HashSet<Pid> = roots.iter().copied().collect();

        let parent_of: HashMap<Pid, Pid> = {
            let mut scratch = System::new();
            #[cfg(test)]
            {
                self.enumerations += 1;
            }
            // `ProcessRefreshKind::nothing()` still populates `parent()`
            // (verified: 542 of 840 records reported one, the rest being
            // kernel-owned processes with genuinely no parent). It also means
            // command lines, executable paths and environments of every process
            // on the machine are never read into this address space.
            //
            // `without_tasks()` is not tidiness. `nothing()` leaves `tasks`
            // **on** -- sysinfo says so in its own doc, because on Linux a
            // thread is a process in its own right -- and with
            // `ProcessesToUpdate::All` sysinfo inserts every thread as a
            // separate record whose `parent()` is its thread-group leader. The
            // walk below then resolves each of those to a real root, and
            // `sample` reads their memory from `/proc/<pid>/task/<tid>/statm`,
            // which reports the *whole process's* RSS. Summing it once per
            // thread multiplied a project's RAM by its thread count: measured
            // at 16.9 MB against a true 2.1 MB for one process with seven
            // threads. macOS and Windows never had the fault, which is why it
            // survived a suite that only ever ran on macOS.
            scratch.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().without_tasks(),
            );
            scratch
                .processes()
                .iter()
                .filter_map(|(pid, process)| {
                    process
                        .parent()
                        .map(|parent| (pid.as_u32(), parent.as_u32()))
                })
                .collect()
        }; // `scratch`'s ~1300 records are freed here, before any sampling.

        let mut pairs: Vec<(Pid, Pid)> = roots.iter().map(|&root| (root, root)).collect();
        let mut seen = HashSet::new();
        for &pid in parent_of.keys() {
            if root_set.contains(&pid) {
                continue; // Already paired with itself above.
            }
            if let Some(root) = Self::root_of(pid, &root_set, &parent_of, &mut seen) {
                pairs.push((pid, root));
            }
        }

        let discovered: HashSet<Pid> = pairs.iter().map(|&(pid, _)| pid).collect();
        if !discovered.is_superset(&self.tracked) {
            // Something needs evicting and there is no way to evict one entry,
            // so the whole map goes. Rebuild only on shrink, never on growth,
            // so the common case keeps its CPU baselines.
            self.narrow = System::new();
            self.primed = false;
            self.tracked = discovered;
            // Prime the fresh `System` with one narrow refresh so the *next*
            // fast tick has a >= 3 s delta to difference against. Deliberately
            // not done when no rebuild happened: an extra refresh moments
            // before `sample` would collapse the interval and make
            // `cpu_usage()` report near-zero for every healthy process.
            self.sample(&self.tracked.iter().copied().collect::<Vec<_>>());
            self.primed = false;
        } else {
            self.tracked = discovered;
        }

        pairs
    }

    fn sample(&mut self, pids: &[Pid]) -> Vec<(Pid, u64, Option<f32>)> {
        if pids.is_empty() {
            return Vec::new();
        }

        let sys_pids: Vec<sysinfo::Pid> =
            pids.iter().copied().map(sysinfo::Pid::from_u32).collect();
        // `remove_dead_processes: false` -- eviction is `descendants`' job via
        // the rebuild, and `true` would not evict anything absent from this
        // list anyway (see the module docs).
        // `resident_cost` below is not free on Linux: `smaps_rollup` walks
        // every VMA of the target with its `mmap_lock` held, where `statm` is
        // O(1). Measured in an aarch64 container -- 35 us at ~74 VMAs, 171 us
        // at ~2000, 746 us at ~8000 -- so a handful of large language servers
        // can put single-digit milliseconds on this tick, not the ~165 us the
        // module doc quotes for the refresh alone on macOS. It runs on the
        // background executor every ~3 s, so that is affordable; it is written
        // down because an unqualified figure is what this crate keeps being
        // bitten by.
        //
        // `without_tasks()` here cannot plant a phantom entry the way it could
        // in `descendants` -- sysinfo discards the thread list for anything but
        // `ProcessesToUpdate::All`. It discards it *after* building it, though,
        // so asking for it still costs a `read_dir` of `/proc/<pid>/task` per
        // sampled PID on Linux for a list nothing here reads. Not measured, so
        // no figure is claimed; the point is that both call sites now ask for
        // what they actually use.
        self.narrow.refresh_processes_specifics(
            ProcessesToUpdate::Some(&sys_pids),
            false,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_memory()
                .with_cpu(),
        );

        let samples = pids
            .iter()
            .zip(sys_pids.iter())
            .filter_map(|(&pid, sys_pid)| {
                let process = self.narrow.process(*sys_pid)?;
                let cpu = self.primed.then(|| process.cpu_usage());
                Some((pid, crate::footprint_memory::resident_cost(process), cpu))
            })
            .collect();
        self.primed = true;

        // A PID handed only to a narrow tick (e.g. a terminal opened and
        // closed inside one ~30 s discovery window) is what `refresh_processes_specifics`
        // above just planted an entry for in `narrow`. `tracked` is otherwise
        // written only by `descendants`, so without this union that entry has
        // no ledger row and `discovered.is_superset(&self.tracked)` in
        // `descendants` can never see it die -- an unbounded leak, which the
        // repo's rules forbid outright. Folding every narrow-tick PID in here
        // means the superset check fails, and therefore rebuilds, more often
        // than it used to: `narrow` loses its CPU baseline (see `primed`)
        // whenever any PID it has ever held -- not just ones `descendants`
        // itself found -- turns out to be gone. That is the deliberate price
        // of not leaking.
        self.tracked.extend(pids.iter().copied());

        samples
    }

    fn cpu_count(&self) -> usize {
        self.cpu_count
    }
}

#[cfg(test)]
impl SysinfoProcessSampler {
    pub(crate) fn tracked_len(&self) -> usize {
        self.tracked.len()
    }

    /// How many process records `narrow` is actually holding, as opposed to how
    /// many the ledger says it might be.
    ///
    /// The two are different numbers and only this one moves when the rebuild
    /// runs -- `tracked` is assigned on both branches of the superset check, so
    /// nothing that reads the ledger can tell a rebuild from a skipped one.
    /// Without this accessor the bounded-cache property is unobservable, which
    /// is how a test came to claim it was pinning a rebuild it never touched.
    pub(crate) fn retained_len(&self) -> usize {
        self.narrow.processes().len()
    }

    pub(crate) fn enumerations(&self) -> usize {
        self.enumerations
    }

    pub(crate) fn is_tracked(&self, pid: Pid) -> bool {
        self.tracked.contains(&pid)
    }
}
