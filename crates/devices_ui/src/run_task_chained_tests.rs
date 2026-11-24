use std::path::{Path, PathBuf};

use devices::{HostOs, OpenInstalledAndroidApp, RunPlan};
use task::{Shell, ShellKind};

use crate::run_task::spawn_in_terminal;
use crate::run_task_tests::{ALL_SHELLS, plan, shell_command, step};

const UDID: &str = "00000000-0000-4000-8000-000000000005";
const METADATA: &str = "app/build/outputs/apk/debug/output-metadata.json";

fn gradle(host_os: HostOs, adb: &str) -> RunPlan {
    let (wrapper, metadata) = match host_os {
        HostOs::Windows => (".\\gradlew.bat", METADATA.replace('/', "\\")),
        HostOs::MacOs | HostOs::Linux => ("./gradlew", METADATA.to_string()),
    };
    let mut plan = plan(vec![step(wrapper, &[":app:installDebug"])]);
    plan.env = vec![("ANDROID_SERIAL".to_string(), "emulator-5556".to_string())];
    plan.open_installed_android_app = Some(OpenInstalledAndroidApp {
        adb: PathBuf::from(adb),
        serial: "emulator-5556".to_string(),
        output_metadata: metadata,
    });
    plan
}

fn xcode(app: &str) -> RunPlan {
    plan(vec![
        step(
            "/usr/bin/xcodebuild",
            &[
                "-workspace",
                "My App.xcworkspace",
                "-scheme",
                "My App",
                "build",
            ],
        ),
        step("/usr/bin/xcrun", &["simctl", "install", UDID, app]),
        step(
            "/usr/bin/xcrun",
            &[
                "simctl",
                "launch",
                "--terminate-running-process",
                UDID,
                "dev.example.app",
            ],
        ),
    ])
}

#[test]
fn chained_runs_use_sh_or_cmd_whatever_the_users_shell() {
    for user_shell in ALL_SHELLS {
        let task = spawn_in_terminal(&xcode("/dd/My App.app"), user_shell, HostOs::MacOs, 1)
            .expect("a safe plan");
        assert_eq!(
            task.shell,
            Shell::Program("/bin/sh".to_string()),
            "{user_shell:?}"
        );
        assert_eq!(
            shell_command(&task, "/bin/sh", false),
            format!(
                "/usr/bin/xcodebuild -workspace 'My App.xcworkspace' -scheme 'My App' build \
                 && /usr/bin/xcrun simctl install {UDID} '/dd/My App.app' \
                 && /usr/bin/xcrun simctl launch --terminate-running-process {UDID} dev.example.app"
            ),
            "{user_shell:?}"
        );

        let task = spawn_in_terminal(
            &gradle(HostOs::Windows, "C:\\Sdk\\adb.exe"),
            user_shell,
            HostOs::Windows,
            1,
        )
        .expect("a safe plan");
        assert!(
            matches!(&task.shell, Shell::Program(program) if program.ends_with("\\cmd.exe")),
            "{user_shell:?}: {:?}",
            task.shell
        );
        assert_eq!(task.command.as_deref(), Some(".\\gradlew.bat"));
        assert_eq!(
            task.env.get("ANDROID_SERIAL").map(String::as_str),
            Some("emulator-5556")
        );
    }
}

#[test]
fn single_step_runs_keep_the_users_shell() {
    let task = spawn_in_terminal(
        &plan(vec![step("flutter", &["run"])]),
        ShellKind::PowerShell,
        HostOs::Windows,
        1,
    )
    .expect("a safe plan");
    assert_eq!(task.shell, Shell::System);
}

#[test]
fn cmd_installs_then_opens_the_id_read_from_the_metadata() {
    let plan = gradle(HostOs::Windows, "C:\\Program Files (x86)\\Sdk\\adb.exe");
    let task =
        spawn_in_terminal(&plan, ShellKind::PowerShell, HostOs::Windows, 1).expect("a safe plan");
    let line = shell_command(&task, "cmd.exe", true);
    let metadata = METADATA.replace('/', "\\");
    assert!(
        line.starts_with("\".\\gradlew.bat :app:installDebug && ("),
        "{line}"
    );
    assert!(
        line.contains("findstr /r /b /c:\" *\\\"applicationId"),
        "{line}"
    );
    assert!(line.contains(&metadata), "{line}");
    assert!(
        line.contains(
            "do \"C:\\Program Files (x86)\\Sdk\\adb.exe\" -s emulator-5556 shell monkey -p %~i \
             -c android.intent.category.LAUNCHER 1"
        ),
        "{line}"
    );
    assert!(line.contains("|| echo Installed."), "{line}");
}

