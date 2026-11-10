//! What environment sync looks like: the vault window, the difference window,
//! and the actions that reach them.
//!
//! Every value this crate can render goes through [`masking`] first. That is
//! not a nicety — a `.env` diff is the artefact people screenshot when
//! something goes wrong, and one that shows `STRIPE_SECRET_KEY=sk_live_…` has
//! leaked the credential before anybody notices.
//!
//! Nothing here runs on its own. No sync at startup, no polling, no background
//! reconciliation: every request is the direct consequence of something the
//! user pressed, which is the same promise `zode_account_ui` makes about
//! signing in.

mod env_diff_modal;
pub mod masking;
mod team_members_modal;
mod team_vault_section;
mod toolbar_button;
mod vault_modal;
mod wire_panel;

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Entity, Window, actions};
use notifications::status_toast::StatusToast;
use project::{Project, worktree_store::RemotePrivateFile};
use ui::prelude::*;
use util::ResultExt as _;
use workspace::Workspace;
use zode_account::Account;
use zode_env_sync::{
    Checkout, EntryId, EnvFile, EnvFileFuture, EnvSession, EnvStatus, EnvSyncError, LocalEnvFile,
};

pub use env_diff_modal::EnvDiffModal;
pub use team_members_modal::TeamMembersModal;
pub use team_vault_section::TeamVaultSection;
pub use toolbar_button::EnvSyncToolbar;
pub use vault_modal::VaultModal;
pub use wire_panel::WirePanel;

actions!(
    team_sharing,
    [
        /// Opens the team's members: key fingerprints, granting access and
        /// rotating the team key.
        OpenTeamMembers
    ]
);

pub fn init(cx: &mut App) {
    // The team session reads nothing and sends nothing until a team window
    // opens; installing it here is what lets the Team tab and the database
    // panel's Team group find it.
    if zode_team_vault::TeamVaultSession::global(cx).is_none()
        && let Some(account) = Account::global(cx)
    {
        zode_team_vault::init(account, cx);
    }
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace
            .register_action(open_team_members)
            .register_action(open_vault)
            .register_action(set_up)
            .register_action(push_env_file)
            .register_action(pull_env_file)
            .register_action(bind_project);
    })
    .detach();
}

/// The session, or a toast saying why there is not one.
///
/// Signed out, the feature has nothing to reach. Every one of these actions
/// used to answer that by returning and doing nothing at all, which from the
/// outside is indistinguishable from the button being broken -- so the reason
/// is said out loud, with the one thing that fixes it attached.
///
/// Checked here rather than in each handler so there is one place that decides
/// and one place that explains.
fn session(workspace: &mut Workspace, cx: &mut Context<Workspace>) -> Option<Entity<EnvSession>> {
    let session = EnvSession::global(cx)?;
    if Account::global(cx).is_some_and(|account| account.read(cx).status().is_signed_in()) {
        return Some(session);
    }

    let toast = StatusToast::new(
        "Sign in to sync environment files with your account",
        cx,
        |this, _cx| {
            this.icon(
                Icon::new(IconName::Person)
                    .size(IconSize::Small)
                    .color(Color::Muted),
            )
            .action("Sign In", |window, cx| {
                window.dispatch_action(Box::new(zed_actions::account::SignIn), cx)
            })
            .dismiss_button(true)
        },
    );
    workspace.toggle_status_toast(toast, cx);
    None
}

fn open_vault(
    workspace: &mut Workspace,
    _: &zed_actions::env_sync::OpenEnvVault,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(session) = session(workspace, cx) else {
        return;
    };
    // Unlocking reads the keychain first and only reaches the network if it
    // finds nothing, so opening this window on a machine that has synced
    // before makes no request at all.
    session
        .update(cx, |session, cx| session.unlock(cx))
        .detach();
    let handle = cx.weak_entity();
    workspace.toggle_modal(window, cx, move |window, cx| {
        VaultModal::new(session.clone(), handle, window, cx)
    });
}

fn open_team_members(
    workspace: &mut Workspace,
    _: &OpenTeamMembers,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if session(workspace, cx).is_none() {
        return;
    }
    let Some(team) = zode_team_vault::TeamVaultSession::global(cx) else {
        return;
    };
    workspace.toggle_modal(window, cx, move |_window, cx| {
        TeamMembersModal::new(team, cx)
    });
}

