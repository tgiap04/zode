//! Real OS reads and one pure parser, so these are plain `#[test]` functions
//! with no GPUI context -- the same rule `sysinfo_process_sampler_tests` states.
//!
//! Read the comment on `memory_is_a_real_number` before trusting this file: it
//! says out loud which test here is evidence and which are only guards.

use super::footprint_memory::{parse_pss_bytes, resident_cost};

/// A `smaps_rollup` as the kernel writes one, trimmed to the fields that matter.
///
/// `Rss:` sits above `Pss:` and `Pss_Dirty:` below it, both on purpose: those
/// are the two lines a careless parser reaches for instead.
const ROLLUP: &str = "\
55a4e0000000-7ffd0a9f9000 ---p 00000000 00:00 0                          [rollup]
Rss:               25124 kB
Pss:               18908 kB
Pss_Dirty:         12000 kB
Pss_Anon:           9000 kB
Shared_Clean:       8000 kB
Private_Dirty:     11000 kB
";

/// The distinguishing test of this phase.
///
/// It fails against a `resident_cost` that ignores the platform and hands back
/// RSS, against a parser that takes the `Rss:` line, against one that takes
/// `Pss_Dirty:`, and against one that forgets kB are not bytes. Every other
/// test in this file passes against all four.
#[test]
fn parse_pss_bytes_reads_the_kilobyte_field() {
    assert_eq!(
        parse_pss_bytes(ROLLUP),
        Some(18_908 * 1024),
        "the proportional set size is the `Pss:` line, converted out of kB"
    );
}

/// A parser that guesses is worse than one that declines: the caller's fallback
/// is a real reading, while a fabricated `Some(0)` renders as a project using
/// no memory at all, confidently.
#[test]
fn parse_pss_bytes_rejects_malformed_input() {
    assert_eq!(parse_pss_bytes(""), None, "empty input");
    assert_eq!(
        parse_pss_bytes("Rss:   25124 kB\nPss_Dirty:   12000 kB\n"),
        None,
        "a rollup with no `Pss:` line of its own"
    );
    assert_eq!(
        parse_pss_bytes("Pss:   notanumber kB\n"),
        None,
        "a value that is not a number"
    );
    assert_eq!(
        parse_pss_bytes("Pss:   18908 MB\n"),
        None,
        "a unit this parser does not know -- guessing would be three orders out"
    );
}

/// This process's memory, read the way its own platform accounts for it.
///
/// **This test does not distinguish the fix from a no-op.** An implementation
/// that ignored the platform entirely and returned `Process::memory()` passes
/// it exactly as written. It is a mutation guard -- it catches an arm that
/// zeroes, panics, or inflates by an order of magnitude -- and nothing more.
/// `parse_pss_bytes_reads_the_kilobyte_field` is what carries the evidence.
///
/// Said out loud because this crate has already shipped a defect certified by a
/// test whose name promised more than its body asserted
/// (`docs/journals/2026-08-27-per-project-cpu-ram-footer.md`).
#[test]
fn memory_is_a_real_number() {
    let (cost, rss) = self_readings();
    assert!(cost > 0, "this process is using some memory; got {cost}");
    assert!(
        cost < rss.saturating_mul(8),
        "a reading {cost} against an RSS of {rss} is not a different accounting \
         of the same process, it is a bug"
    );
}

/// Proportional set size is resident set size with every shared page divided by
/// its sharers, so on Linux it can never exceed RSS.
///
/// Linux only, and deliberately not generalised: macOS's `phys_footprint`
/// legitimately *exceeds* RSS when pages have been compressed, since compressed
/// pages are charged to the process but are not resident. Asserting the bound
/// everywhere would buy a flake on the platform this is developed on.
///
/// **This one does not distinguish the fix from a no-op either.** A
/// `resident_cost` that returned `Process::memory()` satisfies `cost <= rss`
/// with equality. It catches inflation -- a doubled `* 1024`, a parser reading
/// a larger field -- and nothing else. Not tightened to `cost < rss`, because a
/// statically linked process sharing nothing can legitimately have PSS equal to
/// RSS, and that would buy a flake in exchange for no evidence.
#[test]
#[cfg(target_os = "linux")]
fn proportional_memory_never_exceeds_resident_memory() {
    let (cost, rss) = self_readings();
    assert!(
        cost <= rss,
        "PSS {cost} cannot exceed RSS {rss} -- every shared page is a fraction \
         of itself in the first number and whole in the second"
    );
}

/// **There is no test that the platform read failing falls back to RSS**, and
/// the absence is deliberate rather than an oversight.
///
/// `resident_cost` takes a `&sysinfo::Process`, and sysinfo only hands one back
/// for a PID it could already read — so a process whose platform read fails is
/// not a process this signature can be given. The candidates were all dishonest
/// in one way or another: a kernel thread has no `smaps_rollup` *and* an RSS of
/// zero, so it proves nothing; faking the failure needs the arms to take a
/// pid-and-fallback pair instead, which was rejected in the plan because it
/// forces a `#[cfg(windows)]` back out to the call site and defeats the one
/// thing this file exists for.
///
/// What covers the path instead: `parse_pss_bytes_rejects_malformed_input` pins
/// every input that produces the `None` the Linux arm falls back on, and each
/// arm is a single `unwrap_or_else`/early return that review walked. Written
/// down because a missing test nobody explains reads, later, as a path nobody
/// thought about.
///
/// `resident_cost` and `Process::memory()` for this process, read from one
/// refresh so the two describe the same instant.
fn self_readings() -> (u64, u64) {
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        false,
        sysinfo::ProcessRefreshKind::nothing()
            .without_tasks()
            .with_memory(),
    );
    let process = system
        .process(pid)
        .expect("this process must be in its own process table");
    (resident_cost(process), process.memory())
}
