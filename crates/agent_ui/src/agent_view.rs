use crate::RenameAgent;
use agent_sessions::{AgentKind, Fork, ResumeCommand};
use editor::Editor;
use gpui::{
    AnyElement, App, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Subscription,
    Task, WeakEntity, Window,
};
use project::{AgentBinary, AgentBinaryMissing, AgentId, Project, builtin_agent};
use task::{HideStrategy, RevealStrategy, SpawnInTerminal, TaskId};
use terminal::Terminal;
use terminal_view::TerminalView;
use ui::prelude::*;
use workspace::{Pane, Workspace};
use zed_actions::agent::AgentViewMode;

use std::process::ExitStatus;
use util::ResultExt as _;

/// The rail draws hard-coded buttons for the built-in agents, and the tab has to
/// carry the same glyph. Match arms rather than a lookup through
/// `AgentServerStore`: `project` cannot depend on the icon crate, and another
/// agent is a deliberate change to both places, not an accident.
pub fn agent_icon(agent: &str) -> IconName {
    match agent {
        project::CLAUDE_CODE_AGENT_ID => IconName::AiClaude,
        project::CODEX_AGENT_ID => IconName::AiOpenAi,
        project::ANTIGRAVITY_AGENT_ID => IconName::AiAntigravity,
        project::COPILOT_AGENT_ID => IconName::AiCopilot,
        _ => IconName::Sparkle,
    }
}

pub struct AgentView {
    /// Which session this tab belongs to. See [`SessionIntent`].
    intent: SessionIntent,
    agent: AgentId,
    display_name: SharedString,
    /// What the user called this session, if they named it. Two Claude Code tabs
    /// are otherwise the same word twice, which is no help in telling apart the
    /// two conversations the `+` menu exists to let you run.
    custom_name: Option<SharedString>,
    rename_editor: Option<Entity<Editor>>,
    _rename_subscription: Option<Subscription>,
    mode: AgentViewMode,
    state: State,
    project: Entity<Project>,
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    self_handle: WeakEntity<Self>,
    _startup: Option<Task<()>>,
    /// Watching the agent's CLI for the moment it ends, so the tab can hand back
    /// a shell.
    ///
    /// A field rather than detached: a tab closed while the agent is still
    /// running has to stop watching, or it builds a shell for a view that is
    /// already gone. Reused afterwards for the build itself, which the completed
    /// watch no longer needs.
    _exit_watch: Option<Task<()>>,
    /// Carries the shell's own `CloseItem` outward. See [`AgentView::hand_back_a_shell`].
    _shell_close: Option<Subscription>,
}

enum State {
    Starting,
    Terminal(Entity<TerminalView>),
    /// The agent's own CLI is not on this machine. Carries what the install
    /// screen needs, so the view never has to ask which agent it is showing for.
    MissingBinary(AgentBinaryMissing),
    /// This tab owns a session id, the agent's store no longer holds it, and this
    /// agent cannot be told to start one under that id.
    ///
    /// Reachable for Codex and Copilot only. Claude's `--session-id` means
    /// `new_session_command` always answers, so the branch that leads here is
    /// unreachable for it — deliberately left as a real state rather than an
    /// assertion, so the day Claude drops that flag this simply starts working.
    ///
    /// Silently opening a blank session instead would be worse: a tab that looks
    /// like a continuation and has none of the context is the failure this whole
    /// feature exists to prevent.
    SessionGone,
    Failed(SharedString),
    /// The agent's CLI ended and the tab handed back a shell in its folder.
    ///
    /// Still this agent's tab: the name, the icon and the session id are all
    /// untouched, so the existing restart puts the agent back on the same
    /// conversation. The only thing that changed is what the pty holds.
    Shell(Entity<TerminalView>),
}

pub enum AgentViewEvent {
    UpdateTab,
    /// The shell this tab handed back has exited, so the tab goes with it.
    Close,
}

impl AgentView {
    /// Brings this agent's tab forward — the response to accepting a
    /// notification.
    ///
    /// The tab is activated where it stands rather than moved: someone who split
    /// the agent out into its own pane put it there deliberately, and answering a
    /// notification is no reason to undo that.
    ///
    /// `agent` is not optional, and taking "whichever tab comes first" here would
    /// be wrong rather than merely loose. The dock version of this revealed the
    /// whole column, which held every open agent at once, so the question never
    /// arose; a tab is one item and only one can be in front, so a notification
    /// from Codex must not surface a Claude Code tab that happens to be earlier in
    /// pane order.
    pub fn activate_for_agent(
        workspace: Entity<Workspace>,
        agent: &AgentId,
        window: &mut Window,
        cx: &mut App,
    ) {
        workspace.update(cx, |workspace, cx| {
            let Some(view) = workspace
                .items_of_type::<AgentView>(cx)
                .find(|view| view.read(cx).is_agent(agent))
            else {
                return;
            };
            workspace.activate_item(&view, true, true, window, cx);
        });
    }

    /// Public because the rail asks it: a button is lit when a tab for its agent
    /// is open, and the rail lives in another crate.
    pub fn is_agent(&self, agent: &AgentId) -> bool {
        &self.agent == agent
    }
}

impl EventEmitter<AgentViewEvent> for AgentView {}

impl AgentView {
    /// Routes a rail click to a tab of the editor.
    ///
    /// The agent is an item of the centre panes, so it lands wherever the editor's
    /// own items land — beside the file being read, in one tab bar with it.
    pub fn open(
        workspace: &mut Workspace,
        agent: &str,
        mode: Option<AgentViewMode>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::open_inner(workspace, agent, mode, false, window, cx);
    }

    /// Same, but always starts a fresh session even if one is already running.
    ///
    /// The `+` menu in the tab bar is the only caller: opening a second session
    /// is a deliberate act, so nothing that could be a stray click reaches it.
    pub fn open_new(
        workspace: &mut Workspace,
        agent: &str,
        mode: Option<AgentViewMode>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::open_inner(workspace, agent, mode, true, window, cx);
    }

    /// Starts a fresh thread in a pane the caller names.
    ///
    /// For panes that are not the editor's -- the floating window is the only
    /// caller today. Always a new thread, and never the one already running: an
    /// item lives in exactly one pane, so reusing an open thread would move it
    /// out of wherever somebody left it.
    pub fn open_in_pane(
        workspace: &mut Workspace,
        pane: Entity<Pane>,
        agent: &str,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::open_into(workspace, agent, None, true, Some(pane), window, cx);
    }

    fn open_inner(
        workspace: &mut Workspace,
        agent: &str,
        mode: Option<AgentViewMode>,
        always_new: bool,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::open_into(workspace, agent, mode, always_new, None, window, cx);
    }