#[test]
fn a_program_path_a_shell_would_rewrite_is_refused() {
    for adb in [
        "/sdk/$X/adb",
        "/sdk/%X%/adb",
        "/sdk/`id`/adb",
        "/sdk/a\"b/adb",
    ] {
        for host_os in [HostOs::MacOs, HostOs::Windows] {
            let refused = spawn_in_terminal(&gradle(host_os, adb), ShellKind::Posix, host_os, 1);
            assert!(refused.is_err(), "{adb} on {host_os:?}");
        }
    }
    for program in ["gradlew; rm", "../gradlew", "./gradle wrapper", "-gradlew"] {
        let refused = spawn_in_terminal(
            &plan(vec![step(program, &[]), step("true", &[])]),
            ShellKind::Posix,
            HostOs::MacOs,
            1,
        );
        assert!(refused.is_err(), "{program:?}");
    }
}

/// Runs the composed line in a real /bin/sh in `directory`, as the terminal
/// would, and returns its exit code and output.
#[cfg(unix)]
fn run_in_sh(plan: &RunPlan, directory: &Path) -> (i32, String) {
    let task = spawn_in_terminal(plan, ShellKind::Posix, HostOs::MacOs, 1).expect("a safe plan");
    let line = shell_command(&task, "/bin/sh", false);
    let output = smol::block_on(
        smol::process::Command::new("/bin/sh")
            .args(["-c", &line])
            .current_dir(directory)
            .envs(task.env.iter())
            .output(),
    )
    .expect("sh runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), text)
}

#[cfg(unix)]
fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("script written");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("executable");
}

/// A project whose wrapper exits with `gradle_exit`, an adb that prints its
/// arguments, and the build's metadata when given.
#[cfg(unix)]
fn sh_project(gradle_exit: i32, metadata: Option<&str>) -> (tempfile::TempDir, RunPlan) {
    let directory = tempfile::tempdir().expect("temp dir");
    write_script(
        &directory.path().join("gradlew"),
        &format!("echo \"gradle $* on $ANDROID_SERIAL\"; exit {gradle_exit}"),
    );
    let adb = directory.path().join("fake adb");
    write_script(&adb, "echo \"adb $*\"");
    if let Some(metadata) = metadata {
        let path = directory.path().join(METADATA);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, metadata).expect("metadata written");
    }
    let mut plan = gradle(HostOs::MacOs, &adb.to_string_lossy());
    plan.cwd = directory.path().to_path_buf();
    (directory, plan)
}

#[cfg(unix)]
fn recorded_metadata() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../devices/src/fixtures/gradle-output-metadata-debug.json");
    std::fs::read_to_string(path).expect("fixture")
}

#[cfg(unix)]
#[test]
fn sh_opens_the_suffixed_id_the_build_installed() {
    let (directory, plan) = sh_project(0, Some(&recorded_metadata()));
    let (exit_code, output) = run_in_sh(&plan, directory.path());
    assert_eq!(exit_code, 0, "{output}");
    assert!(
        output.contains("gradle :app:installDebug on emulator-5556"),
        "{output}"
    );
    assert!(
        output.contains(
            "adb -s emulator-5556 shell monkey -p dev.zodeverify.native1.debug -c \
             android.intent.category.LAUNCHER 1"
        ),
        "{output}"
    );
}

#[cfg(unix)]
#[test]
fn sh_stops_after_a_failed_build() {
    let (directory, plan) = sh_project(1, Some(&recorded_metadata()));
    let (exit_code, output) = run_in_sh(&plan, directory.path());
    assert_ne!(exit_code, 0);
    assert!(!output.contains("adb"), "{output}");
    assert!(!output.contains("Installed"), "{output}");

    let failing_build =
        crate::run_task_tests::plan(vec![step("false", &[]), step("echo", &["installed"])]);
    let (exit_code, output) = run_in_sh(&failing_build, directory.path());
    assert_ne!(exit_code, 0);
    assert!(!output.contains("installed"), "{output}");
}

#[cfg(unix)]
#[test]
fn sh_says_installed_when_the_id_cannot_be_read() {
    for metadata in [
        None,
        Some("{\n  \"applicationId\": \"x;touch pwned\",\n}\n"),
    ] {
        let (directory, plan) = sh_project(0, metadata);
        let (exit_code, output) = run_in_sh(&plan, directory.path());
        assert_eq!(exit_code, 0, "{output}");
        assert!(!output.contains("adb"), "{metadata:?}: {output}");
        assert!(output.contains("Installed."), "{metadata:?}: {output}");
        assert!(!directory.path().join("pwned").exists());
    }
}

