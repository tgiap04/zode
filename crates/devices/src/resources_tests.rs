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