fn set_up(
    workspace: &mut Workspace,
    _: &zed_actions::env_sync::SetUpEnvSync,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(session) = session(workspace, cx) else {
        return;
    };
    session
        .update(cx, |session, cx| session.create_key(cx))
        .detach_and_log_err(cx);
}

fn bind_project(
    workspace: &mut Workspace,
    _: &zed_actions::env_sync::BindEnvProject,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(session) = session(workspace, cx) else {
        return;
    };
    let Some(checkout) = active_checkout(workspace, cx) else {
        if let Some(reason) = nothing_to_bind(workspace.project().read(cx)) {
            let toast = StatusToast::new(reason, cx, |this, _cx| {
                this.icon(
                    Icon::new(IconName::Info)
                        .size(IconSize::Small)
                        .color(Color::Muted),
                )
                .dismiss_button(true)
            });
            workspace.toggle_status_toast(toast, cx);
        }
        return;
    };
    session
        .update(cx, |session, cx| session.unlock(cx))
        .detach();
    let handle = cx.weak_entity();
    workspace.toggle_modal(window, cx, move |window, cx| {
        VaultModal::binding(session.clone(), handle, checkout, window, cx)
    });
}

/// Why a project has no checkout to bind, when that is a fact about the
/// project rather than about what happens to be open.
fn nothing_to_bind(project: &Project) -> Option<&'static str> {
    project.is_via_collab().then_some(
        "This project is shared with you: its environment files stay with its host, so there is no checkout here to bind",
    )
}

fn push_env_file(
    workspace: &mut Workspace,
    _: &zed_actions::env_sync::PushEnvFile,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    transfer(workspace, window, cx, Direction::Push);
}

fn pull_env_file(
    workspace: &mut Workspace,
    _: &zed_actions::env_sync::PullEnvFile,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    transfer(workspace, window, cx, Direction::Pull);
}

#[derive(Clone, Copy)]
enum Direction {
    Push,
    Pull,
}

fn transfer(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
    direction: Direction,
) {
    let Some(session) = session(workspace, cx) else {
        return;
    };
    let Some(file) = active_env_file(workspace, cx) else {
        return;
    };

    let Some(entry) = file
        .relative_path()
        .and_then(|relative| session.read(cx).entry_at(&file.checkout, &relative))
    else {
        // Not in the catalogue yet. Exactly one fact is missing — which
        // project owns this file — so the window asks for that and nothing
        // else, then sends. Guessing instead would put someone's production
        // environment under the wrong name.
        let handle = cx.weak_entity();
        match direction {
            Direction::Push => workspace.toggle_modal(window, cx, move |window, cx| {
                VaultModal::sending(session.clone(), handle, file, window, cx)
            }),
            // There is nothing to fetch for a file the account has never seen.
            Direction::Pull => workspace.toggle_modal(window, cx, move |window, cx| {
                VaultModal::new(session.clone(), handle, window, cx)
            }),
        }
        return;
    };

    let name = file.display_name();

    match direction {
        Direction::Push => {
            // Built, then shown, then sent — in that order, always. There is no
            // path from this action to the network that does not pass through a
            // window displaying the exact bytes.
            if let Some(local_path) = file.file.local_path() {
                let local_path = local_path.to_path_buf();
                session.update(cx, |session, cx| session.prepare(entry, local_path, cx));
                show_prepared(workspace, &session, entry, window, cx);
                return;
            }
            // A remote file is read over the connection first, so the window
            // opens once that read has answered.
            let prepared = session.update(cx, |session, cx| {
                session.prepare_file(entry, file.file.clone(), file.absolute.clone(), cx)
            });
            cx.spawn_in(window, async move |workspace, cx| {
                prepared.await;
                workspace
                    .update_in(cx, |workspace, window, cx| {
                        show_prepared(workspace, &session, entry, window, cx)
                    })
                    .log_err();
            })
            .detach();
        }
        Direction::Pull => {
            session.update(cx, |session, cx| {
                session.pull_file(entry, file.file.clone(), file.absolute.clone(), cx)
            });
            // A pull that finds a difference holds it rather than writing; this
            // is the window that shows what it would do.
            workspace.toggle_modal(window, cx, |_window, cx| {
                EnvDiffModal::new(session.clone(), name, cx)
            });
        }
    }
}

