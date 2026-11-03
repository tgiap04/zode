//! Creating the repository on a hosting provider, through that provider's own CLI.
//!
//! This is the one place the panel shells out to something that is not git. The
//! reasoning is deliberate: `gh` and `glab` already hold the user's credentials,
//! already handle every host quirk, and are already what these users have
//! installed. Reimplementing that as OAuth inside the editor would mean owning
//! a token store and a per-host error surface for a convenience feature.
//!
//! Three rules hold, and they are the reason this file is safe to have:
//!
//! - the CLI's own auth is never read, written, or logged;
//! - the editor never runs a login command on the user's behalf;
//! - a missing or logged-out CLI produces a clear instruction, never a silent
//!   failure or a swallowed exit code.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use gpui::{Task, Window};
use project::git_store::RepositoryId;
use ui::prelude::*;
use util::command::CommandHost;

use crate::branch_panel::create_repo_modal::CreateRepoModal;

/// A hosting CLI the panel knows how to drive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum HostCli {
    Gh,
    Glab,
}

impl HostCli {
    const ALL: [HostCli; 2] = [HostCli::Gh, HostCli::Glab];

    fn binary(self) -> &'static str {
        match self {
            HostCli::Gh => "gh",
            HostCli::Glab => "glab",
        }
    }

    fn install_hint(self) -> &'static str {
        match self {
            HostCli::Gh => {
                "Install the GitHub CLI (https://cli.github.com) and run `gh auth login`."
            }
            HostCli::Glab => {
                "Install the GitLab CLI (https://gitlab.com/gitlab-org/cli) and run `glab auth login`."
            }
        }
    }
}

/// Picks the CLI to drive, or explains what to install.
///
/// Split out as a pure function so both failure paths are testable without a
/// PATH to fake: "no CLI at all" is the case most likely to be met by a real
/// user, and a silent failure there would be the worst outcome of this feature.
///
/// `remote` names the project's host when it is not this machine: the CLI is
/// looked for there, and "install gh" without saying where sends somebody to
/// install it on the wrong machine.
fn choose_cli(installed: &[HostCli], remote: Option<&str>) -> Result<HostCli, String> {
    match installed.first() {
        Some(&cli) => Ok(cli),
        None => Err(match remote {
            None => format!(
                "No hosting CLI found on PATH.\n{}",
                HostCli::Gh.install_hint()
            ),
            Some(host) => format!(
                "No hosting CLI found on PATH on {host}.\nOn {host}: {}",
                HostCli::Gh.install_hint()
            ),
        }),
    }
}

/// The message for a CLI that is present but logged out. It names the exact
/// command to run, because the editor deliberately will not run it.
fn not_signed_in_message(cli: HostCli, remote: Option<&str>) -> String {
    match remote {
        None => format!(
            "{bin} is installed but not signed in. Run `{bin} auth login` in a terminal, then try again.",
            bin = cli.binary()
        ),
        Some(host) => format!(
            "{bin} is installed on {host} but not signed in. Run `{bin} auth login` in a terminal on {host}, then try again.",
            bin = cli.binary()
        ),
    }
}

/// Exit status a POSIX shell, and `env`, give for a program not on `PATH`.
const COMMAND_NOT_FOUND_EXIT: i32 = 127;

