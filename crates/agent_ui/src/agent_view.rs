use gpui::{
    AnyElement, App, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Task, WeakEntity,
    Window,
};
use project::{AgentBinary, AgentBinaryMissing, AgentId, Project, builtin_agent};
use task::{HideStrategy, RevealStrategy, SpawnInTerminal, TaskId};
use terminal_view::TerminalView;
use ui::{ToggleButtonGroup, ToggleButtonGroupStyle, ToggleButtonSimple, prelude::*};
use workspace::Workspace;
use zed_actions::agent::AgentViewMode;

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
    /// The ACP conversation, driven by the npx adapter. The CLI itself speaks no
    /// ACP, so this mode never runs the binary the terminal mode runs.
    Chat(Entity<crate::conversation_view::ConversationView>),
    /// The agent's own CLI is not on this machine. Carries what the install
    /// screen needs, so the view never has to ask which agent it is showing for.
    MissingBinary(AgentBinaryMissing),
    Failed(SharedString),
}

pub enum AgentViewEvent {
    UpdateTab,
}

impl AgentView {
    /// The conversation this view is showing, if it is showing one.
    ///
    /// `ConversationView` asks for this to answer whether the user is currently
    /// looking at it — the question that decides whether a finished turn is worth
    /// a notification.
    pub fn conversation_view(&self) -> Option<&Entity<crate::conversation_view::ConversationView>> {
        match &self.state {
            State::Chat(view) => Some(view),
            _ => None,
        }
    }

    /// Brings the agent forward — the response to accepting a notification.
    pub fn activate_for_agent(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
        workspace.update(cx, |workspace, cx| {
            workspace.focus_panel::<crate::agent_panel::AgentPanel>(window, cx);
        });
    }

    pub(crate) fn is_agent(&self, agent: &AgentId) -> bool {
        &self.agent == agent
    }
}

impl EventEmitter<AgentViewEvent> for AgentView {}

impl AgentView {
    /// Routes a rail click to the dock.
    ///
    /// The panel decides where the view goes — beside an agent already open, or
    /// into the pane that is there. This only resolves which agent and which mode.
    pub fn open(
        workspace: &mut Workspace,
        agent: &str,
        mode: Option<AgentViewMode>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let agent_id = AgentId::new(agent.to_string());
        let Some(panel) = workspace.panel::<crate::agent_panel::AgentPanel>(cx) else {
            return;
        };
        let db = persistence::AgentViewDb::global(cx);
        let stored_agent = agent_id.to_string();
        cx.spawn_in(window, async move |workspace, cx| {
            let remembered = cx
                .background_executor()
                .spawn(async move { db.preferences(stored_agent) })
                .await
                .log_err()
                .flatten()
                .unwrap_or_default();

            panel
                .update_in(cx, |panel, window, cx| {
                    let (stored_mode, _) = remembered;
                    // A choice is what gets remembered; a plain click never
                    // overwrites the very preference it just read.
                    let chosen = mode;
                    let mode = chosen
                        .or_else(|| stored_mode.as_deref().map(mode_from_name))
                        .unwrap_or_default();
                    if chosen.is_some() {
                        remember_mode(&agent_id, mode, cx);
                    }

                    panel.show(agent_id, mode, window, cx);
                })
                .log_err();

            // Only now, and never before: a panel shown holding nothing closes
            // itself, so the dock has to be opened onto an agent that is already
            // standing in it.
            workspace
                .update_in(cx, |workspace, window, cx| {
                    workspace.focus_panel::<crate::agent_panel::AgentPanel>(window, cx);
                })
                .log_err();
        })
        .detach();
    }

    /// Moves an already-open agent to a mode, if it is not there already.
    pub(crate) fn show(
        &mut self,
        mode: AgentViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode == mode {
            // Coming back to a view that gave up on a missing CLI is a retry: it may
            // well have been installed since.
            if matches!(self.state, State::MissingBinary(_)) {
                self.restart(window, cx);
            }
            return;
        }
        self.set_mode(mode, window, cx);
    }

    /// Opens the ACP conversation.
    ///
    /// Nothing is resolved against the local CLI first: the conversation reaches the
    /// agent through its npx adapter, since the CLI itself speaks no ACP. The
    /// install screen belongs to terminal mode, where the binary is what runs.
    pub(crate) fn start_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agent = crate::Agent::Custom {
            id: self.agent.clone(),
        };
        let connection_store = cx.new(|cx| {
            crate::agent_connection_store::AgentConnectionStore::new(self.project.clone(), cx)
        });
        ensure_prompt_store(cx);
        let prompt_store = prompt_store::PromptStore::global(cx);
        let project = self.project.clone();
        let workspace = self.workspace.clone();
        let server = agent.server();

