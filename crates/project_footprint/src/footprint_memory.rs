//! What this platform's own system monitor calls a process's memory.
//!
//! Summing `Process::memory()` -- resident set size -- over a process tree
//! counts every shared page once per process that maps it. Measured over a tree
//! of five copies of one binary, RSS overstated the real cost by **33% on
//! Linux** and **90% on macOS**, where the dyld shared cache is mapped into
//! every process. The badge therefore disagreed with htop and Activity Monitor,
//! which is the one thing it has to agree with: a number nobody can check
//! against their own machine is not a measurement.
//!
//! Its own file, and the second in this crate to touch `sysinfo`, because it is
//! the only production code here carrying `#[cfg(target_os)]`, `unsafe`, or
//! file I/O, and the only code whose correctness is a claim about an operating
//! system rather than about attribution. Someone asking "what does this app call memory on my
//! platform?" reads one file; someone auditing `unsafe` greps one path.

/// The memory this process is actually responsible for, as its own platform
/// accounts for it.
///
/// Every arm falls back to `Process::memory()` rather than to zero. A process
/// that cannot be read is not a process using nothing, and the badge renders
/// whatever it is handed with full confidence.
///
/// One platform cannot keep that promise, and it is not this code's to keep:
/// sysinfo fills Windows' `memory` and `virtual_memory` from the same
/// `GetProcessMemoryInfo` call, so when that call fails both are zero and the
/// fallback has nothing better to offer. Pre-existing, unchanged here, and
/// stated so the contract above is not read as stronger than it is.
#[cfg(target_os = "linux")]
pub(crate) fn resident_cost(process: &sysinfo::Process) -> u64 {
    // `smaps_rollup` arrived in kernel 4.14 and is readable only for processes
    // this user owns; a re-parented or just-exited PID fails too. Falling back
    // on any of those is the decision here, not a discarded error -- it is the
    // security boundary and the race both working as intended.
    std::fs::read_to_string(format!("/proc/{}/smaps_rollup", process.pid().as_u32()))
        .ok()
        .and_then(|contents| parse_pss_bytes(&contents))
        // A literal `Pss: 0 kB` is the one input that parses to a number this
        // arm should not report. Unreachable in practice -- a process with no
        // `mm` gives an empty read, which is already `None` -- but the other
        // three arms guard their zero and an asymmetry across four arms of one
        // function is an invitation to the wrong edit later.
        .filter(|&bytes| bytes != 0)
        .unwrap_or_else(|| process.memory())
}

/// See the Linux arm for the contract. `ri_phys_footprint` is the number
/// Activity Monitor prints in its "Memory" column.
#[cfg(target_os = "macos")]
pub(crate) fn resident_cost(process: &sysinfo::Process) -> u64 {
    // SAFETY: `rusage_info_v2` is a `repr(C)` struct of one `[u8; 16]` and
    // eighteen `u64` fields -- no references, no enums, no niches -- so the
    // all-zero bit pattern is a valid value of the type.
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a zero-initialised, correctly sized out-parameter for
    // the `RUSAGE_INFO_V2` flavour being requested, living on this stack frame
    // for the whole call. Nothing inside it is read until the return code has
    // been checked below: a non-zero code leaves the buffer untrustworthy, and
    // that is the only thing this `unsafe` block is asserting.
    let code = unsafe {
        libc::proc_pid_rusage(
            process.pid().as_u32() as libc::c_int,
            libc::RUSAGE_INFO_V2,
            &mut info as *mut libc::rusage_info_v2 as *mut *mut libc::c_void,
        )
    };
    if code != 0 || info.ri_phys_footprint == 0 {
        return process.memory();
    }
    info.ri_phys_footprint
}

/// See the Linux arm for the contract.
#[cfg(target_os = "windows")]
pub(crate) fn resident_cost(process: &sysinfo::Process) -> u64 {
    // sysinfo fills `virtual_memory()` from
    // `PROCESS_MEMORY_COUNTERS_EX::PrivateUsage` on Windows -- private commit,
    // the closest thing the platform offers to "memory this process is
    // responsible for", and free because the refresh already collected it.
    //
    // The trade-off, written down because it is real and unverified: commit
    // counts pages that have been paged out, so this reads a little higher than
    // Task Manager's "private working set" column. Nobody has held the two up
    // side by side on a real Windows machine.
    match process.virtual_memory() {
        0 => process.memory(),
        private_commit => private_commit,
    }
}

/// See the Linux arm for the contract. No platform-specific source is known
/// here, so RSS stands -- the same number this crate reported everywhere before.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) fn resident_cost(process: &sysinfo::Process) -> u64 {
    process.memory()
}

/// The `Pss:` field of a `smaps_rollup`, in bytes.
///
/// Proportional set size: each shared page divided by however many processes
/// map it, so summing it across a tree counts every page exactly once. That is
/// the property RSS lacks and the whole reason this file exists.
///
/// Pure string logic with no OS access, and gated on `test` as well as Linux so
/// it can be driven from a literal fixture on a macOS development machine. That
/// gate is deliberate. Every other test of this file passes just as happily
/// against a `resident_cost` that ignores the platform and returns RSS; this
/// one does not, so it is the only one carrying evidence rather than assurance.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_pss_bytes(contents: &str) -> Option<u64> {
    // `strip_prefix("Pss:")` and not `starts_with("Pss")`: newer kernels also
    // write `Pss_Dirty:`, `Pss_Anon:`, `Pss_File:` and `Pss_Shmem:`, and any of
    // them would be a fraction of the answer reported as the whole of it.
    let field = contents.lines().find_map(|line| line.strip_prefix("Pss:"))?;
    // Every size in this file is written in kB. A value carrying any other unit
    // is a format this parser does not know, and guessing would put a number
    // three orders of magnitude out onto the badge -- so it declines instead,
    // and the caller falls back to a reading it can trust.
    let kilobytes: u64 = field.trim().strip_suffix("kB")?.trim().parse().ok()?;
    kilobytes.checked_mul(1024)
}
