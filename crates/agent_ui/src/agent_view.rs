use gpui::{
    AnyElement, App, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Task, WeakEntity,
    Window,
};
use project::{AgentBinary, AgentBinaryMissing, AgentId, Project, builtin_agent};
use task::{HideStrategy, RevealStrategy, SpawnInTerminal, TaskId};
use terminal_view::TerminalView;
use ui::prelude::*;
use workspace::{
    SidebarSide, SplitDirection, Workspace, WorkspaceSettings,
    item::{Item, ItemEvent},
};
use zed_actions::agent::AgentViewMode;

use settings::Settings as _;
use util::ResultExt as _;

/// The rail draws hard-coded buttons for the two built-in agents, and the tab has
/// to carry the same glyph. Two match arms rather than a lookup through
/// `AgentServerStore`: `project` cannot depend on the icon crate, and a third
/// agent is a deliberate change to both places, not an accident.
pub fn agent_icon(agent: &str) -> IconName {
    match agent {
        project::CLAUDE_CODE_AGENT_ID => IconName::AiClaude,
        project::CODEX_AGENT_ID => IconName::AiOpenAi,
        _ => IconName::Sparkle,
    }
}

pub struct AgentView {
    agent: AgentId,
    display_name: SharedString,
    mode: AgentViewMode,
    state: State,
    project: Entity<Project>,
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    _startup: Option<Task<()>>,
}

enum State {
    Starting,
    Terminal(Entity<TerminalView>),
    /// The agent's own CLI is not on this machine. Carries what the install
    /// screen needs, so the view never has to ask which agent it is showing for.
    MissingBinary(AgentBinaryMissing),
    Failed(SharedString),
}

pub enum AgentViewEvent {
    UpdateTab,
}

impl EventEmitter<AgentViewEvent> for AgentView {}

impl AgentView {
    /// Opens `agent`, or brings the one already open to the front.
    ///
    /// Re-activating rather than opening a second copy is what makes the rail
    /// button behave like a toggle instead of a duplicator.
    pub fn open(
        workspace: &mut Workspace,
        agent: &str,
        mode: AgentViewMode,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let agent_id = AgentId::new(agent.to_string());

        // Resolved in its own statement: the iterator borrows the workspace, and
        // activating the item needs it back mutably.
        let existing = workspace
            .items_of_type::<AgentView>(cx)
            .find(|view| view.read(cx).agent == agent_id);
        if let Some(existing) = existing {
            workspace.activate_item(&existing, true, true, window, cx);
            // The CLI may well have been installed since this view gave up on it,
            // so coming back to it is a retry rather than a look at a stale verdict.
            existing.update(cx, |view, cx| {
                if matches!(view.state, State::MissingBinary(_)) {
                    view.restart(window, cx);
                }
            });
            return;
        }

        let display_name = builtin_agent(agent)
            .map(|builtin| SharedString::from(builtin.display_name))
            .unwrap_or_else(|| SharedString::from(agent.to_string()));

        let project = workspace.project().clone();
        let workspace_handle = cx.weak_entity();
        let view = cx.new(|cx| {
            let mut view = Self {
                agent: agent_id.clone(),
                display_name,
                mode,
                state: State::Starting,
                project,
                workspace: workspace_handle,
                focus_handle: cx.focus_handle(),
                _startup: None,
            };
            view.start(window, cx);
            view
        });

        workspace.split_item(agent_split_direction(cx), Box::new(view), window, cx);
    }

