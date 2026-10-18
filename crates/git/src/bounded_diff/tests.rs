use std::{path::Path, process::Command, time::Duration};

use gpui::TestAppContext;
use util::rel_path::RelPath;

use super::*;
use crate::repository::{GitRepository as _, RealGitRepository, repo_path};

#[allow(
    clippy::disallowed_methods,
    reason = "a test setting up a throwaway repository, where blocking is harmless"
)]
fn run_git(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(arguments)
        .current_dir(directory)
        .env("GIT_CONFIG_GLOBAL", "")
        .env("GIT_CONFIG_SYSTEM", "")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {arguments:?}");
}

fn write(directory: &Path, path: &str, contents: &str) {
    let full = directory.join(path);
    std::fs::create_dir_all(full.parent().expect("a parent")).expect("a directory");
    std::fs::write(full, contents).expect("writes");
}

/// A repository with a commit holding `app/inside.txt`, `app/private.txt`,
/// `other/outside.txt` and `root.txt`, all with one line.
fn committed_repository() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("a directory");
    run_git(directory.path(), &["init", "-q", "-b", "main"]);
    for path in [
        "app/inside.txt",
        "app/private.txt",
        "other/outside.txt",
        "root.txt",
    ] {
        write(directory.path(), path, "before\n");
    }
    run_git(directory.path(), &["add", "."]);
    run_git(directory.path(), &["commit", "-q", "-m", "first"]);
    directory
}

fn repository(directory: &Path, cx: &TestAppContext) -> RealGitRepository {
    RealGitRepository::new(
        &directory.join(".git"),
        None,
        Some("git".into()),
        cx.executor(),
    )
    .expect("a repository")
}

fn request(
    cx: &TestAppContext,
    scope: &str,
    keep: impl Fn(&RepoPath) -> bool + Send + Sync + 'static,
) -> ScopedDiff {
    ScopedDiff {
        scope: if scope.is_empty() {
            RepoPath::from_rel_path(RelPath::empty())
        } else {
            repo_path(scope)
        },
        keep: Arc::new(keep),
        max_bytes: 2 * 1024 * 1024,
        deadline: Duration::from_secs(60),
        executor: cx.executor(),
    }
}

#[test]
fn a_cut_lands_on_a_line_end_and_on_a_character() {
    assert_eq!(
        cut_at_line_end("a\nb\n".into(), 10),
        ("a\nb\n".into(), false)
    );
    assert_eq!(cut_at_line_end("a\nbbbb\n".into(), 4), ("a\n".into(), true));
    assert_eq!(cut_at_line_end("ééé".into(), 3), ("é".into(), true));
}

#[test]
fn raw_bytes_are_cut_at_a_line_end_before_they_are_decoded() {
    assert_eq!(
        cut_bytes_at_line_end(b"a\nb\n".to_vec(), 10),
        (b"a\nb\n".to_vec(), false)
    );
    assert_eq!(
        cut_bytes_at_line_end(b"a\n\xe9\xe9\xe9\n".to_vec(), 5),
        (b"a\n".to_vec(), true)
    );
    assert_eq!(
        cut_bytes_at_line_end(b"abcdef".to_vec(), 4),
        (b"abcd".to_vec(), true),
        "a line longer than the limit is cut where the limit falls"
    );
}

#[test]
fn a_changed_path_that_is_not_text_is_left_out_and_the_rest_stay() {
    let listing = b"app/one.txt\0app/\xff\xfe.txt\0app/two.txt\0";
    let names: Vec<&str> = parse_changed_paths(listing)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, ["app/one.txt", "app/two.txt"]);
}

/// Latin-1 text is not valid UTF-8 and grows when decoded, which must not
/// carry the diff past its limit.
#[gpui::test]
async fn text_that_grows_when_decoded_still_fits_the_limit(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    let line = b"\xe9\xe9\xe9\xe9\xe9\xe9\xe9\xe9\n";
    let latin_one: Vec<u8> = line
        .iter()
        .copied()
        .cycle()
        .take(line.len() * 2000)
        .collect();
    std::fs::write(directory.path().join("root.txt"), latin_one).expect("writes");
    let repository = repository(directory.path(), cx);

    let mut limited = request(cx, "", |_| true);
    limited.max_bytes = 4_000;
    let cut = repository
        .diff_head_to_worktree_scoped(limited)
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(cut.truncated);
    assert!(cut.text.len() <= 4_000, "{} bytes", cut.text.len());
    assert!(cut.text.ends_with('\n'));
}

