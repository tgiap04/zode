#[cfg(unix)]
use std::time::Duration;

use crate::host::{ToolFailure, ToolInvocation, ToolRunner, ensure_absolute_program};
use crate::local_host::LocalHost;

/// The tests describe Mac and Linux hosts with paths like `/usr/bin/xcrun`
/// on every system, Windows included, where such a path is not "absolute".
#[test]
fn a_rooted_program_is_accepted_on_every_system_and_a_bare_name_never() {
    let mut accepted = vec!["/usr/bin/xcrun"];
    // A name without `/` is looked up on `PATH` everywhere but Windows.
    let mut refused = vec!["xcrun", "bin/xcrun", "platform-tools\\adb.exe"];
    if cfg!(windows) {
        accepted.extend(["\\tools\\adb.exe", "C:\\sdk\\adb.exe"]);
    } else {
        refused.extend(["\\tools\\adb.exe", "C:\\sdk\\adb.exe"]);
    }
    for program in accepted {
        assert_eq!(
            ensure_absolute_program(&ToolInvocation::new(program, ["--version"])),
            Ok(()),
            "{program}"
        );
    }
    for program in refused {
        assert_eq!(
            ensure_absolute_program(&ToolInvocation::new(program, ["--version"])),
            Err(ToolFailure::RelativeProgram(program.into())),
            "{program}"
        );
    }
}

#[test]
fn a_relative_program_is_refused_before_anything_runs() {
    let result = smol::block_on(LocalHost::new().run(ToolInvocation::new("adb", ["devices"])));
    assert_eq!(result, Err(ToolFailure::RelativeProgram("adb".into())));
    let detached = LocalHost::new().spawn_detached(
        ToolInvocation::new("emulator", ["-avd", "Pixel"]),
        std::path::Path::new("/tmp/never-written.log"),
    );
    assert!(detached.is_err());
}

#[cfg(unix)]
#[test]
fn stdin_reaches_the_tool_and_both_streams_come_back() {
    let output = smol::block_on(
        LocalHost::new().run(
            ToolInvocation::new(
                "/bin/sh",
                ["-c", "read line; echo \"got $line\"; echo oops >&2; exit 3"],
            )
            .with_stdin("123456\n".to_string()),
        ),
    )
    .expect("sh runs");
    assert_eq!(output.exit_code, Some(3));
    assert_eq!(output.stdout, "got 123456\n");
    assert_eq!(output.stderr, "oops\n");
}

/// A tool that exits before reading its input is the common way for a wrong
/// argument to show up; what it wrote to stderr is the whole explanation.
#[cfg(unix)]
#[test]
fn a_tool_that_exits_without_reading_stdin_still_reports_its_own_error() {
    let output = smol::block_on(
        LocalHost::new().run(
            ToolInvocation::new("/bin/sh", ["-c", "echo usage error >&2; exit 2"])
                // Far more than a pipe holds, so the write cannot just be buffered.
                .with_stdin("x".repeat(4 * 1024 * 1024)),
        ),
    )
    .expect("the tool ran, so its output is returned");
    assert_eq!(output.exit_code, Some(2));
    assert_eq!(output.stderr, "usage error\n");
}

#[cfg(unix)]
#[test]
fn a_tool_that_floods_its_output_is_cut_off_and_still_finishes() {
    use crate::local_host::MAX_TOOL_OUTPUT_BYTES;

    let flood = MAX_TOOL_OUTPUT_BYTES * 3;
    let output = smol::block_on(
        LocalHost::new().run(
            ToolInvocation::new(
                "/bin/sh",
                [
                    "-c",
                    &format!("head -c {flood} /dev/zero | tr '\\0' x; echo late >&2; exit 4"),
                ],
            )
            .with_timeout(Duration::from_secs(20)),
        ),
    )
    .expect("the tool runs to its end");
    assert_eq!(output.exit_code, Some(4));
    assert_eq!(output.stdout.len() as u64, MAX_TOOL_OUTPUT_BYTES);
    assert_eq!(output.stderr, "late\n");
}

