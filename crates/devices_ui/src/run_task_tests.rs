use std::path::PathBuf;

use devices::{HostOs, RunPlan, RunStep};
use task::{HideStrategy, RevealStrategy, Shell, ShellKind, SpawnInTerminal};
use terminal_view::terminal_panel::prepare_task_for_spawn;

use crate::run_task::spawn_in_terminal;

pub(crate) const ALL_SHELLS: [ShellKind; 7] = [
    ShellKind::Posix,
    ShellKind::Fish,
    ShellKind::PowerShell,
    ShellKind::Pwsh,
    ShellKind::Cmd,
    ShellKind::Nushell,
    ShellKind::Elvish,
];

pub(crate) fn step(program: &str, args: &[&str]) -> RunStep {
    RunStep {
        program: program.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
    }
}

pub(crate) fn plan(steps: Vec<RunStep>) -> RunPlan {
    RunPlan {
        label: "Run demo on iPhone A".to_string(),
        steps,
        env: vec![("FLUTTER_SUPPRESS_ANALYTICS".to_string(), "true".to_string())],
        cwd: PathBuf::from("/work/demo"),
        confirmation: None,
        exclusive_per_project: false,
        needs_metro: false,
        open_installed_android_app: None,
    }
}

fn flutter(device_id: &str) -> RunPlan {
    plan(vec![step("flutter", &["run", "-d", device_id])])
}

/// What the terminal panel would hand to the shell, as `spawn_task` builds it.
pub(crate) fn shell_command(
    task: &SpawnInTerminal,
    shell_program: &str,
    is_windows: bool,
) -> String {
    let prepared =
        prepare_task_for_spawn(task, &Shell::Program(shell_program.to_string()), is_windows);
    prepared.args.last().cloned().unwrap_or_default()
}

#[test]
fn a_run_is_its_own_new_terminal_that_never_waits_for_an_older_one() {
    let task = spawn_in_terminal(&flutter("ABC-1"), ShellKind::Posix, HostOs::MacOs, 1)
        .expect("a safe plan");
    assert!(task.use_new_terminal);
    assert!(
        task.allow_concurrent_runs,
        "an older run with the same label must not queue this one"
    );
    assert_eq!(task.reveal, RevealStrategy::Always);
    assert_eq!(task.hide, HideStrategy::Never);
    assert!(task.show_command);
    assert!(task.show_rerun);
    assert_eq!(task.shell, Shell::System);
    assert_eq!(task.cwd, Some(PathBuf::from("/work/demo")));
    assert_eq!(
        task.env
            .get("FLUTTER_SUPPRESS_ANALYTICS")
            .map(String::as_str),
        Some("true")
    );
    assert_eq!(task.command.as_deref(), Some("flutter"));
    assert_eq!(task.args, ["run", "-d", "ABC-1"]);
}

#[test]
fn every_run_gets_its_own_id_and_label() {
    let first = spawn_in_terminal(&flutter("ABC-1"), ShellKind::Posix, HostOs::MacOs, 1)
        .expect("a safe plan");
    let second = spawn_in_terminal(&flutter("ABC-1"), ShellKind::Posix, HostOs::MacOs, 2)
        .expect("a safe plan");
    assert_ne!(first.id, second.id);
    assert_ne!(first.full_label, second.full_label);
    assert!(first.full_label.starts_with("Run demo on iPhone A"));
    assert_eq!(first.label, first.full_label);
}

#[test]
fn an_argument_with_a_space_reaches_each_shell_as_one_word() {
    let task = spawn_in_terminal(&flutter("My Phone"), ShellKind::Posix, HostOs::MacOs, 1)
        .expect("quotable");
    assert_eq!(
        shell_command(&task, "/bin/zsh", false),
        "flutter run -d 'My Phone'"
    );

    for (shell_program, kind) in [
        ("powershell", ShellKind::PowerShell),
        ("pwsh", ShellKind::Pwsh),
        ("cmd", ShellKind::Cmd),
    ] {
        let task =
            spawn_in_terminal(&flutter("My Phone"), kind, HostOs::MacOs, 1).expect("quotable");
        let quoted = kind.try_quote("My Phone").expect("quotable").into_owned();
        assert_ne!(quoted, "My Phone", "{kind:?} must quote a space");
        let line = format!("flutter run -d {quoted}");
        // cmd's `/S /C` takes the whole line in one more pair of quotes.
        let expected = if kind == ShellKind::Cmd {
            format!("\"{line}\"")
        } else {
            line
        };
        assert_eq!(
            shell_command(&task, shell_program, true),
            expected,
            "{kind:?}: the shell must see the argument exactly as quoted"
        );
    }
}

#[test]
fn an_argument_a_shell_would_rewrite_is_refused_in_every_shell() {
    for value in ["$HOME", "${HOME}", "%PATH%", "a`whoami`", "id$(x)"] {
        for kind in ALL_SHELLS {
            let refused = spawn_in_terminal(&flutter(value), kind, HostOs::MacOs, 1);
            let error = refused.expect_err(&format!("{value:?} must be refused for {kind:?}"));
            assert_eq!(error.argument, value);
        }
    }
}

#[test]
fn an_argument_the_shell_cannot_quote_is_refused() {
    let refused = spawn_in_terminal(&flutter("nul\0byte"), ShellKind::Posix, HostOs::MacOs, 1);
    assert!(refused.is_err(), "shlex cannot quote a NUL byte");
}

#[test]
fn a_program_that_is_not_a_plain_name_is_refused() {
    for program in ["flutter; rm -rf ~", "./flutter", "flu tter", "$FLUTTER", ""] {
        let refused = spawn_in_terminal(
            &plan(vec![step(program, &["run"])]),
            ShellKind::Posix,
            HostOs::MacOs,
            1,
        );
        assert!(refused.is_err(), "{program:?} must be refused");
    }
}

#[test]
fn several_steps_run_one_after_another_only_while_each_succeeds() {
    let task = spawn_in_terminal(
        &plan(vec![
            step("flutter", &["build", "ios"]),
            step("xcrun", &["simctl", "install", "My Phone"]),
        ]),
        ShellKind::Elvish,
        HostOs::MacOs,
        1,
    )
    .expect("a safe plan");
    // Elvish would chain with `;`; the steps run in /bin/sh instead.
    assert_eq!(task.shell, Shell::Program("/bin/sh".to_string()));
    assert_eq!(
        shell_command(&task, "/bin/sh", false),
        "flutter build ios && xcrun simctl install 'My Phone'"
    );
}