#[gpui::test]
async fn only_changes_under_the_scope_and_kept_by_the_filter_are_returned(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    for path in [
        "app/inside.txt",
        "app/private.txt",
        "other/outside.txt",
        "root.txt",
    ] {
        write(directory.path(), path, "after\n");
    }
    // A change that is only staged still counts.
    run_git(directory.path(), &["add", "app/inside.txt"]);
    let repository = repository(directory.path(), cx);

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "app", |path| {
            path.as_unix_str() != "app/private.txt"
        }))
        .await
        .expect("a diff")
        .expect("a commit to compare with");
    assert!(!scoped.truncated);
    assert!(
        scoped
            .text
            .contains("diff --git a/app/inside.txt b/app/inside.txt")
    );
    assert!(scoped.text.contains("-before\n+after\n"));
    for left_out in ["private.txt", "outside.txt", "root.txt"] {
        assert!(
            !scoped.text.contains(left_out),
            "{left_out}: {}",
            scoped.text
        );
    }

    let whole = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    for file in ["inside.txt", "private.txt", "outside.txt", "root.txt"] {
        assert!(whole.text.contains(file), "{file}");
    }
}

#[gpui::test]
async fn a_scope_that_shares_a_prefix_with_another_folder_does_not_leak_it(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    write(directory.path(), "app-extra/more.txt", "x\n");
    run_git(directory.path(), &["add", "."]);
    run_git(directory.path(), &["commit", "-q", "-m", "second"]);
    write(directory.path(), "app-extra/more.txt", "y\n");
    write(directory.path(), "app/inside.txt", "after\n");
    let repository = repository(directory.path(), cx);

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "app", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(scoped.text.contains("app/inside.txt"));
    assert!(!scoped.text.contains("app-extra"), "{}", scoped.text);
}

#[gpui::test]
async fn a_program_named_by_the_repository_is_not_run(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    let marker = directory.path().join("ran-a-program");
    let program = directory.path().join("program.sh");
    std::fs::write(
        &program,
        format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
    )
    .expect("writes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
            .expect("executable");
    }
    write(directory.path(), ".gitattributes", "*.txt diff=custom\n");
    run_git(
        directory.path(),
        &[
            "config",
            "diff.custom.command",
            program.to_str().expect("text"),
        ],
    );
    run_git(
        directory.path(),
        &[
            "config",
            "diff.custom.textconv",
            program.to_str().expect("text"),
        ],
    );
    write(directory.path(), "root.txt", "after\n");
    let repository = repository(directory.path(), cx);

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(scoped.text.contains("-before\n+after\n"), "{}", scoped.text);
    assert!(!marker.exists(), "a configured program ran");
}

#[gpui::test]
async fn a_diff_over_the_limit_is_cut_at_a_line_end_and_the_program_stopped(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    let big: String = (0..60_000)
        .map(|index| format!("line {index:08} of text\n"))
        .collect();
    write(directory.path(), "root.txt", &big);
    let repository = repository(directory.path(), cx);

    let mut limited = request(cx, "", |_| true);
    limited.max_bytes = 100_000;
    let cut = repository
        .diff_head_to_worktree_scoped(limited)
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(cut.truncated);
    assert!(cut.text.len() <= 100_000);
    assert!(cut.text.starts_with("diff --git"));
    assert!(cut.text.ends_with('\n'));
}

#[gpui::test]
async fn a_repository_without_a_commit_has_nothing_to_compare_with(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("a directory");
    run_git(directory.path(), &["init", "-q", "-b", "main"]);
    write(directory.path(), "a.txt", "a\n");
    let repository = repository(directory.path(), cx);

    let outcome = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await
        .expect("not an error");
    assert_eq!(outcome, None);
}