    /// The one body behind every entry point above.
    ///
    /// `destination` of `None` means the workspace's active pane, which is what
    /// every caller but the floating window wants.
    fn open_into(
        workspace: &mut Workspace,
        agent: &str,
        mode: Option<AgentViewMode>,
        always_new: bool,
        destination: Option<Entity<Pane>>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let agent_id = AgentId::new(agent.to_string());
        let project = workspace.project().clone();
        let db = persistence::AgentViewDb::global(cx);
        let stored_agent = agent_id.to_string();
        // Deferred through `spawn_in` rather than run here, and not only to await
        // the remembered mode: this body is reached from a `register_action`
        // handler, which holds the workspace leased. Reaching back through the
        // handle under that lease aborts the process — a trap this crate has paid
        // for more than once.
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
                    // A choice is what gets remembered; a plain click never
                    // overwrites the very preference it just read.
                    let chosen = mode;
                    let mode = chosen
                        .or_else(|| remembered.as_deref().map(mode_from_name))
                        .unwrap_or_default();
                    if chosen.is_some() {
                        remember_mode(&agent_id, mode, cx);
                    }

                    // A plain click comes back to the session already running,
                    // wherever its tab ended up — including a pane the user split
                    // it out into. Starting a second process for the same agent is
                    // `open_new`'s job, and only the `+` menu reaches that.
                    if !always_new && let Some(view) = Self::already_open(workspace, &agent_id, cx)
                    {
                        workspace.activate_item(&view, true, true, window, cx);
                        view.update(cx, |view, cx| view.show(mode, window, cx));
                        return;
                    }

                    let intent = intent_for_new_tab(&agent_id);
                    let view = cx.new(|cx| {
                        Self::new(
                            agent_id,
                            mode,
                            project,
                            workspace.weak_handle(),
                            intent,
                            window,
                            cx,
                        )
                    });
                    // The active pane, so the agent arrives beside whatever is
                    // being read rather than in a place of its own. The other
                    // half of that bargain is that a file opened from here joins
                    // the same tab bar — which is the point, not a leak.
                    //
                    // Unless a caller named somewhere else, which only a pane
                    // outside the editor ever does.
                    match destination {
                        Some(pane) => pane.update(cx, |pane: &mut Pane, cx| {
                            pane.add_item(Box::new(view), true, true, None, window, cx);
                        }),
                        None => workspace.add_item_to_active_pane(
                            Box::new(view),
                            None,
                            true,
                            window,
                            cx,
                        ),
                    }
                })
                .log_err();
        })
        .detach();
    }

    /// The tab already showing this agent, if one is.
    fn already_open(workspace: &Workspace, agent: &AgentId, cx: &App) -> Option<Entity<AgentView>> {
        workspace
            .items_of_type::<AgentView>(cx)
            .find(|view| view.read(cx).is_agent(agent))
    }

    /// Steps back off this agent's tab to whatever was being read before it.
    ///
    /// This is what makes the rail button a toggle now that there is no dock to
    /// close: a lit button that does nothing when pressed was the original
    /// complaint, and it would be the same complaint here. Put away rather than
    /// closed — the tab keeps its place, its scroll and its live process.
    ///
    /// Answers `false` when the press is not a put-away, which is the caller's
    /// signal to open instead. Only the **active** pane's active item counts: an
    /// agent split out into another pane is somewhere the user put it on purpose,
    /// and a press while reading code there means "take me to it".
    pub(crate) fn put_away(
        workspace: &Workspace,
        agent: &AgentId,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let pane = workspace.active_pane().clone();

        // The whole read is taken through one borrow. Reaching for the pane again
        // partway through is how this crate has previously turned a stale read
        // into an abort.
        let previous = {
            let pane = pane.read(cx);
            let showing_this_agent = pane
                .active_item()
                .and_then(|item| item.downcast::<AgentView>())
                .is_some_and(|view| view.read(cx).is_agent(agent));
            if !showing_this_agent {
                return false;
            }

            // The last entry is the agent that was just activated —
            // `Pane::update_history` dedupes then pushes — so the one before it is
            // where the press goes. Entries are walked past rather than trusted:
            // an item closed since leaves its own behind.
            pane.activation_history()
                .iter()
                .rev()
                .skip(1)
                .find_map(|entry| {
                    pane.items()
                        .position(|item| item.item_id() == entry.entity_id)
                })
        };

        // Nothing to step back to — a pane holding only the agent stays exactly as
        // it is. Closing the tab instead would end a live session over the second
        // press of a button whose entire job is to be pressed twice.
        if let Some(index) = previous {
            pane.update(cx, |pane, cx| {
                pane.activate_item(index, true, true, window, cx);
            });
        }
        true
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

    pub(crate) fn new(
        agent: AgentId,
        mode: AgentViewMode,
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        intent: SessionIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self {
            intent,
            display_name: display_name(&agent),
            agent,
            custom_name: None,
            rename_editor: None,
            _rename_subscription: None,
            mode,
            state: State::Starting,
            project,
            workspace,
            focus_handle: cx.focus_handle(),
            self_handle: cx.entity().downgrade(),
            _startup: None,
            _exit_watch: None,
            _shell_close: None,
        };
        view.start(window, cx);
        view
    }

    /// Opens a session that already exists, in the directory it ran in.
    ///
    /// Always a new tab: resuming is not "show me the agent", it is "run this
    /// conversation again", and the session it continues may have nothing to do
    /// with whatever tab is already open.
    /// Takes the session id alone, not a built command. The flags that resume it
    /// are the agent store's business, and rebuilding them here would be a second
    /// place to update the day a CLI changes one. The cost is one store lookup on
    /// the click path, on a background thread.
    pub fn open_tracked(
        workspace: &mut Workspace,
        agent: &str,
        intent: SessionIntent,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let agent_id = AgentId::new(agent.to_string());
        let project = workspace.project().clone();
        let weak_workspace = workspace.weak_handle();
        // Deferred for the reason `open_inner` is: this runs under a handler that
        // holds the workspace leased, and reaching back through the handle while
        // it is leased aborts the process.
        cx.spawn_in(window, async move |workspace, cx| {
            workspace
                .update_in(cx, |workspace, window, cx| {
                    let view = cx.new(|cx| {
                        Self::new(
                            agent_id,
                            AgentViewMode::Terminal,
                            project,
                            weak_workspace,
                            intent,
                            window,
                            cx,
                        )
                    });
                    workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
                })
                .log_err();
        })
        .detach();
    }

    /// A view that never starts its agent.
    ///
    /// `Self::new` calls `start`, which resolves the agent's CLI against the real
    /// `PATH` and — when it finds one — runs it under a pty. In a test that is a
    /// live child process doing real I/O the deterministic test scheduler cannot
    /// account for, which it reports as a non-determinism failure. It is also
    /// machine-dependent: the same test passes or fails depending on whether the
    /// developer happens to have `claude` installed.
    ///
    /// So tests whose claim is about the *tab* — which one is in front, what its
    /// label says, what gets written down — build the view this way and never
    /// touch a process. Tests whose claim is about the open path itself still go
    /// through `AgentView::open`.
    #[cfg(test)]
    pub(crate) fn test_new(
        agent: AgentId,
        mode: AgentViewMode,
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        intent: SessionIntent,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            intent,
            display_name: display_name(&agent),
            agent,
            custom_name: None,
            rename_editor: None,
            _rename_subscription: None,
            mode,
            state: State::Starting,
            project,
            workspace,
            focus_handle: cx.focus_handle(),
            self_handle: cx.entity().downgrade(),
            _startup: None,
            _exit_watch: None,
            _shell_close: None,
        }
    }

    /// The open rename editor, if the tab is being renamed.
    #[cfg(test)]
    pub(crate) fn rename_editor(&self) -> Option<&Entity<Editor>> {
        self.rename_editor.as_ref()
    }

    /// The terminal this agent's CLI runs in, once it has started.
    ///
    /// Public for `keep_awake`, which needs the terminal's task status: an agent
    /// tab outliving its CLI is deliberate (`agent_task` sets
    /// `HideStrategy::Never`), so the tab being open says nothing about whether
    /// the process is still alive. Only the task status does.
    pub fn terminal(&self) -> Option<&Entity<TerminalView>> {
        match &self.state {
            State::Terminal(view) => Some(view),
            // Deliberately not the shell. This answers "what is the agent
            // running in", and a shell handed back after the agent ended is not
            // that -- `keep_awake` reading its absent task status as "working"
            // would hold the display awake for a bare prompt.
            State::Shell(_)
            | State::Starting
            | State::MissingBinary(_)
            | State::SessionGone
            | State::Failed(_) => None,
        }
    }

    /// Whether this agent's CLI is still running.
    ///
    /// Not "is the tab open": `agent_task` sets `HideStrategy::Never`, so a tab
    /// deliberately outlives the process it hosted. Only the terminal's task
    /// status distinguishes an agent that is working from a tab left behind by
    /// one that finished.
    ///
    /// A terminal that vanished without reporting an exit code reads as not
    /// working -- the same call `keep_awake` makes, for the same reason: a
    /// status nobody will ever update must not be treated as activity.
    pub fn is_working(&self, cx: &App) -> bool {
        self.terminal().is_some_and(|terminal_view| {
            terminal_view
                .read(cx)
                .terminal()
                .read(cx)
                .task()
                .is_some_and(|task| task.status == ::terminal::TaskStatus::Running)
        })
    }

    /// What the tab shows: the name the user gave this session, or the agent's.
    pub fn tab_label(&self) -> SharedString {
        self.custom_name
            .clone()
            .unwrap_or_else(|| self.display_name.clone())
    }

    /// Opens the inline editor over the tab's label.
    ///
    /// Same shape as `TerminalView::rename_terminal`, deliberately: a rename that
    /// behaves differently from the one two tabs over is worse than a duplicated
    /// forty lines. Sharing it would mean a widget owning an editor, a blur
    /// subscription and an `Item` integration — larger than the feature.
    fn start_renaming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| Editor::single_line(window, cx));
        let subscription = cx.subscribe_in(&editor, window, {
            let editor = editor.clone();
            move |_this, _, event, window, cx| {
                if let editor::EditorEvent::Blurred = event {
                    // Deferred so a double-click that lands inside the editor does
                    // not cancel the rename it just opened.
                    let editor = editor.clone();
                    cx.defer_in(window, move |this, window, cx| {
                        let still_open = this
                            .rename_editor
                            .as_ref()
                            .is_some_and(|current| current == &editor);
                        if still_open && !editor.focus_handle(cx).is_focused(window) {
                            this.finish_renaming(false, window, cx);
                        }
                    });
                }
            }
        });

        let current = self.tab_label();
        self.rename_editor = Some(editor.clone());
        self._rename_subscription = Some(subscription);
        editor.update(cx, |editor, cx| {
            editor.set_text(current, window, cx);
            editor.select_all(&editor::actions::SelectAll, window, cx);
            editor.focus_handle(cx).focus(window, cx);
        });
        cx.notify();
    }

    fn finish_renaming(&mut self, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.rename_editor.take() else {
            return;
        };
        self._rename_subscription = None;
        if save {
            let typed = editor.read(cx).text(cx).trim().to_string();
            // Blank, or unchanged from the agent's own name, means "no name of its
            // own" rather than a name that happens to match — so clearing the box
            // is how a session goes back to being called Claude Code.
            self.custom_name = (!typed.is_empty() && typed != self.display_name.as_ref())
                .then(|| SharedString::from(typed));
            cx.emit(AgentViewEvent::UpdateTab);
        }
        cx.notify();
        self.focus_handle.focus(window, cx);
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
        // Dropped with the mode it was watching. A watch left running would be
        // waiting on a terminal nobody can see any more, and `start` is about to
        // install the one that replaces it.
        self._exit_watch = None;
        self._shell_close = None;
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
            State::Terminal(view) => watch("terminal", view, cx),
            State::Shell(view) => watch("shell", view, cx),
            State::Starting | State::MissingBinary(_) | State::SessionGone | State::Failed(_) => {}
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

    /// Waits for the agent's CLI to end, and hands the tab back to a shell when
    /// the way it ended says the person meant to leave it.
    ///
    /// One task from the wait through to the new terminal, deliberately. Storing
    /// the second half in this same field would mean dropping the handle of the
    /// task that is running at that moment, and a reader should not have to work
    /// out whether that is survivable.
    fn watch_for_exit(
        &mut self,
        completed: Task<Option<ExitStatus>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._exit_watch = Some(cx.spawn_in(window, async move |this, cx| {
            if !ends_at_a_prompt(completed.await) {
                return;
            }

            let Ok(building) = this.update(cx, |this, cx| this.build_a_shell(cx)) else {
                return;
            };
            let terminal = match building.await {
                Ok(terminal) => terminal,
                Err(error) => {
                    log::error!("could not open a shell after the agent ended: {error}");
                    return;
                }
            };

            this.update_in(cx, |this, window, cx| {
                // Asked before the swap, because after it the handle belongs to
                // the shell and the answer is always no. If the pty that just
                // died held the keyboard, the shell replacing it has to take it:
                // otherwise a prompt appears, the person types at it, and the
                // keystrokes go to a view on its way out.
                let had_focus = this.focus_handle(cx).contains_focused(window, cx);
                let view = cx.new(|cx| {
                    TerminalView::new(
                        terminal,
                        this.workspace.clone(),
                        None,
                        this.project.downgrade(),
                        window,
                        cx,
                    )
                });
                // Typing `exit` here has to close the tab, the way it closes a
                // terminal tab. `TerminalView` raises `CloseItem` for its own
                // shell, but this tab is the item -- so the event has to be
                // carried the rest of the way out.
                this._shell_close = Some(cx.subscribe(
                    &view,
                    |_this, _view, event: &workspace::item::ItemEvent, cx| {
                        if matches!(event, workspace::item::ItemEvent::CloseItem) {
                            cx.emit(AgentViewEvent::Close);
                        }
                    },
                ));
                this.state = State::Shell(view);
                if had_focus {
                    let handle = this.focus_handle(cx);
                    window.focus(&handle, cx);
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Starts an ordinary shell in the folder the agent was working in.
    ///
    /// A new pty rather than the agent's: the agent's is dead, and a dead one
    /// cannot be reopened. That is the whole reason a failed agent keeps its
    /// terminal instead -- its output is on a surface this would replace.
    ///
    /// The directory is read off the dying terminal rather than recomputed from
    /// the project: a resumed session runs where it ran before, which is not
    /// necessarily where this window happens to be pointed.
    fn build_a_shell(&mut self, cx: &mut Context<Self>) -> Task<anyhow::Result<Entity<Terminal>>> {
        let working_directory = match &self.state {
            State::Terminal(view) => view.read(cx).terminal().read(cx).working_directory(),
            _ => None,
        };
        self.project.update(cx, |project, cx| {
            project.create_terminal_shell(working_directory, cx)
        })
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agent = self.agent.clone();
        let intent = self.intent.clone();
        let project = self.project.clone();
        let workspace = self.workspace.clone();
        let store = project.read(cx).agent_server_store().clone();
        // Read here, on the main thread, because the decision about where a
        // never-run session should start is made on a background thread where the
        // project cannot be read.
        let default_cwd = project
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf());

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

            // Reading the agent's own session store is blocking file and sqlite
            // work, so it goes to a background thread — and it happens here, in
            // the task that was already awaiting the binary, rather than at
            // construction, because a tab has to be able to exist before it knows
            // whether its session is still on disk.
            let session = match resolve_session(&agent, &intent, default_cwd, cx).await {
                SessionStart::Fresh => None,
                SessionStart::Command(command) => Some(command),
                SessionStart::Gone => {
                    this.update(cx, |this, cx| {
                        this.state = State::SessionGone;
                        cx.emit(AgentViewEvent::UpdateTab);
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };

            let terminal = project
                .update(cx, |project, cx| {
                    project.create_terminal_task(
                        agent_task(&agent, binary, session.as_ref(), project, cx),
                        cx,
                    )
                })
                .await;

            this.update_in(cx, |this, window, cx| {
                match terminal {
                    Ok(terminal) => {
                        // Taken before the handle is given away, and from the
                        // `Terminal` rather than the view: this is the only
                        // place both are in hand.
                        let completed = terminal.read(cx).wait_for_completed_task(cx);
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
                        this.watch_for_exit(completed, window, cx);
                    }
                    Err(error) => this.state = State::Failed(SharedString::from(error.to_string())),
                }
                cx.notify();
            })
            .ok();
        }));
    }
}

/// Whether an agent that ended this way should leave a shell behind.
///
/// The three yes-cases are all "the person ended it": killed by a signal, which
/// is what a bare Ctrl+C is; a clean zero, which is what a CLI that catches the
/// interrupt and tidies up reports; and 130, the conventional 128 + SIGINT for
/// one that reports being interrupted rather than merely exiting.
///
/// Only the first was observed -- a child killed by SIGINT yields raw wait
/// status 2, `WIFSIGNALED`, so `code()` is `None`. Which of the other two a
/// given agent's CLI picks cannot be found out without driving its interface by
/// hand, so both are covered rather than guessed between; the cost of guessing
/// wrong is exactly the fault this exists to fix.
///
/// Anything else is a failure, and its terminal is left alone. A shell built
/// over it would take a stack trace off the screen at the moment it is worth
/// most.
///
/// `None` is no status at all -- the channel closed without one, which happens
/// when the terminal is dropped rather than when it ends. Nothing to hand back.
fn ends_at_a_prompt(status: Option<ExitStatus>) -> bool {
    status.is_some_and(|status| user_ended_it(status.code()))
}

/// The rule itself, on the exit code alone.
///
/// Split out because `ExitStatus` has no portable constructor, and a rule this
/// load-bearing should be provable on every platform it runs on rather than only
/// on the one whose raw wait status a test can forge.
fn user_ended_it(code: Option<i32>) -> bool {
    matches!(code, None | Some(0) | Some(130))
}

fn display_name(agent: &AgentId) -> SharedString {
    builtin_agent(agent.as_ref())
        .map(|builtin| SharedString::from(builtin.display_name))
        .unwrap_or_else(|| agent.0.clone())
}

pub(crate) fn mode_name(mode: AgentViewMode) -> &'static str {
    match mode {
        AgentViewMode::Terminal => "terminal",
    }
}

pub(crate) fn mode_from_name(name: &str) -> AgentViewMode {
    match name {
        "terminal" => AgentViewMode::Terminal,
        // Anything else, including a `"chat"` written down by a build that still
        // had a chat mode, comes back as terminal -- the only mode there is.
        _ => AgentViewMode::Terminal,
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

/// The agent's CLI is run as a task rather than typed into a shell, so the pty
/// holds the agent itself: closing the tab ends the session, and no stray shell
/// outlives it. The summary, command echo and rerun button are all off — this is
/// an interactive program, not a build step whose exit code the user waits on.
/// What session, if any, this tab is bound to.
///
/// Replaces an `Option<ResumeTarget>` that was about to have to mean three
/// things — no identity, an id this editor chose, and an id from the agent's own
/// store — which is how the next reader gets it wrong.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum SessionIntent {
    /// No identity. The agent's CLI picks its own session, and this tab cannot be
    /// brought back onto a conversation.
    #[default]
    Untracked,
    /// This tab owns this id. Whether spawning *resumes* it or *starts* it is
    /// decided at spawn time by whether the agent's store holds it — an id
    /// assigned to a tab nobody has typed in yet has no session on disk, and that
    /// is a normal state rather than an error.
    Tracked(SharedString),
}

/// The intent a stored `session_id` column stands for.
///
/// The value reaches this from SQLite as free text and ends up as an element of a
/// spawned process's argv. It goes as its own argument rather than through a
/// shell, so there is no injection path — but `--session-id` accepts only a UUID,
/// and a row that is not one would make the CLI fail in a way nobody could read
/// from the tab. So it is parsed, and anything that will not parse is treated as
/// no id at all: the tab still opens, it simply is not tracked.
fn intent_from_stored(session_id: Option<&str>) -> SessionIntent {
    let Some(id) = session_id else {
        return SessionIntent::Untracked;
    };
    match uuid::Uuid::parse_str(id) {
        Ok(_) => SessionIntent::Tracked(id.to_string().into()),
        Err(error) => {
            log::warn!("ignoring a stored agent session id that is not a UUID: {error}");
            SessionIntent::Untracked
        }
    }
}

/// The identity a freshly opened tab starts life with.
///
/// An id is only assigned where the agent's CLI can be told to use it — Claude's
/// `--session-id`. For Codex and Copilot the CLI picks its own id and never tells
/// anyone, so assigning one here would write down an id that is not going to be
/// there to resume: a tab that looks tracked and comes back on nothing.
///
/// Generated at construction rather than at spawn because the tab has to be able
/// to write its id down before it has spawned anything — serialization can happen
/// first.
fn intent_for_new_tab(agent: &AgentId) -> SessionIntent {
    let Some(kind) = AgentKind::from_builtin_agent_id(agent.as_ref()) else {
        return SessionIntent::Untracked;
    };
    let id = uuid::Uuid::new_v4().to_string();
    // Only *whether* the agent accepts an id is read here; the directory is
    // irrelevant to that answer and this tab has none yet. Asking the provider
    // rather than keeping a list of capable agents is what makes the day Codex
    // gains such a flag a one-line change in one crate.
    if agent_sessions::provider_for(kind)
        .new_session_command(&id, std::path::Path::new(""))
        .is_some()
    {
        SessionIntent::Tracked(id.into())
    } else {
        SessionIntent::Untracked
    }
}

/// The command that puts a tab back on its session, or `None` to let the CLI
/// choose for itself.
///
/// An [`SessionIntent::Untracked`] tab has nothing to look up. A `Tracked` one
/// asks the agent's own store: a session that is there is resumed, and one that
/// is not is started fresh **under the same id**, which is what keeps a tab that
/// was opened and never typed in from coming back broken — Claude writes its
/// transcript only after the first message.
///
/// Every failure lands on `None`, which spawns the agent with no session
/// arguments. That is the same thing every tab did before this feature existed,
/// so the worst case is the old behaviour rather than a dead tab.
async fn resolve_session(
    agent: &AgentId,
    intent: &SessionIntent,
    default_cwd: Option<std::path::PathBuf>,
    cx: &mut gpui::AsyncApp,
) -> SessionStart {
    let SessionIntent::Tracked(id) = intent else {
        return SessionStart::Fresh;
    };
    let Some(kind) = AgentKind::from_builtin_agent_id(agent.as_ref()) else {
        // A user-configured agent: this editor reads three stores and has no
        // business guessing at a fourth's.
        return SessionStart::Fresh;
    };
    let provider = agent_sessions::provider_for(kind);
    let id = id.to_string();
    cx.background_spawn(async move {
        match provider.find(&id) {
            Ok(found) => session_start(provider.as_ref(), &id, default_cwd, found),
            // An unreadable store is not the same as a deleted session, and
            // refusing to start on a transient failure would strand the tab. Fall
            // back to the behaviour every tab had before this feature existed.
            Err(error) => {
                log::warn!("looking up session {id}: {error}");
                SessionStart::Fresh
            }
        }
    })
    .await
}

/// The decision, once the store has answered.
///
/// Split from [`resolve_session`] because that function's other half is blocking
/// file and sqlite work behind an `AsyncApp`, and this half is the part with the
/// reasoning in it. Both `resume_command` and `new_session_command` are pure, so
/// this is directly testable without a window, a store on disk, or a fake.
fn session_start(
    provider: &dyn agent_sessions::SessionProvider,
    id: &str,
    default_cwd: Option<std::path::PathBuf>,
    found: Option<agent_sessions::SessionSummary>,
) -> SessionStart {
    if let Some(session) = found {
        return provider
            .resume_command(&session, Fork::Continue)
            .map(SessionStart::Command)
            // An agent that cannot resume a session it *does* hold is not a state
            // this editor has ever seen, but `resume_command` is allowed to say
            // no. Starting fresh beats refusing to start.
            .unwrap_or(SessionStart::Fresh);
    }

    // The tab owns this id but the store has no session under it. Almost always
    // because nobody has typed in the tab yet — Claude writes its transcript only
    // after the first message — so the right move is to start fresh *under the
    // same id*: that keeps the tab's identity and means the next restart finds a
    // real session.
    //
    // The directory is this window's, not the session's: the session never ran,
    // so it has no directory of its own to prefer. `default_cwd` is that window's
    // first worktree, read before this ran — `agent_task`'s own fallback cannot be
    // relied on here, because a command it is handed carries a `cwd` and that is
    // the one it uses.
    //
    // An agent with no flag for this cannot do it at all, and opening an
    // *unrelated* fresh session in a tab that presents itself as a continuation
    // would be the silent lie. It says so instead.
    let Some(cwd) = default_cwd else {
        // No worktree to start in. Rather than invent one, fall back to the
        // behaviour of every tab before this feature: let the CLI choose.
        return SessionStart::Fresh;
    };
    match provider.new_session_command(id, &cwd) {
        Some(command) => SessionStart::Command(command),
        None => SessionStart::Gone,
    }
}

/// What `start` should do about this tab's session.
enum SessionStart {
    /// Spawn with no session arguments and let the CLI decide — either the tab is
    /// untracked, or looking its session up failed in a way that should not cost
    /// the tab.
    Fresh,
    Command(ResumeCommand),
    /// Tracked, gone, and unrecoverable for this agent. See [`State::SessionGone`].
    Gone,
}

/// Turns a resolved binary and an already-decided session command into the task
/// the terminal runs.
///
/// `session` carries the arguments and the directory but **not** the program:
/// that comes from `binary`, which `AgentServerStore` resolved against the real
/// `PATH`. A `ResumeCommand`'s own `program` is the bare name the agent's store
/// records (`"claude"`), which is not what should be executed.
///
/// Which command it is — resume this session, start one under a chosen id, or
/// nothing at all — is decided in `start`, where the store can be read on a
/// background thread. This function does not choose.
fn agent_task(
    agent: &AgentId,
    binary: std::path::PathBuf,
    session: Option<&ResumeCommand>,
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
        args: session
            .map(|session| session.args.clone())
            .unwrap_or_default(),
        // A resumed session runs where it ran before, not where this window
        // happens to be pointed: the conversation's context is that directory.
        cwd: session.map(|session| session.cwd.clone()).or_else(|| {
            project
                .visible_worktrees(cx)
                .next()
                .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
        }),
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
            State::Terminal(terminal) | State::Shell(terminal) => terminal.focus_handle(cx),
            _ => self.focus_handle.clone(),
        }
    }
}

impl workspace::item::Item for AgentView {
    type Event = AgentViewEvent;

    fn to_item_events(event: &Self::Event, f: &mut dyn FnMut(workspace::item::ItemEvent)) {
        match event {
            AgentViewEvent::UpdateTab => f(workspace::item::ItemEvent::UpdateTab),
            AgentViewEvent::Close => f(workspace::item::ItemEvent::CloseItem),
        }
    }

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> SharedString {
        self.tab_label()
    }

    /// The label, with the rename editor laid over it while one is open.
    ///
    /// Over rather than instead of: swapping the two resizes the tab as the
    /// editor appears, which moves the tab the user just clicked out from under
    /// the pointer. The label stays, drawn transparent, and holds the width.
    ///
    /// The label only — no icon. The pane draws that from `tab_icon` below, and
    /// sizes and mutes it to match every other tab; drawing one here as well put
    /// two agent glyphs on the tab. `TerminalView`, which this is modelled on,
    /// gets away with its own icon because it implements no `tab_icon` at all.
    fn tab_content(
        &self,
        params: workspace::item::TabContentParams,
        _window: &Window,
        _cx: &App,
    ) -> AnyElement {
        let handle = self.self_handle.clone();
        h_flex()
            .gap_1()
            .when(!params.selected, |this| {
                this.track_focus(&self.focus_handle)
            })
            .on_action({
                let handle = handle.clone();
                move |_: &RenameAgent, window, cx| {
                    handle
                        .update(cx, |this, cx| this.start_renaming(window, cx))
                        .ok();
                }
            })
            .child(
                div()
                    .relative()
                    .child(
                        Label::new(self.tab_label())
                            .color(params.text_color())
                            .when(self.rename_editor.is_some(), |this| this.alpha(0.)),
                    )
                    .when_some(self.rename_editor.clone(), |this, editor| {
                        let confirm = handle.clone();
                        let cancel = handle.clone();
                        this.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .child(editor)
                                .on_action(move |_: &menu::Confirm, window, cx| {
                                    confirm
                                        .update(cx, |this, cx| {
                                            this.finish_renaming(true, window, cx)
                                        })
                                        .ok();
                                })
                                .on_action(move |_: &menu::Cancel, window, cx| {
                                    cancel
                                        .update(cx, |this, cx| {
                                            this.finish_renaming(false, window, cx)
                                        })
                                        .ok();
                                }),
                        )
                    }),
            )
            .into_any()
    }

    fn tab_extra_context_menu_actions(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Vec<(SharedString, Box<dyn gpui::Action>)> {
        vec![("Rename".into(), Box::new(RenameAgent))]
    }

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(agent_icon(self.agent.as_ref())))
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        None
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

    /// Restores the tab *and* points it back at its conversation.
    ///
    /// The conversation itself is never reconstructed here — it is the agent's own
    /// state, and an editor that rebuilt a transcript would be inventing one. What
    /// is restored is the tab's **identity**: the session id it owns, handed back
    /// to the agent's own CLI so that `--resume` does the continuing. That is why
    /// this can be faithful rather than a plausible-looking lie.
    ///
    /// Whether the id resumes or starts fresh is not decided here. `start` asks
    /// the agent's store at spawn time, because a tab that was opened and never
    /// typed in has an id and no session on disk yet.
    ///
    /// Starting the agent is the point rather than a side effect — a restored tab
    /// that will not answer until it is clicked is a picture of an agent, not an
    /// agent. `Self::new` runs `start`, which is what does it.
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
            let row = db.get_agent(item_id, workspace_id)?;
            let mode = mode_from_name(&row.mode);
            let intent = intent_from_stored(row.session_id.as_deref());

            cx.update(|window, cx| {
                Ok(cx.new(|cx| {
                    // No width to restore: a workspace comes back with its own
                    // serialized pane flexes, and forcing a remembered width on
                    // top of them would fight the layout the user actually left.
                    let mut view = Self::new(
                        AgentId::new(row.agent),
                        mode,
                        project,
                        workspace,
                        intent,
                        window,
                        cx,
                    );
                    // Set directly rather than through `restore_custom_name`,
                    // which notifies: nothing is watching a view that is still
                    // being constructed.
                    view.custom_name = row.name.map(SharedString::from);
                    view
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
        let row = persistence::AgentViewRow {
            agent: self.agent.to_string(),
            mode: mode_name(self.mode).to_string(),
            name: self.custom_name.as_ref().map(|name| name.to_string()),
            session_id: match &self.intent {
                SessionIntent::Untracked => None,
                SessionIntent::Tracked(id) => Some(id.to_string()),
            },
        };

        let db = persistence::AgentViewDb::global(cx);
        Some(cx.background_spawn(async move { db.save_agent(item_id, workspace_id, row).await }))
    }

    /// `UpdateTab` is the one event worth a write, and it is worth one.
    ///
    /// The previous life of this impl answered `false` here, which is exactly why
    /// a session's name never survived a restart: nothing but the workspace-wide
    /// pass ever wrote the row, and a rename does not trigger that pass. A rename
    /// and a mode switch both arrive as `UpdateTab`, and both belong in the row.
    fn should_serialize(&self, _event: &Self::Event) -> bool {
        true
    }
}

impl Render for AgentView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Built before the theme is borrowed: this needs `cx` mutably, and that
        // borrow lives to the end of the element tree below.
        let body = match &self.state {
            State::Terminal(terminal) | State::Shell(terminal) => {
                terminal.clone().into_any_element()
            }
            State::Starting => centered_message(
                IconName::ArrowCircle,
                format!("Starting {}…", self.display_name).into(),
                None,
                cx,
            ),
            State::MissingBinary(missing) => {
                crate::missing_binary::render(&cx.entity(), &self.display_name, missing, cx)
            }
            State::SessionGone => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Icon::new(IconName::HistoryRerun).color(Color::Muted))
                .child(
                    Label::new(format!(
                        "This {} session is no longer on this machine.",
                        self.display_name
                    ))
                    .color(Color::Muted),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("start-a-new-session", "Start a New Session").on_click(
                                cx.listener(|this, _, window, cx| {
                                    // Drops the id rather than keeping it: the
                                    // session it named is gone and this agent
                                    // cannot recreate it, so holding the id would
                                    // only put the tab back here on the next
                                    // restart.
                                    this.intent = SessionIntent::Untracked;
                                    this.state = State::Starting;
                                    this.start(window, cx);
                                    cx.emit(AgentViewEvent::UpdateTab);
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(
                            Button::new("close-this-tab", "Close Tab")
                                .style(ButtonStyle::Subtle)
                                // Dispatched rather than reaching for the pane:
                                // which pane holds this tab is the workspace's
                                // business, and this action already resolves it
                                // through the focused item.
                                .on_click(cx.listener(|_, _, window, cx| {
                                    window.dispatch_action(
                                        Box::new(workspace::CloseActiveItem {
                                            save_intent: None,
                                            close_pinned: true,
                                        }),
                                        cx,
                                    );
                                })),
                        ),
                )
                .into_any_element(),
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
            // Here, not only on the tab: both the context-menu entry and the
            // double-click dispatch through the *item's* focus handle, which
            // resolves against this element. The copy on `tab_content` is in
            // that path only while the tab is unselected — never the tab someone
            // clicks to rename it — so rename silently did nothing.
            .on_action(cx.listener(|this: &mut Self, _: &RenameAgent, window, cx| {
                this.start_renaming(window, cx);
            }))
            // No header row any more: it existed only to hold the mode switch, and
            // an empty bar with a bottom border is a line across the top of a
            // terminal for no reason.
            //
            // `flex_1` with a floor of zero, in a column so it grows along the axis
            // meant: the body's children size themselves against a definite height,
            // and `size_full` here would run them straight through the view.
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
    use util::{path, paths::PathStyle, rel_path::rel_path};
    use workspace::MultiWorkspace;

    /// The rule that decides whether a tab gets a prompt back.
    ///
    /// Pinned exhaustively because every branch is a different reported defect:
    /// lose the signal case and Ctrl+C leaves a dead window again; lose the
    /// failure case and a stack trace is wiped by a shell the moment it appears.
    #[test]
    fn only_an_ending_the_person_chose_leaves_a_prompt() {
        // A bare Ctrl+C. Measured: a child killed by SIGINT yields raw wait
        // status 2, `WIFSIGNALED`, so no code survives to `code()`.
        assert!(user_ended_it(None), "a signal is the plain Ctrl+C case");
        assert!(user_ended_it(Some(0)), "a CLI that caught the interrupt");
        assert!(user_ended_it(Some(130)), "128 + SIGINT, reported as a code");

        assert!(!user_ended_it(Some(1)), "a failure keeps its terminal");
        assert!(!user_ended_it(Some(2)));
        assert!(!user_ended_it(Some(127)), "command not found");
        assert!(
            !user_ended_it(Some(129)),
            "128 + SIGHUP is not an interrupt, and 130 must not be read as a range"
        );
    }

    /// No status at all is not an ending, and must not build a shell.
    ///
    /// This is what a dropped terminal produces -- the channel closes without a
    /// value. Treating it as an exit would put a shell in a tab that is on its
    /// way out.
    #[test]
    fn a_terminal_that_never_reported_leaves_nothing_behind() {
        assert!(!ends_at_a_prompt(None));
    }

    /// An agent that failed keeps its terminal, so its error stays readable.
    ///
    /// The whole reason the tab does not simply always hand back a shell: the
    /// new pty cannot inherit the dead one's screen, so building one over a
    /// stack trace destroys it at the moment it is worth most.
    #[gpui::test]
    #[cfg(unix)]
    async fn a_failed_agent_keeps_its_terminal(cx: &mut TestAppContext) {
        use std::os::unix::process::ExitStatusExt as _;

        let (agent_view, cx) = an_agent_showing_a_terminal(cx).await;

        agent_view.update_in(cx, |view, window, cx| {
            // Exit code 1: the raw wait status puts the code in the high byte.
            view.watch_for_exit(Task::ready(Some(ExitStatus::from_raw(1 << 8))), window, cx);
        });
        cx.run_until_parked();

        agent_view.read_with(cx, |view, _| {
            assert!(
                matches!(view.state, State::Terminal(_)),
                "a failed agent must keep the terminal holding its error"
            );
        });
    }

    /// A Ctrl+C leaves a working prompt where the agent was.
    ///
    /// The defect this closes: the tab used to stop at a dead terminal with no
    /// prompt and no way to keep working in that folder.
    ///
    /// This one builds a real shell, which is the only way to prove the hand-off
    /// produces something usable rather than merely changing a field. The pty it
    /// starts dies with the view, by the ownership chain
    /// `leaving_a_mode_releases_it` asserts.
    #[gpui::test]
    #[cfg(unix)]
    async fn an_interrupted_agent_hands_back_a_shell(cx: &mut TestAppContext) {
        use std::os::unix::process::ExitStatusExt as _;

        let (agent_view, cx) = an_agent_showing_a_terminal(cx).await;

        agent_view.update_in(cx, |view, window, cx| {
            // Raw wait status 2: killed by SIGINT, which is what Ctrl+C sends.
            view.watch_for_exit(Task::ready(Some(ExitStatus::from_raw(2))), window, cx);
        });
        cx.run_until_parked();

        agent_view.read_with(cx, |view, _| {
            assert!(
                matches!(view.state, State::Shell(_)),
                "Ctrl+C must leave a prompt behind, not a dead terminal"
            );
        });
    }

    /// The shell takes the keyboard the dying terminal was holding.
    ///
    /// Without this the feature looks like it works and is not usable: the
    /// prompt appears, and every keystroke goes to the view being dropped. The
    /// focus has to be handed over with the pty.
    #[gpui::test]
    #[cfg(unix)]
    async fn the_shell_inherits_the_keyboard(cx: &mut TestAppContext) {
        use std::os::unix::process::ExitStatusExt as _;

        let (agent_view, cx) = an_agent_showing_a_terminal(cx).await;

        agent_view.update_in(cx, |view, window, cx| {
            let handle = view.focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.run_until_parked();

        agent_view.update_in(cx, |view, window, cx| {
            view.watch_for_exit(Task::ready(Some(ExitStatus::from_raw(2))), window, cx);
        });
        cx.run_until_parked();

        agent_view.update_in(cx, |view, window, cx| {
            assert!(
                matches!(view.state, State::Shell(_)),
                "the shell must have replaced the terminal for this to mean anything"
            );
            assert!(
                view.focus_handle(cx).contains_focused(window, cx),
                "the prompt is unusable if the keyboard stayed with the dead terminal"
            );
        });
    }

    /// An agent tab showing a terminal, with no agent process behind it.
    ///
    /// Display-only: what these tests drive is the exit path, and a real CLI
    /// would be a process the test then has to be trusted to clean up.
    ///
    /// The tab is the window's root rather than an entity beside it. A view that
    /// is never rendered is not in the dispatch tree, so focus placed on it does
    /// not survive the next frame -- which is the whole subject of one of these
    /// tests.
    ///
    /// Gated with the tests it serves. All three build an `ExitStatus` from a raw
    /// wait status, which only Unix has; without the gate this is dead code on
    /// Windows, and `script/clippy` runs with `--deny warnings`.
    #[cfg(unix)]
    async fn an_agent_showing_a_terminal(
        cx: &mut TestAppContext,
    ) -> (Entity<AgentView>, &mut gpui::VisualTestContext) {
        init_test(cx);
        cx.update(|cx| theme_settings::init(theme::LoadThemes::JustBase, cx));

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/test"), json!({})).await;
        let project = Project::test(fs, [path!("/test").as_ref()], cx).await;

        let (multi_workspace, host_cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
        let workspace = multi_workspace.read_with(host_cx, |multi, _| multi.workspace().clone());

        // Built empty and filled below: the terminal needs a window, and this is
        // the call that makes one.
        let (agent_view, cx) = cx.add_window_view(|_window, cx| AgentView {
            intent: SessionIntent::Untracked,
            agent: AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
            display_name: "Claude Code".into(),
            custom_name: None,
            rename_editor: None,
            _rename_subscription: None,
            mode: AgentViewMode::Terminal,
            state: State::Starting,
            project: project.clone(),
            workspace: workspace.downgrade(),
            focus_handle: cx.focus_handle(),
            self_handle: cx.entity().downgrade(),
            _startup: None,
            _exit_watch: None,
            _shell_close: None,
        });

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
        agent_view.update(cx, |view, cx| {
            view.state = State::Terminal(terminal_view);
            cx.notify();
        });
        cx.run_until_parked();
        (agent_view, cx)
    }

    /// The resume command reaches the pty, and an ordinary open still does not.
    ///
    /// This is the seam between the history panel and the agent: the panel hands
    /// over args and a directory, and `agent_task` is the single place that turns
    /// them into a process. Asserted on the task rather than on the panel because
    /// this is where a wrong `cwd` would send the CLI at the wrong tree.
    #[gpui::test]
    async fn a_resumed_session_runs_its_own_arguments_in_its_own_directory(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/here"), json!({})).await;
        let project = Project::test(fs, [path!("/here").as_ref()], cx).await;
        let agent = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
        let binary = std::path::PathBuf::from("/bin/claude");

        let (fresh, resumed) = cx.update(|cx| {
            let project = project.read(cx);
            let fresh = super::agent_task(&agent, binary.clone(), None, project, cx);
            // A `ResumeCommand` as the agent's own store hands one over. Its
            // `program` is the bare name the store records and is deliberately
            // ignored below — the executed program is the resolved `binary`.
            let resume = ResumeCommand {
                program: "claude".into(),
                args: vec!["--resume".into(), "abc-123".into(), "--fork-session".into()],
                cwd: std::path::PathBuf::from("/elsewhere"),
            };
            let resumed = super::agent_task(&agent, binary.clone(), Some(&resume), project, cx);
            (fresh, resumed)
        });

        // Unchanged for everyone who is not resuming: no arguments, and the cwd is
        // still the window's own worktree.
        assert!(fresh.args.is_empty());
        assert_eq!(fresh.cwd, Some(std::path::PathBuf::from(path!("/here"))));

        assert_eq!(
            resumed.args,
            vec![
                "--resume".to_string(),
                "abc-123".to_string(),
                "--fork-session".to_string()
            ]
        );
        assert_eq!(
            resumed.cwd,
            Some(std::path::PathBuf::from("/elsewhere")),
            "a resumed session runs where the conversation ran, not where this \
             window is pointed"
        );
        assert_eq!(
            resumed.command,
            Some("/bin/claude".to_string()),
            "the resolved binary runs, not the bare name the store recorded"
        );
    }

    fn summary(agent: AgentKind, id: &str) -> agent_sessions::SessionSummary {
        agent_sessions::SessionSummary {
            id: id.into(),
            agent,
            title: String::new(),
            preview: String::new(),
            preview_speaker: None,
            cwd: std::path::PathBuf::from("/w/one"),
            branch: None,
            model: None,
            updated_at: std::time::UNIX_EPOCH,
            log_path: None,
            log_bytes: 0,
        }
    }

    /// A session the store no longer holds, for an agent that cannot be told
    /// which id to start under, is the one case that has nothing to offer but the
    /// truth.
    #[test]
    fn a_session_that_vanished_from_an_agent_that_cannot_recreate_it_is_gone() {
        for kind in [AgentKind::Codex, AgentKind::Copilot] {
            let provider = agent_sessions::provider_for(kind);
            assert!(
                matches!(
                    session_start(
                        provider.as_ref(),
                        "44444444-4444-4444-4444-444444444444",
                        Some(std::path::PathBuf::from("/w/one")),
                        None
                    ),
                    SessionStart::Gone
                ),
                "{} has no --session-id, so a vanished session has no way back",
                kind.label()
            );
        }
    }

    /// The reachability claim on `State::SessionGone`, asserted rather than left
    /// as prose: Claude can always start under its own id, so the branch that
    /// leads to that state never fires for it.
    #[test]
    fn a_claude_session_that_vanished_starts_fresh_under_the_same_id() {
        let provider = agent_sessions::provider_for(AgentKind::Claude);
        let id = "55555555-5555-5555-5555-555555555555";
        let SessionStart::Command(command) = session_start(
            provider.as_ref(),
            id,
            Some(std::path::PathBuf::from("/w/here")),
            None,
        ) else {
            panic!("Claude must never reach the gone state");
        };
        assert_eq!(
            command.args,
            vec!["--session-id".to_string(), id.to_string()],
            "the tab keeps the id it owns rather than being handed a new one"
        );
        assert_eq!(
            command.cwd,
            std::path::PathBuf::from("/w/here"),
            "a session that never ran starts in this window's worktree, not in \
             whatever `.` happens to be"
        );
    }

    /// The bug this asserts against: handing `agent_task` a command whose `cwd`
    /// is a placeholder. `agent_task` uses the `cwd` of any command it is given
    /// and never reaches its own fallback, so a placeholder would silently start
    /// the CLI in the wrong directory.
    #[test]
    fn with_no_worktree_to_start_in_the_cli_chooses_for_itself() {
        let provider = agent_sessions::provider_for(AgentKind::Claude);
        assert!(
            matches!(
                session_start(
                    provider.as_ref(),
                    "77777777-7777-7777-7777-777777777777",
                    None,
                    None
                ),
                SessionStart::Fresh
            ),
            "no directory is not a reason to invent one"
        );
    }

    #[test]
    fn a_session_the_store_still_holds_is_resumed() {
        let provider = agent_sessions::provider_for(AgentKind::Codex);
        let id = "66666666-6666-6666-6666-666666666666";
        let SessionStart::Command(command) = session_start(
            provider.as_ref(),
            id,
            Some(std::path::PathBuf::from("/somewhere/else")),
            Some(summary(AgentKind::Codex, id)),
        ) else {
            panic!("a session that is there is resumed");
        };
        assert_eq!(command.args, vec!["resume".to_string(), id.to_string()]);
        assert_eq!(
            command.cwd,
            std::path::PathBuf::from("/w/one"),
            "the directory comes back with the session, not from this window"
        );
    }

    /// **The commission.** Two tabs of one agent, two conversations, and each has
    /// to come back on its own — which is exactly what a shared "most recent
    /// session" flag (`claude --continue`) could never do.
    #[gpui::test]
    async fn two_tabs_of_one_agent_keep_separate_ids(cx: &mut TestAppContext) {
        init_test(cx);
        let workspaces = cx.update(|cx| workspace::WorkspaceDb::global(cx));
        let workspace_id = workspaces.next_id().await.unwrap();
        let db = cx.update(|cx| persistence::AgentViewDb::global(cx));

        let first = "11111111-1111-1111-1111-111111111111";
        let second = "22222222-2222-2222-2222-222222222222";
        db.save_agent(
            1,
            workspace_id,
            row("claude-acp", "terminal", Some("auth refactor"), Some(first)),
        )
        .await
        .unwrap();
        db.save_agent(
            2,
            workspace_id,
            row("claude-acp", "terminal", Some("ui font"), Some(second)),
        )
        .await
        .unwrap();

        let restored_first = db.get_agent(1, workspace_id).unwrap();
        let restored_second = db.get_agent(2, workspace_id).unwrap();
        assert_eq!(restored_first.session_id.as_deref(), Some(first));
        assert_eq!(restored_second.session_id.as_deref(), Some(second));
        assert_ne!(
            restored_first.session_id, restored_second.session_id,
            "two tabs of the same agent must not share a session"
        );

        // And the ids survive the read back into an intent, which is what `start`
        // actually acts on.
        assert_eq!(
            intent_from_stored(restored_first.session_id.as_deref()),
            SessionIntent::Tracked(first.into())
        );
        assert_eq!(
            intent_from_stored(restored_second.session_id.as_deref()),
            SessionIntent::Tracked(second.into())
        );
    }

    #[gpui::test]
    async fn a_session_id_survives_being_written_down_and_brought_back(cx: &mut TestAppContext) {
        init_test(cx);
        let workspaces = cx.update(|cx| workspace::WorkspaceDb::global(cx));
        let workspace_id = workspaces.next_id().await.unwrap();
        let db = cx.update(|cx| persistence::AgentViewDb::global(cx));

        let id = "33333333-3333-3333-3333-333333333333";
        db.save_agent(
            7,
            workspace_id,
            row("claude-acp", "terminal", None, Some(id)),
        )
        .await
        .unwrap();
        assert_eq!(
            db.get_agent(7, workspace_id).unwrap().session_id.as_deref(),
            Some(id)
        );

        // An untracked tab writes NULL, which is what every row saved before this
        // column existed also reads as — a different answer from "".
        db.save_agent(8, workspace_id, row("codex-acp", "terminal", None, None))
            .await
            .unwrap();
        let untracked = db.get_agent(8, workspace_id).unwrap();
        assert_eq!(untracked.session_id, None);
        assert_eq!(
            intent_from_stored(untracked.session_id.as_deref()),
            SessionIntent::Untracked
        );
    }

    #[test]
    fn a_claude_tab_is_given_a_session_id() {
        let agent = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
        let SessionIntent::Tracked(id) = intent_for_new_tab(&agent) else {
            panic!("Claude has --session-id, so a fresh tab must own an id");
        };
        uuid::Uuid::parse_str(&id).expect("--session-id accepts only a UUID");
    }

    /// The guard against writing down an id the CLI will never agree to. Codex
    /// and Copilot mint their own and tell nobody, so a tab claiming one would
    /// come back on nothing at all.
    #[test]
    fn an_agent_that_cannot_be_told_an_id_is_not_given_one() {
        for id in [
            project::CODEX_AGENT_ID,
            project::COPILOT_AGENT_ID,
            "some-agent-this-editor-never-heard-of",
        ] {
            assert_eq!(
                intent_for_new_tab(&AgentId::new(id.to_string())),
                SessionIntent::Untracked,
                "{id} has no way to accept an id"
            );
        }
    }

    #[test]
    fn a_stored_id_that_is_not_a_uuid_restores_as_untracked() {
        assert_eq!(intent_from_stored(None), SessionIntent::Untracked);
        for hostile in [
            "",
            "not-a-uuid",
            "../../etc/passwd",
            "11111111-2222-3333-4444",
        ] {
            assert_eq!(
                intent_from_stored(Some(hostile)),
                SessionIntent::Untracked,
                "{hostile:?} must not reach argv"
            );
        }
        let good = "11111111-2222-3333-4444-555555555555";
        assert_eq!(
            intent_from_stored(Some(good)),
            SessionIntent::Tracked(good.into())
        );
    }

    /// Shorthand for a stored row, so a test reads as what it is asserting
    /// rather than as four positional arguments of which two are strings.
    fn row(
        agent: &str,
        mode: &str,
        name: Option<&str>,
        session_id: Option<&str>,
    ) -> persistence::AgentViewRow {
        persistence::AgentViewRow {
            agent: agent.to_string(),
            mode: mode.to_string(),
            name: name.map(str::to_string),
            session_id: session_id.map(str::to_string),
        }
    }

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
    }

    /// Leaving a state has to end it. The terminal state owns the agent's CLI
    /// under a pty, and nothing kills that but `Drop`, which runs only on the last
    /// strong handle. Assigning over `state` looks like it is enough and silently
    /// is not the moment anything else keeps a handle, so this asserts the count
    /// really reached zero.
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
            intent: SessionIntent::Untracked,
            agent: AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
            display_name: "Claude Code".into(),
            custom_name: None,
            rename_editor: None,
            _rename_subscription: None,
            mode: AgentViewMode::Terminal,
            state: State::Terminal(terminal_view.clone()),
            project: project.clone(),
            workspace: workspace.downgrade(),
            focus_handle: cx.focus_handle(),
            self_handle: cx.entity().downgrade(),
            _startup: None,
            _exit_watch: None,
            _shell_close: None,
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
            Some(Some("terminal".into())),
        );
        assert_eq!(
            db.preferences("codex-acp".into()).unwrap(),
            Some(Some("chat".into())),
            "one agent's choice must not stand for the other's"
        );

        db.save_mode("claude-acp".into(), "chat".into())
            .await
            .unwrap();
        assert_eq!(
            db.preferences("claude-acp".into()).unwrap(),
            Some(Some("chat".into())),
            "a second choice replaces the first"
        );
    }

    /// A tab has to come back as the agent, mode and name it was left as.
    ///
    /// The name is the half that has never worked: the previous life of
    /// `SerializableItem` answered `should_serialize` with `false`, so only the
    /// workspace-wide pass ever wrote a row, and a rename does not trigger that
    /// pass. It is asserted here as `Some` and as `None`, because "never named" is
    /// a different answer from "named the empty string".
    ///
    /// `agent_views` has a foreign key into `workspaces`, so the row needs a real
    /// parent. `WorkspaceDb::next_id()` is the public way to make one -- the same
    /// mechanism `editor::items`' own deserialize tests use from outside
    /// `crates/workspace`.
    #[gpui::test]
    async fn a_tab_comes_back_as_the_agent_mode_and_name_it_was_left_as(cx: &mut TestAppContext) {
        let (_workspace, _project, cx) = workspace_with_agents(cx).await;

        let workspaces = cx.update(|_, cx| workspace::WorkspaceDb::global(cx));
        let workspace_id = workspaces.next_id().await.unwrap();
        let db = cx.update(|_, cx| persistence::AgentViewDb::global(cx));

        assert!(
            db.get_agent(1, workspace_id).is_err(),
            "an item nobody recorded must not resolve to a tab"
        );

        db.save_agent(1, workspace_id, row("claude-acp", "terminal", None, None))
            .await
            .unwrap();
        db.save_agent(
            2,
            workspace_id,
            row("codex-acp", "chat", Some("refactor"), None),
        )
        .await
        .unwrap();

        assert_eq!(
            db.get_agent(1, workspace_id).unwrap(),
            row("claude-acp", "terminal", None, None),
            "a tab never renamed must come back without a name, not with a blank one"
        );
        assert_eq!(
            db.get_agent(2, workspace_id).unwrap(),
            row("codex-acp", "chat", Some("refactor"), None),
            "the name a session was given is the whole reason two Claude tabs \
             can be told apart"
        );

        // A rename over an existing row: the path `should_serialize` now opens.
        db.save_agent(
            2,
            workspace_id,
            row("codex-acp", "chat", Some("review"), None),
        )
        .await
        .unwrap();
        assert_eq!(
            db.get_agent(2, workspace_id).unwrap().name,
            Some("review".into()),
            "renaming twice must leave the second name, not the first"
        );

        // Only item 2 came back, so item 1's row is what `cleanup` is for.
        db.delete_unloaded(workspace_id, vec![2]).await.unwrap();
        assert!(
            db.get_agent(1, workspace_id).is_err(),
            "a tab the workspace did not restore must not leave its row behind"
        );
        assert!(
            db.get_agent(2, workspace_id).is_ok(),
            "and a tab it did restore must keep its own"
        );
    }

    /// The whole restore path, end to end: a named tab written down by `serialize`
    /// and brought back by `deserialize` as the same agent, mode and name.
    ///
    /// This is the claim the feature actually rests on, and the one the DB
    /// round-trip above only half covers — that test proves the row survives, this
    /// proves the *view* is rebuilt from it. `Workspace::set_database_id` is public
    /// precisely so a test can stand the workspace on a real `workspaces` row,
    /// which `agent_views`' foreign key requires.
    ///
    /// The conversation is deliberately NOT part of the claim: a thread is the
    /// agent's own state, reachable through its CLI's resume. Restoring a tab and
    /// lying about its history would be worse than restoring an empty one.
    ///
    /// Nor does this cover `should_serialize` — it calls `serialize` outright,
    /// which is the workspace's job to schedule. That gate is what decides whether
    /// a rename ever reaches this path at all, and it has its own test below.
    #[gpui::test]
    async fn a_named_tab_survives_being_written_down_and_brought_back(cx: &mut TestAppContext) {
        use workspace::item::SerializableItem as _;

        let (workspace, project, cx) = workspace_with_agents(cx).await;

        let workspaces = cx.update(|_, cx| workspace::WorkspaceDb::global(cx));
        let workspace_id = workspaces.next_id().await.unwrap();
        workspace.update(cx, |workspace, _| {
            workspace.set_database_id(workspace_id);
        });

        open_claude(&workspace, cx);
        let view = workspace.read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<AgentView>(cx)
                .next()
                .expect("the agent just opened")
        });

        // Renamed the way a user does — dispatch, type, Enter — so what gets
        // written down is what a real rename produces.
        cx.update(|window, cx| {
            view.read(cx)
                .focus_handle(cx)
                .dispatch_action(&crate::RenameAgent, window, cx);
        });
        cx.run_until_parked();
        let editor = view
            .read_with(cx, |view, _| view.rename_editor().cloned())
            .expect("Rename must open the editor");
        editor.update_in(cx, |editor, window, cx| {
            editor.set_text("the refactor", window, cx);
        });
        cx.update(|window, cx| {
            editor
                .read(cx)
                .focus_handle(cx)
                .dispatch_action(&menu::Confirm, window, cx);
        });
        cx.run_until_parked();

        let item_id: workspace::ItemId = 4321;
        let write = workspace
            .update_in(cx, |workspace, window, cx| {
                view.update(cx, |view, cx| {
                    view.serialize(workspace, item_id, false, window, cx)
                })
            })
            .expect("a workspace with a database id must produce a write");
        write.await.unwrap();

        let restored = cx
            .update(|window, cx| {
                AgentView::deserialize(
                    project,
                    workspace.downgrade(),
                    workspace_id,
                    item_id,
                    window,
                    cx,
                )
            })
            .await
            .expect("the row was just written, so it must deserialize");
        cx.run_until_parked();

        restored.read_with(cx, |restored, _| {
            assert!(
                restored.is_agent(&AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string())),
                "the tab must come back as the same agent"
            );
            assert_eq!(
                restored.mode,
                AgentViewMode::Terminal,
                "and in the mode it was left in"
            );
            assert_eq!(
                restored.tab_label(),
                "the refactor",
                "and under the name it was given"
            );
        });
    }

    /// A rename has to be written down, and only `should_serialize` opens that door.
    ///
    /// The previous life of this impl answered `false`, so nothing but the
    /// workspace-wide pass ever wrote the row — and a rename does not trigger that
    /// pass. That is exactly why a session's name never survived a restart, and it
    /// is a one-word regression to reintroduce.
    #[gpui::test]
    async fn a_rename_is_worth_writing_down(cx: &mut TestAppContext) {
        init_test(cx);
        cx.update(|cx| theme_settings::init(theme::LoadThemes::JustBase, cx));

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/test"), json!({})).await;
        let project = Project::test(fs, [path!("/test").as_ref()], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
        let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());

        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                AgentView::new(
                    AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()),
                    AgentViewMode::Terminal,
                    project.clone(),
                    workspace.downgrade(),
                    SessionIntent::Untracked,
                    window,
                    cx,
                )
            })
        });

        view.read_with(cx, |view, _| {
            use workspace::item::SerializableItem as _;
            assert!(
                view.should_serialize(&AgentViewEvent::UpdateTab),
                "a rename arrives as `UpdateTab`, and it belongs in the row"
            );
        });
    }

    /// The one mode still survives the round trip through its name, and — the
    /// part worth asserting — **anything else read from disk comes back as
    /// terminal**. Rows written by a build that still had a chat mode are on real
    /// machines right now, and `"chat"` must not resolve to a mode that no longer
    /// exists.
    #[gpui::test]
    async fn any_stored_mode_name_comes_back_as_terminal(_cx: &mut TestAppContext) {
        assert_eq!(
            mode_from_name(mode_name(AgentViewMode::Terminal)),
            AgentViewMode::Terminal
        );
        assert_eq!(
            mode_from_name("chat"),
            AgentViewMode::Terminal,
            "a row left behind by a build with a chat mode has to open something"
        );
        assert_eq!(mode_from_name("nonsense"), AgentViewMode::Terminal);
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

    /// A workspace with `crate::init` already run, so the rail's actions are
    /// registered on it and a dispatch reaches the real handler.
    ///
    /// Dispatching rather than calling the handler body is not thoroughness for
    /// its own sake: `register_action` hands the handler a leased `Workspace`, and
    /// a body that reaches back through a workspace handle aborts the process
    /// under that lease while behaving perfectly when called directly. Only a
    /// dispatch proves the real path.
    async fn workspace_with_agents(
        cx: &mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<project::Project>,
        &mut gpui::VisualTestContext,
    ) {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            crate::init(cx);
            // Without this `open_path` has nothing to build a file into, and the
            // half of the claim about files sharing the agent's tab bar cannot be
            // asserted at all.
            workspace::register_project_item::<Editor>(cx);
        });

        let fs = FakeFs::new(cx.executor());
        fs.insert_tree(path!("/test"), json!({ "a.rs": "fn a() {}" }))
            .await;
        let project = Project::test(fs, [path!("/test").as_ref()], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
        let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());
        (workspace, project, cx)
    }

    fn open_claude(workspace: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) {
        workspace.update_in(cx, |workspace, window, cx| {
            AgentView::open(
                workspace,
                project::CLAUDE_CODE_AGENT_ID,
                Some(AgentViewMode::Terminal),
                window,
                cx,
            );
        });
        cx.run_until_parked();
    }

    fn agent_tabs(workspace: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> usize {
        workspace.read_with(cx, |workspace, cx| {
            workspace.items_of_type::<AgentView>(cx).count()
        })
    }

    /// The whole point of the change: the agent is an item of the editor's panes,
    /// in one tab bar with the code, rather than a column of its own.
    #[gpui::test]
    async fn an_agent_opens_as_a_tab_of_the_editors_pane(cx: &mut TestAppContext) {
        let (workspace, _project, cx) = workspace_with_agents(cx).await;
        open_claude(&workspace, cx);

        let (in_a_pane, is_active) = workspace.read_with(cx, |workspace, cx| {
            let view = workspace
                .items_of_type::<AgentView>(cx)
                .next()
                .expect("opening an agent must produce a tab");
            (
                workspace.pane_for(&view).is_some(),
                workspace
                    .active_item(cx)
                    .is_some_and(|item| item.item_id() == view.entity_id()),
            )
        });

        assert!(
            in_a_pane,
            "the agent must belong to one of the workspace's own panes -- that \
             membership is what puts it in the editor's tab bar"
        );
        assert!(is_active, "and opening it must bring it to the front");
    }

    /// A plain press comes back to the session already running. Starting a second
    /// process for the same agent is `open_new`'s job, and only the `+` menu
    /// reaches that.
    #[gpui::test]
    async fn a_second_press_comes_back_to_the_running_session(cx: &mut TestAppContext) {
        let (workspace, _project, cx) = workspace_with_agents(cx).await;
        open_claude(&workspace, cx);
        let first = workspace.read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<AgentView>(cx)
                .next()
                .expect("the agent just opened")
                .entity_id()
        });

        open_claude(&workspace, cx);

        assert_eq!(
            agent_tabs(&workspace, cx),
            1,
            "a second press must not start a second Claude process"
        );
        assert_eq!(
            workspace.read_with(cx, |workspace, cx| workspace
                .items_of_type::<AgentView>(cx)
                .next()
                .unwrap()
                .entity_id()),
            first,
            "and it must be the same session, not a replacement"
        );
    }

    /// `open_new` is the other half: a deliberate second session, side by side
    /// with the first, which is why two Claude tabs have to be nameable.
    #[gpui::test]
    async fn a_deliberate_new_session_stands_beside_the_first(cx: &mut TestAppContext) {
        let (workspace, _project, cx) = workspace_with_agents(cx).await;
        open_claude(&workspace, cx);

        workspace.update_in(cx, |workspace, window, cx| {
            AgentView::open_new(
                workspace,
                project::CLAUDE_CODE_AGENT_ID,
                Some(AgentViewMode::Terminal),
                window,
                cx,
            );
        });
        cx.run_until_parked();

        assert_eq!(
            agent_tabs(&workspace, cx),
            2,
            "the `+` menu exists precisely to run two sessions of one agent"
        );
    }

    /// The behaviour this change was asked for, and the one `c056596` moved the
    /// agent into a dock to prevent: a file opened while the agent has focus lands
    /// as a tab beside it rather than displacing it.
    ///
    /// It is the same mechanism either way -- `open_path` falls back to the active
    /// pane -- so what changed is not the code but whether that is wanted. It is,
    /// so it is asserted rather than guarded against.
    #[gpui::test]
    async fn a_file_opened_from_the_agents_tab_joins_the_same_tab_bar(cx: &mut TestAppContext) {
        let (workspace, project, cx) = workspace_with_agents(cx).await;
        open_claude(&workspace, cx);

        let agent_pane = workspace.read_with(cx, |workspace, cx| {
            let view = workspace.items_of_type::<AgentView>(cx).next().unwrap();
            workspace.pane_for(&view).expect("the agent is in a pane")
        });

        let worktree_id = project.read_with(cx, |project, cx| {
            project.worktrees(cx).next().unwrap().read(cx).id()
        });
        let opened = workspace.update_in(cx, |workspace, window, cx| {
            workspace.open_path((worktree_id, rel_path("a.rs")), None, true, window, cx)
        });
        opened.await.expect("a.rs is in the fake tree");
        cx.run_until_parked();

        let (items, still_holds_agent) = agent_pane.read_with(cx, |pane, _| {
            (
                pane.items_len(),
                pane.items()
                    .any(|item| item.downcast::<AgentView>().is_some()),
            )
        });
        assert_eq!(
            items, 2,
            "the file must arrive as a second tab in the agent's own pane, not \
             somewhere else and not in place of it"
        );
        assert!(
            still_holds_agent,
            "and the agent must survive the file being opened"
        );
    }

    /// The rail button has to stay a toggle now that there is no dock to close.
    ///
    /// Dispatched, not called: see `workspace_with_agents`.
    #[gpui::test]
    async fn the_rail_button_steps_back_to_the_file_and_forward_again(cx: &mut TestAppContext) {
        let (workspace, project, cx) = workspace_with_agents(cx).await;

        let worktree_id = project.read_with(cx, |project, cx| {
            project.worktrees(cx).next().unwrap().read(cx).id()
        });
        let opened = workspace.update_in(cx, |workspace, window, cx| {
            workspace.open_path((worktree_id, rel_path("a.rs")), None, true, window, cx)
        });
        let file = opened.await.expect("a.rs is in the fake tree");
        cx.run_until_parked();

        let toggle = zed_actions::agent::ToggleAgent {
            agent: project::CLAUDE_CODE_AGENT_ID.to_string(),
        };

        // First press: no agent open, so this is the open path.
        cx.update(|window, cx| window.dispatch_action(Box::new(toggle.clone()), cx));
        cx.run_until_parked();
        let agent = workspace.read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<AgentView>(cx)
                .next()
                .expect("the first press must open the agent")
                .entity_id()
        });
        assert_eq!(
            workspace.read_with(cx, |workspace, cx| workspace
                .active_item(cx)
                .map(|item| item.item_id())),
            Some(agent),
            "the agent is what the first press brings forward"
        );

        // Second press: put away, back to the file.
        cx.update(|window, cx| window.dispatch_action(Box::new(toggle.clone()), cx));
        cx.run_until_parked();
        assert_eq!(
            workspace.read_with(cx, |workspace, cx| workspace
                .active_item(cx)
                .map(|item| item.item_id())),
            Some(file.item_id()),
            "a second press must step back to the tab that was being read"
        );
        assert_eq!(
            agent_tabs(&workspace, cx),
            1,
            "put away, not closed -- the session and its process stay"
        );

        // Third press: forward again, to the session that was never ended.
        cx.update(|window, cx| window.dispatch_action(Box::new(toggle), cx));
        cx.run_until_parked();
        assert_eq!(
            workspace.read_with(cx, |workspace, cx| workspace
                .active_item(cx)
                .map(|item| item.item_id())),
            Some(agent),
            "and a third press comes back to the same session, not a new one"
        );
    }

    /// Every agent the tab bar's `+` menu offers must actually open.
    ///
    /// That menu builds one entry per `project::BUILTIN_AGENTS`, each dispatching
    /// `NewAgent { agent: agent.id }` (see `default_render_tab_bar_buttons` in
    /// `crates/workspace/src/pane.rs`). An id no handler resolves would be a menu
    /// entry that silently does nothing when clicked — the failure mode the rail's
    /// own `every_rail_agent_is_a_registered_builtin` guards against, on the other
    /// route in. Dispatched rather than called, so the registered handler is what
    /// answers.
    ///
    /// What this does NOT cover: that the menu widget renders those entries. The
    /// labels live inside a `PopoverMenu::menu` closure that only runs when the
    /// popover opens, and `ContextMenu`'s item list is private to `crates/ui`, so
    /// the text itself is out of reach from here.
    #[gpui::test]
    async fn every_agent_the_new_menu_offers_can_actually_be_opened(cx: &mut TestAppContext) {
        let (workspace, _project, cx) = workspace_with_agents(cx).await;

        assert!(
            !project::BUILTIN_AGENTS.is_empty(),
            "an empty list would make this pass while the menu offered nothing"
        );

        for (opened, agent) in project::BUILTIN_AGENTS.iter().enumerate() {
            cx.update(|window, cx| {
                window.dispatch_action(
                    Box::new(zed_actions::agent::NewAgent {
                        agent: agent.id.to_string(),
                        mode: None,
                    }),
                    cx,
                )
            });
            cx.run_until_parked();

            assert_eq!(
                agent_tabs(&workspace, cx),
                opened + 1,
                "the `+` menu offers `{}`, so dispatching its action must open a tab",
                agent.id
            );
            assert!(
                workspace.read_with(cx, |workspace, cx| {
                    let id = AgentId::new(agent.id.to_string());
                    workspace
                        .items_of_type::<AgentView>(cx)
                        .any(|view| view.read(cx).is_agent(&id))
                }),
                "and the tab it opens must be `{}` rather than whatever came first",
                agent.id
            );
        }
    }

    /// Accepting a notification must surface the agent that raised it.
    ///
    /// The dock version revealed the whole column, which held every open agent at
    /// once, so "which one" never came up. A tab is one item and only one can be in
    /// front, so taking whichever `AgentView` iterates first would answer a Codex
    /// notification with a Claude Code tab — silently, and only when two different
    /// agents happen to be open, which is why no earlier test caught it.
    ///
    /// Built with `test_new` rather than opened: the claim is about which tab comes
    /// forward, and starting two agents for it would run two real CLIs.
    #[gpui::test]
    async fn a_notification_surfaces_the_agent_that_raised_it(cx: &mut TestAppContext) {
        let (workspace, project, cx) = workspace_with_agents(cx).await;

        let claude = AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string());
        let codex = AgentId::new(project::CODEX_AGENT_ID.to_string());

        // Claude added first, so it is the one an unfiltered `.next()` reaches for.
        for agent in [&claude, &codex] {
            let agent = agent.clone();
            let project = project.clone();
            workspace.update_in(cx, |workspace, window, cx| {
                let handle = workspace.weak_handle();
                let view = cx.new(|cx| {
                    AgentView::test_new(
                        agent,
                        AgentViewMode::Terminal,
                        project,
                        handle,
                        SessionIntent::Untracked,
                        cx,
                    )
                });
                workspace.add_item_to_active_pane(Box::new(view), None, true, window, cx);
            });
        }
        cx.run_until_parked();
        assert_eq!(agent_tabs(&workspace, cx), 2, "both agents must be open");

        // Codex is in front; a Claude notification has to reach past it.
        cx.update(|window, cx| {
            AgentView::activate_for_agent(workspace.clone(), &claude, window, cx)
        });
        cx.run_until_parked();
        assert!(
            workspace.read_with(cx, |workspace, cx| workspace
                .active_item(cx)
                .and_then(|item| item.downcast::<AgentView>())
                .is_some_and(|view| view.read(cx).is_agent(&claude))),
            "a Claude notification must bring the Claude tab forward"
        );

        // And back the other way, so this cannot pass by picking first-in-order.
        cx.update(|window, cx| {
            AgentView::activate_for_agent(workspace.clone(), &codex, window, cx)
        });
        cx.run_until_parked();
        assert!(
            workspace.read_with(cx, |workspace, cx| workspace
                .active_item(cx)
                .and_then(|item| item.downcast::<AgentView>())
                .is_some_and(|view| view.read(cx).is_agent(&codex))),
            "and a Codex notification must bring the Codex tab forward"
        );
    }

    /// An agent split out into another pane is somewhere the user put it on
    /// purpose. Pressing the rail button while reading code in a *different* pane
    /// therefore means "take me to it", not "put it away" — `put_away` only ever
    /// looks at the active pane's active item.
    #[gpui::test]
    async fn the_rail_button_reaches_an_agent_in_another_pane(cx: &mut TestAppContext) {
        let (workspace, _project, cx) = workspace_with_agents(cx).await;
        open_claude(&workspace, cx);

        let agent = workspace.read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<AgentView>(cx)
                .next()
                .expect("the agent just opened")
        });

        // A second pane beside it, holding something else and holding focus.
        workspace.update_in(cx, |workspace, window, cx| {
            let agent_pane = workspace.active_pane().clone();
            let other =
                workspace.split_pane(agent_pane, workspace::SplitDirection::Right, window, cx);
            let item = cx.new(workspace::item::test::TestItem::new);
            other.update(cx, |pane, cx| {
                pane.add_item(Box::new(item), true, true, None, window, cx);
            });
        });
        cx.run_until_parked();
        assert!(
            workspace.read_with(cx, |workspace, cx| workspace
                .active_item(cx)
                .is_some_and(|item| item.downcast::<AgentView>().is_none())),
            "the second pane must be the active one, or this proves nothing"
        );

        cx.update(|window, cx| {
            window.dispatch_action(
                Box::new(zed_actions::agent::ToggleAgent {
                    agent: project::CLAUDE_CODE_AGENT_ID.to_string(),
                }),
                cx,
            )
        });
        cx.run_until_parked();

        assert_eq!(
            workspace.read_with(cx, |workspace, cx| workspace
                .active_item(cx)
                .map(|item| item.item_id())),
            Some(agent.entity_id()),
            "the press must carry the user to the agent where it stands"
        );
        assert_eq!(
            agent_tabs(&workspace, cx),
            1,
            "and must not have started a second session to do it"
        );
    }

    /// Rename has to reach the view, not just the tab.
    ///
    /// Both ways in — the tab's context menu and a double-click on the tab —
    /// dispatch through the *item's* focus handle (`Pane::render_tab`). A handler
    /// registered only on the tab's own element is in that path just while the tab
    /// is unselected, which is never the tab someone right-clicks to rename.
    /// Nothing happened, and nothing errored either.
    ///
    /// Carried over from the agent column's own tests when that column came down:
    /// the column is gone, renaming a session is not, and a feature that outlives
    /// its test is a feature nobody will notice breaking.
    #[gpui::test]
    async fn rename_reaches_the_agent_and_enter_commits_it(cx: &mut TestAppContext) {
        let (workspace, _project, cx) = workspace_with_agents(cx).await;
        open_claude(&workspace, cx);

        let view = workspace.read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<AgentView>(cx)
                .next()
                .expect("the agent just opened")
        });
        assert_eq!(
            view.read_with(cx, |view, _| view.tab_label()),
            "Claude Code",
            "the tab starts on the agent's own name"
        );

        // Exactly what `Pane::render_tab` does for both the menu entry and the
        // double-click: dispatch through the item's focus handle.
        cx.update(|window, cx| {
            view.read(cx)
                .focus_handle(cx)
                .dispatch_action(&crate::RenameAgent, window, cx);
        });
        cx.run_until_parked();

        let editor = view
            .read_with(cx, |view, _| view.rename_editor().cloned())
            .expect("dispatching Rename must open the editor, or the menu entry does nothing");
        assert!(
            cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)),
            "and the editor has to take focus, or there is nothing to type into"
        );
        assert_eq!(
            editor.read_with(cx, |editor, cx| editor.text(cx)),
            "Claude Code",
            "opening it selects the current name, so typing replaces it"
        );

        editor.update_in(cx, |editor, window, cx| {
            editor.set_text("Refactor run", window, cx);
        });
        cx.update(|window, cx| {
            editor
                .focus_handle(cx)
                .dispatch_action(&menu::Confirm, window, cx);
        });
        cx.run_until_parked();

        assert_eq!(
            view.read_with(cx, |view, _| view.tab_label()),
            "Refactor run",
            "Enter must commit the new name"
        );
        assert!(
            view.read_with(cx, |view, _| view.rename_editor().is_none()),
            "and close the editor behind it"
        );
    }

    /// A pane holding nothing but the agent has nowhere to step back to. It must
    /// stay exactly as it is: closing the tab would end a live session over the
    /// second press of a button whose whole job is to be pressed twice.
    #[gpui::test]
    async fn a_pane_holding_only_the_agent_keeps_it_on_a_second_press(cx: &mut TestAppContext) {
        let (workspace, _project, cx) = workspace_with_agents(cx).await;

        let toggle = zed_actions::agent::ToggleAgent {
            agent: project::CLAUDE_CODE_AGENT_ID.to_string(),
        };
        cx.update(|window, cx| window.dispatch_action(Box::new(toggle.clone()), cx));
        cx.run_until_parked();
        assert_eq!(agent_tabs(&workspace, cx), 1, "the agent opened");

        cx.update(|window, cx| window.dispatch_action(Box::new(toggle), cx));
        cx.run_until_parked();

        assert_eq!(
            agent_tabs(&workspace, cx),
            1,
            "with nothing to step back to, the agent stays open"
        );
        assert!(
            workspace.read_with(cx, |workspace, cx| workspace
                .active_item(cx)
                .is_some_and(|item| item.downcast::<AgentView>().is_some())),
            "and stays in front, rather than leaving an empty pane behind"
        );
    }
}