#[cfg(unix)]
#[test]
fn a_tool_that_runs_past_its_timeout_is_stopped() {
    let started = std::time::Instant::now();
    let result = smol::block_on(
        LocalHost::new()
            .run(ToolInvocation::new("/bin/sleep", ["5"]).with_timeout(Duration::from_millis(200))),
    );
    assert!(
        matches!(result, Err(ToolFailure::TimedOut { .. })),
        "{result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn a_missing_program_is_not_found() {
    let result = smol::block_on(LocalHost::new().run(ToolInvocation::new(
        std::env::temp_dir().join("zode-devices-no-such-tool"),
        ["--version"],
    )));
    assert!(
        matches!(result, Err(ToolFailure::NotFound(_))),
        "{result:?}"
    );
}

#[test]
fn a_bounded_read_refuses_a_file_over_the_limit() {
    use crate::host::HostSystem as _;

    let directory =
        std::env::temp_dir().join(format!("zode-devices-bounded-read-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp directory is writable");
    let file = directory.join("package.json");
    std::fs::write(&file, "0123456789").expect("temp file is writable");
    let host = LocalHost::new();

    assert!(host.is_dir(&directory));
    assert!(!host.is_dir(&file));
    assert_eq!(
        host.read_to_string_at_most(&file, 10).expect("fits"),
        "0123456789"
    );
    let error = host
        .read_to_string_at_most(&file, 9)
        .expect_err("one byte too many");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

    std::fs::remove_dir_all(&directory).expect("temp directory is removable");
}

#[cfg(unix)]
#[test]
fn a_detached_tool_reports_its_exit_code() {
    let directory =
        std::env::temp_dir().join(format!("zode-devices-detached-exit-{}", std::process::id()));
    let log_file = directory.join("tool.log");
    let exit = LocalHost::new()
        .spawn_detached(
            ToolInvocation::new("/bin/sh", ["-c", "echo started; exit 3"]),
            &log_file,
        )
        .expect("sh starts")
        .exit;
    assert_eq!(smol::block_on(exit), Ok(Some(3)));
    assert_eq!(
        std::fs::read_to_string(&log_file).expect("the log was written"),
        "started\n"
    );
    std::fs::remove_dir_all(&directory).expect("temp directory is removable");
}

#[cfg(unix)]
#[test]
fn a_log_that_grew_past_the_limit_is_emptied_before_it_is_written_again() {
    use crate::detached_spawn::MAX_REUSED_LOG_BYTES;

    let directory =
        std::env::temp_dir().join(format!("zode-devices-log-limit-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("temp directory is writable");
    let reused = directory.join("big.log");
    // Sparse: it takes no disk, only a length.
    std::fs::File::create(&reused)
        .and_then(|file| file.set_len(MAX_REUSED_LOG_BYTES + 1))
        .expect("temp file is writable");
    let small = directory.join("small.log");
    std::fs::write(&small, "earlier run\n").expect("temp file is writable");

    for log_file in [&reused, &small] {
        let exit = LocalHost::new()
            .spawn_detached(
                ToolInvocation::new("/bin/sh", ["-c", "echo started"]),
                log_file,
            )
            .expect("sh starts")
            .exit;
        assert_eq!(smol::block_on(exit), Ok(Some(0)));
    }
    assert_eq!(
        std::fs::read_to_string(&reused).expect("the log was written"),
        "started\n",
        "over the limit: emptied first"
    );
    assert_eq!(
        std::fs::read_to_string(&small).expect("the log was written"),
        "earlier run\nstarted\n",
        "under the limit: appended to"
    );
    std::fs::remove_dir_all(&directory).expect("temp directory is removable");
}

#[cfg(unix)]
#[test]
fn a_detached_tool_can_be_stopped_by_its_process_group() {
    use crate::actions::stop_detached_emulator;

    let directory =
        std::env::temp_dir().join(format!("zode-devices-detached-kill-{}", std::process::id()));
    let log_file = directory.join("tool.log");
    let host = LocalHost::new();
    let started = host
        .spawn_detached(
            // The shell forks a child, as the emulator launcher does.
            ToolInvocation::new("/bin/sh", ["-c", "sleep 30 & wait"]),
            &log_file,
        )
        .expect("sh starts");
    let mut exit = started.exit;
    assert_eq!(exit.try_recv(), Ok(None), "still running");

    smol::block_on(stop_detached_emulator(
        &host,
        &host,
        started.process_id,
        &mut exit,
    ))
    .expect("stops");
    assert_eq!(
        smol::block_on(exit),
        Ok(None),
        "ended by a signal, not an exit code"
    );
    std::fs::remove_dir_all(&directory).expect("temp directory is removable");
}

#[cfg(unix)]
#[test]
fn a_process_group_that_is_already_gone_counts_as_stopped() {
    let directory =
        std::env::temp_dir().join(format!("zode-devices-gone-group-{}", std::process::id()));
    let host = LocalHost::new();
    let started = host
        .spawn_detached(
            ToolInvocation::new("/bin/sh", ["-c", "exit 0"]),
            &directory.join("tool.log"),
        )
        .expect("sh starts");
    assert_eq!(smol::block_on(started.exit), Ok(Some(0)));
    host.terminate_process_group(started.process_id)
        .expect("no such process is success");
    std::fs::remove_dir_all(&directory).expect("temp directory is removable");
}

#[cfg(unix)]
#[test]
fn ids_that_would_signal_zode_itself_or_everything_are_refused() {
    let host = LocalHost::new();
    // kill(-0) hits Zode's own group; kill(-1) hits every process it may signal.
    for process_id in [0, 1, u32::MAX] {
        assert!(
            host.terminate_process_group(process_id).is_err(),
            "{process_id}"
        );
    }
}