/// `app/data` was a file and is now a folder holding a private file. The
/// file's name, kept, is a pathspec that also names everything under it.
#[gpui::test]
async fn a_file_that_became_a_folder_does_not_carry_its_private_files_along(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    write(directory.path(), "app/data", "plain\n");
    run_git(directory.path(), &["add", "."]);
    run_git(directory.path(), &["commit", "-q", "-m", "a file"]);
    std::fs::remove_file(directory.path().join("app/data")).expect("removes");
    write(directory.path(), "app/data/.env", "SECRET=1\n");
    write(directory.path(), "app/data/readme", "hello\n");
    run_git(directory.path(), &["add", "-A"]);
    let repository = repository(directory.path(), cx);

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "app", |path| {
            !path.as_unix_str().ends_with(".env")
        }))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(!scoped.text.contains("SECRET"), "{}", scoped.text);
    assert!(!scoped.text.contains(".env"), "{}", scoped.text);
    assert!(
        scoped
            .text
            .contains("diff --git a/app/data/readme b/app/data/readme"),
        "{}",
        scoped.text
    );
    assert!(
        scoped.text.contains("diff --git a/app/data b/app/data"),
        "the file that went away is still shown: {}",
        scoped.text
    );
}

#[gpui::test]
async fn a_file_named_like_the_base_revision_does_not_break_the_diff(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    write(directory.path(), "HEAD", "before\n");
    run_git(directory.path(), &["add", "."]);
    run_git(directory.path(), &["commit", "-q", "-m", "odd name"]);
    write(directory.path(), "HEAD", "after\n");
    let repository = repository(directory.path(), cx);

    let whole = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(
        whole.text.contains("diff --git a/HEAD b/HEAD"),
        "{}",
        whole.text
    );
}

#[gpui::test]
async fn a_repository_that_cannot_be_read_is_an_error_and_not_an_empty_history(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    std::fs::write(directory.path().join(".git/HEAD"), "garbage\n").expect("writes");
    let repository = repository(directory.path(), cx);

    let outcome = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await;
    let error = outcome.expect_err("not the same as having no commit");
    assert!(
        format!("{error:#}").contains("not a git repository"),
        "git's own words are kept: {error:#}"
    );
}

#[test]
fn names_are_split_into_runs_that_fit_a_command_line_and_stay_in_order() {
    let long = "x".repeat(MAX_PATHSPEC_BYTES_PER_RUN / 2 - 1);
    let names: Vec<String> = vec![
        format!("a-{long}"),
        format!("b-{long}"),
        format!("c-{long}"),
        "d".into(),
    ];
    let runs = pathspec_runs(&names);
    assert!(runs.len() > 1, "{} runs", runs.len());
    for run in &runs {
        let bytes: usize = run.iter().map(|name| name.len() + 1).sum();
        assert!(bytes <= MAX_PATHSPEC_BYTES_PER_RUN, "{bytes} bytes");
    }
    let joined: Vec<&str> = runs.into_iter().flatten().collect();
    assert_eq!(joined, names.iter().map(String::as_str).collect::<Vec<_>>());

    assert!(pathspec_runs(&[]).is_empty());
    let huge = vec!["y".repeat(MAX_PATHSPEC_BYTES_PER_RUN * 2)];
    assert_eq!(
        pathspec_runs(&huge).len(),
        1,
        "a name too long for a run on its own still gets one"
    );
}

/// Whether a process of this id is in the process table at all, a zombie
/// included.
#[cfg(unix)]
#[allow(
    clippy::disallowed_methods,
    reason = "`ps` is a read-only query in a test, where blocking is harmless"
)]
fn is_in_the_process_table(pid: &str) -> bool {
    let listing = Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .expect("ps runs");
    !String::from_utf8_lossy(&listing.stdout).trim().is_empty()
}

