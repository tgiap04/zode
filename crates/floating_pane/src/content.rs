//! Building the pane, and the things it can be asked to hold.
//!
//! Every one of them is an existing `workspace::Item`. Nothing here invents a
//! kind of tab -- it constructs one that already exists and hands it to the
//! pane, which is why a terminal in this window behaves exactly like a terminal
//! in the editor.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use gpui::Action as _;
use gpui::{App, Context, Entity, WeakEntity, Window};
use ui::prelude::*;
use workspace::{DraggedTab, Pane, Workspace};

use crate::host::FloatingPane;

impl FloatingPane {
    /// Takes the weak handle as a parameter rather than deriving it with
    /// `cx.weak_entity()`, and `&mut App` rather than `Context<Self>`: the
    /// drag-to-split path calls this from inside a `Workspace`
    /// lease, where only a bare `&mut App` is available. `Context<Self>`
    /// derefs to `&mut App`, so today's caller only has to add
    /// `cx.weak_entity()` at the call site.
    pub(crate) fn build_pane(
        workspace: &WeakEntity<Workspace>,
        project: Entity<project::Project>,
        this: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Pane> {
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
            // The pane's own tab-bar buttons are New File, New Terminal and
            // the agents as workspace actions, which resolve against the
            // editor's active pane -- this pane is not one of those, so
            // every entry opened behind the window. Replaced with the
            // window's own lists on both sides, the way the terminal panel
            // replaces the right slot for the same reason: a Split control
            // aimed at this window's own `split_active` in the left slot,
            // and this window's `+` in the right.
            pane.set_render_tab_bar_buttons(cx, {
                let this = this.clone();
                move |_pane, _window, _cx| {
                    (
                        Some(crate::render::tab_bar_split_button(this.clone())),
                        Some(crate::entries::tab_bar_menu(this.clone())),
                    )
                }
            });
            // `handle_drag_move` refuses to record an edge as a split target
            // unless this says so, and it answers `false` when unset -- so
            // without this the hook below is never reached and a tab dropped on
            // an edge simply joins the pane. The editor's centre pane and the
            // terminal panel each set their own; this window had none.
            pane.set_can_split(Some(Arc::new(
                move |pane: &mut Pane,
                      dragged_item: &dyn std::any::Any,
                      _window,
                      cx: &mut Context<Pane>| {
                    let Some(tab) = dragged_item.downcast_ref::<DraggedTab>() else {
                        return false;
                    };
                    // The one drag that undoes itself: a pane's only tab
                    // dropped on that same pane's edge leaves the source empty,
                    // and the collapse rule removes it the instant the split is
                    // made. Refused rather than performed and reverted.
                    tab.pane != cx.entity() || pane.items_len() > 1
                },
            )));
            // Without this, `handle_tab_drop` routes through
            // `Workspace::split_pane` and splits the editor's centre group --
            // the drop lands behind this window, on code the user was not
            // touching. Set on every pane this window builds, so a drop on a
            // pane created by an earlier drop behaves the same.
            //
            // Runs while the workspace entity is leased (`pane.rs`'s
            // `cx.defer_in` inside `workspace.update`). Nothing below may read
            // or update `self.workspace`.
            pane.set_split_for_drop(Some(Arc::new({
                let this = this.clone();
                move |to_pane: &Entity<Pane>, direction, window, cx: &mut App| match this
                    .update(cx, |this, cx| {
                        this.split_off(to_pane, direction, window, cx)
                    }) {
                    Ok(new_pane) => new_pane,
                    Err(error) => {
                        log::error!(
                            "the floating window went while a tab was dropped on it: {error}"
                        );
                        None
                    }
                }
            })));
            pane
        })
    }

    pub(crate) fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self.project.clone();
        let workspace = self.workspace.clone();
        // Captured now rather than read back off `self` inside the spawn: by
        // the time the terminal is built, focus may have moved to a different
        // tab, but the item still lands where the user asked for it.
        let pane = self.active_pane.clone();
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
        let project = self.project.clone();
        // Captured now, not read back off `self` inside the spawn: the note
        // lands where the user asked for it even if focus moved while the
        // buffer was still being created.
        let pane = self.active_pane.clone();
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
        let project = self.project.clone();
        // Captured now, not read back off `self` inside the spawn: the file
        // picker can take a while, and the note still lands where the user
        // asked for it even if focus moved in the meantime.
        let pane = self.active_pane.clone();
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            show_hidden: false,
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

    /// The database view, as a tab of this window.
    ///
    /// Built synchronously, unlike the terminal and the notes: `standalone`
    /// reads its languages and settings straight from what is already in
    /// memory, so there is nothing here to await.
    pub(crate) fn open_database(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.workspace.clone();
        let pane = self.active_pane.clone();
        let languages = self.project.read(cx).languages().clone();
        let view =
            cx.new(|cx| database_ui::DatabasePanel::standalone(workspace, languages, window, cx));
        pane.update(cx, |pane, cx| {
            pane.add_item(Box::new(view), true, true, None, window, cx);
        });
    }

    /// One of the container engines, pre-selected, as a tab of this window.
    ///
    /// `self.workspace.clone()` is passed through rather than left `None`:
    /// `ContainerPanel.workspace` is `None` only for the OS window, which
    /// cannot reach a workspace to build itself in. A floating *pane* lives
    /// inside one, and without this its terminals and confirm modals would
    /// silently do nothing -- the defect this panel has already shipped once.
    pub(crate) fn open_containers(
        &mut self,
        engine: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let pane = self.active_pane.clone();
        let view = cx.new(|cx| container_ui::standalone_in_workspace(engine, workspace, cx));
        pane.update(cx, |pane, cx| {
            pane.add_item(Box::new(view), true, true, None, window, cx);
        });
    }

    /// A fresh git panel, as a tab of this window.
    ///
    /// Built through `GitPanel::load`, the same constructor the dock uses, so
    /// it reads the workspace's real git state instead of a bespoke copy that
    /// would drift the moment upstream added a field. This is a second,
    /// independent `GitPanel` entity -- see `PanelItem`'s doc comment for what
    /// that costs.
    pub(crate) fn open_git_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.workspace.clone();
        let pane = self.active_pane.clone();

        self.opening = Some(cx.spawn_in(window, async move |this, cx| {
            let panel = git_ui::git_panel::GitPanel::load(workspace, cx.clone()).await;
            let outcome = this.update_in(cx, |_this, window, cx| match panel {
                Ok(panel) => {
                    let view = cx.new(|_cx| crate::panel_item::PanelItem::new(panel, "Git Panel"));
                    pane.update(cx, |pane, cx| {
                        pane.add_item(Box::new(view), true, true, None, window, cx);
                    });
                }
                Err(error) => {
                    log::error!("could not open a git panel in the floating window: {error}")
                }
            });
            if let Err(error) = outcome {
                log::error!("the floating window went while its git panel opened: {error}");
            }
        }));
    }

    /// A fresh project panel, as a tab of this window. See `open_git_panel`
    /// for why the dock's own constructor is reused rather than duplicated.
    pub(crate) fn open_project_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.workspace.clone();
        let pane = self.active_pane.clone();

        self.opening = Some(cx.spawn_in(window, async move |this, cx| {
            let panel = project_panel::ProjectPanel::load(workspace, cx.clone()).await;
            let outcome = this.update_in(cx, |_this, window, cx| match panel {
                Ok(panel) => {
                    let view =
                        cx.new(|_cx| crate::panel_item::PanelItem::new(panel, "Project Panel"));
                    pane.update(cx, |pane, cx| {
                        pane.add_item(Box::new(view), true, true, None, window, cx);
                    });
                }
                Err(error) => {
                    log::error!("could not open a project panel in the floating window: {error}")
                }
            });
            if let Err(error) = outcome {
                log::error!("the floating window went while its project panel opened: {error}");
            }
        }));
    }

    /// A fresh debug panel, as a tab of this window. See `open_git_panel` for
    /// why the dock's own constructor is reused rather than duplicated.
    pub(crate) fn open_debug_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.workspace.clone();
        let pane = self.active_pane.clone();

        self.opening = Some(cx.spawn_in(window, async move |this, cx| {
            let panel = debugger_ui::debugger_panel::DebugPanel::load(workspace, cx).await;
            let outcome = this.update_in(cx, |_this, window, cx| match panel {
                Ok(panel) => {
                    let view =
                        cx.new(|_cx| crate::panel_item::PanelItem::new(panel, "Debug Panel"));
                    pane.update(cx, |pane, cx| {
                        pane.add_item(Box::new(view), true, true, None, window, cx);
                    });
                }
                Err(error) => {
                    log::error!("could not open a debug panel in the floating window: {error}")
                }
            });
            if let Err(error) = outcome {
                log::error!("the floating window went while its debug panel opened: {error}");
            }
        }));
    }

    /// A fresh thread with one agent, as a tab of this window.
    ///
    /// Always a new thread rather than the one already running: an agent tab
    /// lives in exactly one pane, and pulling an existing thread in here would
    /// take it out of the editor where somebody left it.
    pub(crate) fn open_agent(&mut self, agent: &str, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.active_pane.clone();
        let agent = agent.to_string();
        let updated = self.workspace.update(cx, |workspace, cx| {
            agent_ui::AgentView::open_in_pane(workspace, pane, &agent, window, cx);
        });
        if let Err(error) = updated {
            log::error!("could not reach the workspace to open an agent: {error}");
        }
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
