use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime};

use util::ResultExt as _;

use crate::MemoryStatus;
use crate::host::HostSystem;

/// Each simulator or emulator took about 3 GB when measured, so below 4 GB
/// free another one would leave the machine swapping.
pub const LOW_MEMORY_THRESHOLD_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const EMULATOR_LOGS_KEPT: usize = 5;

const LOG_TAIL_BYTES: u64 = 8 * 1024;

pub fn memory_status(available_bytes: Option<u64>, running_count: usize) -> MemoryStatus {
    MemoryStatus {
        running_count,
        available_bytes,
        low: available_bytes.is_some_and(|bytes| bytes < LOW_MEMORY_THRESHOLD_BYTES),
    }
}

/// Turns the emulator's own log lines into advice, for the failures a user can
/// fix themselves.
pub fn explain_emulator_failure(log_tail: &str) -> Option<String> {
    let explanation = if log_tail.contains("Running multiple emulators with the same AVD")
        || log_tail.contains("Another emulator instance is running")
    {
        "This AVD is already running. Close its window before starting it again."
    } else if log_tail.contains("KVM is required") || log_tail.contains("/dev/kvm") {
        "The emulator needs KVM. Enable virtualization in the BIOS, install KVM, and add \
         your user to the kvm group."
    } else if log_tail.contains("HAX is not working") || log_tail.contains("HAXM") {
        "Intel HAXM is not working. Install the Android Emulator hypervisor driver, or \
         turn on Windows Hypervisor Platform."
    } else if log_tail.contains("VT-x") || log_tail.contains("AMD-V") {
        "Hardware virtualization (VT-x or AMD-V) is disabled. Enable it in the BIOS."
    } else if log_tail.contains("emulation currently requires hardware acceleration") {
        "This system image needs hardware acceleration, which is not available on this \
         machine."
    } else {
        return None;
    };
    Some(explanation.to_string())
}

/// Milliseconds are zero-padded so that names sort in start order.
pub(crate) fn emulator_log_file_name(avd_name: &str, started_at: SystemTime) -> String {
    let milliseconds = started_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    format!("emulator-{avd_name}-{milliseconds:016}.log")
}

fn is_log_of(file_name: &str, avd_name: &str) -> bool {
    // The digits-only check keeps `Pixel` from claiming `Pixel-2`'s logs.
    file_name
        .strip_prefix("emulator-")
        .and_then(|rest| rest.strip_prefix(avd_name))
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.strip_suffix(".log"))
        .is_some_and(|stamp| !stamp.is_empty() && stamp.chars().all(|digit| digit.is_ascii_digit()))
}

/// Deletes all but the newest `keep` logs of one AVD; other files are left.
pub fn prune_emulator_logs(host: &dyn HostSystem, log_dir: &Path, avd_name: &str, keep: usize) {
    let Ok(names) = host.read_dir_names(log_dir) else {
        return;
    };
    let mut logs: Vec<String> = names
        .into_iter()
        .filter(|name| is_log_of(name, avd_name))
        .collect();
    logs.sort_unstable_by(|left, right| right.cmp(left));
    for old_log in logs.iter().skip(keep) {
        host.remove_file(&log_dir.join(old_log)).log_err();
    }
}

/// Logs older than this are deleted whichever AVD wrote them.
pub const EMULATOR_LOG_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// What the logs of all AVDs together may take at the next start.
pub const EMULATOR_LOGS_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// Makes room at a start: deletes the logs of every AVD that are older than
/// [`EMULATOR_LOG_MAX_AGE`], then the oldest of the rest until the others fit
/// [`EMULATOR_LOGS_TOTAL_BYTES`]. A chatty emulator's log grows while it runs
/// and nothing here can stop that; this only keeps old runs from piling up.
/// The newest log of each AVD is never cut for size: it may be the log of an
/// emulator that is still running (or of the one this start is about to
/// write), and "Open log" must keep working for it.
pub fn prune_stale_emulator_logs(host: &dyn HostSystem, log_dir: &Path, now: SystemTime) {
    prune_logs_within(
        host,
        log_dir,
        now,
        EMULATOR_LOG_MAX_AGE,
        EMULATOR_LOGS_TOTAL_BYTES,
    );
}

pub(crate) fn prune_logs_within(
    host: &dyn HostSystem,
    log_dir: &Path,
    now: SystemTime,
    max_age: Duration,
    max_total_bytes: u64,
) {
    let Ok(names) = host.read_dir_names(log_dir) else {
        return;
    };
    let oldest_kept = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis())
        .saturating_sub(max_age.as_millis());
    let mut logs: Vec<ParsedLog> = names
        .into_iter()
        .filter_map(|name| {
            let (avd_name, started) = parse_log_name(&name)?;
            Some(ParsedLog {
                avd_name: avd_name.to_string(),
                started,
                name,
            })
        })
        .collect();
    // Newest first, so the budget goes to the logs most likely to be read.
    logs.sort_unstable_by(|left, right| {
        (right.started, &right.name).cmp(&(left.started, &left.name))
    });
    let mut seen_avds = HashSet::new();
    let mut total_bytes = 0u64;
    let mut budgeted = Vec::new();
    for log in logs {
        let path = log_dir.join(&log.name);
        let newest_of_avd = seen_avds.insert(log.avd_name.clone());
        if log.started < oldest_kept {
            host.remove_file(&path).log_err();
        } else if newest_of_avd {
            total_bytes = total_bytes.saturating_add(host.file_len(&path).unwrap_or(0));
        } else {
            budgeted.push(path);
        }
    }
    // What the spared logs leave is spent on the newer of the others first.
    for path in budgeted {
        let size = host.file_len(&path).unwrap_or(0);
        if total_bytes.saturating_add(size) <= max_total_bytes {
            total_bytes += size;
        } else {
            host.remove_file(&path).log_err();
        }
    }
}

struct ParsedLog {
    avd_name: String,
    started: u128,
    name: String,
}

/// The AVD and the start time in a name made by [`emulator_log_file_name`]. An
/// AVD name may hold `-`, so the stamp is whatever follows the last one.
fn parse_log_name(file_name: &str) -> Option<(&str, u128)> {
    let (avd_name, stamp) = file_name
        .strip_prefix("emulator-")?
        .strip_suffix(".log")?
        .rsplit_once('-')?;
    if avd_name.is_empty() || stamp.is_empty() || !stamp.chars().all(|digit| digit.is_ascii_digit())
    {
        return None;
    }
    Some((avd_name, stamp.parse().ok()?))
}

pub fn read_emulator_log_tail(host: &dyn HostSystem, log_file: &Path) -> Option<String> {
    host.read_file_tail(log_file, LOG_TAIL_BYTES).log_err()
}