    /// Re-runs startup from scratch — the way back after the CLI is installed.
    pub(crate) fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = State::Starting;
        self.start(window, cx);
        cx.notify();
    }

    pub(crate) fn open_install_terminal(
        &mut self,
        command: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace
            .update(cx, |workspace, cx| {
                crate::missing_binary::open_install_terminal(workspace, command, window, cx);
            })
            .log_err();
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == AgentViewMode::Chat {
            // Phase 04 brings the ACP conversation view; until then say so plainly
            // rather than opening an empty pane.
            self.state = State::Failed("The chat view is not available yet.".into());
            return;
        }

        let agent = self.agent.clone();
        let project = self.project.clone();
        let workspace = self.workspace.clone();
        let store = project.read(cx).agent_server_store().clone();

        self._startup = Some(cx.spawn_in(window, async move |this, cx| {
            // `store` and `project` are strong handles owned by this task, so the
            // updates cannot fail — only `this` is weak, and it is checked.
            let resolve = store.update(cx, |store, cx| store.resolve_agent_binary(&agent, cx));

            let binary = match resolve.await {
                Ok(AgentBinary::Found(path)) => path,
                Ok(AgentBinary::Missing(missing)) => {
                    this.update(cx, |this, cx| {
                        // Both a toast and the screen behind it: the screen is the
                        // durable record, the toast is what reaches someone who
                        // clicked and then looked elsewhere.
                        crate::missing_binary::notify(&workspace, &missing, cx);
                        this.state = State::MissingBinary(missing);
                        cx.emit(AgentViewEvent::UpdateTab);
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                Err(error) => {
                    this.update(cx, |this, cx| {
                        this.state = State::Failed(SharedString::from(error.to_string()));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };

            let terminal = project
                .update(cx, |project, cx| {
                    project.create_terminal_task(agent_task(&agent, binary, project, cx), cx)
                })
                .await;

            this.update_in(cx, |this, window, cx| {
                match terminal {
                    Ok(terminal) => {
                        let view = cx.new(|cx| {
                            TerminalView::new(
                                terminal,
                                workspace.clone(),
                                None,
                                project.downgrade(),
                                window,
                                cx,
                            )
                        });
                        this.state = State::Terminal(view);
                    }
                    Err(error) => this.state = State::Failed(SharedString::from(error.to_string())),
                }
                cx.notify();
            })
            .ok();
        }));
    }
}

/// Agents open on the same side of the window as the rail button that opened
/// them; a button at one edge opening a pane at the other puts the width of the
/// screen between cause and effect.
///
/// This reads the very setting `sidebar::rail_side` reads, so the two cannot end
/// up mirrored against each other — the same reasoning the rail already applies
/// to its own separator and active-project pill.
fn agent_split_direction(cx: &App) -> SplitDirection {
    match WorkspaceSettings::get_global(cx).multi_project.sidebar_side {
        SidebarSide::Left => SplitDirection::Left,
        SidebarSide::Right => SplitDirection::Right,
    }
}

/// The agent's CLI is run as a task rather than typed into a shell, so the pty
/// holds the agent itself: closing the tab ends the session, and no stray shell
/// outlives it. The summary, command echo and rerun button are all off — this is
/// an interactive program, not a build step whose exit code the user waits on.
fn agent_task(
    agent: &AgentId,
    binary: std::path::PathBuf,
    project: &Project,
    cx: &App,
) -> SpawnInTerminal {
    let label = builtin_agent(agent.as_ref())
        .map(|builtin| builtin.display_name.to_string())
        .unwrap_or_else(|| agent.to_string());

    SpawnInTerminal {
        id: TaskId(format!("agent:{agent}")),
        full_label: label.clone(),
        label,
        command_label: binary.to_string_lossy().into_owned(),
        command: Some(binary.to_string_lossy().into_owned()),
        args: Vec::new(),
        cwd: project
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf()),
        reveal: RevealStrategy::Never,
        hide: HideStrategy::Never,
        show_summary: false,
        show_command: false,
        show_rerun: false,
        ..SpawnInTerminal::default()
    }
}

impl Focusable for AgentView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.state {
            State::Terminal(terminal) => terminal.focus_handle(cx),
            _ => self.focus_handle.clone(),
        }
    }
}

impl Item for AgentView {
    type Event = AgentViewEvent;

    fn to_item_events(_event: &Self::Event, f: &mut dyn FnMut(ItemEvent)) {
        f(ItemEvent::UpdateTab);
    }

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        self.display_name.clone()
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(agent_icon(self.agent.as_ref())))
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        None
    }
}

impl Render for AgentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        div()
            .size_full()
            .bg(colors.editor_background)
            .track_focus(&self.focus_handle)
            .child(match &self.state {
                State::Terminal(terminal) => terminal.clone().into_any_element(),
                State::Starting => centered_message(
                    IconName::ArrowCircle,
                    format!("Starting {}…", self.display_name).into(),
                    None,
                    cx,
                ),
                State::MissingBinary(missing) => {
                    crate::missing_binary::render(&cx.entity(), &self.display_name, missing, cx)
                }
                State::Failed(error) => {
                    centered_message(IconName::Warning, error.clone(), None, cx)
                }
            })
    }
}

fn centered_message(
    icon: IconName,
    message: SharedString,
    detail: Option<SharedString>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .child(Icon::new(icon).color(Color::Muted))
        .child(Label::new(message).color(Color::Muted))
        .children(detail.map(|detail| {
            div()
                .px_2()
                .py_1()
                .rounded_sm()
                .bg(cx.theme().colors().element_background)
                .child(Label::new(detail).size(LabelSize::Small).buffer_font(cx))
        }))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, UpdateGlobal as _};
    use settings::SettingsStore;

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
    }

    /// A rail button on one edge that opens a pane on the other is the width of
    /// the screen between cause and effect, so the side has to track the setting
    /// rather than being fixed.
    #[gpui::test]
    async fn the_agent_pane_opens_on_the_rail_side(cx: &mut TestAppContext) {
        init_test(cx);

        for (side, expected) in [
            (SidebarSide::Left, SplitDirection::Left),
            (SidebarSide::Right, SplitDirection::Right),
        ] {
            cx.update(|cx| {
                SettingsStore::update_global(cx, |settings, cx| {
                    settings.update_user_settings(cx, |settings| {
                        settings
                            .workspace
                            .multi_project
                            .get_or_insert_default()
                            .sidebar_side = Some(side);
                    });
                });
            });

            assert_eq!(
                cx.update(|cx| agent_split_direction(cx)),
                expected,
                "a {side:?} rail must open its agents on the {side:?}"
            );
        }
    }

    #[gpui::test]
    async fn every_built_in_agent_has_its_own_glyph(_cx: &mut TestAppContext) {
        let claude = agent_icon(project::CLAUDE_CODE_AGENT_ID);
        let codex = agent_icon(project::CODEX_AGENT_ID);
        assert_ne!(
            claude, codex,
            "the two agents must be told apart on the rail and the tab"
        );
        assert_ne!(claude, agent_icon("something-else"));
    }
}