        self._startup = Some(cx.spawn_in(window, async move |this, cx| {
            let prompt_store = prompt_store.await.log_err();
            this.update_in(cx, |this, window, cx| {
                let conversation = cx.new(|cx| {
                    crate::conversation_view::ConversationView::new(
                        server,
                        connection_store,
                        agent,
                        None,
                        None,
                        None,
                        None,
                        None,
                        workspace,
                        project,
                        prompt_store,
                        "agent_view",
                        window,
                        cx,
                    )
                });
                this.state = State::Chat(conversation);
                cx.emit(AgentViewEvent::UpdateTab);
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn new(
        agent: AgentId,
        mode: AgentViewMode,
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            display_name: display_name(&agent),
            agent,
            mode,
            state: State::Starting,
            project,
            workspace,
            focus_handle: cx.focus_handle(),
            _startup: None,
        };
        view.start(window, cx);
        view
    }

    /// Moves this view to the other mode, restarting the agent.
    ///
    /// The conversation and the terminal are two different processes — an npx
    /// adapter against ACP, and the CLI itself — so holding the idle one open
    /// would leave a second agent running for something nobody is looking at. The
    /// cost is that switching ends the session in progress, which is the same
    /// bargain the plan already struck for restoring a tab.
    pub(crate) fn set_mode(
        &mut self,
        mode: AgentViewMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        remember_mode(&self.agent, mode, cx);
        self.restart(window, cx);
        cx.emit(AgentViewEvent::UpdateTab);
    }

    /// Re-runs startup from scratch — the way back after the CLI is installed, and
    /// the way across when the mode changes.
    pub(crate) fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Taken out by name rather than overwritten in place, because what becomes
        // of this value is the whole point. Each mode owns a child process, and the
        // chain that kills one is entirely `Drop`: dropping `TerminalView` drops
        // its `Terminal`, whose `Drop` shuts the pty down and terminates the child;
        // dropping `ConversationView` runs its `on_release`, which closes the ACP
        // sessions and the notification windows before `AcpConnection::drop` kills
        // the npx adapter.
        //
        // Every link fires only on the *last* strong handle. The other holders were
        // audited — `AgentDiff` keeps the thread weakly, `AcpConnectionRegistry`
        // keeps no entity at all, `Project` keeps terminals as weak handles — but an
        // audit does not survive the next change, which is what the check is for.
        self.end_previous_mode(cx);
        self.start(window, cx);
        cx.notify();
    }

    /// Drops whatever the view was showing, and with it the process behind it.
    ///
    /// Separate from `restart` so a test can reach the teardown without also
    /// starting an agent — which is the only way to assert the release, since
    /// `start` would spawn a real one.
    fn end_previous_mode(&mut self, cx: &mut Context<Self>) {
        let previous = std::mem::replace(&mut self.state, State::Starting);
        Self::warn_if_retained(&previous, cx);
        drop(previous);
    }

    /// Reports a mode that outlived the switch away from it.
    ///
    /// A handle held anywhere else costs a whole child process — an npx adapter, or
    /// the CLI under a pty — left running with no view attached to it, invisible
    /// precisely because the surface that showed it is gone. Debug builds only:
    /// this is a tripwire for whoever changes the ownership next, not a check the
    /// shipped app spends anything on.
    ///
    /// One holder trips it legitimately: an open `AgentDiffPane` keeps its
    /// `Entity<AcpThread>` strongly, so reviewing a diff and then switching mode
    /// holds the conversation's connection until that tab closes.
    fn warn_if_retained(previous: &State, cx: &mut Context<Self>) {
        if !cfg!(debug_assertions) {
            return;
        }

        fn watch<T: 'static>(label: &'static str, entity: &Entity<T>, cx: &mut Context<AgentView>) {
            let weak = entity.downgrade();
            cx.spawn(async move |_, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(5))
                    .await;
                if weak.upgrade().is_some() {
                    log::warn!(
                        "agent {label} outlived the switch away from it — something \
                         still holds it, and its child process is still running"
                    );
                }
            })
            .detach();
        }

        match previous {
            State::Chat(view) => watch("conversation", view, cx),
            State::Terminal(view) => watch("terminal", view, cx),
            State::Starting | State::MissingBinary(_) | State::Failed(_) => {}
        }
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
        let agent = self.agent.clone();
        let mode = self.mode;
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

            if mode == AgentViewMode::Chat {
                this.update_in(cx, |this, window, cx| this.start_chat(window, cx))
                    .ok();
                return;
            }

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

fn display_name(agent: &AgentId) -> SharedString {
    builtin_agent(agent.as_ref())
        .map(|builtin| SharedString::from(builtin.display_name))
        .unwrap_or_else(|| agent.0.clone())
}

fn mode_name(mode: AgentViewMode) -> &'static str {
    match mode {
        AgentViewMode::Terminal => "terminal",
        AgentViewMode::Chat => "chat",
    }
}

fn mode_from_name(name: &str) -> AgentViewMode {
    match name {
        "terminal" => AgentViewMode::Terminal,
        _ => AgentViewMode::Chat,
    }
}

/// Writes down which mode this agent should come back in.
///
/// Recorded per agent and not per workspace: it is a habit of the person using
/// the editor, not a property of the project they happen to have open. Claude in
/// conversation and Codex in its terminal is a perfectly ordinary pair of habits,
/// which is why the row is keyed by agent rather than shared between them.
fn remember_mode(agent: &AgentId, mode: AgentViewMode, cx: &mut App) {
    let db = persistence::AgentViewDb::global(cx);
    let agent = agent.to_string();
    cx.background_spawn(async move { db.save_mode(agent, mode_name(mode).to_string()).await })
        .detach_and_log_err(cx);
}

fn ensure_prompt_store(cx: &mut App) {
    if !cx.has_global::<prompt_store::GlobalPromptStore>() {
        prompt_store::init(cx);
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

impl AgentView {
    /// The switch between the conversation and the agent's own terminal.
    ///
    /// The mode is picked when the agent opens — a click on the rail for the
    /// conversation, a right-click for the terminal — which left anyone already
    /// looking at one mode with no way to reach the other, and nothing on screen
    /// naming the two modes at all. This is that way, and that naming.
    fn render_mode_switch(&self, cx: &mut Context<Self>) -> AnyElement {
        ToggleButtonGroup::single_row(
            "agent-view-mode",
            [
                ToggleButtonSimple::new(
                    "Chat",
                    cx.listener(|this, _, window, cx| {
                        this.set_mode(AgentViewMode::Chat, window, cx)
                    }),
                ),
                ToggleButtonSimple::new(
                    "Terminal",
                    cx.listener(|this, _, window, cx| {
                        this.set_mode(AgentViewMode::Terminal, window, cx)
                    }),
                ),
            ],
        )
        .label_size(LabelSize::Small)
        .style(ToggleButtonGroupStyle::Outlined)
        .auto_width()
        .selected_index(match self.mode {
            AgentViewMode::Chat => 0,
            AgentViewMode::Terminal => 1,
        })
        .into_any_element()
    }
}

impl workspace::item::Item for AgentView {
    type Event = AgentViewEvent;

    fn to_item_events(_event: &Self::Event, f: &mut dyn FnMut(workspace::item::ItemEvent)) {
        f(workspace::item::ItemEvent::UpdateTab);
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
        // Both of these need `cx` mutably, so they are built before the theme is
        // borrowed — that borrow lives to the end of the element tree below.
        let mode_switch = self.render_mode_switch(cx);
        let body = match &self.state {
            State::Terminal(terminal) => terminal.clone().into_any_element(),
            State::Chat(conversation) => conversation.clone().into_any_element(),
            State::Starting => centered_message(
                IconName::ArrowCircle,
                format!("Starting {}…", self.display_name).into(),
                None,
                cx,
            ),
            State::MissingBinary(missing) => crate::missing_binary::render(
                &cx.entity(),
                &self.display_name,
                missing,
                self.mode == AgentViewMode::Chat,
                cx,
            ),
            State::Failed(error) => centered_message(IconName::Warning, error.clone(), None, cx),
        };
        let colors = cx.theme().colors();

        // No border and no gutter here: the dock draws its own edge against the
        // editor and its own resize handle, which is exactly the seam this view
        // spent two attempts failing to build for itself from the inside.
        v_flex()
            .id("agent-view")
            .debug_selector(|| "agent-view".into())
            .size_full()
            .bg(colors.editor_background)
            .track_focus(&self.focus_handle)
            .child(
                h_flex()
                    .flex_none()
                    .w_full()
                    .px_2()
                    .py_1()
                    .justify_end()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(mode_switch),
            )
            // `flex_1` with a floor of zero, in a column so it grows along the axis
            // meant: the body's children size themselves against a definite height,
            // and `size_full` here would run them straight through the header.
            .child(v_flex().flex_1().min_h_0().child(body))
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
    use gpui::TestAppContext;
    use project::FakeFs;
    use serde_json::json;
    use settings::SettingsStore;
    use terminal::terminal_settings::{AlternateScroll, CursorShape};
    use util::{path, paths::PathStyle};
    use workspace::MultiWorkspace;

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
    }

    /// Opening a conversation asked for a prompt store that nothing in this fork
    /// had set, and `PromptStore::global` unwraps its global — so the very first
    /// click on a chat agent aborted the process. No test reached that line
    /// because every test that touches the prompt store seeds it itself.
    #[gpui::test]
    fn a_conversation_can_ask_for_the_prompt_store_on_a_bare_app(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(
                !cx.has_global::<prompt_store::GlobalPromptStore>(),
                "no startup path in this fork seeds the prompt store; \
                 if one appears, this view should stop seeding it too"
            );

            ensure_prompt_store(cx);
            drop(prompt_store::PromptStore::global(cx));
        });
    }

    /// Leaving a mode has to end it. Each mode owns a child process — an npx
    /// adapter over ACP, or the agent's CLI under a pty — and nothing kills those
    /// but `Drop`, which runs only on the last strong handle. Assigning over
    /// `state` looks like it is enough and silently is not the moment anything else
    /// keeps a handle, so this asserts the count really reached zero.
    #[gpui::test]
    async fn leaving_a_mode_releases_it(cx: &mut TestAppContext) {
        init_test(cx);
        cx.update(|cx| theme_settings::init(theme::LoadThemes::JustBase, cx));

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/test"), json!({})).await;
        let project = Project::test(fs, [path!("/test").as_ref()], cx).await;

        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
        let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());

        // Display-only: the claim under test is about ownership, and a real pty
        // would be a process this test then has to be trusted to clean up.
        let terminal_view = cx.new_window_entity(|window, cx| {
            let terminal = cx.new(|cx| {
                terminal::TerminalBuilder::new_display_only(
                    CursorShape::default(),
                    AlternateScroll::On,
                    None,
                    0,
                    cx.background_executor(),
                    PathStyle::local(),
                )
                .expect("a display-only terminal needs nothing from the system")
                .subscribe(cx)
            });
            TerminalView::new(
                terminal,
                workspace.downgrade(),
                None,
                project.downgrade(),
                window,
                cx,
            )
        });

        let agent_view = cx.new_window_entity(|_window, cx| AgentView {
            agent: AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
            display_name: "Claude Code".into(),
            mode: AgentViewMode::Terminal,
            state: State::Terminal(terminal_view.clone()),
            project: project.clone(),
            workspace: workspace.downgrade(),
            focus_handle: cx.focus_handle(),
            _startup: None,
        });

        let left_behind = terminal_view.downgrade();
        drop(terminal_view);

        agent_view.update(cx, |view, cx| view.end_previous_mode(cx));
        cx.run_until_parked();

        assert!(
            left_behind.upgrade().is_none(),
            "the terminal outlived the switch away from it, so its child process is still running"
        );
    }