/// A `clean` filter that never answers: the repository's own program, which
/// is still run, holds git for as long as it likes. Stopping git must stop it
/// as well, or it stays for as long as it sleeps.
#[cfg(unix)]
#[gpui::test]
async fn a_git_that_never_answers_is_stopped_when_the_deadline_passes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    let filter_pid = directory.path().join("filter-pid");
    write(directory.path(), ".gitattributes", "*.txt filter=slow\n");
    run_git(
        directory.path(),
        &[
            "config",
            "filter.slow.clean",
            &format!("echo $$ > '{}'; sleep 20; cat", filter_pid.display()),
        ],
    );
    write(directory.path(), "root.txt", "after\n");
    let repository = repository(directory.path(), cx);

    let mut impatient = request(cx, "", |_| true);
    impatient.deadline = Duration::from_secs(2);
    let started = std::time::Instant::now();
    let error = repository
        .diff_head_to_worktree_scoped(impatient)
        .await
        .expect_err("it ran out of time");
    assert!(format!("{error:#}").contains("took longer"), "{error:#}");
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "it was waited for: {:?}",
        started.elapsed()
    );

    let pid = std::fs::read_to_string(&filter_pid).expect("the filter ran");
    let give_up = std::time::Instant::now() + Duration::from_secs(10);
    while is_in_the_process_table(pid.trim()) {
        assert!(
            std::time::Instant::now() < give_up,
            "the filter (process {}) outlived the git that was waiting for it",
            pid.trim()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[gpui::test]
async fn a_file_git_does_not_track_yet_is_part_of_the_changes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    write(directory.path(), "app/new file.txt", "fresh\n");
    write(directory.path(), "app/inside.txt", "after\n");
    write(directory.path(), "other/new.txt", "elsewhere\n");
    let repository = repository(directory.path(), cx);

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "app", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(!scoped.truncated);
    assert!(
        scoped
            .text
            .contains("diff --git a/app/new file.txt b/app/new file.txt\nnew file mode"),
        "{}",
        scoped.text
    );
    assert!(scoped.text.contains("+fresh\n"), "{}", scoped.text);
    assert!(scoped.text.contains("diff --git a/app/inside.txt"));
    assert!(!scoped.text.contains("other/new.txt"), "{}", scoped.text);
}

#[gpui::test]
async fn a_new_file_the_filter_turns_down_or_git_ignores_is_not_shown(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    write(directory.path(), ".gitignore", "ignored.log\n");
    write(directory.path(), "app/.env", "SECRET=1\n");
    write(directory.path(), "app/ignored.log", "noise\n");
    write(directory.path(), "app/plain.txt", "plain\n");
    let repository = repository(directory.path(), cx);

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "app", |path| {
            !path.as_unix_str().ends_with(".env")
        }))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(scoped.text.contains("app/plain.txt"), "{}", scoped.text);
    assert!(!scoped.text.contains("SECRET"), "{}", scoped.text);
    assert!(!scoped.text.contains(".env"), "{}", scoped.text);
    assert!(!scoped.text.contains("ignored.log"), "{}", scoped.text);
}

#[gpui::test]
async fn only_so_many_new_files_are_shown_and_the_diff_says_it_left_some_out(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    for index in 0..MAX_UNTRACKED_FILES + 5 {
        write(directory.path(), &format!("gen/file-{index:04}.txt"), "x\n");
    }
    let repository = repository(directory.path(), cx);

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "gen", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(scoped.truncated);
    // At most the cap. A slow runner can also run out of the time new files get first, so the
    // exact count is not pinned; what matters is that the five over the cap never appear.
    let shown = scoped.text.matches("diff --git").count();
    assert!(
        (1..=MAX_UNTRACKED_FILES).contains(&shown),
        "{shown} new files shown"
    );
    assert!(scoped.text.contains("gen/file-0000.txt"));
    assert!(!scoped.text.contains("gen/file-0100.txt"));
}

#[gpui::test]
async fn new_files_share_the_byte_limit_with_the_rest_of_the_diff(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    let big: String = (0..20_000)
        .map(|index| format!("line {index:08} of text\n"))
        .collect();
    write(directory.path(), "gen/big.txt", &big);
    let repository = repository(directory.path(), cx);

    let mut limited = request(cx, "gen", |_| true);
    limited.max_bytes = 50_000;
    let cut = repository
        .diff_head_to_worktree_scoped(limited)
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(cut.truncated);
    assert!(cut.text.len() <= 50_000, "{} bytes", cut.text.len());
    assert!(cut.text.starts_with("diff --git a/gen/big.txt"));
    assert!(cut.text.ends_with('\n'));
}

