//! Real OS reads, so these are plain `#[test]` functions with no GPUI context --
//! the deterministic scheduler treats a genuine syscall as non-determinism.
//! Every PID used here already exists (this test process and the one that
//! launched it), so no process is ever spawned: a real `Terminal` has caused
//! SIGABRT under that scheduler before, and there is no reason to invite it
//! here.

use std::{thread::sleep, time::Duration};

use super::{Pid, ProcessSampler, sysinfo_process_sampler::SysinfoProcessSampler};

fn self_pid() -> Pid {
    std::process::id()
}

/// A root with more than one descendant, on any platform.
///
/// The process that launched this one: whatever runs the tests is alive for as
/// long as they are, and its descendants include at least itself and this
/// process. That is all the eviction test needs -- a "wide" set it can then
/// shrink.
///
/// This used to be PID 1, which is `init` on Unix and **nothing at all on
/// Windows**: the widest possible root there found one process, and the test
/// failed on Windows alone while passing everywhere it was written and run.
fn launcher_pid() -> Pid {
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );
    system
        .process(sysinfo::Pid::from_u32(self_pid()))
        .and_then(|process| process.parent())
        .map(|parent| parent.as_u32())
        .expect("a test binary is launched by something, and that something is still running")
}

#[test]
fn a_root_is_its_own_descendant() {
    let mut sampler = SysinfoProcessSampler::new();
    let pairs = sampler.descendants(&[self_pid()]);
    assert!(
        pairs.contains(&(self_pid(), self_pid())),
        "the root itself must be attributed to itself; got {pairs:?}"
    );
}

#[test]
fn sampling_this_process_reports_real_memory() {
    let mut sampler = SysinfoProcessSampler::new();
    let samples = sampler.sample(&[self_pid()]);
    let (_, rss, _) = samples
        .first()
        .copied()
        .expect("this process must be sampleable");
    assert!(rss > 0, "expected a positive RSS for the test process");
}

#[test]
fn core_count_is_never_zero() {
    // A zero would become a division by zero in `collect`'s normalization.
    assert!(SysinfoProcessSampler::new().core_count() >= 1);
}

#[test]
fn cpu_is_none_until_a_baseline_exists() {
    let mut sampler = SysinfoProcessSampler::new();
    let first = sampler.sample(&[self_pid()]);
    assert_eq!(
        first.first().and_then(|&(_, _, cpu)| cpu),
        None,
        "a freshly built System has nothing to difference against, so CPU must \
         read as unknown rather than as zero"
    );

    sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(50));
    let second = sampler.sample(&[self_pid()]);
    assert!(
        second.first().and_then(|&(_, _, cpu)| cpu).is_some(),
        "once a baseline exists CPU must be reported"
    );
}

#[test]
fn shrinking_the_discovered_set_evicts_the_retained_map() {
    let mut sampler = SysinfoProcessSampler::new();

    // The launcher owns at least itself and this process.
    let wide = sampler.descendants(&[launcher_pid()]);
    let wide_tracked = sampler.tracked_len();
    assert!(
        wide_tracked > 1,
        "expected the launching process to have descendants; got {} pairs",
        wide.len()
    );

    let narrow = sampler.descendants(&[self_pid()]);
    assert_eq!(
        sampler.tracked_len(),
        narrow.len(),
        "after a shrink the retained map must hold exactly the newly discovered \
         set -- this is the assertion that would fail if the rebuild were removed"
    );
    assert!(
        sampler.tracked_len() < wide_tracked,
        "the retained map must actually shrink ({} -> {})",
        wide_tracked,
        sampler.tracked_len()
    );
}

#[test]
fn a_pid_seen_only_by_a_narrow_tick_does_not_survive_the_next_discovery() {
    // A PID sysinfo will never find, standing in for a real process that a
    // narrow tick was handed (per `footprint_indicator`'s per-tick
    // `collect_roots` call, which reaches `sample` even outside a discovery
    // pass) and that had already exited by the time the next discovery tick
    // ran. Before Fix 1, `sample` never wrote to `tracked`, so this PID had no
    // ledger row, the `discovered.is_superset(&self.tracked)` guard in
    // `descendants` never saw it as missing, and it stayed in `narrow`
    // forever -- the unbounded leak this test pins shut.
    const PHANTOM_PID: Pid = 999_999;

    let mut sampler = SysinfoProcessSampler::new();
    sampler.sample(&[PHANTOM_PID]);
    assert!(
        sampler.is_tracked(PHANTOM_PID),
        "sample() must fold every PID it was handed into `tracked`, even one \
         sysinfo could not find, or the eviction ledger can never learn about it"
    );

    // Self is not an ancestor of the phantom PID, so it is absent from this
    // discovery pass's `discovered` set.
    sampler.descendants(&[self_pid()]);

    assert!(
        !sampler.is_tracked(PHANTOM_PID),
        "a PID that only a narrow sample ever touched must not survive a \
         discovery pass once it is gone -- otherwise it leaks in `narrow` forever"
    );
}

#[test]
fn no_roots_means_no_enumeration() {
    let mut sampler = SysinfoProcessSampler::new();
    assert!(sampler.descendants(&[]).is_empty());
    assert_eq!(
        sampler.enumerations(),
        0,
        "an idle window must not pay for a full process enumeration"
    );
}
