//! Building the pane, and the things it can be asked to hold.
//!
//! Every one of them is an existing `workspace::Item`. Nothing here invents a
//! kind of tab -- it constructs one that already exists and hands it to the
//! pane, which is why a terminal in this window behaves exactly like a terminal
//! in the editor.

use anyhow::{Result, anyhow};
use gpui::Action as _;
use gpui::{Context, Entity, WeakEntity, Window};
use ui::prelude::*;
use workspace::{Pane, Workspace};

use crate::host::FloatingPane;

/// The agents the menu offers, in the order the rail shows them.
///
/// The same four, deliberately: two lists that disagree about which agents exist
/// is a bug nobody notices until one of them is wrong.
pub(crate) const AGENTS: &[(&str, IconName, &str)] = &[
    (
        project::CLAUDE_CODE_AGENT_ID,
        IconName::AiClaude,
        "Claude Code",
    ),
    (project::CODEX_AGENT_ID, IconName::AiOpenAi, "Codex"),
    (
        project::ANTIGRAVITY_AGENT_ID,
        IconName::AiAntigravity,
        "Antigravity",
    ),
    (project::COPILOT_AGENT_ID, IconName::AiCopilot, "Copilot"),
];

impl FloatingPane {
    pub(crate) fn build_pane(
        workspace: &WeakEntity<Workspace>,
        project: Entity<project::Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Pane> {
        let this = cx.weak_entity();
        cx.new(|cx| {
            let mut pane = Pane::new(
                workspace.clone(),
                project,
                Default::default(),
                None,
                zed_actions::floating_pane::NewTerminal.boxed_clone(),
                false,
                window,
                cx,
            );
            // Back and forward would navigate this window using the editor's
            // history, which is not what a scratch window's tabs are.
            pane.set_can_navigate(false, cx);
            pane.display_nav_history_buttons(None);
            // Always: a tab bar that appeared as the second tab arrived would
            // shift every tab under the pointer at the moment of the click.
            pane.set_should_display_tab_bar(|_, _| true);
            pane.set_zoom_out_on_close(false);
            // The pane's own `+` offers New File, New Terminal and the agents
            // as workspace actions, which resolve against the editor's active
            // pane -- this pane is not one of those, so every entry opened
            // behind the window. Replaced with the window's own list, the way
            // the terminal panel replaces it for the same reason.
            pane.set_render_tab_bar_buttons(cx, move |_pane, _window, _cx| {
                (None, Some(crate::render::tab_bar_menu(this.clone())))
            });
            // The pane's own `+` offers New File, New Terminal and the agents
            // as workspace actions, which resolve against the editor's active
            // pane -- this pane is not one of those, so every entry opened
            // behind the window. Replaced with the window's own list, the way
            // the terminal panel replaces it for the same reason.
            pane
        })
    }

    pub(crate) fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project(cx) else {
            return;
        };
        let workspace = self.workspace.clone();
        let pane = self.pane.clone();
        let working_directory = project
            .read(cx)
            .active_project_directory(cx)
            .map(|path| path.to_path_buf());

        let built = project.update(cx, |project, cx| {
            project.create_terminal_shell(working_directory, cx)
        });

        // Held in a field rather than detached: it ends in a shell process, and
        // a detached build would keep producing one after this window is gone.
        self.opening = Some(cx.spawn_in(window, async move |this, cx| {
            let terminal = built.await;
            let outcome = this.update_in(cx, |_this, window, cx| match terminal {
                Ok(terminal) => {
                    let view = cx.new(|cx| {
                        terminal_view::TerminalView::new(
                            terminal,
                            workspace,
                            None,
                            project.downgrade(),
                            window,
                            cx,
                        )
                    });
                    pane.update(cx, |pane, cx| {
                        pane.add_item(Box::new(view), true, true, None, window, cx);
                    });
                }
                Err(error) => {
                    log::error!("could not open a terminal in the floating window: {error}")
                }
            });
            if let Err(error) = outcome {
                log::error!("the floating window went while its terminal opened: {error}");
            }
        }));
    }