#[gpui::test]
async fn a_new_file_whose_name_could_be_an_option_is_left_out_and_the_diff_says_so(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    write(directory.path(), "--output=written.txt", "x\n");
    write(directory.path(), "fine.txt", "x\n");
    let repository = repository(directory.path(), cx);

    let whole = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(whole.truncated);
    assert!(whole.text.contains("fine.txt"), "{}", whole.text);
    assert!(!whole.text.contains("written.txt"), "{}", whole.text);
    assert!(
        !directory.path().join("written.txt").exists(),
        "the name was taken as an option"
    );
}

/// Git says "could not access" with the same exit code as "they differ", and
/// prints nothing.
#[gpui::test]
async fn a_new_folder_that_is_a_repository_of_its_own_is_left_out_and_the_diff_says_so(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    write(directory.path(), "nested/n.txt", "n\n");
    let nested = directory.path().join("nested");
    run_git(&nested, &["init", "-q", "-b", "main"]);
    run_git(&nested, &["add", "."]);
    run_git(&nested, &["commit", "-q", "-m", "inner"]);
    write(directory.path(), "fine.txt", "x\n");
    let repository = repository(directory.path(), cx);

    let whole = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(whole.truncated, "something was left out: {}", whole.text);
    assert!(whole.text.contains("fine.txt"), "{}", whole.text);
    assert!(!whole.text.contains("nested"), "{}", whole.text);
}

#[cfg(unix)]
#[gpui::test]
async fn a_new_link_to_a_folder_is_left_out_and_the_diff_says_so(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    std::os::unix::fs::symlink("app", directory.path().join("linkdir")).expect("a link");
    write(directory.path(), "fine.txt", "x\n");
    let repository = repository(directory.path(), cx);

    let whole = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(whole.truncated, "something was left out: {}", whole.text);
    assert!(whole.text.contains("fine.txt"), "{}", whole.text);
    assert!(!whole.text.contains("linkdir"), "{}", whole.text);
}

/// With `diff.submodule = log` git writes a change to a submodule as a line of
/// text with no header, which would be read as part of the file before it.
#[gpui::test]
async fn a_submodule_change_keeps_its_header_whatever_the_users_settings_say(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    write(directory.path(), "sub/s.txt", "1\n");
    let sub = directory.path().join("sub");
    run_git(&sub, &["init", "-q", "-b", "main"]);
    run_git(&sub, &["add", "."]);
    run_git(&sub, &["commit", "-q", "-m", "first"]);
    run_git(directory.path(), &["add", "."]);
    run_git(
        directory.path(),
        &["commit", "-q", "-m", "with a submodule"],
    );
    write(directory.path(), "sub/s.txt", "2\n");
    run_git(&sub, &["commit", "-q", "-am", "second"]);
    write(directory.path(), "root.txt", "after\n");
    run_git(directory.path(), &["config", "diff.submodule", "log"]);
    let repository = repository(directory.path(), cx);

    let everything = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(
        everything.text.contains("diff --git a/sub b/sub"),
        "{}",
        everything.text
    );
    assert!(
        everything.text.contains("Subproject commit"),
        "{}",
        everything.text
    );
    assert!(
        !everything.text.contains("Submodule sub"),
        "{}",
        everything.text
    );

    let without_it = repository
        .diff_head_to_worktree_scoped(request(cx, "", |path| path.as_unix_str() != "sub"))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(without_it.text.contains("root.txt"), "{}", without_it.text);
    assert!(!without_it.text.contains("sub"), "{}", without_it.text);
}