#[test]
fn cmd_always_quotes_an_absolute_program_and_refuses_cmd_metacharacters() {
    let task = spawn_in_terminal(
        &gradle(HostOs::Windows, "C:\\R&D\\sdk\\platform-tools\\adb.exe"),
        ShellKind::PowerShell,
        HostOs::Windows,
        1,
    )
    .expect("a safe plan");
    let line = shell_command(&task, "cmd.exe", true);
    assert!(
        line.contains("\"C:\\R&D\\sdk\\platform-tools\\adb.exe\" -s emulator-5556"),
        "{line}"
    );
    assert!(!line.contains(" C:\\R&D"), "{line}");

    let refused = spawn_in_terminal(
        &gradle(HostOs::Windows, "C:\\sdk^x\\adb.exe"),
        ShellKind::PowerShell,
        HostOs::Windows,
        1,
    );
    assert!(refused.is_err());
}

#[test]
fn cmd_opens_only_an_application_id_of_safe_characters() {
    let task = spawn_in_terminal(
        &gradle(HostOs::Windows, "C:\\Sdk\\adb.exe"),
        ShellKind::PowerShell,
        HostOs::Windows,
        1,
    )
    .expect("a safe plan");
    let line = shell_command(&task, "cmd.exe", true);
    let id_character = "[abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.]";
    let strict = format!(
        "findstr /r /b /c:\" *\\\"applicationId\\\": \\\"{id_character}{id_character}*\\\",\" "
    );
    assert_eq!(line.matches(&strict).count(), 2, "{line}");
    assert!(!line.contains("%~i|"), "nothing is piped: {line}");
}

#[test]
fn the_chain_runs_in_the_systems_own_cmd_by_absolute_path() {
    use crate::run_task::windows_command_interpreter;

    assert_eq!(
        windows_command_interpreter(Some("C:\\Windows\\system32\\cmd.exe"), Some("C:\\Windows")),
        "C:\\Windows\\system32\\cmd.exe"
    );
    for planted in [Some("cmd.exe"), Some("C:\\work\\repo\\cmd.bat"), None] {
        assert_eq!(
            windows_command_interpreter(planted, Some("D:\\Windows")),
            "D:\\Windows\\System32\\cmd.exe",
            "{planted:?}"
        );
    }
    assert_eq!(
        windows_command_interpreter(None, None),
        "C:\\Windows\\System32\\cmd.exe"
    );
    // The terminal tells cmd from PowerShell by the file stem, case-sensitively.
    assert_eq!(
        windows_command_interpreter(Some("C:\\WINDOWS\\SYSTEM32\\CMD.EXE"), None),
        "C:\\WINDOWS\\SYSTEM32\\cmd.exe"
    );
    let task = spawn_in_terminal(
        &gradle(HostOs::Windows, "C:\\Sdk\\adb.exe"),
        ShellKind::Cmd,
        HostOs::Windows,
        1,
    )
    .expect("a safe plan");
    let Shell::Program(program) = &task.shell else {
        panic!("a chained run names its shell: {:?}", task.shell);
    };
    assert!(
        program.ends_with("\\cmd.exe") && program.get(1..3) == Some(":\\"),
        "{program}"
    );
}

#[test]
fn an_sdk_set_with_forward_slashes_on_windows_still_runs() {
    let host = devices::FakeHost::new(HostOs::Windows);
    host.set_environment_variable("ANDROID_HOME", "C:/Android/sdk");
    host.add_file("C:/Android/sdk/platform-tools/adb.exe", "");
    let sdk = devices::locate_android_sdk(&host).expect("the SDK is found");
    let task = spawn_in_terminal(
        &gradle(HostOs::Windows, &sdk.adb.to_string_lossy()),
        ShellKind::PowerShell,
        HostOs::Windows,
        1,
    )
    .expect("a forward-slash drive path is absolute");
    let line = shell_command(&task, "cmd.exe", true);
    assert!(
        line.contains("\"C:\\Android\\sdk\\platform-tools\\adb.exe\" -s emulator-5556"),
        "{line}"
    );
}

