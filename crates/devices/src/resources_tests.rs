use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::fake_host::FakeHost;
use crate::host::HostOs;
use crate::resources::*;
use crate::test_fixtures::tool_output;

const GIB: u64 = 1024 * 1024 * 1024;

#[test]
fn memory_is_low_only_below_four_gigabytes() {
    assert!(memory_status(Some(LOW_MEMORY_THRESHOLD_BYTES - 1), 2).low);
    assert!(!memory_status(Some(LOW_MEMORY_THRESHOLD_BYTES), 2).low);
    assert!(!memory_status(Some(12 * GIB), 0).low);
    let unknown = memory_status(None, 1);
    assert!(!unknown.low, "an unknown amount never warns");
    assert_eq!(unknown.running_count, 1);
    assert_eq!(LOW_MEMORY_THRESHOLD_BYTES, 4 * GIB);
}

#[test]
fn known_emulator_failures_are_explained() {
    for (log, expected) in [
        (
            tool_output("emulator-second-launch-same-avd.txt"),
            "already running",
        ),
        (
            "ERROR | x86_64 emulation currently requires hardware acceleration!".to_string(),
            "hardware acceleration",
        ),
        (
            "ERROR | KVM is required to run this AVD. /dev/kvm is not found.".to_string(),
            "KVM",
        ),
        (
            "emulator: ERROR: HAX is not working and emulator runs in emulation mode".to_string(),
            "HAXM",
        ),
        ("ERROR | VT-x is disabled in BIOS".to_string(), "BIOS"),
    ] {
        let explanation =
            explain_emulator_failure(&log).unwrap_or_else(|| panic!("no explanation for {log:?}"));
        assert!(
            explanation.contains(expected),
            "{explanation:?} should mention {expected:?}"
        );
    }
}

#[test]
fn a_healthy_launch_log_has_nothing_to_explain() {
    assert_eq!(
        explain_emulator_failure(&tool_output("emulator-first-launch-head.log")),
        None
    );
    assert_eq!(
        explain_emulator_failure(&tool_output("emulator-accel-check.txt")),
        None
    );
}

fn log_dir() -> PathBuf {
    PathBuf::from("/logs/emulator")
}

#[test]
fn pruning_keeps_the_newest_logs_of_that_avd_only() {
    let host = FakeHost::new(HostOs::MacOs);
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let mut pixel_logs = Vec::new();
    for minute in 0..7 {
        let name = emulator_log_file_name("Pixel", start + Duration::from_secs(minute * 60));
        host.add_file(log_dir().join(&name), "");
        pixel_logs.push(name);
    }
    let other_avd = emulator_log_file_name("Pixel_2", start);
    let dashed_avd = emulator_log_file_name("Pixel-2", start);
    host.add_file(log_dir().join(&other_avd), "");
    host.add_file(log_dir().join(&dashed_avd), "");
    host.add_file(log_dir().join("notes.txt"), "");

    prune_emulator_logs(&host, &log_dir(), "Pixel", EMULATOR_LOGS_KEPT);

    let kept: Vec<bool> = pixel_logs
        .iter()
        .map(|name| host.has_file(&log_dir().join(name)))
        .collect();
    assert_eq!(kept, [false, false, true, true, true, true, true]);
    assert!(host.has_file(&log_dir().join(&other_avd)));
    assert!(host.has_file(&log_dir().join(&dashed_avd)));
    assert!(host.has_file(&log_dir().join("notes.txt")));
}

#[test]
fn log_names_sort_by_start_time_even_across_digit_counts() {
    let early = emulator_log_file_name("A", SystemTime::UNIX_EPOCH + Duration::from_millis(999));
    let late = emulator_log_file_name("A", SystemTime::UNIX_EPOCH + Duration::from_millis(1000));
    assert!(early < late, "{early} < {late}");
    assert!(early.starts_with("emulator-A-") && early.ends_with(".log"));
}

#[test]
fn pruning_a_missing_directory_does_nothing() {
    let host = FakeHost::new(HostOs::Linux);
    prune_emulator_logs(&host, Path::new("/nowhere"), "Pixel", EMULATOR_LOGS_KEPT);
}

#[test]
fn only_the_tail_of_the_log_is_read() {
    let host = FakeHost::new(HostOs::MacOs);
    let log_file = log_dir().join("emulator-Pixel-1.log");
    let mut contents = "x".repeat(20 * 1024);
    contents.push_str("FATAL | the last line");
    host.add_file(&log_file, contents);

    let tail = read_emulator_log_tail(&host, &log_file).expect("the log exists");
    assert!(tail.len() <= 8 * 1024, "read {} bytes", tail.len());
    assert!(tail.ends_with("FATAL | the last line"));
    assert_eq!(
        read_emulator_log_tail(&host, Path::new("/missing.log")),
        None
    );
}

fn at(seconds_after_epoch: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds_after_epoch)
}