/// Opens the window showing what a built push would send, or says why there
/// is nothing to show.
fn show_prepared(
    workspace: &mut Workspace,
    session: &Entity<EnvSession>,
    entry: EntryId,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(wire) = session.read(cx).prepared().cloned() else {
        // Nothing to send, or it failed to build. This used to return on the
        // strength of "the status already says which" -- but no window opens
        // on this path to carry it, and `Done` is deliberately absent from the
        // status bar. Pushing an unchanged file a second time therefore
        // produced the correct answer and showed it nowhere at all.
        let outcome = session.read(cx).status().clone();
        report(workspace, &outcome, cx);
        return;
    };
    let session = session.clone();
    workspace.toggle_modal(window, cx, |window, cx| {
        WirePanel::new(session, wire, entry, window, cx)
    });
}

/// Says something that has no window of its own to appear in.
///
/// The push path either opens the window showing what would be sent, or it
/// has nothing to open -- and that second case is still an answer.
fn report(workspace: &mut Workspace, status: &EnvStatus, cx: &mut Context<Workspace>) {
    let failed = matches!(
        status,
        EnvStatus::Failed(_)
            | EnvStatus::KeyMismatch
            | EnvStatus::Rollback { .. }
            | EnvStatus::NeedsRecoveryKey
    );
    let sentence = status.sentence().to_string();

    let toast = StatusToast::new(sentence, cx, move |this, _cx| {
        this.icon(
            Icon::new(if failed {
                IconName::XCircle
            } else {
                IconName::Check
            })
            .size(IconSize::Small)
            .color(if failed { Color::Error } else { Color::Muted }),
        )
        .dismiss_button(true)
    });
    workspace.toggle_status_toast(toast, cx);
}

/// An environment file open in the active editor.
#[derive(Clone)]
pub struct ActiveEnvFile {
    /// Where reads and writes go: this machine's disk, or the project's host.
    pub file: Arc<dyn EnvFile>,
    /// The checkout the file is in, and the machine that checkout is on.
    pub checkout: Checkout,
    /// The file's path on its own machine. For a remote project that is a
    /// path on the host, shown and never opened here.
    pub absolute: PathBuf,
}

impl std::fmt::Debug for ActiveEnvFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveEnvFile")
            .field("checkout", &self.checkout)
            .field("absolute", &self.absolute)
            .finish_non_exhaustive()
    }
}

impl ActiveEnvFile {
    /// A file on this machine.
    pub fn local(worktree_root: PathBuf, absolute: PathBuf) -> Self {
        Self {
            file: Arc::new(LocalEnvFile::new(absolute.clone())),
            checkout: Checkout::local(worktree_root),
            absolute,
        }
    }

    /// The file's own name, for a window title.
    pub fn display_name(&self) -> SharedString {
        self.absolute
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".env".into())
            .into()
    }

    /// The slash-separated path the catalogue records, when the file is inside
    /// its checkout.
    pub fn relative_path(&self) -> Option<String> {
        self.checkout.relative(&self.absolute)
    }

    /// Where the file sits inside its checkout.
    ///
    /// Shown rather than the absolute path because the absolute path is both a
    /// small disclosure — it carries the user's home directory — and the wrong
    /// thing to reason about: what gets stored, and what a pull on another
    /// machine reproduces, is this.
    pub fn relative_label(&self) -> SharedString {
        self.relative_path()
            .map(SharedString::from)
            .unwrap_or_else(|| self.display_name())
    }
}

/// A private file on a remote project's host.
///
/// Only the file's bytes cross the connection. The key, the decryption, the
/// sync state and the backup of what a pull replaces all stay on this machine.
struct RemoteEnvFile(RemotePrivateFile);

impl EnvFile for RemoteEnvFile {
    fn read(&self) -> EnvFileFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            self.0
                .load()
                .await
                .map_err(|error| EnvSyncError::Io(format!("{error:#}")))
        })
    }

    fn write_atomic(&self, contents: Vec<u8>) -> EnvFileFuture<'_, ()> {
        Box::pin(async move {
            self.0
                .write(contents)
                .await
                .map_err(|error| EnvSyncError::Io(format!("{error:#}")))
        })
    }
}