/// Runs the composed line in the system's cmd.exe in `directory`, with the
/// command line the terminal builds for cmd: program and arguments joined by
/// spaces, unescaped (`ShellKind::tty_escape_args` is false for cmd).
#[cfg(windows)]
fn run_in_cmd(plan: &RunPlan, directory: &Path) -> (i32, String) {
    use smol::process::windows::CommandExt as _;
    use terminal_view::terminal_panel::prepare_task_for_spawn;

    let task =
        spawn_in_terminal(plan, ShellKind::PowerShell, HostOs::Windows, 1).expect("a safe plan");
    let prepared = prepare_task_for_spawn(&task, &task.shell, true);
    let program = prepared.command.clone().expect("a program");
    let output = smol::block_on(
        smol::process::Command::new(&program)
            .raw_arg(prepared.args.join(" "))
            .current_dir(directory)
            .envs(task.env.iter())
            .output(),
    )
    .expect("cmd runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), text)
}

/// A project whose `gradlew.bat` exits with `gradle_exit`, an adb in a
/// folder whose name cmd would split if it were not quoted, and the build's
/// metadata when given. The adb is a batch file: cmd runs it like an `.exe`.
#[cfg(windows)]
fn cmd_project(gradle_exit: i32, metadata: Option<&str>) -> (tempfile::TempDir, RunPlan) {
    let directory = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        directory.path().join("gradlew.bat"),
        format!("@echo gradle %* on %ANDROID_SERIAL%\r\n@exit /b {gradle_exit}\r\n"),
    )
    .expect("wrapper written");
    let tools = directory.path().join("R&D (x86)").join("platform-tools");
    std::fs::create_dir_all(&tools).expect("tools folder");
    let adb = tools.join("adb.bat");
    std::fs::write(&adb, "@echo adb %*\r\n").expect("adb written");
    if let Some(metadata) = metadata {
        let path = directory.path().join(METADATA.replace('/', "\\"));
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, metadata).expect("metadata written");
    }
    let mut plan = gradle(HostOs::Windows, &adb.to_string_lossy());
    plan.cwd = directory.path().to_path_buf();
    (directory, plan)
}

#[cfg(windows)]
#[test]
fn cmd_installs_then_opens_the_suffixed_id_the_build_installed() {
    let metadata = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../devices/src/fixtures/gradle-output-metadata-debug.json"),
    )
    .expect("fixture");
    let (directory, plan) = cmd_project(0, Some(&metadata));
    let (exit_code, output) = run_in_cmd(&plan, directory.path());
    assert_eq!(exit_code, 0, "{output}");
    assert!(
        output.contains("gradle :app:installDebug on emulator-5556"),
        "{output}"
    );
    assert!(
        output.contains(
            "adb -s emulator-5556 shell monkey -p dev.zodeverify.native1.debug -c \
             android.intent.category.LAUNCHER 1"
        ),
        "{output}"
    );
}

/// The id line as AGP writes it, with LF line endings as it does on every
/// system, and with CRLF as an editor or checkout might leave it.
#[cfg(windows)]
#[test]
fn cmd_opens_the_id_whatever_the_line_endings() {
    for line_ending in ["\n", "\r\n"] {
        let metadata = [
            "{",
            "  \"version\": 3,",
            "  \"applicationId\": \"dev.example.app\",",
            "  \"variantName\": \"debug\"",
            "}",
            "",
        ]
        .join(line_ending);
        let (directory, plan) = cmd_project(0, Some(&metadata));
        let (exit_code, output) = run_in_cmd(&plan, directory.path());
        assert_eq!(exit_code, 0, "{line_ending:?}: {output}");
        assert!(
            output.contains("adb -s emulator-5556 shell monkey -p dev.example.app -c"),
            "{line_ending:?}: {output}"
        );
    }
}

#[cfg(windows)]
#[test]
fn cmd_stops_after_a_failed_build() {
    let (directory, plan) = cmd_project(1, None);
    let (exit_code, output) = run_in_cmd(&plan, directory.path());
    assert_ne!(exit_code, 0, "{output}");
    assert!(!output.contains("adb"), "{output}");
    assert!(!output.contains("Installed"), "{output}");
}

#[cfg(windows)]
#[test]
fn cmd_says_installed_when_the_id_is_missing_or_unsafe() {
    for metadata in [
        None,
        Some("{\r\n  x ;reboot; \"applicationId\": \"a\",\r\n}\r\n"),
        Some("{\r\n  \"applicationId\": \"a&echo pwned\",\r\n}\r\n"),
    ] {
        let (directory, plan) = cmd_project(0, metadata);
        let (exit_code, output) = run_in_cmd(&plan, directory.path());
        assert!(!output.contains("adb "), "{metadata:?}: {output}");
        assert!(!output.contains("pwned"), "{metadata:?}: {output}");
        assert!(output.contains("Installed."), "{metadata:?}: {output}");
        assert_eq!(exit_code, 0, "{metadata:?}: {output}");
    }
}
