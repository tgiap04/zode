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
use notifications::status_toast::StatusToast;
use ui::prelude::*;
use workspace::Workspace;
use zode_account::Account;
use zode_env_sync::EnvSession;

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
    let Some(root) = active_worktree_root(workspace, cx) else {
        return;
    };
    session
        .update(cx, |session, cx| session.unlock(cx))
        .detach();
    let handle = cx.weak_entity();
    workspace.toggle_modal(window, cx, move |window, cx| {
        VaultModal::binding(session.clone(), handle, root, window, cx)
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
    let Some(session) = session(workspace, cx) else {
        return;
    };
    let Some(file) = active_env_file(workspace, cx) else {
        return;
    };

    let Some(entry) = session
        .read(cx)
        .entry_for(&file.worktree_root, &file.absolute)
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
                EnvDiffModal::new(session.clone(), name, cx)
            });
        }
    }
}

/// An environment file open in the active editor.
#[derive(Clone, Debug)]
pub struct ActiveEnvFile {
    pub absolute: PathBuf,
    pub worktree_root: PathBuf,
}

impl ActiveEnvFile {
    /// The file's own name, for a window title.
    pub fn display_name(&self) -> SharedString {
        self.absolute
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".env".into())
            .into()
    }

    /// Where the file sits inside its checkout.
    ///
    /// Shown rather than the absolute path because the absolute path is both a
    /// small disclosure — it carries the user's home directory — and the wrong
    /// thing to reason about: what gets stored, and what a pull on another
    /// machine reproduces, is this.
    pub fn relative_label(&self) -> SharedString {
        self.absolute
            .strip_prefix(&self.worktree_root)
            .ok()
            .map(|rest| rest.to_string_lossy().replace('\\', "/"))
            .map(SharedString::from)
            .unwrap_or_else(|| self.display_name())
    }
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