    /// Each agent remembers its own mode, so the rail's plain click can reopen the
    /// one it was last used in.
    ///
    /// The width used to live in this row too, written by a different gesture than
    /// the mode; the dock owns it now, which is why the write no longer has to
    /// guard one column against the other.
    #[gpui::test]
    async fn each_agent_remembers_its_own_mode() {
        // Opened first and under the same name: `agent_views` carries a foreign key
        // into `workspaces`, and sqlez sweeps orphans out of every table that has
        // one as part of migrating. Without the parent table in place that sweep is
        // a query against nothing, and the whole migration fails.
        let _workspaces =
            workspace::WorkspaceDb::open_test_db("each_agent_remembers_its_own_mode").await;
        let db = persistence::AgentViewDb::open_test_db("each_agent_remembers_its_own_mode").await;

        assert_eq!(
            db.preferences("claude-acp".into()).unwrap(),
            None,
            "an agent nobody has opened should have nothing written down"
        );

        db.save_mode("claude-acp".into(), "terminal".into())
            .await
            .unwrap();
        db.save_mode("codex-acp".into(), "chat".into())
            .await
            .unwrap();

        assert_eq!(
            db.preferences("claude-acp".into()).unwrap(),
            Some((Some("terminal".into()), None)),
        );
        assert_eq!(
            db.preferences("codex-acp".into()).unwrap(),
            Some((Some("chat".into()), None)),
            "one agent's choice must not stand for the other's"
        );

        db.save_mode("claude-acp".into(), "chat".into())
            .await
            .unwrap();
        assert_eq!(
            db.preferences("claude-acp".into()).unwrap(),
            Some((Some("chat".into()), None)),
            "a second choice replaces the first"
        );
    }

