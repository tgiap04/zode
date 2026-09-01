//! Fetch, pull and push from the panel header.
//!
//! None of the network machinery is reimplemented here: askpass, the remote
//! output view and the error toasts are the same ones the git panel uses. What
//! this module adds is the in-flight flag, so a user leaning on the fetch
//! button gets one `git fetch`, not eight.

use askpass::AskPassDelegate;
use git::repository::FetchOptions;
use gpui::{Entity, Task, Window};
use project::git_store::{Repository, RepositoryId};
use ui::prelude::*;

use crate::askpass_modal::AskPassModal;
use crate::branch_panel::panel::BranchPanel;
use crate::git_panel::show_error_toast;
use crate::remote_output::{RemoteAction, show_remote_output};

/// The network operations the header offers. One in-flight slot per kind, so a
/// fetch and a push can overlap but two fetches cannot.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum RemoteOp {
    Fetch,
    Pull,
    Push,
}

impl RemoteOp {
    pub(crate) fn label(self) -> &'static str {
        match self {
            RemoteOp::Fetch => "Fetch",
            RemoteOp::Pull => "Pull",
            RemoteOp::Push => "Push",
        }
    }

    fn git_label(self) -> &'static str {
        match self {
            RemoteOp::Fetch => "git fetch",
            RemoteOp::Pull => "git pull",
            RemoteOp::Push => "git push",
        }
    }

    fn icon(self) -> IconName {
        match self {
            RemoteOp::Fetch => IconName::ArrowCircle,
            RemoteOp::Pull => IconName::ArrowDown,
            RemoteOp::Push => IconName::ArrowUp,
        }
    }
}

/// Where a push should go, and whether it has to set the upstream.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PushTarget {
    /// The branch already tracks a remote; push there.
    Existing { remote: String },
    /// No upstream yet. Publishing sets one on this remote.
    Publish { remote: String },
    /// Nothing to push to, or more than one candidate and no way to guess.
    Undecidable { remotes: Vec<String> },
}

/// Decides the push target from the branch's upstream and the repository's
/// remotes, with no I/O.
///
/// Guessing wrong here pushes a branch somewhere the user did not mean, which
/// is public and hard to walk back -- so with two remotes and no upstream it
/// refuses to guess rather than defaulting to `origin`.
pub(crate) fn choose_push_target(upstream_ref: Option<&str>, remotes: &[String]) -> PushTarget {
    if let Some(upstream) = upstream_ref
        && let Some(remote) = upstream
            .strip_prefix("refs/remotes/")
            .and_then(|rest| rest.split('/').next())
    {
        return PushTarget::Existing {
            remote: remote.to_string(),
        };
    }

    match remotes {
        [only] => PushTarget::Publish {
            remote: only.clone(),
        },
        many => PushTarget::Undecidable {
            remotes: many.to_vec(),
        },
    }
}

impl BranchPanel {
    pub(crate) fn is_running(&self, op: RemoteOp) -> bool {
        self.running_remote_ops.contains(&op)
    }

