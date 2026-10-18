//! The uncommitted changes under one folder of a repository, read for someone
//! who must not be handed more than they asked for: only the paths the caller
//! keeps, never more than a fixed number of bytes held in memory, and never
//! more than a fixed time spent.
//!
//! Uncommitted means staged or not, and new files git does not track yet
//! included -- up to [`MAX_UNTRACKED_FILES`] of them, each costing a run of git.
//!
//! No external diff program and no textconv program is run. The repository's
//! own git filters (a `clean` filter named in its attributes, say) still apply,
//! exactly as they do to any `git status` or `git diff` Zode runs.

mod patch;
mod process;

use std::{
    pin::pin,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use futures::future::{Either, select};
use gpui::BackgroundExecutor;

use self::{patch::retain_allowed_files, process::run};
use crate::repository::{GitBinary, RepoPath};

/// The most bytes of changed-path names read before the repository is judged
/// too busy to list.
const MAX_NAME_LIST_BYTES: usize = 4 * 1024 * 1024;

/// The most bytes of file names given to one run of git, which keeps a command
/// line inside what every platform accepts.
const MAX_PATHSPEC_BYTES_PER_RUN: usize = 16 * 1024;

/// The most files git does not track yet that are shown. Each is read by a run
/// of git of its own, so a folder of generated files costs a bounded number of
/// processes, not one per file.
const MAX_UNTRACKED_FILES: usize = 100;

/// How long, from the start of the read, new files go on being started. They
/// are the least of what is asked for, and each run of git is a process in
/// the repository's queue, so what is left of the deadline is not spent on
/// them.
const UNTRACKED_TIME_BUDGET: Duration = Duration::from_secs(8);

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
    /// How long the whole read may take. When it runs out the git that is
    /// running is stopped and the read is an error.
    pub deadline: Duration,
    /// Keeps the time for `deadline`.
    pub executor: BackgroundExecutor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedDiff {
    pub text: String,
    /// The diff is not all of the changes: it continued past `max_bytes` and
    /// was cut at the end of a line, or files git does not track yet were left
    /// out.
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

/// `names` in runs short enough for one command line each, in order, so the
/// runs joined are the one diff.
fn pathspec_runs(names: &[String]) -> Vec<Vec<&str>> {
    let mut runs: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut current_bytes = 0;
    for name in names {
        if current_bytes + name.len() + 1 > MAX_PATHSPEC_BYTES_PER_RUN && !current.is_empty() {
            runs.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current_bytes += name.len() + 1;
        current.push(name);
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

/// Whether the repository has a commit to compare with. Not having one is the
/// only answer that is not an error: a repository git cannot read is not one
/// with no history.
async fn has_a_commit(git: &GitBinary) -> Result<bool> {
    let output = run(
        git.build_command(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]),
        MAX_NAME_LIST_BYTES,
    )
    .await?;
    // `--quiet` makes a name that is not a commit exit with 1 and say nothing;
    // anything else it says is something wrong with the repository.
    let output = output.ensure_succeeded("rev-parse", &[1])?;
    Ok(output.status.is_some_and(|status| status.success()))
}

/// `None` when the repository has no commit yet, so there is nothing to compare
/// against.
pub(crate) async fn diff_head_to_worktree(
    git: &GitBinary,
    request: ScopedDiff,
) -> Result<Option<BoundedDiff>> {
    let deadline = request.deadline;
    let timer = request.executor.timer(deadline);
    // Dropping the read is what stops the git it is waiting for.
    match select(pin!(read_diff(git, request)), pin!(timer)).await {
        Either::Left((outcome, _)) => outcome,
        Either::Right(((), _)) => bail!("git took longer than {deadline:?}"),
    }
}

async fn read_diff(git: &GitBinary, request: ScopedDiff) -> Result<Option<BoundedDiff>> {
    let started = request.executor.now();
    if !has_a_commit(git).await? {
        return Ok(None);
    }
    let allowed = |path: &RepoPath| path.starts_with(&request.scope) && (request.keep)(path);
    // `--` and a path always, so that a file named like a revision cannot be
    // taken for one. The repository root is `.`.
    let scope = if request.scope.is_empty() {
        "."
    } else {
        request.scope.as_unix_str()
    };

    // The patterns are file names, not globs, however odd the names are.
    let names = run(
        git.build_command(&[
            "--literal-pathspecs",
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD",
            "--",
            scope,
        ]),
        MAX_NAME_LIST_BYTES,
    )
    .await?
    .ensure_succeeded("diff --name-only", &[])?;
    if names.overflowed {
        bail!("too many files have changed to list");
    }
    let kept: Vec<String> = parse_changed_paths(&names.stdout)
        .into_iter()
        .filter(|(_, path)| allowed(path))
        .map(|(name, _)| name.to_string())
        .collect();

    let mut text = Vec::new();
    let mut truncated = false;
    // Several runs when the names would not fit one command line.
    for names in pathspec_runs(&kept) {
        // The prefixes are named so that a user's `diff.mnemonicPrefix` cannot
        // change the headers the result is judged by, and the submodule format
        // so that a user's `diff.submodule = log` cannot print a change with no
        // header at all. For an untrusted repository `build_command` appends
        // one more token after the names, which is read as one more file name.
        let mut arguments = vec![
            "--literal-pathspecs",
            "diff",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--submodule=short",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "HEAD",
            "--",
        ];
        arguments.extend(names);
        let remaining = request.max_bytes.saturating_sub(text.len());
        let output = run(git.build_command(&arguments), remaining)
            .await?
            .ensure_succeeded("diff", &[])?;
        // A name given to git also names what is beneath it, so what came back
        // is judged again.
        text.extend(retain_allowed_files(&output.stdout, allowed));
        if output.overflowed {
            truncated = true;
            break;
        }
    }
    if !truncated {
        truncated = append_untracked(git, scope, &request, &allowed, started, &mut text).await?;
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

/// Adds to `text` the files under `scope` that git does not track and that
/// `allowed` accepts, each as a new file. Whether any were left out: too many,
/// too much text, too long since `started`, or one that could not be read.
async fn append_untracked(
    git: &GitBinary,
    scope: &str,
    request: &ScopedDiff,
    allowed: &impl Fn(&RepoPath) -> bool,
    started: Instant,
    text: &mut Vec<u8>,
) -> Result<bool> {
    let listing = run(
        git.build_command(&[
            "--literal-pathspecs",
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            scope,
        ]),
        MAX_NAME_LIST_BYTES,
    )
    .await?
    .ensure_succeeded("ls-files", &[])?;
    if listing.overflowed {
        return Ok(true);
    }
    let mut left_out = false;
    let mut shown = 0;
    for (name, path) in parse_changed_paths(&listing.stdout) {
        if request.executor.now().saturating_duration_since(started) >= UNTRACKED_TIME_BUDGET {
            left_out = true;
            break;
        }
        if !allowed(&path) {
            continue;
        }
        // The name is given to git as a file to compare, with no `--` to say
        // so, which would make a name that starts with one an option.
        if name.starts_with('-') || shown == MAX_UNTRACKED_FILES {
            left_out = true;
            continue;
        }
        shown += 1;
        let remaining = request.max_bytes.saturating_sub(text.len());
        // No `--` and no prefix: for an untrusted repository `build_command`
        // appends an option after the names, which `--no-index` takes where a
        // `--` would make it a third file to compare.
        let compared = run(
            git.build_command(&[
                "diff",
                "--no-index",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "/dev/null",
                name,
            ]),
            remaining,
        )
        .await
        // Exit code 1 is how git says the files differ.
        .and_then(|output| output.ensure_succeeded("diff --no-index", &[1]));
        match compared {
            // Git says a file differs with exit code 1, and also that it could
            // not read one (a symlink to a folder, a folder that is a
            // repository of its own), with nothing printed.
            Ok(output) if output.stdout.is_empty() => {
                log::warn!("a new file could not be compared: {}", output.stderr);
                left_out = true;
            }
            Ok(output) => {
                text.extend(retain_allowed_files(&output.stdout, allowed));
                if output.overflowed {
                    return Ok(true);
                }
            }
            Err(error) => {
                log::warn!("a new file is left out of the diff: {error:#}");
                left_out = true;
            }
        }
    }
    Ok(left_out)
}

#[cfg(test)]
mod tests;