/// Whether the binary resolves on the host's PATH. Cheap enough to call from a
/// background task, never from `render`.
///
/// `Ok(false)` only for "not there": the spawn's own `NotFound` on this
/// machine, or exit 127 through a remote host's wrapper, which itself always
/// starts. Anything else is the host's error and must not read as "not
/// installed", or somebody is sent to install what they already have.
async fn is_installed(host: &dyn CommandHost, cli: HostCli) -> Result<bool> {
    let mut command = host.command(cli.binary(), &["--version"], &[], None)?;
    let remote = host.remote_name();
    let place = remote
        .as_ref()
        .map(|host| format!(" on {host}"))
        .unwrap_or_default();
    let output = match command.output().await {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && remote.is_none() => {
            return Ok(false);
        }
        Err(error) => {
            return Err(anyhow::Error::new(error)
                .context(format!("could not run {} --version{place}", cli.binary())));
        }
    };
    if output.status.success() {
        return Ok(true);
    }
    if output.status.code() == Some(COMMAND_NOT_FOUND_EXIT) {
        return Ok(false);
    }
    anyhow::bail!(
        "{} --version failed{place}: {}",
        cli.binary(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

/// Whether the CLI already has a session. The output is deliberately discarded
/// rather than logged -- `auth status` prints account details.
async fn is_authenticated(host: &dyn CommandHost, cli: HostCli) -> Result<bool> {
    let mut command = host.command(cli.binary(), &["auth", "status"], &[], None)?;
    Ok(command
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false))
}

/// Refuses a name the CLI would read as one of its own options.
///
/// `gh repo create -dash` does not name a repository `-dash`: it sets the
/// description to `ash` and creates the repository under the directory's name.
/// The same holds for every flag the CLI defines, so the guard is on the
/// leading dash rather than on a list of flags.
fn validate_repository_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.starts_with('-'),
        "A repository name cannot start with `-`, because the hosting CLI would read it as an option."
    );
    Ok(())
}

/// The arguments for the provider's repository-create command.
///
/// `gh` takes `--` to end option parsing, so its name goes last and every flag
/// before it. `glab` is driven without it, which leaves
/// [`validate_repository_name`] as its only protection against a leading dash:
/// call that first.
fn create_args(cli: HostCli, name: &str, private: bool) -> Vec<&str> {
    let visibility = if private { "--private" } else { "--public" };
    match cli {
        HostCli::Gh => vec![
            "repo",
            "create",
            "--source=.",
            "--remote=origin",
            visibility,
            "--",
            name,
        ],
        HostCli::Glab => vec!["repo", "create", name, visibility, "--remoteName", "origin"],
    }
}

