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

        let project = workspace.project().clone();
        let workspace_handle = cx.weak_entity();
        let view = cx.new(|cx| {
            Self::new(
                agent_id.clone(),
                mode,
                project,
                workspace_handle,
                window,
                cx,
            )
        });

        workspace.split_item(agent_split_direction(cx), Box::new(view), window, cx);
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
        };
        view.start(window, cx);
        view
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

impl Render for AgentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();

        div()
            .size_full()
            .bg(colors.editor_background)
            .track_focus(&self.focus_handle)
            .child(match &self.state {
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
            let mode = match mode.as_str() {
                "terminal" => AgentViewMode::Terminal,
                _ => AgentViewMode::Chat,
            };

            cx.update(|window, cx| {
                Ok(cx
                    .new(|cx| Self::new(AgentId::new(agent), mode, project, workspace, window, cx)))
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
        let mode = match self.mode {
            AgentViewMode::Terminal => "terminal",
            AgentViewMode::Chat => "chat",
        };

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

        const MIGRATIONS: &[&str] = &[sql!(
            CREATE TABLE agent_views(
                workspace_id INTEGER,
                item_id INTEGER UNIQUE,

                agent TEXT,
                mode TEXT,

                PRIMARY KEY(workspace_id, item_id),
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                ON DELETE CASCADE
            ) STRICT;
        )];
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
