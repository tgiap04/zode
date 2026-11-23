use std::time::Duration;

use crate::host::{ToolFailure, ToolInvocation, ToolRunner};
use crate::local_host::LocalHost;

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
        .expect("sh starts");
    assert_eq!(smol::block_on(exit), Ok(Some(3)));
    assert_eq!(
        std::fs::read_to_string(&log_file).expect("the log was written"),
        "started\n"
    );
    std::fs::remove_dir_all(&directory).expect("temp directory is removable");
}
