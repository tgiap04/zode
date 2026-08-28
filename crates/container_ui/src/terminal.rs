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

use container::ResourceKind;
use gpui::{Context, Window};
use task::{HideStrategy, RevealStrategy, SpawnInTerminal, TaskId};
use terminal_view::terminal_panel::TerminalPanel;

use crate::container_panel::ContainerPanel;

/// Which of the two terminals is being opened.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum TerminalIntent {
    FollowLogs,
    Shell,
}

impl TerminalIntent {
    fn label(self) -> &'static str {
        match self {
            TerminalIntent::FollowLogs => "logs",
            TerminalIntent::Shell => "exec",
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
        let Some(backend) = self.backend() else {
            return;
        };
        let built = match intent {
            TerminalIntent::FollowLogs => backend.logs_command(self.active_kind, &id),
            TerminalIntent::Shell => backend.exec_command(self.active_kind, &id),
        };
        let Some((program, args)) = built else {
            return;
        };
        let kind = self.active_kind;
        // `None` in a floating window: it has no workspace to open a tab in.
        let Some(workspace) = self.workspace.clone() else {
            return;
        };

        let task = spawn_in_terminal(intent, &program, args, &name, kind);

        // Done here rather than in a spawned future: `window` is in hand, and
        // `spawn_task` needs one. Updating the *workspace* from inside this
        // panel's own update is safe -- it is a different entity, and the trap
        // this plan keeps meeting is re-entering the same one.
        let spawned = workspace.update(cx, |workspace, cx| {
            workspace
                .panel::<TerminalPanel>(cx)
                .map(|panel| panel.update(cx, |panel, cx| panel.spawn_task(&task, window, cx)))
        });

        match spawned {
            Ok(Some(spawned)) => {
                // Awaited rather than dropped: `spawn_task` refuses outright for
                // a collaboration guest, and a button that silently does nothing
                // is the exact defect this plan has already shipped once.
                cx.spawn(async move |_this, _cx| {
                    if let Err(error) = spawned.await {
                        log::error!("could not open a container terminal: {error}");
                    }
                })
                .detach();
            }
            Ok(None) => log::warn!("no terminal panel to open a container terminal in"),
            Err(error) => log::error!("could not reach the workspace: {error}"),
        }
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

fn spawn_in_terminal(
    intent: TerminalIntent,
    program: &str,
    args: Vec<String>,
    name: &str,
    kind: ResourceKind,
) -> SpawnInTerminal {
    let label = format!("{}: {name}", intent.label());
    SpawnInTerminal {
        // Distinct per resource *and* per intent, so following two containers'
        // logs at once opens two tabs rather than one replacing the other.
        id: TaskId(format!("container:{}:{kind:?}:{name}", intent.label())),
        full_label: label.clone(),
        label,
        command_label: format!("{program} {}", args.join(" ")),
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