mod persistence {
    use anyhow::Context as _;
    use db::{
        sqlez::{domain::Domain, thread_safe_connection::ThreadSafeConnection},
        sqlez_macros::sql,
    };
    use workspace::{ItemId, WorkspaceDb, WorkspaceId};

    /// One row of `agent_views`.
    ///
    /// A struct rather than a tuple because the tuple had already reached three
    /// fields of which two were `String` — a four-field version would be a shape
    /// where swapping two arguments compiles and restores the wrong thing.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AgentViewRow {
        pub agent: String,
        pub mode: String,
        pub name: Option<String>,
        /// `None` for a tab with no session identity, and for every row written
        /// before the column existed.
        pub session_id: Option<String>,
    }

    pub struct AgentViewDb(ThreadSafeConnection);

    impl Domain for AgentViewDb {
        const NAME: &str = stringify!(AgentViewDb);

        const MIGRATIONS: &[&str] = &[
            // What `SerializableItem for AgentView` writes an agent tab into.
            //
            // It backed that impl while the agent lived among the editor's tabs,
            // then sat unread from `c056596` — which moved the agent into a dock,
            // where the workspace's item-restore machinery does not reach — until
            // the agent rejoined those tabs and the impl came back with it.
            //
            // The table survived that round trip only because these migrations
            // are append-only: editing a past one orphans every install that has
            // already run it. That is also why the custom name arrives as an
            // `ALTER` below rather than a fourth column here.
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
            // Deliberately without a workspace: this is how the person likes to
            // work with an agent, and that travels with them from project to
            // project rather than belonging to one.
            sql!(
                CREATE TABLE agent_preferences(
                    agent TEXT PRIMARY KEY,

                    mode TEXT,
                    width REAL
                ) STRICT;
            ),
            // The name the user gave this session, appended rather than folded
            // into the `CREATE TABLE` above for the reason stated there.
            //
            // Nullable, and read as such: most tabs are never renamed, and a
            // blank string is not the same answer as "never named".
            sql!(
                ALTER TABLE agent_views ADD COLUMN name TEXT;
            ),
            // The session this tab belongs to, so reopening the app puts it back
            // on that conversation rather than on a fresh one.
            //
            // One column, and only the id. The arguments that resume it are
            // derivable from the id through the agent's own session provider,
            // which is the single place that knows each CLI's flags — storing
            // them here would mean a row spawning a stale command the day one of
            // those flags changes, with nothing to catch it. The directory comes
            // back with the session the provider finds.
            //
            // Nullable and read as such: every row written before this column
            // existed has no id, and NULL is a different answer from "".
            sql!(
                ALTER TABLE agent_views ADD COLUMN session_id TEXT;
            ),
        ];
    }

    db::static_connection!(AgentViewDb, [WorkspaceDb]);

    impl AgentViewDb {
        /// The mode this agent was last left in, if it ever has been.
        ///
        /// Nullable: someone can have opened an agent without ever choosing a
        /// mode for it.
        ///
        /// The row still carries a `width`, which nothing reads or writes any
        /// more — the dock owns the column's width now. The column stays
        /// because these migrations are append-only and a past one may not be
        /// edited without orphaning every install that already ran it.
        pub fn preferences(&self, agent: String) -> anyhow::Result<Option<Option<String>>> {
            let sql_stmt = sql!(
                SELECT mode FROM agent_preferences WHERE agent = ?
            );
            self.select_row_bound::<String, Option<String>>(sql_stmt)?(agent)
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

        /// Writes down one agent tab, so the workspace can put it back.
        ///
        /// `INSERT OR REPLACE` is safe here where it would not be on
        /// `agent_preferences`: this row has one writer, and it sets every
        /// column it has.
        pub async fn save_agent(
            &self,
            item_id: ItemId,
            workspace_id: WorkspaceId,
            row: AgentViewRow,
        ) -> anyhow::Result<()> {
            self.write(move |connection| {
                let sql_stmt = sql!(
                    INSERT OR REPLACE INTO agent_views(item_id, workspace_id, agent, mode, name, session_id)
                    VALUES (?, ?, ?, ?, ?, ?)
                );
                let mut query = connection.exec_bound::<(
                    ItemId,
                    WorkspaceId,
                    String,
                    String,
                    Option<String>,
                    Option<String>,
                )>(sql_stmt)?;
                query((
                    item_id,
                    workspace_id,
                    row.agent,
                    row.mode,
                    row.name,
                    row.session_id,
                ))
                .context(format!(
                    "exec_bound failed to execute or parse for: {}",
                    sql_stmt
                ))
            })
            .await
        }

        /// What a tab was left as.
        pub fn get_agent(
            &self,
            item_id: ItemId,
            workspace_id: WorkspaceId,
        ) -> anyhow::Result<AgentViewRow> {
            let sql_stmt = sql!(
                SELECT agent, mode, name, session_id FROM agent_views WHERE item_id = ? AND workspace_id = ?
            );
            let row = self.select_row_bound::<(ItemId, WorkspaceId), (
                String,
                String,
                Option<String>,
                Option<String>,
            )>(sql_stmt)?((item_id, workspace_id))?
            .context("no agent tab was recorded under that item")?;
            Ok(AgentViewRow {
                agent: row.0,
                mode: row.1,
                name: row.2,
                session_id: row.3,
            })
        }

        /// Drops the rows of tabs the workspace did not bring back.
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