/// The active item, if it is a file this editor treats as private.
///
/// `private_files` already defaults to `**/.env*`, `**/*.pem`, `**/*.key` and
/// the rest, and it is user-configurable. Asking the worktree rather than
/// carrying a second glob table here is what stops the two drifting — a
/// second table is the same class of bug as two copies of one alphabet.
///
/// A remote project's file is offered too, and read and written on its host:
/// the encryption key never leaves this machine, because only the file's bytes
/// travel. The host asks `private_files` again, but its user layer is this
/// machine's synced settings, so that is agreement rather than a boundary;
/// what the host enforces is that only this machine is served, that the path
/// stays inside the worktree, and that the file is written owner-only.
pub fn active_env_file(workspace: &Workspace, cx: &App) -> Option<ActiveEnvFile> {
    let project = workspace.project().read(cx);
    let path = workspace.active_item(cx)?.project_path(cx)?;
    let worktree = project.worktree_for_id(path.worktree_id, cx)?;
    let worktree = worktree.read(cx);
    let absolute = worktree.absolutize(&path.path);
    let root = worktree.abs_path().to_path_buf();

    if let Some(local) = worktree.as_local() {
        if !local.is_path_private(&path.path) {
            return None;
        }
        return Some(ActiveEnvFile::local(root, absolute));
    }

    let checkout = checkout_of(project, root, cx)?;
    let worktree_store = project.worktree_store();
    let worktree_store = worktree_store.read(cx);
    if !worktree_store.is_path_private_here(&path, cx) {
        return None;
    }
    let remote = worktree_store.remote_private_file(path.worktree_id, path.path, cx)?;
    Some(ActiveEnvFile {
        file: Arc::new(RemoteEnvFile(remote)),
        checkout,
        absolute,
    })
}

/// The checkout `root` names in `project`, on the machine it is a directory
/// on. `None` for a collaboration guest: those directories are on someone
/// else's machine, and no binding here speaks for them.
fn checkout_of(project: &Project, root: PathBuf, cx: &App) -> Option<Checkout> {
    match project.remote_connection_options(cx) {
        Some(options) => Some(Checkout::remote(
            workspace::remote_connection_identity(&options).stable_key(),
            root,
        )),
        None if project.is_local() => Some(Checkout::local(root)),
        None => None,
    }
}

fn active_checkout(workspace: &Workspace, cx: &App) -> Option<Checkout> {
    let project = workspace.project().read(cx);
    // The worktree the active item belongs to, falling back to the only one
    // when nothing is open. Guessing between several would bind the wrong
    // checkout, so several plus no active item means no answer.
    let root = if let Some(path) = workspace
        .active_item(cx)
        .and_then(|item| item.project_path(cx))
    {
        project
            .worktree_for_id(path.worktree_id, cx)?
            .read(cx)
            .abs_path()
            .to_path_buf()
    } else {
        let mut worktrees = project.visible_worktrees(cx);
        let only = worktrees.next()?;
        if worktrees.next().is_some() {
            return None;
        }
        only.read(cx).abs_path().to_path_buf()
    };
    checkout_of(project, root, cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use project::FakeFs;

    #[gpui::test]
    async fn a_shared_project_says_why_there_is_nothing_to_bind(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        project.read_with(cx, |project, _| {
            assert_eq!(
                nothing_to_bind(project),
                None,
                "a local project can be bound"
            );
        });

        project.update(cx, |project, _| project.mark_as_collab_for_testing());
        project.read_with(cx, |project, _| {
            let reason = nothing_to_bind(project).expect("a guest is told why");
            assert!(reason.contains("host"), "{reason}");
        });
    }

    #[gpui::test]
    async fn a_remote_checkout_is_keyed_by_its_host(
        cx: &mut TestAppContext,
        server_cx: &mut TestAppContext,
    ) {
        let fs = FakeFs::new(cx.executor());
        let (project, _server) = Project::test_remote(fs, cx, server_cx).await;

        let checkout = project
            .read_with(cx, |project, cx| {
                checkout_of(project, PathBuf::from("/home/me/acme"), cx)
            })
            .expect("a remote project's checkout has a host");

        assert!(
            !checkout.is_local(),
            "a remote root was keyed as a directory on this machine"
        );
        assert_ne!(
            checkout,
            Checkout::local("/home/me/acme"),
            "a remote and a local root with one path must not share a binding"
        );
    }
}
