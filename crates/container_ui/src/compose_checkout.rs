//! The git side of "Switch Branch...": what a compose project's checkout is on,
//! what it could be moved to, and the move itself.
//!
//! Everything here shells out to `git -C <root>` with arguments, never through
//! a shell: branch names come from the repository, and a name with a space or a
//! semicolon in it must not be able to become a second command. Nothing here
//! touches the UI; the panel runs these on the background executor.
//!
//! The checkout is only ever **detached**. Compose containers run whatever is on
//! disk, so what matters is the commit, and a detached HEAD cannot disturb a
//! local branch somebody else is working on.

use std::fmt;
use std::path::{Path, PathBuf};

use util::command::{Command, Stdio};

/// Why a git step did not go through, in git's own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GitFailure {
    /// What was being attempted -- "fetch", "switch" -- for the modal's title.
    pub(crate) step: &'static str,
    /// git's stderr, trimmed. Untrusted text: shown, never acted on.
    pub(crate) message: String,
    /// The paths git named as being in the way, when it listed any.
    pub(crate) conflicting_files: Vec<String>,
}

impl fmt::Display for GitFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        // git already prints the files, but only for the one message that has
        // them; naming them again here keeps them visible if the message is cut.
        if !self.conflicting_files.is_empty() && !self.message.contains(&self.conflicting_files[0])
        {
            write!(f, "\nIn the way: {}", self.conflicting_files.join(", "))?;
        }
        Ok(())
    }
}

/// What HEAD is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CheckoutState {
    Branch(String),
    Detached {
        short_sha: String,
        /// What names HEAD's commit: remote branches if any, else local
        /// branches, else tags. Never `<remote>/HEAD`.
        names: Vec<String>,
    },
}

impl CheckoutState {
    pub(crate) fn label(&self) -> String {
        match self {
            CheckoutState::Branch(name) => name.clone(),
            CheckoutState::Detached { short_sha, names } if names.is_empty() => {
                format!("{short_sha} \u{b7} no branch points here (deleted?)")
            }
            CheckoutState::Detached { short_sha, names } => {
                format!("{short_sha} \u{b7} {}", names.join(", "))
            }
        }
    }

    /// Detached with nothing pointing at it: the branch it was taken from is
    /// gone. The files on disk are unaffected; the warning is so the next
    /// person to look knows the commit is no longer named by anything.
    pub(crate) fn points_nowhere(&self) -> bool {
        matches!(self, CheckoutState::Detached { names, .. } if names.is_empty())
    }
}

/// A git checkout and what it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checkout {
    pub(crate) root: PathBuf,
    pub(crate) state: CheckoutState,
}

/// Somewhere the checkout can be moved to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    RemoteBranch {
        remote: String,
        /// Without the `<remote>/` prefix.
        name: String,
    },
    /// Another worktree of the same repository. Only its HEAD commit is taken:
    /// whatever it has uncommitted stays in that worktree.
    Worktree {
        path: PathBuf,
        branch: Option<String>,
        commit: String,
    },
}

impl Target {
    pub(crate) fn remote_branch(remote: &str, name: &str) -> Self {
        Target::RemoteBranch {
            remote: remote.to_string(),
            name: name.to_string(),
        }
    }

    pub(crate) fn title(&self) -> String {
        match self {
            Target::RemoteBranch { remote, name } => format!("{remote}/{name}"),
            Target::Worktree {
                path,
                branch,
                commit,
            } => {
                let folder = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                match branch {
                    Some(branch) => format!("{folder} ({branch})"),
                    None => format!("{folder} ({})", short(commit)),
                }
            }
        }
    }

    pub(crate) fn kind_label(&self) -> &'static str {
        match self {
            Target::RemoteBranch { .. } => "Remote branch",
            Target::Worktree { .. } => "Worktree",
        }
    }

    /// The ref or commit to resolve. A full ref name for a remote branch, so a
    /// local branch or tag of the same name cannot be picked instead.
    fn revision(&self) -> String {
        match self {
            Target::RemoteBranch { remote, name } => format!("refs/remotes/{remote}/{name}"),
            Target::Worktree { commit, .. } => commit.clone(),
        }
    }
}

pub(crate) fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