    /// Two tables store a mode by name — the per-tab row and the per-agent
    /// preference — and a tab restored from one must not disagree with a rail click
    /// reading the other.
    #[gpui::test]
    async fn a_mode_survives_the_round_trip_through_its_name(_cx: &mut TestAppContext) {
        for mode in [AgentViewMode::Chat, AgentViewMode::Terminal] {
            assert_eq!(mode_from_name(mode_name(mode)), mode);
        }
    }

    /// A plain click carries no chosen mode and, the first time an agent is
    /// opened, finds no stored preference either — `AgentView::open` falls
    /// all the way through to this default. The CLI is what most people
    /// already have installed and know how to drive; the chat view is the
    /// one extra hop through an npx adapter.
    #[gpui::test]
    async fn a_first_click_opens_the_cli_rather_than_chat(_cx: &mut TestAppContext) {
        assert_eq!(AgentViewMode::default(), AgentViewMode::Terminal);
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

mod persistence {
    use anyhow::Context as _;
    use db::{
        sqlez::{domain::Domain, thread_safe_connection::ThreadSafeConnection},
        sqlez_macros::sql,
    };
    use workspace::WorkspaceDb;

    pub struct AgentViewDb(ThreadSafeConnection);

    impl Domain for AgentViewDb {
        const NAME: &str = stringify!(AgentViewDb);

        const MIGRATIONS: &[&str] = &[
            sql!(
                CREATE TABLE agent_views(
                    workspace_id INTEGER,
                    item_id INTEGER UNIQUE,

                    agent TEXT,
                    mode TEXT,

                    PRIMARY KEY(workspace_id, item_id),
                    FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                    ON DELETE CASCADE
                ) STRICT;
            ),
            // Deliberately without a workspace: `agent_views` restores the tabs a
            // project had open, while this is how the person likes to work with an
            // agent, and that travels with them from project to project.
            sql!(
                CREATE TABLE agent_preferences(
                    agent TEXT PRIMARY KEY,

                    mode TEXT,
                    width REAL
                ) STRICT;
            ),
        ];
    }

    db::static_connection!(AgentViewDb, [WorkspaceDb]);

    impl AgentViewDb {
        /// The mode and width this agent was last left at, if it ever has been.
        ///
        /// Both columns are nullable and read as a pair: someone can have chosen a
        /// mode without ever having dragged the divider, and the two are written by
        /// different gestures.
        pub fn preferences(
            &self,
            agent: String,
        ) -> anyhow::Result<Option<(Option<String>, Option<f64>)>> {
            let sql_stmt = sql!(
                SELECT mode, width FROM agent_preferences WHERE agent = ?
            );
            self.select_row_bound::<String, (Option<String>, Option<f64>)>(sql_stmt)?(agent)
        }

        pub async fn save_mode(&self, agent: String, mode: String) -> anyhow::Result<()> {
            self.write(move |connection| {
                Self::ensure_row(connection, &agent)?;
                let sql_stmt = sql!(
                    UPDATE agent_preferences SET mode = ? WHERE agent = ?
                );
                let mut query = connection.exec_bound::<(String, String)>(sql_stmt)?;
                query((mode, agent)).context(format!(
                    "exec_bound failed to execute or parse for: {}",
                    sql_stmt
                ))
            })
            .await
        }

        /// Insert-then-update rather than one upsert: the two writers set different
        /// columns, and `INSERT OR REPLACE` would have each of them blank the
        /// other's value on the way past.
        fn ensure_row(
            connection: &db::sqlez::connection::Connection,
            agent: &str,
        ) -> anyhow::Result<()> {
            let sql_stmt = sql!(
                INSERT OR IGNORE INTO agent_preferences(agent) VALUES (?)
            );
            let mut query = connection.exec_bound::<String>(sql_stmt)?;
            query(agent.to_string()).context(format!(
                "exec_bound failed to execute or parse for: {}",
                sql_stmt
            ))
        }
    }
}