/// Each new file is a run of git in the repository's queue, so they are given
/// only so much of the time.
#[gpui::test]
async fn new_files_stop_being_started_when_their_share_of_the_time_is_spent(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    for index in 0..30 {
        write(directory.path(), &format!("gen/file-{index:02}.txt"), "x\n");
    }
    let repository = repository(directory.path(), cx);
    // Every look at a file's name takes three seconds of the clock.
    let executor = cx.executor();
    let slow = move |_: &RepoPath| {
        executor.advance_clock(Duration::from_secs(3));
        true
    };

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "gen", slow))
        .await
        .expect("a diff")
        .expect("a commit");
    let shown = scoped.text.matches("diff --git").count();
    assert!(scoped.truncated);
    assert!((1..=5).contains(&shown), "{shown} files were shown");
}

#[gpui::test]
async fn a_new_file_named_like_the_base_revision_is_shown(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    write(directory.path(), "HEAD", "new\n");
    let repository = repository(directory.path(), cx);

    let whole = repository
        .diff_head_to_worktree_scoped(request(cx, "", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(
        whole
            .text
            .contains("diff --git a/HEAD b/HEAD\nnew file mode"),
        "{}",
        whole.text
    );
}

/// The headers are what the result is judged by, so a user's own setting
/// for how they read must not change them.
#[gpui::test]
async fn headers_keep_their_prefixes_whatever_the_users_settings_say(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    run_git(directory.path(), &["config", "diff.mnemonicPrefix", "true"]);
    run_git(directory.path(), &["config", "diff.noprefix", "true"]);
    write(directory.path(), "app/inside.txt", "after\n");
    write(directory.path(), "app/new.txt", "fresh\n");
    let repository = repository(directory.path(), cx);

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "app", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(
        scoped
            .text
            .contains("diff --git a/app/inside.txt b/app/inside.txt"),
        "{}",
        scoped.text
    );
    assert!(
        scoped
            .text
            .contains("diff --git a/app/new.txt b/app/new.txt"),
        "{}",
        scoped.text
    );
}

/// Git writes a name with characters that are hard to read back in quotes,
/// escaped, in the header of its patch.
#[gpui::test]
async fn a_path_with_characters_git_quotes_is_still_judged_by_its_name(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    // A quote cannot be in a file name on Windows.
    let mut names = vec!["app/café.txt"];
    if cfg!(unix) {
        names.push("app/quoted\"name.txt");
    }
    for name in &names {
        write(directory.path(), name, "before\n");
    }
    run_git(directory.path(), &["add", "."]);
    run_git(directory.path(), &["commit", "-q", "-m", "odd names"]);
    for name in &names {
        write(directory.path(), name, "after\n");
    }
    let repository = repository(directory.path(), cx);

    let everything = repository
        .diff_head_to_worktree_scoped(request(cx, "app", |_| true))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(everything.text.contains("+after\n"), "{}", everything.text);
    assert_eq!(everything.text.matches("diff --git").count(), names.len());

    let without_the_accent = repository
        .diff_head_to_worktree_scoped(request(cx, "app", |path| !path.as_unix_str().contains('é')))
        .await
        .expect("a diff")
        .expect("a commit");
    assert_eq!(
        without_the_accent.text.matches("diff --git").count(),
        names.len() - 1
    );
    assert!(!without_the_accent.text.contains('é'));
}

#[gpui::test]
async fn names_that_look_like_patterns_are_taken_literally(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = committed_repository();
    // `[a]b.txt` read as a glob would also match `ab.txt`. Both names are
    // valid on every platform, unlike `*` or `?`.
    write(directory.path(), "[a]b.txt", "bracket\n");
    write(directory.path(), "ab.txt", "plain\n");
    run_git(directory.path(), &["add", "."]);
    run_git(directory.path(), &["commit", "-q", "-m", "odd name"]);
    write(directory.path(), "[a]b.txt", "changed\n");
    write(directory.path(), "ab.txt", "changed\n");
    let repository = repository(directory.path(), cx);

    let scoped = repository
        .diff_head_to_worktree_scoped(request(cx, "", |path| path.as_unix_str() == "[a]b.txt"))
        .await
        .expect("a diff")
        .expect("a commit");
    assert!(scoped.text.contains("a/[a]b.txt"));
    assert!(
        !scoped.text.contains("a/ab.txt"),
        "the bracket name was taken as a pattern: {}",
        scoped.text
    );
}
