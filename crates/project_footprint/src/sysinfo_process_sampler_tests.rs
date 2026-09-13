//! Real OS reads, so these are plain `#[test]` functions with no GPUI context --
//! the deterministic scheduler treats a genuine syscall as non-determinism.
//! Every PID used here already exists (this test process and the one that
//! launched it), so no process is ever spawned: a real `Terminal` has caused
//! SIGABRT under that scheduler before, and there is no reason to invite it
//! here.

use std::sync::{Arc, Barrier, mpsc};
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

/// A thread is not a child process, and must not be attributed as one.
///
/// On Linux a thread *is* a process in its own right, and sysinfo lists it as
/// one unless explicitly asked not to -- so before `without_tasks()` this call
/// returned one pair per thread, and `sample` then read each of them from
/// `/proc/<pid>/task/<tid>/statm`, which reports the whole process's RSS. A
/// project's RAM came out multiplied by its thread count.
///
/// **Green on macOS and Windows whether or not the fix is present**, because
/// neither ever enumerated tasks. Its real falsifier is `run_tests_linux` in
/// CI, or the standalone container probe. Recorded here rather than left
/// implicit: this crate has already shipped one defect certified by a test that
/// could not fail (`docs/journals/2026-08-27-per-project-cpu-ram-footer.md`).
#[test]
fn a_thread_is_not_a_descendant_process() {
    const THREADS: usize = 7;

    // Threads, never processes -- see this file's own rule at the top. Parked
    // on a channel rather than spinning, so they are unambiguously alive in
    // `/proc/<pid>/task` for the measurement without adding CPU noise to the
    // other tests sharing this binary.
    let started = Arc::new(Barrier::new(THREADS + 1));
    let mut senders = Vec::with_capacity(THREADS);
    let mut handles = Vec::with_capacity(THREADS);
    for _ in 0..THREADS {
        let (sender, receiver) = mpsc::channel::<()>();
        senders.push(sender);
        let started = Arc::clone(&started);
        handles.push(std::thread::spawn(move || {
            started.wait();
            // A closed channel is the stop signal, not an error to report: the
            // test drops its senders once the measurement is done.
            while receiver.recv().is_ok() {}
        }));
    }
    started.wait();

    let mut sampler = SysinfoProcessSampler::new();
    let pairs = sampler.descendants(&[self_pid()]);

    drop(senders);
    for handle in handles {
        handle.join().expect("a parked test thread must not panic");
    }

    assert_eq!(
        pairs.len(),
        1,
        "a test process with {THREADS} threads and no child processes must \
         resolve to exactly one descendant -- itself. Got {} pairs, which is \
         threads being counted as processes: {pairs:?}",
        pairs.len()
    );
}

#[test]
fn sampling_this_process_reports_real_memory() {
    let mut sampler = SysinfoProcessSampler::new();
    let samples = sampler.sample(&[self_pid()]);
    let (_, memory, _) = samples
        .first()
        .copied()
        .expect("this process must be sampleable");
    assert!(memory > 0, "expected a positive memory reading for the test process");
}

/// The denominator must be the CPUs this process may actually run on, not the
/// physical cores it happens to sit on.
///
/// **Green on Apple Silicon whether or not the fix is present** -- physical
/// equals logical there, which is exactly why the defect shipped from that
/// machine. It fails on SMT x86, where it would get logical/2, and inside a
/// cpuset-limited container, where `physical_core_count()` measured 10 against
/// an available parallelism of 2. CI may be green on all three runners, so this
/// is a regression guard; the container probe run is the evidence.
#[test]
fn cpu_count_matches_available_parallelism() {
    let Ok(parallelism) = std::thread::available_parallelism() else {
        // No answer to compare against on this machine. The sampler's fallback
        // is what `cpu_count_is_never_zero` covers.
        return;
    };
    assert_eq!(
        SysinfoProcessSampler::new().cpu_count(),
        parallelism.get(),
        "the CPU denominator must follow the CPUs this process may run on"
    );
}

#[test]
fn cpu_count_is_never_zero() {
    // A zero would become a division by zero in `collect`'s normalization.
    assert!(SysinfoProcessSampler::new().cpu_count() >= 1);
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

    // Two roots, not one, and the second is this process rather than something
    // the launcher happens to own.
    //
    // The premise used to be "the launching process has at least one other
    // descendant", which is a claim about the tree *above* this binary that the
    // test does not control and cannot hold still: it is resolved across two
    // separate enumerations, and a re-parent between them leaves the launcher
    // owning nothing. Observed failing once in ~1200 concurrent runs, with
    // `got 1 pairs`.
    //
    // `descendants` pairs every root with itself unconditionally, before it
    // walks anything, so two roots is >= 2 whatever the machine is doing. The
    // launcher stays in the list because a real tree is still worth walking --
    // it just no longer carries the assertion. Worth removing rather than
    // living with: `without_tasks()` shrinks this count on Linux, which is
    // where the rarity was coming from and where CI runs.
    let wide = sampler.descendants(&[launcher_pid(), self_pid()]);
    let wide_tracked = sampler.tracked_len();
    assert!(
        wide_tracked > 1,
        "two roots are two tracked PIDs before any walking happens; got {} pairs",
        wide.len()
    );

    // Sample the whole wide set, so `narrow` is actually holding records for
    // the rebuild to throw away.
    let wide_pids: Vec<Pid> = wide.iter().map(|&(pid, _)| pid).collect();
    sampler.sample(&wide_pids);
    let retained_wide = sampler.retained_len();

    let narrow = sampler.descendants(&[self_pid()]);
    assert_eq!(
        sampler.tracked_len(),
        narrow.len(),
        "after a shrink the retained map must hold exactly the newly discovered set"
    );
    assert!(
        sampler.tracked_len() < wide_tracked,
        "the retained map must actually shrink ({} -> {})",
        wide_tracked,
        sampler.tracked_len()
    );

    // The rebuild itself, which neither assertion above reaches.
    //
    // `tracked = discovered` runs on *both* branches of the superset check, so
    // deleting `self.narrow = System::new()` leaves both of them green --
    // verified by deleting it. The comment on the first assertion used to claim
    // it would fail, and it would not; that claim is gone.
    //
    // `narrow` is what the rebuild actually discards, and sysinfo has no
    // per-PID removal, so this is the assertion standing between the retained
    // map and unbounded growth.
    assert!(
        sampler.retained_len() < retained_wide,
        "the retained process records must be thrown away on a shrink, not just \
         the ledger that names them ({} -> {})",
        retained_wide,
        sampler.retained_len()
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