    /// Builds the askpass delegate the same way the git panel does, so a
    /// repository that needs a passphrase prompts identically from either.
    fn askpass(
        &self,
        operation: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AskPassDelegate {
        let workspace = self.workspace.clone();
        let window = window.window_handle();
        AskPassDelegate::new(&mut cx.to_async(), move |prompt, tx, cx| {
            window
                .update(cx, |_, window, cx| {
                    workspace
                        .update(cx, |workspace, cx| {
                            workspace.toggle_modal(window, cx, |window, cx| {
                                AskPassModal::new(operation.into(), prompt.into(), tx, window, cx)
                            });
                        })
                        .ok();
                })
                .ok();
        })
    }

    pub(crate) fn run_remote_op(
        &mut self,
        id: RepositoryId,
        op: RemoteOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_running(op) {
            return;
        }
        let Some(repo) = self.repository(id, cx) else {
            return;
        };

        let task = match op {
            RemoteOp::Fetch => self.spawn_fetch(repo, window, cx),
            RemoteOp::Pull => self.spawn_pull(repo, window, cx),
            RemoteOp::Push => {
                let Some(task) = self.spawn_push(repo, window, cx) else {
                    return;
                };
                task
            }
        };

        self.running_remote_ops.insert(op);
        cx.notify();

        let workspace = self.workspace.clone();
        let label = op.git_label();
        cx.spawn(async move |panel, cx| {
            let result = task.await;
            panel
                .update(cx, |panel, cx| {
                    panel.running_remote_ops.remove(&op);
                    cx.notify();
                })
                .ok();
            if let Err(error) = result
                && let Some(workspace) = workspace.upgrade()
            {
                cx.update(|cx| show_error_toast(workspace, label, error, cx));
            }
        })
        .detach();
    }

    fn spawn_fetch(
        &self,
        repo: Entity<Repository>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<()>> {
        let askpass = self.askpass("git fetch", window, cx);
        let workspace = self.workspace.clone();
        cx.spawn(async move |panel, cx| {
            let output = repo
                .update(cx, |repo, cx| repo.fetch(FetchOptions::All, askpass, cx))
                .await??;
            report_success(panel, workspace, RemoteAction::Fetch(None), output, cx);
            Ok(())
        })
    }

    fn spawn_pull(
        &self,
        repo: Entity<Repository>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<()>> {
        let askpass = self.askpass("git pull", window, cx);
        let Some((branch, remote)) = self.upstream_of(&repo, cx) else {
            return Task::ready(Err(anyhow::anyhow!(
                "This branch has no upstream to pull from."
            )));
        };
        cx.spawn(async move |_, cx| {
            repo.update(cx, |repo, cx| {
                // rebase=false: merge is git's default and the panel should
                // not quietly rewrite the user's history for them.
                repo.pull(Some(branch.into()), remote.into(), false, askpass, cx)
            })
            .await??;
            Ok(())
        })
    }

    /// Pushes to the branch's upstream, or sets one up if it has none. The
    /// second case is what a freshly created branch always hits.
    fn spawn_push(
        &self,
        repo: Entity<Repository>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<anyhow::Result<()>>> {
        let askpass = self.askpass("git push", window, cx);
        let snapshot = repo.read(cx);
        let branch = snapshot.branch.as_ref()?.clone();
        let name = branch.name().to_string();

        // The snapshot only carries origin and upstream URLs, which is exactly
        // the pair that matters here: anything else and the user gets asked.
        let mut remotes = Vec::new();
        if snapshot.remote_origin_url.is_some() {
            remotes.push("origin".to_string());
        }
        if snapshot.remote_upstream_url.is_some() {
            remotes.push("upstream".to_string());
        }

        let (remote, options) = match choose_push_target(
            branch.upstream.as_ref().map(|up| up.ref_name.as_ref()),
            &remotes,
        ) {
            PushTarget::Existing { remote } => (remote, None),
            PushTarget::Publish { remote } => {
                (remote, Some(git::repository::PushOptions::SetUpstream))
            }
            PushTarget::Undecidable { remotes } if remotes.is_empty() => {
                return Some(Task::ready(Err(anyhow::anyhow!(
                    "This repository has no remote to push to."
                ))));
            }
            PushTarget::Undecidable { remotes } => {
                let chosen = self.ask_which_remote(&remotes, &name, window, cx);
                let workspace = self.workspace.clone();
                let repo = repo.clone();
                return Some(cx.spawn(async move |panel, cx| {
                    let Some(remote) = chosen.await else {
                        return Ok(());
                    };
                    let (branch_name, remote_name) = (name.clone(), remote.clone());
                    let output = repo
                        .update(cx, |repo, cx| {
                            repo.push(
                                name.clone().into(),
                                name.into(),
                                remote.into(),
                                Some(git::repository::PushOptions::SetUpstream),
                                askpass,
                                cx,
                            )
                        })
                        .await??;
                    report_success(
                        panel,
                        workspace,
                        RemoteAction::Push(
                            branch_name.into(),
                            git::repository::Remote {
                                name: remote_name.into(),
                            },
                        ),
                        output,
                        cx,
                    );
                    Ok(())
                }));
            }
        };

        let workspace = self.workspace.clone();
        Some(cx.spawn(async move |panel, cx| {
            let (branch_name, remote_name) = (name.clone(), remote.clone());
            let output = repo
                .update(cx, |repo, cx| {
                    repo.push(
                        name.clone().into(),
                        name.into(),
                        remote.into(),
                        options,
                        askpass,
                        cx,
                    )
                })
                .await??;
            report_success(
                panel,
                workspace,
                RemoteAction::Push(
                    branch_name.into(),
                    git::repository::Remote {
                        name: remote_name.into(),
                    },
                ),
                output,
                cx,
            );
            Ok(())
        }))
    }

    /// Asks which remote to publish to. Only reached when the repository has
    /// more than one and the branch tracks neither.
    fn ask_which_remote(
        &self,
        remotes: &[String],
        branch: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Option<String>> {
        let labels: Vec<&str> = remotes.iter().map(String::as_str).collect();
        let answer = window.prompt(
            gpui::PromptLevel::Info,
            &format!("Publish {branch} to which remote?"),
            None,
            &labels,
            cx,
        );
        let remotes = remotes.to_vec();
        cx.background_spawn(
            async move { answer.await.ok().and_then(|ix| remotes.get(ix).cloned()) },
        )
    }

    fn upstream_of(&self, repo: &Entity<Repository>, cx: &App) -> Option<(String, String)> {
        let snapshot = repo.read(cx);
        let branch = snapshot.branch.as_ref()?;
        let upstream = branch.upstream.as_ref()?;
        let remote = upstream
            .ref_name
            .strip_prefix("refs/remotes/")
            .and_then(|rest| rest.split('/').next())?;
        Some((branch.name().to_string(), remote.to_string()))
    }

    /// Whether the current branch has somewhere to push to yet. Drives the
    /// difference between "Push" and "Publish Branch".
    pub(crate) fn branch_has_upstream(&self, id: RepositoryId, cx: &App) -> bool {
        self.repository(id, cx)
            .and_then(|repo| repo.read(cx).branch.as_ref().map(|b| b.upstream.is_some()))
            .unwrap_or(false)
    }

    pub(crate) fn remote_button(
        &self,
        id: RepositoryId,
        op: RemoteOp,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let running = self.is_running(op);
        let label = if op == RemoteOp::Push && !self.branch_has_upstream(id, cx) {
            "Publish Branch"
        } else {
            op.label()
        };

        IconButton::new(("remote-op", op as usize), op.icon())
            .icon_size(IconSize::Small)
            .disabled(running)
            .tooltip(move |_, cx| ui::Tooltip::simple(label, cx))
            .on_click(cx.listener(move |panel, _, window, cx| {
                panel.run_remote_op(id, op, window, cx);
            }))
    }
}

/// Reports what the network operation actually did, through the same status
/// toast the git panel uses. Silence on success reads as "nothing happened",
/// which is exactly wrong after a push.
fn report_success(
    panel: gpui::WeakEntity<BranchPanel>,
    workspace: gpui::WeakEntity<workspace::Workspace>,
    action: RemoteAction,
    output: git::repository::RemoteCommandOutput,
    cx: &mut gpui::AsyncApp,
) {
    let Some(workspace) = workspace.upgrade() else {
        return;
    };
    panel
        .update(cx, |_, cx| {
            show_remote_output(workspace, action, output, cx);
        })
        .ok();
}

#[cfg(test)]
mod tests;
