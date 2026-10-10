//! The uncommitted changes under one folder of a repository, read for someone
//! who must not be handed more than they asked for: only the paths the caller
//! keeps, and never more than a fixed number of bytes held in memory.
//!
//! No external diff program and no textconv program is run. The repository's
//! own git filters (a `clean` filter named in its attributes, say) still apply,
//! exactly as they do to any `git status` or `git diff` Zode runs.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use futures::AsyncReadExt as _;
use util::command::Stdio;

use crate::repository::{GitBinary, RepoPath};

/// The most bytes of changed-path names read before the repository is judged
/// too busy to list.
const MAX_NAME_LIST_BYTES: usize = 4 * 1024 * 1024;

/// The most bytes of file names given to one run of git, which keeps a command
/// line inside what every platform accepts.
const MAX_PATHSPEC_BYTES_PER_RUN: usize = 16 * 1024;

/// Decides, for a path inside the repository, whether its changes may be sent.
pub type PathFilter = Arc<dyn Fn(&RepoPath) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct ScopedDiff {
    /// Only paths inside this folder of the repository are considered. The
    /// repository root is the empty path.
    pub scope: RepoPath,
    /// Of those, only the ones this accepts appear in the diff.
    pub keep: PathFilter,
    /// The most bytes of diff returned.
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedDiff {
    pub text: String,
    /// The diff continued past `max_bytes` and was cut at the end of a line.
    pub truncated: bool,
}

/// Cuts `text` to `max_bytes` at the end of a line, so a reader never sees half
/// of one.
fn cut_at_line_end(text: String, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(newline) = text[..end].rfind('\n') {
        end = newline + 1;
    }
    let mut text = text;
    text.truncate(end);
    (text, true)
}

/// Cuts `bytes` to `max_bytes` at the end of a line, before anything is decoded,
/// so what is held for decoding is already within bounds. A single line longer
/// than the limit is cut where the limit falls.
fn cut_bytes_at_line_end(mut bytes: Vec<u8>, max_bytes: usize) -> (Vec<u8>, bool) {
    if bytes.len() <= max_bytes {
        return (bytes, false);
    }
    let end = bytes[..max_bytes]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(max_bytes, |newline| newline + 1);
    bytes.truncate(end);
    (bytes, true)
}

/// The changed paths git listed, as text. A name that is not text, or that is
/// not a path inside a repository, is left out: nothing could judge whether it
/// may be shown, so it is not.
fn parse_changed_paths(listing: &[u8]) -> Vec<(&str, RepoPath)> {
    listing
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
        .filter_map(|name| {
            let Ok(name) = std::str::from_utf8(name) else {
                log::warn!("a changed path is not valid text and is left out of the diff");
                return None;
            };
            match RepoPath::new(name) {
                Ok(path) => Some((name, path)),
                Err(error) => {
                    log::warn!("a changed path is left out of the diff: {error:#}");
                    None
                }
            }
        })
        .collect()
}

struct Collected {
    bytes: Vec<u8>,
    /// More was available than `limit` allowed, and the program was stopped.
    overflowed: bool,
}

/// Runs `command` and keeps at most `limit` bytes of its output. A program that
/// has more to say is killed rather than waited for, so neither its output nor
/// its running time is bounded by what it chooses to produce.
async fn collect_bounded(mut command: util::command::Command, limit: usize) -> Result<Collected> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().context("could not start git")?;
    let stdout = child.stdout.take().context("git has no output to read")?;
    let mut bytes = Vec::new();
    stdout
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .context("could not read what git printed")?;
    if bytes.len() > limit {
        child.kill().context("could not stop git")?;
        return Ok(Collected {
            bytes,
            overflowed: true,
        });
    }
    let status = child.status().await.context("could not wait for git")?;
    if !status.success() {
        bail!("git exited with {status}");
    }
    Ok(Collected {
        bytes,
        overflowed: false,
    })
}