struct GitOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl GitOutput {
    fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Commands that only read. They run with `--no-optional-locks` so that asking
/// what a repository in use is on never takes its index lock and makes the
/// person's own `git commit` fail.
fn is_read_only(args: &[&str]) -> bool {
    matches!(
        args.first().copied(),
        Some("status" | "for-each-ref" | "worktree" | "rev-parse" | "symbolic-ref" | "remote")
    )
}

async fn run_git(root: &Path, args: &[&str]) -> Result<GitOutput, String> {
    let mut command = Command::new("git");
    if is_read_only(args) {
        command.arg("--no-optional-locks");
    }
    command
        .arg("-C")
        .arg(root)
        .args(args)
        // Nobody can see a prompt from here, so a command that wants a
        // password, a passphrase or a host-key answer must fail instead.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        // English, so "not a git repository" can be told from a real failure.
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        // Dropping the future (a closed dialog, a timeout) must stop git, not
        // leave a fetch running against the network.
        .kill_on_drop(true);
    // Left alone when the person has their own, so an ssh wrapper or a
    // specific key still works.
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        command.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let output = command.output().await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "git is not installed".to_string()
        } else {
            format!("could not run git: {error}")
        }
    })?;
    Ok(GitOutput {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// `git <args>` that must succeed; stdout on success, git's words on failure.
async fn git(root: &Path, step: &'static str, args: &[&str]) -> Result<String, GitFailure> {
    let failure = |message: String| GitFailure {
        step,
        conflicting_files: conflicting_files(&message),
        message,
    };
    let output = run_git(root, args).await.map_err(failure)?;
    if output.success() {
        return Ok(output.stdout);
    }
    let said = output.stderr.trim();
    let said = if said.is_empty() {
        output.stdout.trim()
    } else {
        said
    };
    Err(failure(if said.is_empty() {
        format!("git {step} exited with {:?}", output.code)
    } else {
        said.to_string()
    }))
}

/// The tab-indented lines git uses to list paths ("would be overwritten by
/// checkout:\n\ta.txt").
fn conflicting_files(message: &str) -> Vec<String> {
    message
        .lines()
        .filter(|line| line.starts_with('\t'))
        .map(|line| line.trim().to_string())
        .collect()
}

/// The checkout `directory` belongs to.
///
/// `Ok(None)` means git said it is not a repository: most compose directories
/// are not checkouts, and those simply get no button. `Err` is anything else
/// (git missing, a lock, a broken repository) -- the caller must not treat that
/// as "no checkout", or a transient failure would make the row blink out.
pub(crate) async fn inspect(directory: PathBuf) -> Result<Option<Checkout>, GitFailure> {
    let toplevel = match run_git(&directory, &["rev-parse", "--show-toplevel"]).await {
        Ok(output) if output.success() => output.stdout,
        Ok(output) if output.stderr.contains("not a git repository") => return Ok(None),
        Ok(output) => {
            return Err(failure(
                "inspect",
                output.stderr.trim().to_string(),
                output.code,
            ));
        }
        Err(message) => return Err(failure("inspect", message, None)),
    };
    let root = PathBuf::from(toplevel.trim());
    let state = current_state(&root).await?;
    Ok(Some(Checkout { root, state }))
}

fn failure(step: &'static str, message: String, code: Option<i32>) -> GitFailure {
    GitFailure {
        step,
        message: if message.is_empty() {
            format!("git {step} exited with {code:?}")
        } else {
            message
        },
        conflicting_files: Vec::new(),
    }
}

/// `Some(branch)` when HEAD is on one, `None` when it is detached.
async fn current_branch(root: &Path) -> Result<Option<String>, GitFailure> {
    let symbolic = run_git(root, &["symbolic-ref", "--short", "--quiet", "HEAD"])
        .await
        .map_err(|message| failure("inspect", message, None))?;
    if symbolic.success() {
        return Ok(Some(symbolic.stdout.trim().to_string()));
    }
    // `--quiet` makes "HEAD is not a symbolic ref" exit 1 without a word; any
    // other failure has something to say and is a real error.
    if !symbolic.stderr.trim().is_empty() {
        return Err(failure(
            "inspect",
            symbolic.stderr.trim().to_string(),
            symbolic.code,
        ));
    }
    Ok(None)
}

pub(crate) async fn current_state(root: &Path) -> Result<CheckoutState, GitFailure> {
    if let Some(branch) = current_branch(root).await? {
        return Ok(CheckoutState::Branch(branch));
    }
    let short_sha = git(root, "inspect", &["rev-parse", "--short", "HEAD"])
        .await?
        .trim()
        .to_string();
    let pointing = git(
        root,
        "inspect",
        &[
            "for-each-ref",
            "--points-at",
            "HEAD",
            "--format=%(refname)",
            "refs/remotes",
            "refs/heads",
            "refs/tags",
        ],
    )
    .await?;
    Ok(CheckoutState::Detached {
        short_sha,
        names: names_pointing_at_head(&pointing),
    })
}

/// Remote branches if there are any, else local branches, else tags: the first
/// group that has something to say, so the label stays one short phrase.
fn names_pointing_at_head(refnames: &str) -> Vec<String> {
    let names = |prefix: &str| -> Vec<String> {
        refnames
            .lines()
            .filter_map(|line| line.trim().strip_prefix(prefix))
            .filter(|name| !name.ends_with("/HEAD"))
            .map(str::to_string)
            .collect()
    };
    [
        names("refs/remotes/"),
        names("refs/heads/"),
        names("refs/tags/")
            .into_iter()
            .map(|tag| format!("tag {tag}"))
            .collect(),
    ]
    .into_iter()
    .find(|group| !group.is_empty())
    .unwrap_or_default()
}

/// The short id of HEAD when it is detached and no branch or tag contains it:
/// the commits there are reachable only through the reflog once the checkout
/// moves away.
pub(crate) async fn orphaned_head(root: &Path) -> Result<Option<String>, GitFailure> {
    if current_branch(root).await?.is_some() {
        return Ok(None);
    }
    let containing = git(
        root,
        "inspect",
        &[
            "for-each-ref",
            "--contains",
            "HEAD",
            "--count=1",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )
    .await?;
    if !containing.trim().is_empty() {
        return Ok(None);
    }
    let short_sha = git(root, "inspect", &["rev-parse", "--short", "HEAD"]).await?;
    Ok(Some(short_sha.trim().to_string()))
}

/// How many paths `git status` reports as changed, untracked included.
pub(crate) async fn uncommitted_changes(root: &Path) -> Result<usize, GitFailure> {
    let status = git(root, "status", &["status", "--porcelain"]).await?;
    Ok(status
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count())
}

/// The remote to fetch and list: `origin` when there is one, otherwise the
/// first listed. `None` for a repository with no remote at all.
pub(crate) async fn select_remote(root: &Path) -> Result<Option<String>, GitFailure> {
    let remotes = git(root, "list remotes", &["remote"]).await?;
    let names: Vec<&str> = remotes
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    Ok(names
        .iter()
        .find(|name| **name == "origin")
        .or_else(|| names.first())
        .map(|name| name.to_string()))
}

pub(crate) async fn fetch(root: &Path, remote: &str) -> Result<(), GitFailure> {
    git(root, "fetch", &["fetch", remote, "--prune"])
        .await
        .map(|_| ())
}

/// Runs `work` against `timer`; whichever finishes first wins, and a lost
/// `work` is dropped, which kills the git it was running.
pub(crate) async fn within(
    work: impl std::future::Future<Output = Result<(), GitFailure>>,
    timer: impl std::future::Future<Output = ()>,
    what: &'static str,
) -> Result<(), GitFailure> {
    use futures::future::{Either, select};
    futures::pin_mut!(work, timer);
    match select(work, timer).await {
        Either::Left((outcome, _)) => outcome,
        Either::Right(((), _)) => Err(GitFailure {
            step: what,
            message: "timed out".to_string(),
            conflicting_files: Vec::new(),
        }),
    }
}

/// Branches of `remote` (when there is one), then the other worktrees.
pub(crate) async fn list_targets(
    root: &Path,
    remote: Option<&str>,
) -> Result<Vec<Target>, GitFailure> {
    let mut targets: Vec<Target> = Vec::new();
    if let Some(remote) = remote {
        let prefix = format!("refs/remotes/{remote}/");
        let refs = git(
            root,
            "list branches",
            &["for-each-ref", "--format=%(refname)", &prefix],
        )
        .await?;
        targets.extend(
            refs.lines()
                .filter_map(|line| line.trim().strip_prefix(prefix.as_str()))
                .filter(|name| *name != "HEAD")
                .map(|name| Target::remote_branch(remote, name)),
        );
    }

    let worktrees = git(root, "list worktrees", &["worktree", "list", "--porcelain"]).await?;
    targets.extend(
        parse_worktrees(&worktrees)
            .into_iter()
            .filter(|target| !matches!(target, Target::Worktree { path, .. } if path == root)),
    );
    Ok(targets)
}

/// Reads `git worktree list --porcelain`: blocks separated by a blank line,
/// each `worktree <path>`, `HEAD <sha>`, then `branch <ref>` or `detached`.
/// Bare and prunable entries have no usable files and are left out.
fn parse_worktrees(text: &str) -> Vec<Target> {
    let mut targets = Vec::new();
    for block in text.split("\n\n") {
        let mut path = None;
        let mut commit = None;
        let mut branch = None;
        let mut skip = false;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(value));
            } else if let Some(value) = line.strip_prefix("HEAD ") {
                commit = Some(value.to_string());
            } else if let Some(value) = line.strip_prefix("branch ") {
                branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_string(),
                );
            } else if line == "bare" || line.starts_with("prunable") {
                skip = true;
            }
        }
        if let (false, Some(path), Some(commit)) = (skip, path, commit) {
            targets.push(Target::Worktree {
                path,
                branch,
                commit,
            });
        }
    }
    targets
}