#[test]
fn logs_older_than_a_week_go_whichever_avd_wrote_them() {
    let host = FakeHost::new(HostOs::MacOs);
    let now = at(1_800_000_000);
    let day = Duration::from_secs(24 * 60 * 60);
    let mut old_logs = Vec::new();
    let mut fresh_logs = Vec::new();
    for (avd, age_days) in [
        ("Pixel", 8),
        ("Tablet", 30),
        ("Pixel-2", 8),
        ("Pixel", 1),
        ("Tablet", 0),
        ("Pixel-2", 6),
    ] {
        let name = emulator_log_file_name(avd, now - day * age_days);
        host.add_file(log_dir().join(&name), "log");
        if age_days > 7 {
            old_logs.push(name);
        } else {
            fresh_logs.push(name);
        }
    }
    host.add_file(log_dir().join("notes.txt"), "");
    host.add_file(log_dir().join("emulator-unrelated.log"), "");

    prune_stale_emulator_logs(&host, &log_dir(), now);

    for name in &old_logs {
        assert!(
            !host.has_file(&log_dir().join(name)),
            "{name} is a week old"
        );
    }
    for name in &fresh_logs {
        assert!(host.has_file(&log_dir().join(name)), "{name} is recent");
    }
    assert!(host.has_file(&log_dir().join("notes.txt")));
    assert!(host.has_file(&log_dir().join("emulator-unrelated.log")));
}

#[test]
fn a_log_exactly_one_week_old_is_kept_and_one_millisecond_older_is_not() {
    let host = FakeHost::new(HostOs::MacOs);
    let now = at(1_800_000_000);
    let boundary = emulator_log_file_name("A", now - EMULATOR_LOG_MAX_AGE);
    let past_it =
        emulator_log_file_name("B", now - EMULATOR_LOG_MAX_AGE - Duration::from_millis(1));
    host.add_file(log_dir().join(&boundary), "");
    host.add_file(log_dir().join(&past_it), "");
    prune_stale_emulator_logs(&host, &log_dir(), now);
    assert!(host.has_file(&log_dir().join(&boundary)));
    assert!(!host.has_file(&log_dir().join(&past_it)));
}

#[test]
fn older_logs_fit_the_size_budget_after_the_newest_log_of_each_avd() {
    let host = FakeHost::new(HostOs::MacOs);
    let now = at(1_800_000_000);
    let minute = Duration::from_secs(60);
    let newest = emulator_log_file_name("Pixel", now - minute);
    let middle = emulator_log_file_name("Pixel", now - minute * 2);
    let oldest = emulator_log_file_name("Pixel", now - minute * 3);
    host.add_file(log_dir().join(&newest), "x".repeat(40));
    host.add_file(log_dir().join(&middle), "x".repeat(40));
    host.add_file(log_dir().join(&oldest), "x");

    prune_logs_within(&host, &log_dir(), now, EMULATOR_LOG_MAX_AGE, 80);
    assert!(host.has_file(&log_dir().join(&newest)));
    assert!(
        host.has_file(&log_dir().join(&middle)),
        "80 bytes fit exactly"
    );
    assert!(!host.has_file(&log_dir().join(&oldest)), "81 would not");

    prune_logs_within(&host, &log_dir(), now, EMULATOR_LOG_MAX_AGE, 79);
    assert!(host.has_file(&log_dir().join(&newest)), "it may be live");
    assert!(!host.has_file(&log_dir().join(&middle)));
}

#[test]
fn the_newest_log_of_an_avd_may_be_a_running_emulators_so_the_budget_spares_it() {
    let host = FakeHost::new(HostOs::MacOs);
    let now = at(1_800_000_000);
    let live = emulator_log_file_name("Pixel", now - Duration::from_secs(60));
    let finished = emulator_log_file_name("Tablet", now - Duration::from_secs(120));
    let older_finished = emulator_log_file_name("Tablet", now - Duration::from_secs(180));
    host.add_file(log_dir().join(&live), "x".repeat(200));
    host.add_file(log_dir().join(&finished), "x".repeat(10));
    host.add_file(log_dir().join(&older_finished), "x");
    prune_logs_within(&host, &log_dir(), now, EMULATOR_LOG_MAX_AGE, 100);
    assert!(
        host.has_file(&log_dir().join(&live)),
        "over the budget on its own, still open"
    );
    assert!(host.has_file(&log_dir().join(&finished)));
    assert!(
        !host.has_file(&log_dir().join(&older_finished)),
        "what the live log leaves is spent on newer logs first"
    );
}

#[test]
fn the_log_a_start_is_about_to_write_is_never_pruned() {
    let host = FakeHost::new(HostOs::MacOs);
    let now = at(1_800_000_000);
    // The same start time names the same file, and it may already be big.
    let same_start = emulator_log_file_name("Pixel", now);
    let earlier = emulator_log_file_name("Pixel", now - Duration::from_secs(60));
    host.add_file(log_dir().join(&same_start), "x".repeat(200));
    host.add_file(log_dir().join(&earlier), "x");
    prune_logs_within(&host, &log_dir(), now, EMULATOR_LOG_MAX_AGE, 100);
    assert!(host.has_file(&log_dir().join(&same_start)));
    assert!(!host.has_file(&log_dir().join(&earlier)));
}

#[test]
fn pruning_stale_logs_of_a_missing_directory_does_nothing() {
    let host = FakeHost::new(HostOs::Linux);
    prune_stale_emulator_logs(&host, Path::new("/nowhere"), at(1_800_000_000));
}
