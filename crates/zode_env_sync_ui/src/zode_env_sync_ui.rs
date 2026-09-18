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
mod toolbar_button;
mod vault_modal;
mod wire_panel;

use std::path::PathBuf;

use gpui::{App, Entity, Window};
use ui::prelude::*;
use workspace::Workspace;
use zode_account::Account;
use zode_env_sync::{EnvSession, EnvStatus};

pub use env_diff_modal::EnvDiffModal;
pub use toolbar_button::EnvSyncToolbar;
pub use vault_modal::VaultModal;
pub use wire_panel::WirePanel;

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace
            .register_action(open_vault)
            .register_action(set_up)
            .register_action(push_env_file)
            .register_action(pull_env_file)
            .register_action(bind_project);
    })
    .detach();
}

fn session(cx: &App) -> Option<Entity<EnvSession>> {
    let session = EnvSession::global(cx)?;
    // Signed out, the feature has nothing to reach. Checked here rather than
    // in every handler so there is one place that decides.
    Account::global(cx)?
        .read(cx)
        .status()
        .is_signed_in()
        .then_some(session)
}

fn open_vault(
    workspace: &mut Workspace,
    _: &zed_actions::env_sync::OpenEnvVault,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(session) = session(cx) else {
        return;
    };
    // Unlocking reads the keychain first and only reaches the network if it
    // finds nothing, so opening this window on a machine that has synced
    // before makes no request at all.
    session
        .update(cx, |session, cx| session.unlock(cx))
        .detach();
    workspace.toggle_modal(window, cx, |window, cx| {
        VaultModal::new(session.clone(), window, cx)
    });
}

fn set_up(
    _: &mut Workspace,
    _: &zed_actions::env_sync::SetUpEnvSync,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(session) = session(cx) else {
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
    let Some(session) = session(cx) else {
        return;
    };
    let Some(root) = active_worktree_root(workspace, cx) else {
        return;
    };
    session
        .update(cx, |session, cx| session.unlock(cx))
        .detach();
    workspace.toggle_modal(window, cx, move |window, cx| {
        VaultModal::binding(session.clone(), root, window, cx)
    });
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
    let Some(session) = session(cx) else {
        return;
    };
    let Some(file) = active_env_file(workspace, cx) else {
        return;
    };

    let Some(entry) = session
        .read(cx)
        .entry_for(&file.worktree_root, &file.absolute)
    else {
        // Not in the catalogue. Sending the user to the vault is the honest
        // move: adding a file is a decision about which project owns it, and
        // guessing would put someone's production environment under the wrong
        // name.
        workspace.toggle_modal(window, cx, |window, cx| {
            VaultModal::new(session.clone(), window, cx)
        });
        return;
    };

    let name = file
        .absolute
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".env".into());

    match direction {
        Direction::Push => {
            // Built, then shown, then sent — in that order, always. There is no
            // path from this action to the network that does not pass through a
            // window displaying the exact bytes.
            session.update(cx, |session, cx| {
                session.prepare(entry, file.absolute.clone(), cx)
            });
            let Some(wire) = session.read(cx).prepared().cloned() else {
                // Nothing to send, or it failed to build. The session's status
                // already says which.
                return;
            };
            workspace.toggle_modal(window, cx, |_window, cx| {
                WirePanel::new(session.clone(), wire, name, cx)
            });
        }
        Direction::Pull => {
            session.update(cx, |session, cx| {
                session.pull(entry, file.absolute.clone(), cx)
            });
            // A pull that finds a difference holds it rather than writing; this
            // is the window that shows what it would do.
            workspace.toggle_modal(window, cx, |_window, cx| {
                EnvDiffModal::new(session.clone(), cx)
            });
        }
    }
}

/// An environment file open in the active editor.
pub struct ActiveEnvFile {
    pub absolute: PathBuf,
    pub worktree_root: PathBuf,
}

/// The active item, if it is a file this editor treats as private.
///
/// `private_files` already defaults to `**/.env*`, `**/*.pem`, `**/*.key` and
/// the rest, and it is user-configurable. Asking the worktree rather than
/// carrying a second glob table here is what stops the two drifting — a
/// second table is the same class of bug as two copies of one alphabet.
pub fn active_env_file(workspace: &Workspace, cx: &App) -> Option<ActiveEnvFile> {
    let project = workspace.project().read(cx);
    let path = workspace.active_item(cx)?.project_path(cx)?;
    let worktree = project.worktree_for_id(path.worktree_id, cx)?;
    let worktree = worktree.read(cx);

    // Local only, and `as_local` returning `None` for a remote worktree is the
    // right answer rather than a gap: decision D7 keeps sync on the machine
    // running the UI, so the encryption key never travels to a remote host.
    let local = worktree.as_local()?;
    if !local.is_path_private(&path.path) {
        return None;
    }
    Some(ActiveEnvFile {
        absolute: worktree.absolutize(&path.path),
        worktree_root: worktree.abs_path().to_path_buf(),
    })
}

fn active_worktree_root(workspace: &Workspace, cx: &App) -> Option<PathBuf> {
    let project = workspace.project().read(cx);
    // The worktree the active item belongs to, falling back to the only one
    // when nothing is open. Guessing between several would bind the wrong
    // checkout, so several plus no active item means no answer.
    if let Some(path) = workspace
        .active_item(cx)
        .and_then(|item| item.project_path(cx))
    {
        return Some(
            project
                .worktree_for_id(path.worktree_id, cx)?
                .read(cx)
                .abs_path()
                .to_path_buf(),
        );
    }
    let mut worktrees = project.visible_worktrees(cx);
    let only = worktrees.next()?;
    worktrees
        .next()
        .is_none()
        .then(|| only.read(cx).abs_path().to_path_buf())
}

/// One sentence describing where environment sync stands, for a status bar or
/// a menu item.
pub fn status_sentence(status: &EnvStatus) -> SharedString {
    match status {
        EnvStatus::Idle => "ready".into(),
        EnvStatus::Working => "working…".into(),
        EnvStatus::Done(message) => message.clone(),
        EnvStatus::NeedsRecoveryKey => "enter your recovery key first".into(),
        EnvStatus::KeyMismatch => "encrypted with a different key".into(),
        EnvStatus::Rollback { .. } => {
            "the server offered an older version — nothing was written".into()
        }
        EnvStatus::Failed(message) => message.clone(),
    }
}