/// Runs the provider's repository-create command in `work_dir`.
///
/// The name is passed as an argument rather than interpolated into a shell
/// line, so a name containing shell metacharacters is inert. `--source=.` with
/// an explicit working directory is what keeps this from ever creating a
/// repository for whatever happens to be the editor's cwd.
async fn create(
    host: &dyn CommandHost,
    cli: HostCli,
    work_dir: &Path,
    name: &str,
    private: bool,
) -> Result<()> {
    let output = host
        .command(
            cli.binary(),
            &create_args(cli, name, private),
            &[],
            Some(work_dir),
        )?
        .output()
        .await
        .with_context(|| format!("running {}", cli.binary()))?;

    let place = host
        .remote_name()
        .map(|host| format!(" on {host}"))
        .unwrap_or_default();
    anyhow::ensure!(
        output.status.success(),
        "{} repo create failed{place}:\n{}",
        cli.binary(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

/// Picks the CLI on `host`, checks its session, and creates the repository in
/// `work_dir`, a path on that host.
async fn create_on_host(
    host: &dyn CommandHost,
    work_dir: &Path,
    name: &str,
    private: bool,
) -> Result<HostCli> {
    validate_repository_name(name)?;

    let mut installed = Vec::new();
    for cli in HostCli::ALL {
        if is_installed(host, cli).await? {
            installed.push(cli);
        }
    }

    let remote = host.remote_name();
    let cli = match choose_cli(&installed, remote.as_deref()) {
        Ok(cli) => cli,
        Err(message) => anyhow::bail!(message),
    };

    if !is_authenticated(host, cli).await? {
        anyhow::bail!(not_signed_in_message(cli, remote.as_deref()));
    }

    create(host, cli, work_dir, name, private).await?;
    Ok(cli)
}

impl crate::branch_panel::panel::BranchPanel {
    /// True when the repository has no remote at all -- the only state in which
    /// offering to create one online makes sense. A repository that has a
    /// remote but a branch with no upstream is Publish Branch's job instead.
    pub(crate) fn has_no_remote(&self, id: RepositoryId, cx: &App) -> bool {
        self.repository(id, cx)
            .map(|repo| {
                let repo = repo.read(cx);
                repo.remote_origin_url.is_none() && repo.remote_upstream_url.is_none()
            })
            .unwrap_or(false)
    }

    /// Asks for a name and a visibility, then hands both to the host's CLI.
    ///
    /// Detection and auth checks happen after the form, not before: they touch
    /// the filesystem and a subprocess, and doing that on every render just to
    /// decide whether a button is live would be a poll nobody asked for.
    pub(crate) fn create_remote_repository(
        &mut self,
        id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.repository(id, cx) else {
            return;
        };
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let work_dir = repo.read(cx).work_directory_abs_path.to_path_buf();
        let default_name: gpui::SharedString = work_dir
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default()
            .into();

        let modal = workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, |window, cx| {
                CreateRepoModal::new(default_name, window, cx)
            });
            workspace.active_modal::<CreateRepoModal>(cx)
        });
        let Some(modal) = modal else {
            return;
        };

        cx.subscribe_in(
            &modal,
            window,
            move |panel, modal, _: &gpui::DismissEvent, window, cx| {
                let Some((name, private)) = modal.read(cx).confirmed() else {
                    return;
                };
                panel.run_create(work_dir.clone(), name, private, window, cx);
            },
        )
        .detach();
    }

    fn run_create(
        &mut self,
        work_dir: std::path::PathBuf,
        name: String,
        private: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The project's host, not this machine: a WSL or SSH checkout is
        // created from the CLI and the credentials that live beside it.
        let host = match self.workspace.upgrade() {
            Some(workspace) => {
                let project = workspace.read(cx).project().clone();
                project.update(cx, |project, cx| project.command_host(cx))
            }
            None => Task::ready(Err(anyhow::anyhow!("the workspace was closed"))),
        };
        let task: Task<Result<HostCli>> = cx.background_spawn(async move {
            let host: Arc<dyn CommandHost> = host.await?;
            create_on_host(host.as_ref(), &work_dir, &name, private).await
        });

        let workspace = self.workspace.clone();
        let announced = cx.spawn(async move |_, cx| {
            let cli = task.await?;
            // Silence after a network action reads as "it did not work". The
            // toast is how the user learns the remote now exists and Publish
            // Branch is the next step.
            if let Some(workspace) = workspace.upgrade() {
                cx.update(|cx| {
                    workspace.update(cx, |workspace, cx| {
                        let toast = notifications::status_toast::StatusToast::new(
                            format!("Created the repository on {}", cli.binary()),
                            cx,
                            |this, _| {
                                this.icon(
                                    ui::Icon::new(ui::IconName::Github)
                                        .size(ui::IconSize::Small)
                                        .color(ui::Color::Muted),
                                )
                                .dismiss_button(true)
                            },
                        );
                        workspace.toggle_status_toast(toast, cx)
                    });
                });
            }
            Ok(())
        });

        self.report_failure(announced, "create remote repository", window, cx);
    }

    pub(crate) fn render_create_repo_prompt(
        &self,
        id: RepositoryId,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.has_no_remote(id, cx) {
            return None;
        }
        Some(
            h_flex()
                .w_full()
                .px_2()
                .py_1p5()
                .gap_1()
                .child(
                    Label::new("No remote")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    Button::new("create-remote-repo", "Create on Host…")
                        .label_size(LabelSize::Small)
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.create_remote_repository(id, window, cx);
                        })),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no CLI on PATH the user gets an instruction, not an empty failure.
    /// This is the single most likely outcome of pressing the button, so it is
    /// the one that must never be silent.
    #[test]
    fn no_cli_installed_explains_what_to_install() {
        let error = choose_cli(&[], None).unwrap_err();
        assert!(error.contains("No hosting CLI found"));
        assert!(
            error.contains("cli.github.com"),
            "the message has to carry somewhere to go: {error}"
        );
    }

    /// `gh` is listed first in `HostCli::ALL`, so a machine with both installed
    /// gets GitHub. Deterministic beats clever here.
    #[test]
    fn the_first_installed_cli_wins() {
        assert_eq!(
            choose_cli(&[HostCli::Gh, HostCli::Glab], None).unwrap(),
            HostCli::Gh
        );
        assert_eq!(choose_cli(&[HostCli::Glab], None).unwrap(), HostCli::Glab);
    }

    /// A logged-out CLI is told apart from a missing one, and the message names
    /// the exact command -- the editor will not run it on the user's behalf.
    #[test]
    fn a_logged_out_cli_names_the_login_command() {
        let message = not_signed_in_message(HostCli::Glab, None);
        assert!(message.contains("glab auth login"));
        assert!(!message.contains("No hosting CLI"));
    }

    /// A name starting with `-` is the one shape a CLI parses as an option, and
    /// it is refused with a message that says why rather than handed over.
    #[test]
    fn a_name_that_starts_with_a_dash_is_refused() {
        for name in ["-dash", "--public", "-"] {
            let error = validate_repository_name(name).unwrap_err().to_string();
            assert!(
                error.contains("cannot start with `-`"),
                "{name:?} gave an unclear message: {error}"
            );
        }
    }

    #[test]
    fn ordinary_names_pass_the_guard() {
        for name in ["zode", "my-repo", "my_repo.rs", "a-"] {
            validate_repository_name(name).unwrap();
        }
    }

    /// `gh` stops reading options at `--`, so the name has to be the last
    /// argument and nothing that is a flag may follow it -- a flag after `--`
    /// is read as a second positional and the command fails.
    #[test]
    fn gh_receives_the_name_after_the_end_of_options_marker() {
        let args = create_args(HostCli::Gh, "-dash", true);
        let marker = args
            .iter()
            .position(|argument| *argument == "--")
            .expect("gh must be given `--`");

        assert_eq!(args.last(), Some(&"-dash"));
        assert_eq!(marker, args.len() - 2, "the name directly follows `--`");
        assert!(args[..marker].contains(&"--private"));
        assert!(args[..marker].contains(&"--source=."));
    }

    #[test]
    fn the_visibility_flag_follows_the_choice() {
        assert!(create_args(HostCli::Gh, "zode", false).contains(&"--public"));
        assert!(create_args(HostCli::Glab, "zode", true).contains(&"--private"));
    }

    /// A host that runs `answer` for every command and writes down what it was
    /// asked, or refuses the way a remote-control connection does.
    struct ScriptedHost {
        name: Option<String>,
        /// What runs in place of every command: `sh -c answer` by default.
        program: &'static str,
        answer: Option<&'static str>,
        asked: std::sync::Mutex<Vec<(String, Vec<String>, Option<std::path::PathBuf>)>>,
    }

    impl ScriptedHost {
        fn remote(name: &str, answer: Option<&'static str>) -> Self {
            Self {
                name: Some(name.to_string()),
                program: "sh",
                answer,
                asked: std::sync::Mutex::new(Vec::new()),
            }
        }

        /// Every command is a program that is not there; `name` is `None`
        /// for this machine.
        fn missing_program(name: Option<&str>) -> Self {
            Self {
                name: name.map(str::to_string),
                program: "zode-no-such-program",
                ..Self::remote("unused", Some(""))
            }
        }

        fn asked(&self) -> Vec<(String, Vec<String>, Option<std::path::PathBuf>)> {
            self.asked.lock().expect("not poisoned").clone()
        }
    }

    impl CommandHost for ScriptedHost {
        fn command(
            &self,
            program: &str,
            args: &[&str],
            _env: &[(&str, &str)],
            cwd: Option<&Path>,
        ) -> Result<util::command::Command> {
            self.asked.lock().expect("not poisoned").push((
                program.to_string(),
                args.iter().map(|arg| arg.to_string()).collect(),
                cwd.map(Path::to_path_buf),
            ));
            let Some(answer) = self.answer else {
                anyhow::bail!(
                    "{program} cannot be run on {}: no local command reaches it",
                    self.name.as_deref().unwrap_or("this machine")
                );
            };
            let mut command = util::command::Command::new(self.program);
            command.args(["-c", answer]);
            Ok(command)
        }

        fn path_style(&self) -> util::paths::PathStyle {
            util::paths::PathStyle::Posix
        }

        fn remote_name(&self) -> Option<String> {
            self.name.clone()
        }

        fn environment_variable(&self, _name: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn a_cli_missing_on_the_remote_host_names_that_host() {
        let error = choose_cli(&[], Some("Ubuntu-22.04")).unwrap_err();
        assert!(
            error.contains("on Ubuntu-22.04"),
            "the message must say where to install it: {error}"
        );
        let message = not_signed_in_message(HostCli::Gh, Some("Ubuntu-22.04"));
        assert!(message.contains("gh auth login"));
        assert!(message.contains("on Ubuntu-22.04"));
    }

    /// Every step -- detection, the session check and the create -- is asked of
    /// the project's host, and the create runs in the checkout on that host.
    #[cfg(unix)]
    #[test]
    fn the_cli_is_found_checked_and_run_on_the_project_host() {
        let host = ScriptedHost::remote("Ubuntu-22.04", Some("exit 0"));
        let checkout = Path::new("/home/me/zode");

        let cli = smol::block_on(create_on_host(&host, checkout, "zode", true))
            .expect("every scripted command succeeds");

        assert_eq!(cli, HostCli::Gh);
        let words = |words: &[&str]| words.iter().map(|word| word.to_string()).collect();
        assert_eq!(
            host.asked(),
            vec![
                ("gh".to_string(), words(&["--version"]), None),
                ("glab".to_string(), words(&["--version"]), None),
                ("gh".to_string(), words(&["auth", "status"]), None),
                (
                    "gh".to_string(),
                    words(&create_args(HostCli::Gh, "zode", true)),
                    Some(checkout.to_path_buf()),
                ),
            ]
        );
    }

    /// Through `wsl.exe` the wrapper always starts; a CLI that is not there is
    /// the shell's exit 127, and the message names the host it is missing on.
    #[cfg(unix)]
    #[test]
    fn no_cli_on_the_remote_host_is_reported_for_that_host() {
        let host = ScriptedHost::remote("Ubuntu-22.04", Some("exit 127"));

        let error = smol::block_on(create_on_host(
            &host,
            Path::new("/home/me/zode"),
            "zode",
            true,
        ))
        .unwrap_err()
        .to_string();

        assert!(
            error.contains("No hosting CLI found on PATH on Ubuntu-22.04"),
            "{error}"
        );
    }

    /// Only "not there" means not installed: a CLI that fails for another
    /// reason, or a wrapper that cannot start, is the host's error and must
    /// not send somebody off to install what they have.
    #[cfg(unix)]
    #[test]
    fn a_failure_other_than_not_found_is_the_host_s_error() {
        let failing = ScriptedHost::remote(
            "Ubuntu-22.04",
            Some("echo 'the distribution is shutting down' >&2; exit 1"),
        );
        let no_wrapper = ScriptedHost::missing_program(Some("Ubuntu-22.04"));

        for (case, host) in [("failing CLI", failing), ("missing wrapper", no_wrapper)] {
            let error = smol::block_on(create_on_host(
                &host,
                Path::new("/home/me/zode"),
                "zode",
                true,
            ))
            .unwrap_err();
            let message = format!("{error:#}");
            assert!(!message.contains("No hosting CLI"), "{case}: {message}");
            assert!(message.contains("Ubuntu-22.04"), "{case}: {message}");
        }
    }

    #[test]
    fn a_program_this_machine_does_not_have_is_not_installed() {
        let host = ScriptedHost::missing_program(None);

        let error = smol::block_on(create_on_host(
            &host,
            Path::new("/home/me/zode"),
            "zode",
            true,
        ))
        .unwrap_err()
        .to_string();

        assert!(error.contains("No hosting CLI found on PATH."), "{error}");
    }

    #[test]
    fn a_host_that_cannot_run_the_cli_says_so_rather_than_not_installed() {
        let host = ScriptedHost::remote("relay-box", None);

        let error = smol::block_on(create_on_host(
            &host,
            Path::new("/home/me/zode"),
            "zode",
            true,
        ))
        .unwrap_err()
        .to_string();

        assert!(error.contains("gh cannot be run on relay-box"), "{error}");
        assert!(!error.contains("No hosting CLI"), "{error}");
    }
}
