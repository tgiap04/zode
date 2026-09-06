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

use anyhow::{Context as _, Result};
use gpui::{Task, Window};
use project::git_store::RepositoryId;
use ui::prelude::*;
use util::command::Command;

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
fn choose_cli(installed: &[HostCli]) -> Result<HostCli, String> {
    match installed.first() {
        Some(&cli) => Ok(cli),
        None => Err(format!(
            "No hosting CLI found on PATH.\n{}",
            HostCli::Gh.install_hint()
        )),
    }
}

/// The message for a CLI that is present but logged out. It names the exact
/// command to run, because the editor deliberately will not run it.
fn not_signed_in_message(cli: HostCli) -> String {
    format!(
        "{bin} is installed but not signed in. Run `{bin} auth login` in a terminal, then try again.",
        bin = cli.binary()
    )
}

/// Whether the binary resolves on PATH. Cheap enough to call from a background
/// task, never from `render`.
async fn is_installed(cli: HostCli) -> bool {
    Command::new(cli.binary())
        .arg("--version")
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Whether the CLI already has a session. The output is deliberately discarded
/// rather than logged -- `auth status` prints account details.
async fn is_authenticated(cli: HostCli) -> bool {
    Command::new(cli.binary())
        .args(["auth", "status"])
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Runs the provider's repository-create command in `work_dir`.
///
/// The name is passed as an argument rather than interpolated into a shell
/// line, so a name containing shell metacharacters is inert. `--source=.` with
/// an explicit working directory is what keeps this from ever creating a
/// repository for whatever happens to be the editor's cwd.
async fn create(cli: HostCli, work_dir: &Path, name: &str, private: bool) -> Result<()> {
    let visibility = if private { "--private" } else { "--public" };
    let args: Vec<&str> = match cli {
        HostCli::Gh => vec![
            "repo",
            "create",
            name,
            "--source=.",
            "--remote=origin",
            visibility,
        ],
        HostCli::Glab => vec!["repo", "create", name, visibility, "--remoteName", "origin"],
    };

    let output = Command::new(cli.binary())
        .current_dir(work_dir)
        .args(&args)
        .output()
        .await
        .with_context(|| format!("running {}", cli.binary()))?;

    anyhow::ensure!(
        output.status.success(),
        "{} repo create failed:\n{}",
        cli.binary(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
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
        let task: Task<Result<HostCli>> = cx.background_spawn(async move {
            let mut installed = Vec::new();
            for cli in HostCli::ALL {
                if is_installed(cli).await {
                    installed.push(cli);
                }
            }

            let cli = match choose_cli(&installed) {
                Ok(cli) => cli,
                Err(message) => anyhow::bail!(message),
            };

            if !is_authenticated(cli).await {
                anyhow::bail!(not_signed_in_message(cli));
            }

            create(cli, &work_dir, &name, private).await?;
            Ok(cli)
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
                .py_1()
                .gap_1()
                .child(
                    Label::new("No remote")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    Button::new("create-remote-repo", "Create on host…")
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
        let error = choose_cli(&[]).unwrap_err();
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
            choose_cli(&[HostCli::Gh, HostCli::Glab]).unwrap(),
            HostCli::Gh
        );
        assert_eq!(choose_cli(&[HostCli::Glab]).unwrap(), HostCli::Glab);
    }

    /// A logged-out CLI is told apart from a missing one, and the message names
    /// the exact command -- the editor will not run it on the user's behalf.
    #[test]
    fn a_logged_out_cli_names_the_login_command() {
        let message = not_signed_in_message(HostCli::Glab);
        assert!(message.contains("glab auth login"));
        assert!(!message.contains("No hosting CLI"));
    }
}
