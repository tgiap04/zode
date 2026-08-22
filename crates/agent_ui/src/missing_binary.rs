use gpui::{AnyElement, ClipboardItem, Entity, WeakEntity, Window};
use project::AgentBinaryMissing;
use terminal_view::terminal_panel::TerminalPanel;
use ui::{Tooltip, prelude::*};
use util::ResultExt as _;
use workspace::{
    Workspace,
    notifications::{DetachAndPromptErr as _, NotificationId},
};

use crate::AgentView;

/// Type tag for the notification id. Paired with the agent's binary name it gives
/// one notification per agent, so asking for both in turn produces two toasts
/// rather than one overwriting the other, while asking twice for the same agent
/// refreshes its toast instead of stacking a second copy.
struct MissingAgentBinary;

pub(crate) fn notify(
    workspace: &WeakEntity<Workspace>,
    missing: &AgentBinaryMissing,
    cx: &mut App,
) {
    let binary = missing.binary;
    let install = missing.install_command;
    let docs = missing.docs_url;
    let message = format!("`{binary}` was not found on this machine.");

    workspace
        .update(cx, |workspace, cx| {
            workspace.show_notification(
                NotificationId::composite::<MissingAgentBinary>(binary),
                cx,
                |cx| {
                    cx.new(|cx| {
                        workspace::notifications::simple_message_notification::MessageNotification::new(
                            message.clone(),
                            cx,
                        )
                        .primary_message("Copy install command")
                        .primary_icon(IconName::Copy)
                        .primary_on_click(move |_window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(install.to_string()));
                        })
                        .more_info_message("Installation docs")
                        .more_info_url(docs)
                    })
                },
            );
        })
        .log_err();
}

/// Opens a terminal with the install command already typed but **not** run.
///
/// Deliberately stops one keystroke short: installing software is the user's
/// call, and `curl … | bash` is not something an editor should set going on its
/// own. Nothing is sent after the command text — no newline, no carriage return.
pub(crate) fn open_install_terminal(
    workspace: &mut Workspace,
    command: &'static str,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let cwd = workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|worktree| worktree.read(cx).abs_path().to_path_buf());

    let terminal = TerminalPanel::add_center_terminal(workspace, window, cx, move |project, cx| {
        project.create_terminal_shell(cwd, cx)
    });

    cx.spawn_in(window, async move |_, cx| {
        let terminal = terminal.await?;
        terminal.update(cx, |terminal, _| {
            terminal.input(command.as_bytes().to_vec());
        })?;
        anyhow::Ok(())
    })
    .detach_and_prompt_err("Failed to open a terminal", window, cx, |_, _, _| None);
}

pub(crate) fn render(
    view: &Entity<AgentView>,
    display_name: &SharedString,
    missing: &AgentBinaryMissing,
    cx: &mut App,
) -> AnyElement {
    let colors = cx.theme().colors();
    let install = missing.install_command;
    let docs = missing.docs_url;
    let binary = missing.binary;

    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .p_8()
        .gap_3()
        .child(Icon::new(IconName::Warning).color(Color::Warning))
        .child(Headline::new(format!("{display_name} is not installed")).size(HeadlineSize::Small))
        .child(
            Label::new(format!(
                "zode could not find `{binary}` in your shell environment."
            ))
            .color(Color::Muted),
        )
        .child(
            h_flex()
                .mt_2()
                .max_w_full()
                .px_3()
                .py_2()
                .gap_2()
                .rounded_md()
                .bg(colors.editor_subheader_background)
                .border_1()
                .border_color(colors.border)
                .child(
                    div().overflow_x_hidden().child(
                        Label::new(install)
                            .size(LabelSize::Small)
                            .buffer_font(cx)
                            .color(Color::Default),
                    ),
                )
                .child(
                    IconButton::new("copy-install-command", IconName::Copy)
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text("Copy"))
                        .on_click(move |_, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(install.to_string()));
                        }),
                ),
        )
        .child(
            h_flex()
                .mt_1()
                .gap_2()
                .child(
                    Button::new("install-now", "Install Now")
                        .style(ButtonStyle::Filled)
                        .tooltip(Tooltip::text(
                            "Opens a terminal with the command typed, ready for you to run",
                        ))
                        .on_click({
                            let view = view.downgrade();
                            move |_, window, cx| {
                                view.update(cx, |view, cx| {
                                    view.open_install_terminal(install, window, cx)
                                })
                                .log_err();
                            }
                        }),
                )
                // The CLI can be installed while this screen is open, so there has
                // to be a way back that is not "close the tab and click again".
                .child(Button::new("check-again", "Check Again").on_click({
                    let view = view.downgrade();
                    move |_, window, cx| {
                        view.update(cx, |view, cx| view.restart(window, cx))
                            .log_err();
                    }
                }))
                .child(
                    Button::new("install-docs", "Docs")
                        .style(ButtonStyle::Subtle)
                        .on_click(move |_, _window, cx| cx.open_url(docs)),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::MissingAgentBinary;
    use workspace::notifications::NotificationId;

    /// Both agents can be missing at once. Keying the toast by agent is what stops
    /// the second one from silently replacing the first.
    #[test]
    fn each_agent_gets_its_own_toast() {
        let claude = NotificationId::composite::<MissingAgentBinary>("claude");
        let codex = NotificationId::composite::<MissingAgentBinary>("codex");
        assert_ne!(claude, codex);
        assert_eq!(
            claude,
            NotificationId::composite::<MissingAgentBinary>("claude"),
            "asking twice for the same agent must refresh one toast, not stack two"
        );
    }
}
