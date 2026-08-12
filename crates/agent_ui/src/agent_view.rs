use gpui::{
    AnyElement, App, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Task, WeakEntity,
    Window, canvas,
};
use project::{AgentBinary, AgentBinaryMissing, AgentId, Project, builtin_agent};
use task::{HideStrategy, RevealStrategy, SpawnInTerminal, TaskId};
use terminal_view::TerminalView;
use ui::{ToggleButtonGroup, ToggleButtonGroupStyle, ToggleButtonSimple, prelude::*};
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
    /// The width this agent was last left at, waiting for a laid-out pane to
    /// measure against. Cleared once applied — or once it is clear it cannot be.
    pending_width: Option<Pixels>,
    /// Last width written down, so an unchanged frame costs nothing.
    recorded_width: Option<Pixels>,
    _record_width: Option<Task<()>>,
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

    /// Brings an agent tab forward — the response to accepting a notification.
    pub fn activate_for_agent(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
        workspace.update(cx, |workspace, cx| {
            let chat = workspace
                .items_of_type::<AgentView>(cx)
                .find(|view| matches!(view.read(cx).state, State::Chat(_)));
            if let Some(chat) = chat {
                workspace.activate_item(&chat, true, true, window, cx);
            }
        });
    }
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
        mode: Option<AgentViewMode>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let agent_id = AgentId::new(agent.to_string());

        // Resolved in its own statement: the iterator borrows the workspace, and
        // activating the item needs it back mutably.
        let existing = Self::already_open(workspace, &agent_id, cx);
        if let Some(existing) = existing {
            workspace.activate_item(&existing, true, true, window, cx);
            existing.update(cx, |view, cx| {
                // Asking for a particular mode has to move this view to it. Without
                // this the rail's right-click just re-focuses whichever mode is
                // already open, which reads as the button doing nothing at all. A
                // plain click asks for no mode in particular and leaves it alone.
                if let Some(mode) = mode {
                    view.set_mode(mode, window, cx);
                }
                // The CLI may well have been installed since this view gave up on
                // it, so coming back to it is a retry rather than a look at a
                // stale verdict.
                if matches!(view.state, State::MissingBinary(_)) {
                    view.restart(window, cx);
                }
            });
            return;
        }

        let project = workspace.project().clone();
        let workspace_handle = cx.weak_entity();
        let db = persistence::AgentViewDb::global(cx);
        let stored_agent = agent_id.to_string();

        // Opening waits on one indexed sqlite read for the mode and width this
        // agent was last left at. The alternative — opening in one mode and jumping
        // to the other a frame later — is worse to watch than a tab that arrives a
        // beat late.
        cx.spawn_in(window, async move |workspace, cx| {
            let remembered = cx
                .background_executor()
                .spawn(async move { db.preferences(stored_agent) })
                .await
                .log_err()
                .flatten()
                .unwrap_or_default();

            workspace
                .update_in(cx, |workspace, window, cx| {
                    // Checked again on this side of the await: two quick clicks both
                    // miss the first check, and the second would otherwise open a
                    // duplicate of the pane the first one is still building.
                    if let Some(existing) = Self::already_open(workspace, &agent_id, cx) {
                        workspace.activate_item(&existing, true, true, window, cx);
                        return;
                    }

                    let (stored_mode, stored_width) = remembered;
                    let chosen = mode;
                    let mode = chosen
                        .or_else(|| stored_mode.as_deref().map(mode_from_name))
                        .unwrap_or_default();
                    // A choice is what gets remembered; a plain click never
                    // overwrites the very preference it just read.
                    if chosen.is_some() {
                        remember_mode(&agent_id, mode, cx);
                    }

                    let view = cx.new(|cx| {
                        Self::new(
                            agent_id.clone(),
                            mode,
                            project,
                            workspace_handle,
                            stored_width.map(|width| px(width as f32)),
                            window,
                            cx,
                        )
                    });

                    workspace.split_item(agent_split_direction(cx), Box::new(view), window, cx);
                })
                .log_err();
        })
        .detach();
    }

    fn already_open(workspace: &Workspace, agent: &AgentId, cx: &App) -> Option<Entity<AgentView>> {
        workspace
            .items_of_type::<AgentView>(cx)
            .find(|view| view.read(cx).agent == *agent)
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

    fn new(
        agent: AgentId,
        mode: AgentViewMode,
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        width: Option<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let display_name = builtin_agent(agent.as_ref())
            .map(|builtin| SharedString::from(builtin.display_name))
            .unwrap_or_else(|| agent.0.clone());

        let mut view = Self {
            agent,
            display_name,
            mode,
            state: State::Starting,
            project,
            workspace,
            focus_handle: cx.focus_handle(),
            _startup: None,
            pending_width: width,
            recorded_width: None,
            _record_width: None,
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

    /// Keeps the pane's width in step with what this agent was left at.
    ///
    /// Handed this view's own laid-out width from paint, rather than asking the
    /// workspace for its pane's bounds. That question deadlocks the entire
    /// application: `PaneAxis::bounding_box_for_pane` takes a lock the pane group
    /// is already holding while it renders its children, so an item asking it from
    /// inside `render` parks the main thread against itself, and the window stops
    /// drawing altogether. Everything here that touches the workspace is deferred
    /// past the end of the frame for the same reason.
    fn width_measured(&mut self, width: Pixels, window: &mut Window, cx: &mut Context<Self>) {
        if width <= px(0.) {
            return;
        }

        if let Some(target) = self.pending_width.take() {
            let delta = target - width;
            if delta.abs() > px(1.) {
                cx.defer_in(window, move |this, window, cx| {
                    let Some(workspace) = this.workspace.upgrade() else {
                        return;
                    };
                    // `resize_pane` moves whichever pane is active, and the one just
                    // opened is exactly that. A restored tab is not: it comes back
                    // with the editor focused, and there the pane group's own
                    // serialized flexes already hold the layout — resizing then
                    // would drag a pane the user is working in.
                    let item = cx.entity();
                    let is_active = workspace.read_with(cx, |workspace, _| {
                        workspace
                            .pane_for(&item)
                            .is_some_and(|pane| workspace.active_pane() == &pane)
                    });
                    if !is_active {
                        return;
                    }

                    workspace.update(cx, |workspace, cx| {
                        workspace.resize_pane(gpui::Axis::Horizontal, delta, window, cx);
                    });
                });
            }
            return;
        }

        // Under four pixels is the divider being dragged past; four is someone
        // meaning it.
        if self
            .recorded_width
            .is_some_and(|recorded| (recorded - width).abs() < px(4.))
        {
            return;
        }
        self.recorded_width = Some(width);

        let db = persistence::AgentViewDb::global(cx);
        let agent = self.agent.to_string();
        self._record_width = Some(cx.spawn(async move |_, cx| {
            // Debounced, because a drag crosses dozens of widths and only the one it
            // stops at is worth a row. Held in a field, so the drop that ends this
            // view also cancels a write it no longer means.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(400))
                .await;
            db.save_width(agent, width.to_f64()).await.log_err();
        }));
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

/// Seeds the prompt store the first time a conversation asks for it.
///
/// Upstream seeded it from `rules_library::init`, called out of `agent_ui::init`;
/// this fork removed that crate, so the global had no owner left and
/// `PromptStore::global` panicked on a store nobody had set. Seeding it here
/// rather than at startup keeps an LMDB open off the cold path of every session
/// that never opens an agent — the store is a shared future, so the first
/// conversation pays for it and the rest join.
/// The name a mode is stored under, in both the per-tab row and the per-agent
/// preference. One spelling, so a tab restored from one cannot disagree with a
/// rail click reading the other.
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

/// Room between the agent pane and the editor next to it, so the two do not read
/// as one surface.
///
/// It sits inside this view rather than in the workspace's pane group: widening
/// the divider there would push apart every split in the window, and the editor
/// next to an agent is the only seam this is about.
const EDITOR_GAP: Pixels = px(8.);

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

impl Render for AgentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Nothing here may ask the workspace about this view's pane — see
        // `width_measured`. The width arrives from the canvas below instead, out of
        // paint, where no pane-group lock is held.
        let measure = {
            let view = cx.entity();
            canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    view.update(cx, |this, cx| {
                        this.width_measured(bounds.size.width, window, cx)
                    });
                },
            )
            .absolute()
            .size_full()
        };

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

        // Which edge faces the editor follows the side the agent opened on, the
        // same setting `agent_split_direction` reads to open it there.
        let opens_left = matches!(agent_split_direction(cx), SplitDirection::Left);

        div()
            .size_full()
            .bg(colors.background)
            .track_focus(&self.focus_handle)
            .when(opens_left, |this| this.pr(EDITOR_GAP))
            .when(!opens_left, |this| this.pl(EDITOR_GAP))
            .child(measure)
            .child(
                v_flex()
                    .size_full()
                    .bg(colors.editor_background)
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
                    // `flex_1` with a floor of zero, in a column so it grows along
                    // the axis meant: the body's children size themselves against a
                    // definite height, and `size_full` here would run them straight
                    // through the header.
                    .child(v_flex().flex_1().min_h_0().child(body)),
            )
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

    /// Drawing the view inside a real pane, which is the only place the bug this
    /// guards against can appear.
    ///
    /// Asking the workspace for this view's pane bounds during `render` takes a
    /// lock the pane group already holds while it renders its children, and parks
    /// the main thread against itself — the whole window stops, not just the agent.
    /// A regression here does not fail this test, it **hangs** it; that is the
    /// signal, and the stack will point straight at the culprit.
    #[gpui::test]
    async fn the_view_draws_inside_a_pane_without_deadlocking(cx: &mut TestAppContext) {
        init_test(cx);
        cx.update(|cx| theme_settings::init(theme::LoadThemes::JustBase, cx));

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/test"), json!({})).await;
        let project = Project::test(fs, [path!("/test").as_ref()], cx).await;

        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
        let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());

        // Built by hand rather than through `open`: starting for real would resolve
        // the agent's CLI and spawn it, and this is about the frame, not the agent.
        let view = cx.new_window_entity(|_window, cx| AgentView {
            agent: AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
            display_name: "Claude Code".into(),
            mode: AgentViewMode::Chat,
            state: State::Starting,
            project: project.clone(),
            workspace: workspace.downgrade(),
            focus_handle: cx.focus_handle(),
            _startup: None,
            pending_width: Some(px(400.)),
            recorded_width: None,
            _record_width: None,
        });

        // Split, not added to the active pane: with one pane the group's root is a
        // `Member::Pane` and `bounding_box_for_pane` returns before it touches a
        // lock. The deadlock needs the `Member::Axis` that a second pane creates —
        // which is exactly how an agent opens.
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.split_item(SplitDirection::Right, Box::new(view), window, cx);
        });

        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
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
            pending_width: None,
            recorded_width: None,
            _record_width: None,
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

    /// The mode and the width are written by two different gestures — a click on
    /// the switch, a drag on the divider — and each writes one column. A plain
    /// `INSERT OR REPLACE` would have each of them quietly blank the other's value
    /// on the way past, which is why the write is an insert followed by an update.
    #[gpui::test]
    async fn writing_one_preference_leaves_the_other_alone() {
        // Opened first and under the same name: `agent_views` carries a foreign key
        // into `workspaces`, and sqlez sweeps orphans out of every table that has
        // one as part of migrating. Without the parent table in place that sweep is
        // a query against nothing, and the whole migration fails.
        let _workspaces =
            workspace::WorkspaceDb::open_test_db("writing_one_preference_leaves_the_other_alone")
                .await;
        let db =
            persistence::AgentViewDb::open_test_db("writing_one_preference_leaves_the_other_alone")
                .await;

        assert_eq!(
            db.preferences("claude-acp".into()).unwrap(),
            None,
            "an agent nobody has opened should have nothing written down"
        );

        db.save_width("claude-acp".into(), 480.).await.unwrap();
        db.save_mode("claude-acp".into(), "terminal".into())
            .await
            .unwrap();
        assert_eq!(
            db.preferences("claude-acp".into()).unwrap(),
            Some((Some("terminal".into()), Some(480.))),
            "choosing a mode must not forget the width"
        );

        // The other order, because the two gestures have no fixed sequence.
        db.save_mode("codex-acp".into(), "chat".into())
            .await
            .unwrap();
        db.save_width("codex-acp".into(), 320.).await.unwrap();
        assert_eq!(
            db.preferences("codex-acp".into()).unwrap(),
            Some((Some("chat".into()), Some(320.))),
            "dragging the divider must not forget the mode"
        );

        db.save_mode("claude-acp".into(), "chat".into())
            .await
            .unwrap();
        assert_eq!(
            db.preferences("claude-acp".into()).unwrap(),
            Some((Some("chat".into()), Some(480.))),
            "a second choice replaces the first without touching the width"
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

impl workspace::item::SerializableItem for AgentView {
    fn serialized_item_kind() -> &'static str {
        "AgentView"
    }

    fn cleanup(
        workspace_id: workspace::WorkspaceId,
        alive_items: Vec<workspace::ItemId>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<()>> {
        let db = persistence::AgentViewDb::global(cx);
        cx.background_spawn(async move { db.delete_unloaded(workspace_id, alive_items).await })
    }

    /// Restores the tab, not the conversation.
    ///
    /// The agent and the mode are all that is kept: a thread is the agent's own
    /// state, reachable through its CLI (`claude --resume`) rather than anything
    /// this editor could reconstruct. Restoring a tab and lying about its history
    /// would be worse than restoring an empty one.
    fn deserialize(
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        workspace_id: workspace::WorkspaceId,
        item_id: workspace::ItemId,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Entity<Self>>> {
        let db = persistence::AgentViewDb::global(cx);
        window.spawn(cx, async move |cx| {
            let (agent, mode) = db.get_agent(item_id, workspace_id)?;
            let mode = mode_from_name(&mode);

            cx.update(|window, cx| {
                Ok(cx.new(|cx| {
                    // No width to restore here: a workspace comes back with its own
                    // serialized pane flexes, and forcing a remembered width on top
                    // of them would fight the layout the user actually left.
                    Self::new(
                        AgentId::new(agent),
                        mode,
                        project,
                        workspace,
                        None,
                        window,
                        cx,
                    )
                }))
            })?
        })
    }

    fn serialize(
        &mut self,
        workspace: &mut Workspace,
        item_id: workspace::ItemId,
        _closing: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<anyhow::Result<()>>> {
        let workspace_id = workspace.database_id()?;
        let agent = self.agent.to_string();
        let mode = mode_name(self.mode);

        let db = persistence::AgentViewDb::global(cx);
        Some(cx.background_spawn(async move {
            db.save_agent(item_id, workspace_id, agent, mode.to_string())
                .await
        }))
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }
}

mod persistence {
    use anyhow::Context as _;
    use db::{
        sqlez::{domain::Domain, thread_safe_connection::ThreadSafeConnection},
        sqlez_macros::sql,
    };
    use workspace::{ItemId, WorkspaceDb, WorkspaceId};

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
        pub async fn save_agent(
            &self,
            item_id: ItemId,
            workspace_id: WorkspaceId,
            agent: String,
            mode: String,
        ) -> anyhow::Result<()> {
            self.write(move |connection| {
                let sql_stmt = sql!(
                    INSERT OR REPLACE INTO agent_views(item_id, workspace_id, agent, mode)
                    VALUES (?, ?, ?, ?)
                );
                let mut query =
                    connection.exec_bound::<(ItemId, WorkspaceId, String, String)>(sql_stmt)?;
                query((item_id, workspace_id, agent, mode)).context(format!(
                    "exec_bound failed to execute or parse for: {}",
                    sql_stmt
                ))
            })
            .await
        }

        pub fn get_agent(
            &self,
            item_id: ItemId,
            workspace_id: WorkspaceId,
        ) -> anyhow::Result<(String, String)> {
            let sql_stmt = sql!(
                SELECT agent, mode FROM agent_views WHERE item_id = ? AND workspace_id = ?
            );
            self.select_row_bound::<(ItemId, WorkspaceId), (String, String)>(sql_stmt)?((
                item_id,
                workspace_id,
            ))?
            .context("no agent view saved for this item")
        }

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

        pub async fn save_width(&self, agent: String, width: f64) -> anyhow::Result<()> {
            self.write(move |connection| {
                Self::ensure_row(connection, &agent)?;
                let sql_stmt = sql!(
                    UPDATE agent_preferences SET width = ? WHERE agent = ?
                );
                let mut query = connection.exec_bound::<(f64, String)>(sql_stmt)?;
                query((width, agent)).context(format!(
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

        pub async fn delete_unloaded(
            &self,
            workspace_id: WorkspaceId,
            alive_items: Vec<ItemId>,
        ) -> anyhow::Result<()> {
            let placeholders = alive_items
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(", ");
            let query = format!(
                "DELETE FROM agent_views WHERE workspace_id = ? AND item_id NOT IN ({placeholders})"
            );
            self.write(move |connection| {
                let mut statement = db::sqlez::statement::Statement::prepare(connection, query)?;
                let mut next_index = statement.bind(&workspace_id, 1)?;
                for id in alive_items {
                    next_index = statement.bind(&id, next_index)?;
                }
                statement.exec()
            })
            .await
        }
    }
}
