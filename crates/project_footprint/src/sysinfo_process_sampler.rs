//! The only file in this crate that touches `sysinfo`.
//!
//! Two nested rhythms, both measured on a 10-core macOS box carrying 817 live
//! processes:
//!
//! - `descendants` performs a full enumeration -- **12-15 ms**, touching ~1300
//!   records -- and exists only to learn which PIDs descend from a project's
//!   roots. It runs on the slow (~30 s) cadence.
//! - `sample` performs a narrow refresh of exactly the known PIDs -- **~165 us**,
//!   ~80x cheaper -- on the fast (~3 s) cadence.
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

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use super::{Pid, ProcessSampler};

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
    core_count: usize,
    #[cfg(test)]
    enumerations: usize,
}

impl SysinfoProcessSampler {
    pub fn new() -> Self {
        Self {
            narrow: System::new(),
            tracked: HashSet::new(),
            primed: false,
            // Read once: the physical core count cannot change at runtime.
            // Deliberately not `System::cpus().len()`, which is **empty** on a
            // `System` that has only refreshed processes (measured: 0) and
            // would turn the caller's normalization into a division by zero.
            core_count: System::physical_core_count().unwrap_or(1).max(1),
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
            scratch.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing(),
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
        self.narrow.refresh_processes_specifics(
            ProcessesToUpdate::Some(&sys_pids),
            false,
            ProcessRefreshKind::nothing().with_memory().with_cpu(),
        );

        let samples = pids
            .iter()
            .zip(sys_pids.iter())
            .filter_map(|(&pid, sys_pid)| {
                let process = self.narrow.process(*sys_pid)?;
                let cpu = self.primed.then(|| process.cpu_usage());
                Some((pid, process.memory(), cpu))
            })
            .collect();
        self.primed = true;
        samples
    }

    fn core_count(&self) -> usize {
        self.core_count
    }
}

#[cfg(test)]
impl SysinfoProcessSampler {
    pub(crate) fn tracked_len(&self) -> usize {
        self.tracked.len()
    }

    pub(crate) fn enumerations(&self) -> usize {
        self.enumerations
    }
}
