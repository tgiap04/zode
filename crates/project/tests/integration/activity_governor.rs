use fs::FakeFs;
use gpui::TestAppContext;
use project::{Project, ProjectActivity};
use serde_json::json;
use task::SpawnInTerminal;
use terminal::TaskStatus;

use crate::init_test;

/// Phase 2 of the multi-project-window-switching plan (`ProjectActivity`
/// governor) — the single most important test in that phase, carried over
/// explicitly from Phase 5 so it can't be lost if Phase 5 ends in a no-op:
/// a background project's terminal must never have its process stopped,
/// suspended, or killed by an activity transition, and output from that
/// process must still arrive after the project wakes.
///
/// `Project::set_activity` is exercised directly here rather than through
/// `MultiWorkspace`'s timer-driven governor (tested in the `workspace`
/// crate): the invariant is a property of `set_activity` itself — it only
/// ever touches the `activity` field and emits an event, with no code path
/// that reaches `self.terminals` at all — so it holds identically no matter
/// who calls it or why. A real terminal (a real spawned process, not a
/// fake/mocked one) is what actually proves this; mirrors the existing
/// real-process pattern already used for `wait_for_completed_task` in
/// `crates/terminal/src/terminal.rs`'s own test suite.
#[cfg(unix)]
#[gpui::test]
async fn test_activity_transitions_never_disturb_a_running_terminal_process(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    // Awaiting the real process's completion below parks the test thread
    // until the background PTY-reader thread delivers its exit status.
    cx.executor().allow_parking();

    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "file.txt": "" })).await;
    let project = Project::test(fs, ["/root".as_ref()], cx).await;

    // `sleep 1` needs no shell quoting/escaping (a single command, a single
    // numeric argument) and is available unmodified on every Unix `sleep`,
    // GNU or BSD alike — the same reason the one other real-terminal test
    // in this repo restricts itself to simple argv, not a compound shell
    // string.
    let spawn_task = SpawnInTerminal {
        command: Some("sleep".into()),
        args: vec!["1".into()],
        ..SpawnInTerminal::default()
    };
    let terminal = project
        .update(cx, |project, cx| project.create_terminal_task(spawn_task, cx))
        .await
        .expect("spawning `sleep 1` in a real terminal should succeed");

    terminal.read_with(cx, |terminal, _cx| {
        assert_eq!(
            terminal.task().map(|task| task.status),
            Some(TaskStatus::Running),
            "the process should still be running immediately after spawning"
        );
    });

    // Drive exactly the sequence `MultiWorkspace`'s governor drives, but
    // directly against `Project::set_activity`, checked synchronously with
    // no time elapsed in between — proving the transitions themselves have
    // zero effect on the terminal, not merely that the process hadn't
    // happened to die yet.
    project.update(cx, |project, cx| {
        project.set_activity(ProjectActivity::Warm, cx);
    });
    terminal.read_with(cx, |terminal, _cx| {
        assert_eq!(
            terminal.task().map(|task| task.status),
            Some(TaskStatus::Running),
            "going Warm must not touch the terminal's process"
        );
    });

    project.update(cx, |project, cx| {
        project.set_activity(ProjectActivity::Hibernated, cx);
    });
    terminal.read_with(cx, |terminal, _cx| {
        assert_eq!(
            terminal.task().map(|task| task.status),
            Some(TaskStatus::Running),
            "hibernating must not stop, suspend, or kill the terminal's process"
        );
    });

    // Simulate the project waking back up (its workspace being re-activated).
    project.update(cx, |project, cx| {
        project.set_activity(ProjectActivity::Active, cx);
    });

    // Let the real `sleep 1` actually finish. Its exit status is the most
    // fundamental form of "output" a spawned process can produce — this
    // must still arrive, and arrive clean (not killed), after the wake.
    let exit_status = terminal
        .read_with(cx, |terminal, cx| terminal.wait_for_completed_task(cx))
        .await;
    assert!(
        exit_status.is_some_and(|status| status.success()),
        "the process must run to a normal completion, proving no transition ever killed it"
    );
}

/// FR6: an `Active` project can never be coerced directly to `Hibernated` —
/// not just by `MultiWorkspace` (which never asks for that), but by any
/// caller, including a future one (e.g. a memory-pressure fuse). The only
/// valid path is `Active` -> `Warm` -> `Hibernated`.
#[gpui::test]
async fn test_active_project_cannot_be_coerced_directly_to_hibernated(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "file.txt": "" })).await;
    let project = Project::test(fs, ["/root".as_ref()], cx).await;

    project.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Active,
            "a freshly constructed project starts Active"
        );
    });

    project.update(cx, |project, cx| {
        project.set_activity(ProjectActivity::Hibernated, cx);
    });
    project.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Active,
            "a direct Active -> Hibernated request must be ignored"
        );
    });

    // The guard is specific to that one illegal edge, not a freeze: the
    // normal Active -> Warm -> Hibernated path still works.
    project.update(cx, |project, cx| {
        project.set_activity(ProjectActivity::Warm, cx);
        project.set_activity(ProjectActivity::Hibernated, cx);
    });
    project.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Hibernated,
            "Active -> Warm -> Hibernated must still work normally"
        );
    });
}

/// Symmetric to the guard above: `Hibernated -> Warm` is equally off the
/// state diagram — the diagram's only edge out of `Hibernated` goes back to
/// `Active` via `activate()`. Unreachable through any wired caller today
/// (nothing calls `set_activity(Warm)` on a project that's currently
/// `Hibernated`), but guarded anyway for the same reason FR6's guard exists:
/// this state machine is what Phase 3/4/5 all hang their own resource logic
/// off of, so the invariant is structural rather than a convention every
/// future caller has to remember.
#[gpui::test]
async fn test_hibernated_project_cannot_be_coerced_directly_to_warm(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree("/root", json!({ "file.txt": "" })).await;
    let project = Project::test(fs, ["/root".as_ref()], cx).await;

    project.update(cx, |project, cx| {
        project.set_activity(ProjectActivity::Warm, cx);
        project.set_activity(ProjectActivity::Hibernated, cx);
    });
    project.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Hibernated,
            "setup should reach Hibernated via the normal Active -> Warm -> Hibernated path"
        );
    });

    project.update(cx, |project, cx| {
        project.set_activity(ProjectActivity::Warm, cx);
    });
    project.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Hibernated,
            "a direct Hibernated -> Warm request must be ignored"
        );
    });

    // The guard is specific to that one illegal edge, not a freeze: the
    // normal Hibernated -> Active path still works.
    project.update(cx, |project, cx| {
        project.set_activity(ProjectActivity::Active, cx);
    });
    project.read_with(cx, |project, _cx| {
        assert_eq!(
            project.activity(),
            ProjectActivity::Active,
            "Hibernated -> Active must still work normally"
        );
    });
}
