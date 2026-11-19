//! `logs -f` and `exec` as real Zode terminal tabs.
//!
//! Always a real terminal, never a text box this crate fills: copy, paste,
//! scrollback, selection, search and Ctrl-C all work because they are the
//! terminal's, and a reimplementation would be missing most of them.
//!
//! Two places put one, from the one command built here: this module opens a tab
//! beside the code, and `detail.rs` embeds one inside the container view.
//!
//! The command is built by the backend, never here: `docker logs -f x` and
//! `kubectl logs -f -n ns x` are the same intention in two vocabularies, and a
//! view that knew either would have to know both.

use container::{ComposeOp, Op, ResourceKind, mask_secrets_in_text};
use gpui::{Context, Window};
use task::{HideStrategy, RevealStrategy, SpawnInTerminal, TaskId};
use terminal_view::terminal_panel::TerminalPanel;

use crate::container_panel::ContainerPanel;
use crate::resource_form::command_line;

/// Which terminal is being opened.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum TerminalIntent {
    FollowLogs,
    Shell,
    /// A compose verb, already built by the backend. Its output is why it runs
    /// in a terminal: `up`, `pull` and `build` take minutes and say a great
    /// deal while they do.
    Compose(ComposeOp),
    /// `pull` for an image, in a terminal for the progress it prints.
    Pull,
    /// `run` for an image with whatever the form asked for.
    Run,
}

impl TerminalIntent {
    fn label(self) -> &'static str {
        match self {
            TerminalIntent::FollowLogs => "logs",
            TerminalIntent::Shell => "exec",
            TerminalIntent::Compose(ComposeOp::Up) => "compose up",
            TerminalIntent::Compose(ComposeOp::Restart) => "compose restart",
            TerminalIntent::Compose(ComposeOp::Stop) => "compose stop",
            TerminalIntent::Compose(ComposeOp::Start) => "compose start",
            TerminalIntent::Compose(ComposeOp::Pull) => "compose pull",
            TerminalIntent::Compose(ComposeOp::Build) => "compose build",
            TerminalIntent::Compose(ComposeOp::Logs) => "compose logs",
            TerminalIntent::Pull => "pull",
            TerminalIntent::Run => "run",
        }
    }
}

impl ContainerPanel {
    /// Whether this intent is available for the kind on screen.
    ///
    /// Asked of the backend rather than decided here, so a button is drawn only
    /// where the engine has a command for it.
    pub(crate) fn terminal_available(&self, intent: TerminalIntent) -> bool {
        self.backend().is_some_and(|backend| {
            match intent {
                TerminalIntent::FollowLogs => backend.logs_command(self.active_kind, "probe"),
                TerminalIntent::Shell => backend.exec_command(self.active_kind, "probe"),
                TerminalIntent::Compose(_) => {
                    return backend.offers(Op::Compose);
                }
                TerminalIntent::Pull => return backend.offers(Op::ImagePull),
                TerminalIntent::Run => return backend.offers(Op::ImageRun),
            }
            .is_some()
        })
    }

    /// Opens a terminal tab running the engine's own command for `id`.
    pub(crate) fn open_terminal(
        &mut self,
        intent: TerminalIntent,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_terminal_of(self.active_kind, intent, id, name, window, cx);
    }

    /// [`Self::open_terminal`] for a resource of a kind other than the one on
    /// screen: a compose service's shell is a container's, while the list shows
    /// compose projects.
    pub(crate) fn open_terminal_of(
        &mut self,
        kind: ResourceKind,
        intent: TerminalIntent,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(backend) = self.backend() else {
            return;
        };
        let built = match intent {
            TerminalIntent::FollowLogs => backend.logs_command(kind, &id),
            TerminalIntent::Shell => backend.exec_command(kind, &id),
            // Built by the caller, which knows the project and the services,
            // or the form that asked for them.
            TerminalIntent::Compose(_) | TerminalIntent::Pull | TerminalIntent::Run => None,
        };
        let Some((program, args)) = built else {
            return;
        };
        self.open_command_terminal(intent, kind, program, args, name, window, cx);
    }

    /// Opens a terminal tab running `program args`, which the backend built.
    pub(crate) fn open_command_terminal(
        &mut self,
        intent: TerminalIntent,
        kind: ResourceKind,
        program: String,
        args: Vec<String>,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `None` in a floating window: it has no workspace to open a tab in.
        let Some(workspace) = self.workspace.clone() else {
            return;
        };

        let task = spawn_in_terminal(intent, &program, args, &name, kind);

        // The panel is taken *out* of the workspace before it is driven:
        // `spawn_task` reads the workspace entity itself, so an open
        // `workspace.update` around it leaves that entity leased and GPUI
        // panics on the read rather than opening anything.
        let terminal_panel =
            match workspace.read_with(cx, |workspace, cx| workspace.panel::<TerminalPanel>(cx)) {
                Ok(terminal_panel) => terminal_panel,
                Err(error) => {
                    log::error!("could not reach the workspace: {error}");
                    return;
                }
            };
        let Some(terminal_panel) = terminal_panel else {
            log::warn!("no terminal panel to open a container terminal in");
            return;
        };

        // Done here rather than in a spawned future: `window` is in hand, and
        // `spawn_task` needs one.
        let spawned = terminal_panel.update(cx, |panel, cx| panel.spawn_task(&task, window, cx));

        // Awaited rather than dropped: `spawn_task` refuses outright for a
        // collaboration guest, and a button that silently does nothing is the
        // exact defect this file has already shipped once.
        cx.spawn(async move |_this, _cx| {
            if let Err(error) = spawned.await {
                log::error!("could not open a container terminal: {error}");
            }
        })
        .detach();
    }
}

/// The engine's own `logs -f`, as a task a terminal can run.
///
/// Shared with the embedded log view in `detail.rs` so the tab and the panel run
/// the *same* command. Two builders would be two chances for them to drift, and
/// the drift would be invisible until somebody compared two screens.
pub(crate) fn logs_task(
    program: &str,
    args: Vec<String>,
    name: &str,
    kind: ResourceKind,
) -> SpawnInTerminal {
    spawn_in_terminal(TerminalIntent::FollowLogs, program, args, name, kind)
}

pub(crate) fn spawn_in_terminal(
    intent: TerminalIntent,
    program: &str,
    args: Vec<String>,
    name: &str,
    kind: ResourceKind,
) -> SpawnInTerminal {
    // What is shown is masked; what runs is not. The tab title and the command
    // line printed under the output are text on a screen that may be shared, and
    // a variable or a flag on it can be a password.
    let label = format!("{}: {}", intent.label(), mask_secrets_in_text(name));
    SpawnInTerminal {
        // Distinct per resource *and* per intent, so following two containers'
        // logs at once opens two tabs rather than one replacing the other.
        id: TaskId(format!("container:{}:{kind:?}:{name}", intent.label())),
        full_label: label.clone(),
        label,
        command_label: mask_secrets_in_text(&command_line(program, &args)),
        command: Some(program.to_string()),
        // The one thing that must never become a single string: see
        // `ContainerBackend::logs_command`.
        args,
        // A new tab every time, and several at once allowed: watching two
        // containers side by side is the ordinary reason to open these.
        use_new_terminal: true,
        allow_concurrent_runs: true,
        reveal: RevealStrategy::Always,
        hide: HideStrategy::Never,
        show_summary: false,
        show_command: true,
        show_rerun: true,
        ..SpawnInTerminal::default()
    }
}