    /// A markdown buffer nobody has saved yet.
    ///
    /// A scratch buffer rather than a file: a note wanted for the length of one
    /// thought should not make somebody choose a filename first. It is an
    /// ordinary untitled buffer, so closing it asks to save exactly the way any
    /// other unsaved buffer does -- nothing is quietly thrown away.
    pub(crate) fn new_markdown_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project(cx) else {
            return;
        };
        let pane = self.pane.clone();
        let languages = project.read(cx).languages().clone();

        self.opening = Some(cx.spawn_in(window, async move |this, cx| {
            // Markdown by name, because an untitled buffer has no extension for
            // the registry to read. Without it the note opens as plain text and
            // loses every affordance the menu entry promised.
            let markdown = languages.language_for_name("Markdown").await.ok();

            let outcome: Result<()> = async {
                let buffer = project
                    .update(cx, |project, cx| project.create_buffer(markdown, false, cx))
                    .await?;
                this.update_in(cx, |_this, window, cx| {
                    let editor = editor_over(buffer, project.clone(), window, cx);
                    pane.update(cx, |pane, cx| {
                        pane.add_item(Box::new(editor), true, true, None, window, cx);
                    });
                })?;
                Ok(())
            }
            .await;

            if let Err(error) = outcome {
                log::error!("could not open a note in the floating window: {error}");
            }
        }));
    }

    /// An existing markdown file, chosen from a system open dialog.
    ///
    /// A dialog rather than the fuzzy file finder: the finder is a modal of the
    /// workspace, and a modal that opens over the editor to fill a window
    /// floating above it puts two things in front of each other with no obvious
    /// order. The dialog is also the only one of the two that can reach a note
    /// kept outside the project, which for scratch notes is the common case.
    pub(crate) fn open_markdown_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project(cx) else {
            return;
        };
        let pane = self.pane.clone();
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });

        self.opening = Some(cx.spawn_in(window, async move |this, cx| {
            let outcome: Result<()> = async {
                // Three layers of nothing, and each means something different:
                // the prompt failed, the prompt was cancelled, or it answered
                // with an empty list. Only the first is worth a log line.
                let Some(paths) = chosen.await?? else {
                    return Ok(());
                };
                let Some(path) = paths.into_iter().next() else {
                    return Ok(());
                };
                let opened = project
                    .update(cx, |project, cx| {
                        let found = project
                            .find_project_path(&path, cx)
                            .ok_or_else(|| anyhow!("{path:?} is not inside an open project"))?;
                        anyhow::Ok(project.open_buffer(found, cx))
                    })?
                    .await?;

                this.update_in(cx, |_this, window, cx| {
                    let editor = editor_over(opened, project.clone(), window, cx);
                    pane.update(cx, |pane, cx| {
                        pane.add_item(Box::new(editor), true, true, None, window, cx);
                    });
                })?;
                Ok(())
            }
            .await;

            if let Err(error) = outcome {
                log::error!("could not open that note in the floating window: {error}");
            }
        }));
    }

    /// A fresh thread with one agent, as a tab of this window.
    ///
    /// Always a new thread rather than the one already running: an agent tab
    /// lives in exactly one pane, and pulling an existing thread in here would
    /// take it out of the editor where somebody left it.
    pub(crate) fn open_agent(&mut self, agent: &str, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.pane.clone();
        let agent = agent.to_string();
        let updated = self.workspace.update(cx, |workspace, cx| {
            agent_ui::AgentView::open_in_pane(workspace, pane, &agent, window, cx);
        });
        if let Err(error) = updated {
            log::error!("could not reach the workspace to open an agent: {error}");
        }
    }

    pub(crate) fn project(&self, cx: &gpui::App) -> Option<Entity<project::Project>> {
        self.workspace
            .read_with(cx, |workspace, _| workspace.project().clone())
            .ok()
    }
}

fn editor_over(
    buffer: Entity<language::Buffer>,
    project: Entity<project::Project>,
    window: &mut Window,
    cx: &mut Context<FloatingPane>,
) -> Entity<editor::Editor> {
    let multibuffer = cx.new(|cx| editor::MultiBuffer::singleton(buffer, cx));
    cx.new(|cx| editor::Editor::for_multibuffer(multibuffer, Some(project), window, cx))
}