/// `None` when the repository has no commit yet, so there is nothing to compare
/// against.
pub(crate) async fn diff_head_to_worktree(
    git: &GitBinary,
    request: ScopedDiff,
) -> Result<Option<BoundedDiff>> {
    let head_exists = git
        .build_command(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .context("could not start git")?
        .success();
    if !head_exists {
        return Ok(None);
    }

    // The patterns are file names, not globs, however odd the names are.
    let mut names_arguments = vec![
        "--literal-pathspecs".to_string(),
        "diff".into(),
        "--name-only".into(),
        "-z".into(),
        "--no-renames".into(),
        "--no-ext-diff".into(),
        "--no-textconv".into(),
        "HEAD".into(),
    ];
    // Left off when there is no folder to name: for an untrusted repository
    // `build_command` appends an option after the arguments, which a bare `--`
    // would turn into a file name that matches nothing.
    if !request.scope.is_empty() {
        names_arguments.push("--".into());
        names_arguments.push(request.scope.as_unix_str().to_string());
    }
    let names = collect_bounded(git.build_command(&names_arguments), MAX_NAME_LIST_BYTES).await?;
    if names.overflowed {
        bail!("too many files have changed to list");
    }
    let kept: Vec<String> = parse_changed_paths(&names.bytes)
        .into_iter()
        .filter(|(_, path)| path.starts_with(&request.scope) && (request.keep)(path))
        .map(|(name, _)| name.to_string())
        .collect();

    // Several runs when the names would not fit one command line; the names
    // arrive in order, so the runs joined are the one diff.
    let mut text = Vec::new();
    let mut truncated = false;
    let mut chunk: Vec<&str> = Vec::new();
    let mut chunk_bytes = 0;
    let mut chunks: Vec<Vec<&str>> = Vec::new();
    for name in &kept {
        if chunk_bytes + name.len() + 1 > MAX_PATHSPEC_BYTES_PER_RUN && !chunk.is_empty() {
            chunks.push(std::mem::take(&mut chunk));
            chunk_bytes = 0;
        }
        chunk_bytes += name.len() + 1;
        chunk.push(name);
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    for names in chunks {
        let mut arguments: Vec<&str> = vec![
            "--literal-pathspecs",
            "diff",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "HEAD",
            "--",
        ];
        // For an untrusted repository `build_command` appends one more token
        // here, which is read as one more file name and matches nothing.
        arguments.extend(names);
        let remaining = request.max_bytes - text.len();
        let output = collect_bounded(git.build_command(&arguments), remaining).await?;
        text.extend_from_slice(&output.bytes);
        if output.overflowed {
            truncated = true;
            break;
        }
    }
    let (text, cut_raw) = cut_bytes_at_line_end(text, request.max_bytes);
    // Bytes that are not valid text become longer when decoded, so the decoded
    // text is held to the limit again.
    let (text, cut_decoded) = cut_at_line_end(
        String::from_utf8_lossy(&text).into_owned(),
        request.max_bytes,
    );
    Ok(Some(BoundedDiff {
        text,
        truncated: truncated || cut_raw || cut_decoded,
    }))
}

#[cfg(test)]
mod tests {
    use std::{path::Path, process::Command};

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

        let mut limited = request("", |_| true);
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

    /// A program with no end to what it prints is stopped at the limit; an
    /// unbounded read would never return.
    #[cfg(unix)]
    #[allow(
        clippy::disallowed_methods,
        reason = "the program under test is `yes`, which has no counterpart in util::command's allowed constructors"
    )]
    #[gpui::test]
    async fn a_program_that_never_stops_printing_is_stopped_at_the_limit(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let collected = collect_bounded(util::command::new_command("yes"), 1000)
            .await
            .expect("collected");
        assert!(collected.overflowed);
        assert_eq!(collected.bytes.len(), 1001);
    }

    #[gpui::test]
    async fn only_changes_under_the_scope_and_kept_by_the_filter_are_returned(
        cx: &mut TestAppContext,
    ) {
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
            .diff_head_to_worktree_scoped(request("app", |path| {
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
            .diff_head_to_worktree_scoped(request("", |_| true))
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
            .diff_head_to_worktree_scoped(request("app", |_| true))
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
            .diff_head_to_worktree_scoped(request("", |_| true))
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

        let mut limited = request("", |_| true);
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
            .diff_head_to_worktree_scoped(request("", |_| true))
            .await
            .expect("not an error");
        assert_eq!(outcome, None);
    }

    #[gpui::test]
    async fn names_that_look_like_patterns_are_taken_literally(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let directory = committed_repository();
        write(directory.path(), "[a]*.txt", "star\n");
        run_git(directory.path(), &["add", "."]);
        run_git(directory.path(), &["commit", "-q", "-m", "odd name"]);
        write(directory.path(), "[a]*.txt", "changed\n");
        write(directory.path(), "root.txt", "after\n");
        let repository = repository(directory.path(), cx);

        let scoped = repository
            .diff_head_to_worktree_scoped(request("", |path| path.as_unix_str() == "[a]*.txt"))
            .await
            .expect("a diff")
            .expect("a commit");
        assert!(scoped.text.contains("[a]*.txt"));
        assert!(!scoped.text.contains("root.txt"));
    }
}