/// Detaches the checkout at `target`, and returns the commit it landed on.
///
/// The target is resolved to a commit id first and the switch is to that id: a
/// name that git cannot resolve fails here with a clear message instead of
/// being handed to `switch`, which would try to read it as a branch to create.
pub(crate) async fn switch(root: &Path, target: &Target) -> Result<String, GitFailure> {
    let revision = target.revision();
    let spec = format!("{revision}^{{commit}}");
    let resolved = run_git(root, &["rev-parse", "--verify", "--quiet", &spec])
        .await
        .map_err(|message| GitFailure {
            step: "switch",
            message,
            conflicting_files: Vec::new(),
        })?;
    let commit = resolved.stdout.trim().to_string();
    if !resolved.success() || commit.is_empty() {
        return Err(GitFailure {
            step: "switch",
            message: format!(
                "{} no longer exists. It may have been deleted on the remote.",
                target.title()
            ),
            conflicting_files: Vec::new(),
        });
    }
    git(root, "switch", &["switch", "--detach", &commit]).await?;
    Ok(commit)
}

/// A real repository to run against, shared with the panel's tests.
#[cfg(test)]
pub(crate) mod fixture {
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;
    use util::command::Command;

    fn null_device() -> &'static str {
        if cfg!(windows) { "NUL" } else { "/dev/null" }
    }

    pub(crate) fn run(directory: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(directory)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "protocol.file.allow=always",
            ])
            .args(args)
            // The person's own git config must not reach the fixture: a global
            // `commit.gpgsign`, hooks path or `init.defaultBranch` would make
            // these tests pass or fail by machine.
            .env("GIT_CONFIG_GLOBAL", null_device())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com");
        let output = smol::block_on(command.output()).expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    pub(crate) fn commit_file(directory: &Path, file: &str, content: &str) {
        std::fs::write(directory.join(file), content).expect("write");
        run(directory, &["add", file]);
        run(directory, &["commit", "-m", content]);
    }

    /// An origin with `main` and `fix.x` (one commit ahead, changing `a.txt`),
    /// and a clone of it sitting on `main`.
    pub(crate) struct Fixture {
        _temp: TempDir,
        pub(crate) origin: PathBuf,
        pub(crate) work: PathBuf,
    }

    pub(crate) fn fixture() -> Fixture {
        let temp = tempfile::tempdir().expect("tempdir");
        // Canonical: git reports the real path, and on macOS the temp dir is a
        // symlink, which would make `work == root` comparisons fail for no
        // reason that matters.
        let base = dunce::canonicalize(temp.path()).expect("canonicalize");
        let origin = base.join("origin.git");
        let seed = base.join("seed");
        let work = base.join("work");
        std::fs::create_dir_all(&origin).expect("mkdir");
        run(&origin, &["init", "--bare", "--initial-branch=main"]);
        run(&base, &["clone", origin.to_str().expect("utf8"), "seed"]);
        run(&seed, &["checkout", "-b", "main"]);
        commit_file(&seed, "a.txt", "one");
        run(&seed, &["push", "origin", "main"]);
        run(&seed, &["checkout", "-b", "fix.x"]);
        commit_file(&seed, "a.txt", "two");
        run(&seed, &["push", "origin", "fix.x"]);
        run(&base, &["clone", origin.to_str().expect("utf8"), "work"]);
        Fixture {
            _temp: temp,
            origin,
            work,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{fixture, run};
    use super::*;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        smol::block_on(future)
    }

    #[test]
    fn on_a_branch_the_state_is_its_name() {
        let fixture = fixture();
        let checkout = block(inspect(fixture.work))
            .expect("inspects")
            .expect("a checkout");
        assert_eq!(checkout.state, CheckoutState::Branch("main".into()));
        assert_eq!(checkout.state.label(), "main");
    }

    #[test]
    fn a_directory_that_is_not_a_checkout_has_none() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert_eq!(block(inspect(temp.path().to_path_buf())), Ok(None));
    }

    #[test]
    fn a_subdirectory_resolves_to_the_repository_root() {
        let fixture = fixture();
        let nested = fixture.work.join("deep");
        std::fs::create_dir_all(&nested).expect("mkdir");
        let checkout = block(inspect(nested))
            .expect("inspects")
            .expect("a checkout");
        assert_eq!(checkout.root, fixture.work);
    }

    #[test]
    fn detached_with_a_branch_on_it_names_the_branch() {
        let fixture = fixture();
        block(fetch(&fixture.work, "origin")).expect("fetch");
        block(switch(
            &fixture.work,
            &Target::remote_branch("origin", "fix.x"),
        ))
        .expect("switch");
        let state = block(current_state(&fixture.work)).expect("state");
        let CheckoutState::Detached { short_sha, names } = &state else {
            panic!("expected detached, got {state:?}");
        };
        assert_eq!(names, &vec!["origin/fix.x".to_string()]);
        assert_eq!(state.label(), format!("{short_sha} \u{b7} origin/fix.x"));
        assert!(!state.points_nowhere());
    }

    /// The case the user asked about: the branch the containers run is deleted.
    /// The files stay; only the name is gone, and the state must say so.
    #[test]
    fn deleting_the_branch_the_checkout_was_detached_from_reads_as_no_branch_here() {
        let fixture = fixture();
        block(fetch(&fixture.work, "origin")).expect("fetch");
        block(switch(
            &fixture.work,
            &Target::remote_branch("origin", "fix.x"),
        ))
        .expect("switch");

        run(&fixture.origin, &["branch", "-D", "fix.x"]);
        block(fetch(&fixture.work, "origin")).expect("fetch --prune");

        let state = block(current_state(&fixture.work)).expect("state");
        assert!(state.points_nowhere(), "got {state:?}");
        assert!(state.label().ends_with("no branch points here (deleted?)"));
        assert_eq!(
            std::fs::read_to_string(fixture.work.join("a.txt")).expect("read"),
            "two",
            "the files the containers run must not move"
        );
    }

    #[test]
    fn targets_are_the_origin_branches_without_head_plus_other_worktrees() {
        let fixture = fixture();
        let other = fixture.work.parent().expect("parent").join("other");
        run(
            &fixture.work,
            &[
                "worktree",
                "add",
                "--detach",
                other.to_str().expect("utf8"),
                "origin/fix.x",
            ],
        );
        let targets = block(list_targets(&fixture.work, Some("origin"))).expect("targets");

        let branches: Vec<&str> = targets
            .iter()
            .filter_map(|target| match target {
                Target::RemoteBranch { name, .. } => Some(name.as_str()),
                Target::Worktree { .. } => None,
            })
            .collect();
        assert_eq!(branches, vec!["fix.x", "main"], "no origin/HEAD");

        let worktrees: Vec<&PathBuf> = targets
            .iter()
            .filter_map(|target| match target {
                Target::Worktree { path, .. } => Some(path),
                Target::RemoteBranch { .. } => None,
            })
            .collect();
        assert_eq!(
            worktrees,
            vec![&other],
            "the checkout itself is not offered"
        );
    }

    #[test]
    fn switching_to_a_worktree_takes_its_commit_and_not_its_uncommitted_changes() {
        let fixture = fixture();
        let other = fixture.work.parent().expect("parent").join("other");
        run(
            &fixture.work,
            &[
                "worktree",
                "add",
                "--detach",
                other.to_str().expect("utf8"),
                "origin/fix.x",
            ],
        );
        std::fs::write(other.join("scratch.txt"), "wip").expect("write");

        let targets = block(list_targets(&fixture.work, Some("origin"))).expect("targets");
        let worktree = targets
            .into_iter()
            .find(|target| matches!(target, Target::Worktree { .. }))
            .expect("a worktree");
        block(switch(&fixture.work, &worktree)).expect("switch");

        assert_eq!(
            std::fs::read_to_string(fixture.work.join("a.txt")).expect("read"),
            "two"
        );
        assert!(!fixture.work.join("scratch.txt").exists());
    }

    #[test]
    fn uncommitted_changes_are_counted_untracked_included() {
        let fixture = fixture();
        assert_eq!(block(uncommitted_changes(&fixture.work)), Ok(0));
        std::fs::write(fixture.work.join("a.txt"), "local").expect("write");
        std::fs::write(fixture.work.join("new.txt"), "new").expect("write");
        assert_eq!(block(uncommitted_changes(&fixture.work)), Ok(2));
    }

    #[test]
    fn a_conflicting_local_change_makes_the_switch_fail_and_names_the_file() {
        let fixture = fixture();
        block(fetch(&fixture.work, "origin")).expect("fetch");
        std::fs::write(fixture.work.join("a.txt"), "local").expect("write");

        let failure = block(switch(
            &fixture.work,
            &Target::remote_branch("origin", "fix.x"),
        ))
        .expect_err("git must refuse");
        assert_eq!(failure.step, "switch");
        assert_eq!(failure.conflicting_files, vec!["a.txt".to_string()]);
        assert_eq!(
            block(current_state(&fixture.work)),
            Ok(CheckoutState::Branch("main".into())),
            "a refused switch leaves the checkout where it was"
        );
    }

    #[test]
    fn a_non_conflicting_local_change_is_carried_over() {
        let fixture = fixture();
        block(fetch(&fixture.work, "origin")).expect("fetch");
        std::fs::write(fixture.work.join("mine.txt"), "mine").expect("write");
        block(switch(
            &fixture.work,
            &Target::remote_branch("origin", "fix.x"),
        ))
        .expect("switch");
        assert!(fixture.work.join("mine.txt").exists());
    }

    #[test]
    fn a_branch_that_no_longer_exists_fails_before_anything_moves() {
        let fixture = fixture();
        let failure = block(switch(
            &fixture.work,
            &Target::remote_branch("origin", "gone"),
        ))
        .expect_err("no such ref");
        assert!(failure.message.contains("origin/gone"), "{failure}");
    }

    #[test]
    fn a_failing_fetch_is_reported_as_a_fetch_failure() {
        let fixture = fixture();
        std::fs::remove_dir_all(&fixture.origin).expect("remove origin");
        let failure = block(fetch(&fixture.work, "origin")).expect_err("origin is gone");
        assert_eq!(failure.step, "fetch");
        assert!(!failure.message.is_empty());
    }

    #[test]
    fn worktree_blocks_are_read_and_bare_and_prunable_are_left_out() {
        let text = "worktree /r\nHEAD aaaaaaaaaaaa\nbranch refs/heads/main\n\n\
                    worktree /w\nHEAD bbbbbbbbbbbb\ndetached\n\n\
                    worktree /gone\nHEAD cccccccccccc\ndetached\nprunable gitdir file points to non-existent location\n\n\
                    worktree /bare\nbare\n";
        assert_eq!(
            parse_worktrees(text),
            vec![
                Target::Worktree {
                    path: "/r".into(),
                    branch: Some("main".into()),
                    commit: "aaaaaaaaaaaa".into()
                },
                Target::Worktree {
                    path: "/w".into(),
                    branch: None,
                    commit: "bbbbbbbbbbbb".into()
                },
            ]
        );
    }

    #[test]
    fn head_at_a_local_branch_tip_is_named_by_that_branch() {
        let fixture = fixture();
        run(&fixture.work, &["switch", "--detach", "main"]);
        run(&fixture.work, &["branch", "-r", "-d", "origin/main"]);
        let state = block(current_state(&fixture.work)).expect("state");
        assert_eq!(state.label().split(" \u{b7} ").nth(1), Some("main"));
        assert!(!state.points_nowhere());
    }

    #[test]
    fn head_at_a_tag_is_named_by_the_tag_and_not_reported_as_deleted() {
        let fixture = fixture();
        run(&fixture.work, &["switch", "--detach", "origin/fix.x"]);
        run(&fixture.work, &["branch", "-r", "-d", "origin/fix.x"]);
        run(&fixture.work, &["tag", "v1"]);
        let state = block(current_state(&fixture.work)).expect("state");
        assert_eq!(state.label().split(" \u{b7} ").nth(1), Some("tag v1"));
        assert!(!state.points_nowhere());
    }

    #[test]
    fn a_remote_branch_is_preferred_over_a_local_branch_and_a_tag() {
        let fixture = fixture();
        run(&fixture.work, &["switch", "--detach", "origin/fix.x"]);
        run(&fixture.work, &["branch", "local-fix"]);
        run(&fixture.work, &["tag", "v1"]);
        let state = block(current_state(&fixture.work)).expect("state");
        assert_eq!(state.label().split(" \u{b7} ").nth(1), Some("origin/fix.x"));
    }

    #[test]
    fn a_detached_head_no_ref_contains_is_orphaned() {
        let fixture = fixture();
        assert_eq!(block(orphaned_head(&fixture.work)), Ok(None), "on a branch");
        run(&fixture.work, &["switch", "--detach", "origin/fix.x"]);
        assert_eq!(
            block(orphaned_head(&fixture.work)),
            Ok(None),
            "a branch holds it"
        );

        run(&fixture.work, &["commit", "--allow-empty", "-m", "loose"]);
        let loose = run(&fixture.work, &["rev-parse", "--short", "HEAD"]);
        assert_eq!(block(orphaned_head(&fixture.work)), Ok(Some(loose)));
    }

    #[test]
    fn a_remote_named_upstream_is_fetched_listed_and_switched_to() {
        let fixture = fixture();
        run(&fixture.work, &["remote", "rename", "origin", "upstream"]);
        assert_eq!(
            block(select_remote(&fixture.work)),
            Ok(Some("upstream".to_string()))
        );
        block(fetch(&fixture.work, "upstream")).expect("fetch");
        let targets = block(list_targets(&fixture.work, Some("upstream"))).expect("targets");
        assert_eq!(
            targets,
            vec![
                Target::remote_branch("upstream", "fix.x"),
                Target::remote_branch("upstream", "main"),
            ]
        );
        assert_eq!(targets[0].title(), "upstream/fix.x");
        block(switch(&fixture.work, &targets[0])).expect("switch");
        assert_eq!(
            std::fs::read_to_string(fixture.work.join("a.txt")).expect("read"),
            "two"
        );
    }

    #[test]
    fn origin_is_preferred_and_no_remote_offers_worktrees_only() {
        let fixture = fixture();
        run(
            &fixture.work,
            &[
                "remote",
                "add",
                "aaa",
                fixture.origin.to_str().expect("utf8"),
            ],
        );
        assert_eq!(
            block(select_remote(&fixture.work)),
            Ok(Some("origin".to_string())),
            "origin wins over an earlier name"
        );
        run(&fixture.work, &["remote", "remove", "aaa"]);
        run(&fixture.work, &["remote", "remove", "origin"]);
        assert_eq!(block(select_remote(&fixture.work)), Ok(None));
        assert_eq!(block(list_targets(&fixture.work, None)), Ok(Vec::new()));
    }

    #[test]
    fn work_that_outlives_the_timer_is_reported_as_timed_out() {
        let outcome = block(within(
            std::future::pending(),
            std::future::ready(()),
            "fetch",
        ));
        let failure = outcome.expect_err("the timer won");
        assert_eq!(
            (failure.step, failure.message.as_str()),
            ("fetch", "timed out")
        );
        let finished = block(within(
            std::future::ready(Ok(())),
            std::future::pending(),
            "fetch",
        ));
        assert_eq!(finished, Ok(()));
    }

    #[test]
    fn only_commands_that_read_run_without_optional_locks() {
        for read in [
            "status",
            "for-each-ref",
            "worktree",
            "rev-parse",
            "symbolic-ref",
            "remote",
        ] {
            assert!(is_read_only(&[read]), "{read}");
        }
        for write in ["fetch", "switch"] {
            assert!(!is_read_only(&[write]), "{write}");
        }
    }
}
