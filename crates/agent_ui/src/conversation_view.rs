//! Mid-port note: some items here are unreachable at the moment.
//!
//! Their only call sites upstream were in `agent_panel` — dragging files onto the
//! agent, inserting the editor's selection, re-authenticating — or in the error
//! mapping for Zed's own model providers, which an external agent replaces with its
//! own ACP errors. Each is either a real affordance waiting to be re-homed on
//! `AgentView` or a variant the protocol may yet report, so they are kept rather
//! than deleted.
#![allow(dead_code)]

use acp_thread::{
    AcpThread, AcpThreadEvent, AgentSessionInfo, AgentThreadEntry, AssistantMessage,
    AssistantMessageChunk, AuthRequired, LoadError, MaxOutputTokensError, MentionUri,
    PermissionOptionChoice, PermissionOptions, PermissionPattern, RetryStatus,
    SelectedPermissionOutcome, ThreadStatus, ToolCall, ToolCallContent, ToolCallStatus,
    UserMessageId,
};
use acp_thread::{AgentConnection, Plan};
use action_log::{ActionLog, ActionLogTelemetry, DiffStats};
use agent_client_protocol::schema as acp;
use agent_servers::{AgentServer, GEMINI_TERMINAL_AUTH_METHOD_ID};
use agent_settings::AgentSettings;
use anyhow::{Result, anyhow};
#[cfg(feature = "audio")]
use audio::{Audio, Sound};
use buffer_diff::BufferDiff;
use collections::{HashMap, HashSet, IndexMap};
use editor::scroll::Autoscroll;
use editor::{
    Editor, EditorEvent, EditorMode, MultiBuffer, SelectionEffects, SizingBehavior,
};
use feature_flags::{AgentSharingFeatureFlag, FeatureFlagAppExt as _};
use file_icons::FileIcons;
use futures::FutureExt as _;
use gpui::{
    Action, Animation, AnimationExt, AnyView, App, ClickEvent, ClipboardItem, CursorStyle, Empty, Entity, EventEmitter, FocusHandle, Focusable, Hsla, ListOffset, ListState,
    ObjectFit, PlatformDisplay, ScrollHandle, SharedString, Subscription, Task, TextStyle,
    WeakEntity, Window, WindowHandle, div, img, linear_color_stop, linear_gradient,
    list, point, pulsating_between,
};
use language::Buffer;
use markdown::{Markdown, MarkdownElement, MarkdownFont, MarkdownStyle};
use parking_lot::RwLock;
use project::{AgentId, AgentServerStore, Project, ProjectEntryId};
use prompt_store::{PromptId, PromptStore};

use crate::DEFAULT_THREAD_TITLE;
use crate::message_editor::SessionCapabilities;
use rope::Point;
use settings::{
    NewThreadLocation, NotifyWhenAgentWaiting, Settings as _, SettingsStore, SidebarSide,
    ThinkingBlockDisplay,
};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use std::{collections::BTreeMap, rc::Rc, time::Duration};
use terminal_view::terminal_panel::TerminalPanel;
use text::Anchor;
use ui::{
    Callout, CircularProgress, CommonAnimationExt, ContextMenu, ContextMenuEntry, CopyButton,
    DecoratedIcon, DiffStat, Disclosure, Divider, DividerColor, IconDecoration, IconDecorationKind,
    KeyBinding, PopoverMenu, PopoverMenuHandle, TintColor, Tooltip, WithScrollbar, prelude::*,
    right_click_menu,
};
use util::{ResultExt, size::format_file_size, time::duration_alt_display};
use util::{debug_panic, defer};
use workspace::PathList;
use workspace::{
    CollaboratorId, MultiWorkspace, NewTerminal, Workspace,
};
use zed_actions::agent::{Chat, ToggleModelSelector};
use zed_actions::assistant::OpenRulesLibrary;

use super::entry_view_state::EntryViewState;
use crate::agent_connection_store::{
    AgentConnectedState, AgentConnectionEntryEvent, AgentConnectionStore,
};
use crate::entry_view_state::{EntryViewEvent, ViewEvent};
use crate::message_editor::{MessageEditor, MessageEditorEvent};

use crate::ThreadId;
use crate::config_options::ConfigOptionsView;
use crate::mode_selector::ModeSelector;
use crate::model_selector_popover::ModelSelectorPopover;
use crate::ui::{AgentNotification, AgentNotificationEvent};
use crate::{
    Agent, AgentInitialContent, AllowAlways, AllowOnce,
    AuthorizeToolCall, ClearMessageQueue, CycleFavoriteModels, CycleModeSelector, EditFirstQueuedMessage, ExpandMessageEditor, Follow, KeepAll, NewThread,
    OpenAddContextMenu, OpenAgentDiff, RejectAll, RejectOnce, RemoveFirstQueuedMessage,
    ScrollOutputLineDown, ScrollOutputLineUp, ScrollOutputPageDown, ScrollOutputPageUp,
    ScrollOutputToBottom, ScrollOutputToNextMessage, ScrollOutputToPreviousMessage,
    ScrollOutputToTop, SendImmediately, SendNextQueuedMessage,
    ToggleProfileSelector, UndoLastReject,
};

const STOPWATCH_THRESHOLD: Duration = Duration::from_secs(30);
const TOKEN_THRESHOLD: u64 = 250;

mod thread_view;
pub use thread_view::*;

pub struct QueuedMessage {
    pub content: Vec<acp::ContentBlock>,
    pub tracked_buffers: Vec<Entity<Buffer>>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ThreadFeedback {
    Positive,
    Negative,
}

#[derive(Debug)]
pub(crate) enum ThreadError {
    PaymentRequired,
    Refusal,
    AuthenticationRequired(SharedString),
    RateLimitExceeded {
        provider: SharedString,
    },
    ServerOverloaded {
        provider: SharedString,
    },
    PromptTooLarge,
    NoApiKey {
        provider: SharedString,
    },
    StreamError {
        provider: SharedString,
    },
    InvalidApiKey {
        provider: SharedString,
    },
    PermissionDenied {
        provider: SharedString,
    },
    RequestFailed,
    MaxOutputTokens,
    NoModelSelected,
    ApiError {
        provider: SharedString,
    },
    Other {
        message: SharedString,
        acp_error_code: Option<SharedString>,
    },
}

impl From<anyhow::Error> for ThreadError {
    fn from(error: anyhow::Error) -> Self {
        // Upstream also unpacked `LanguageModelCompletionError` here, mapping a
        // dozen provider-specific failures (rate limits, overload, bad API key,
        // payment required) onto thread errors. Those describe a request this
        // editor made itself; an external agent talks to its provider on its own
        // and reports back over ACP, so the ACP error is the only one to read.
        if error.is::<MaxOutputTokensError>() {
            Self::MaxOutputTokens
        } else if let Some(acp_error) = error.downcast_ref::<acp::Error>()
            && acp_error.code == acp::ErrorCode::AuthRequired
        {
            Self::AuthenticationRequired(acp_error.message.clone().into())
        } else {
            let message: SharedString = format!("{:#}", error).into();
            let acp_error_code = error
                .downcast_ref::<acp::Error>()
                .map(|acp_error| SharedString::from(acp_error.code.to_string()));

            Self::Other {
                message,
                acp_error_code,
            }
        }
    }
}


#[derive(Default)]
pub(crate) struct Conversation {
    threads: HashMap<acp::SessionId, Entity<AcpThread>>,
    permission_requests: IndexMap<acp::SessionId, Vec<acp::ToolCallId>>,
    subscriptions: Vec<Subscription>,
    updated_at: Option<Instant>,
}

impl Conversation {
    pub fn register_thread(&mut self, thread: Entity<AcpThread>, cx: &mut Context<Self>) {
        let session_id = thread.read(cx).session_id().clone();
        let subscription = cx.subscribe(&thread, {
            let session_id = session_id.clone();
            move |this, _thread, event, _cx| {
                this.updated_at = Some(Instant::now());
                match event {
                    AcpThreadEvent::ToolAuthorizationRequested(id) => {
                        this.permission_requests
                            .entry(session_id.clone())
                            .or_default()
                            .push(id.clone());
                    }
                    AcpThreadEvent::ToolAuthorizationReceived(id) => {
                        if let Some(tool_calls) = this.permission_requests.get_mut(&session_id) {
                            tool_calls.retain(|tool_call_id| tool_call_id != id);
                            if tool_calls.is_empty() {
                                this.permission_requests.shift_remove(&session_id);
                            }
                        }
                    }
                    AcpThreadEvent::NewEntry
                    | AcpThreadEvent::TitleUpdated
                    | AcpThreadEvent::TokenUsageUpdated
                    | AcpThreadEvent::EntryUpdated(_)
                    | AcpThreadEvent::EntriesRemoved(_)
                    | AcpThreadEvent::Retry(_)
                    | AcpThreadEvent::SubagentSpawned(_)
                    | AcpThreadEvent::Stopped(_)
                    | AcpThreadEvent::Error
                    | AcpThreadEvent::LoadError(_)
                    | AcpThreadEvent::PromptCapabilitiesUpdated
                    | AcpThreadEvent::Refusal
                    | AcpThreadEvent::AvailableCommandsUpdated(_)
                    | AcpThreadEvent::ModeUpdated(_)
                    | AcpThreadEvent::ConfigOptionsUpdated(_)
                    | AcpThreadEvent::WorkingDirectoriesUpdated
                    | AcpThreadEvent::PromptUpdated => {}
                }
            }
        });
        self.subscriptions.push(subscription);
        self.threads.insert(session_id, thread);
    }

    pub fn pending_tool_call<'a>(
        &'a self,
        session_id: &acp::SessionId,
        cx: &'a App,
    ) -> Option<(acp::SessionId, acp::ToolCallId, &'a PermissionOptions)> {
        let thread = self.threads.get(session_id)?;
        let is_subagent = thread.read(cx).parent_session_id().is_some();
        let (result_session_id, thread, tool_id) = if is_subagent {
            let id = self.permission_requests.get(session_id)?.iter().next()?;
            (session_id.clone(), thread, id)
        } else {
            let (id, tool_calls) = self.permission_requests.first()?;
            let thread = self.threads.get(id)?;
            let tool_id = tool_calls.iter().next()?;
            (id.clone(), thread, tool_id)
        };
        let (_, tool_call) = thread.read(cx).tool_call(tool_id)?;

        let ToolCallStatus::WaitingForConfirmation { options, .. } = &tool_call.status else {
            return None;
        };
        Some((result_session_id, tool_id.clone(), options))
    }

    pub fn subagents_awaiting_permission(&self, cx: &App) -> Vec<(acp::SessionId, usize)> {
        self.permission_requests
            .iter()
            .filter_map(|(session_id, tool_call_ids)| {
                let thread = self.threads.get(session_id)?;
                if thread.read(cx).parent_session_id().is_some() && !tool_call_ids.is_empty() {
                    Some((session_id.clone(), tool_call_ids.len()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn authorize_pending_tool_call(
        &mut self,
        session_id: &acp::SessionId,
        kind: acp::PermissionOptionKind,
        cx: &mut Context<Self>,
    ) -> Option<()> {
        let (authorize_session_id, tool_call_id, options) =
            self.pending_tool_call(session_id, cx)?;
        let option = options.first_option_of_kind(kind)?;
        self.authorize_tool_call(
            authorize_session_id,
            tool_call_id,
            SelectedPermissionOutcome::new(option.option_id.clone(), option.kind),
            cx,
        );
        Some(())
    }

    pub fn authorize_tool_call(
        &mut self,
        session_id: acp::SessionId,
        tool_call_id: acp::ToolCallId,
        outcome: SelectedPermissionOutcome,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self.threads.get(&session_id) else {
            return;
        };
        let agent_telemetry_id = thread.read(cx).connection().telemetry_id();
        let session_id = thread.read(cx).session_id().clone();

        telemetry::event!(
            "Agent Tool Call Authorized",
            agent = agent_telemetry_id,
            session = session_id,
            option = outcome.option_kind
        );

        thread.update(cx, |thread, cx| {
            thread.authorize_tool_call(tool_call_id, outcome, cx);
        });
        cx.notify();
    }

    fn set_work_dirs(&mut self, work_dirs: PathList, cx: &mut Context<Self>) {
        for thread in self.threads.values() {
            thread.update(cx, |thread, cx| {
                thread.set_work_dirs(work_dirs.clone(), cx);
            });
        }
    }
}

pub(crate) struct RootThreadUpdated;

impl EventEmitter<RootThreadUpdated> for ConversationView {}

fn affects_thread_metadata(event: &AcpThreadEvent) -> bool {
    match event {
        AcpThreadEvent::NewEntry
        | AcpThreadEvent::TitleUpdated
        | AcpThreadEvent::ToolAuthorizationRequested(_)
        | AcpThreadEvent::ToolAuthorizationReceived(_)
        | AcpThreadEvent::Stopped(_)
        | AcpThreadEvent::Error
        | AcpThreadEvent::LoadError(_)
        | AcpThreadEvent::Refusal
        | AcpThreadEvent::WorkingDirectoriesUpdated => true,
        // --
        AcpThreadEvent::EntryUpdated(_)
        | AcpThreadEvent::EntriesRemoved(_)
        | AcpThreadEvent::Retry(_)
        | AcpThreadEvent::TokenUsageUpdated
        | AcpThreadEvent::PromptCapabilitiesUpdated
        | AcpThreadEvent::AvailableCommandsUpdated(_)
        | AcpThreadEvent::ModeUpdated(_)
        | AcpThreadEvent::ConfigOptionsUpdated(_)
        | AcpThreadEvent::SubagentSpawned(_)
        | AcpThreadEvent::PromptUpdated => false,
    }
}

pub enum AcpServerViewEvent {
    ActiveThreadChanged,
}

impl EventEmitter<AcpServerViewEvent> for ConversationView {}

pub struct ConversationView {
    agent: Rc<dyn AgentServer>,
    connection_store: Entity<AgentConnectionStore>,
    connection_key: Agent,
    agent_server_store: Entity<AgentServerStore>,
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    prompt_store: Option<Entity<PromptStore>>,
    pub(crate) thread_id: ThreadId,
    pub(crate) root_session_id: Option<acp::SessionId>,
    server_state: ServerState,
    focus_handle: FocusHandle,
    notifications: Vec<WindowHandle<AgentNotification>>,
    notification_subscriptions: HashMap<WindowHandle<AgentNotification>, Vec<Subscription>>,
    auth_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl ConversationView {
    pub fn has_auth_methods(&self) -> bool {
        self.as_connected().map_or(false, |connected| {
            !connected.connection.auth_methods().is_empty()
        })
    }

    pub fn active_thread(&self) -> Option<&Entity<ThreadView>> {
        match &self.server_state {
            ServerState::Connected(connected) => connected.active_view(),
            _ => None,
        }
    }

    pub fn pending_tool_call<'a>(
        &'a self,
        cx: &'a App,
    ) -> Option<(acp::SessionId, acp::ToolCallId, &'a PermissionOptions)> {
        let session_id = self.active_thread()?.read(cx).session_id.clone();
        self.as_connected()?
            .conversation
            .read(cx)
            .pending_tool_call(&session_id, cx)
    }

    pub fn root_thread_has_pending_tool_call(&self, cx: &App) -> bool {
        let Some(root_thread) = self.root_thread_view() else {
            return false;
        };
        let root_session_id = root_thread.read(cx).thread.read(cx).session_id().clone();
        self.as_connected().is_some_and(|connected| {
            connected
                .conversation
                .read(cx)
                .pending_tool_call(&root_session_id, cx)
                .is_some()
        })
    }


    pub fn root_thread_view(&self) -> Option<Entity<ThreadView>> {
        self.root_session_id
            .as_ref()
            .and_then(|id| self.thread_view(id))
    }

    pub fn thread_view(&self, session_id: &acp::SessionId) -> Option<Entity<ThreadView>> {
        let connected = self.as_connected()?;
        connected.threads.get(session_id).cloned()
    }

    pub fn as_connected(&self) -> Option<&ConnectedServerState> {
        match &self.server_state {
            ServerState::Connected(connected) => Some(connected),
            _ => None,
        }
    }

    pub fn as_connected_mut(&mut self) -> Option<&mut ConnectedServerState> {
        match &mut self.server_state {
            ServerState::Connected(connected) => Some(connected),
            _ => None,
        }
    }

    pub fn updated_at(&self, cx: &App) -> Option<Instant> {
        self.as_connected()
            .and_then(|connected| connected.conversation.read(cx).updated_at)
    }

    pub fn navigate_to_thread(
        &mut self,
        session_id: acp::SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(connected) = self.as_connected_mut() else {
            return;
        };

        connected.navigate_to_thread(session_id);
        if let Some(view) = self.active_thread() {
            view.focus_handle(cx).focus(window, cx);
        }
        cx.emit(AcpServerViewEvent::ActiveThreadChanged);
        cx.notify();
    }

    pub fn set_work_dirs(&mut self, work_dirs: PathList, cx: &mut Context<Self>) {
        if let Some(connected) = self.as_connected() {
            connected.conversation.update(cx, |conversation, cx| {
                conversation.set_work_dirs(work_dirs.clone(), cx);
            });
        }
    }
}

enum ServerState {
    Loading { _loading: Entity<LoadingView> },
    LoadError { error: LoadError },
    Connected(ConnectedServerState),
}

// current -> Entity
// hashmap of threads, current becomes session_id
pub struct ConnectedServerState {
    auth_state: AuthState,
    active_id: Option<acp::SessionId>,
    pub(crate) threads: HashMap<acp::SessionId, Entity<ThreadView>>,
    connection: Rc<dyn AgentConnection>,
    conversation: Entity<Conversation>,
    _connection_entry_subscription: Subscription,
}

enum AuthState {
    Ok,
    Unauthenticated {
        description: Option<Entity<Markdown>>,
        configuration_view: Option<AnyView>,
        pending_auth_method: Option<acp::AuthMethodId>,
        _subscription: Option<Subscription>,
    },
}

impl AuthState {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }
}

struct LoadingView {
    _load_task: Task<()>,
}

impl ConnectedServerState {
    pub fn active_view(&self) -> Option<&Entity<ThreadView>> {
        self.active_id.as_ref().and_then(|id| self.threads.get(id))
    }

    pub fn has_thread_error(&self, cx: &App) -> bool {
        self.active_view()
            .map_or(false, |view| view.read(cx).thread_error.is_some())
    }

    pub fn navigate_to_thread(&mut self, session_id: acp::SessionId) {
        if self.threads.contains_key(&session_id) {
            self.active_id = Some(session_id);
        }
    }

    pub fn close_all_sessions(&self, cx: &mut App) -> Task<()> {
        let tasks = self.threads.values().filter_map(|view| {
            if self.connection.supports_close_session() {
                let session_id = view.read(cx).thread.read(cx).session_id().clone();
                Some(self.connection.clone().close_session(&session_id, cx))
            } else {
                None
            }
        });
        let task = futures::future::join_all(tasks);
        cx.background_spawn(async move {
            task.await;
        })
    }
}

impl ConversationView {
    pub fn new(
        agent: Rc<dyn AgentServer>,
        connection_store: Entity<AgentConnectionStore>,
        connection_key: Agent,
        resume_session_id: Option<acp::SessionId>,
        thread_id: Option<ThreadId>,
        work_dirs: Option<PathList>,
        title: Option<SharedString>,
        initial_content: Option<AgentInitialContent>,
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        prompt_store: Option<Entity<PromptStore>>,
        source: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let agent_server_store = project.read(cx).agent_server_store().clone();
        let subscriptions = vec![
            cx.observe_global_in::<SettingsStore>(window, Self::agent_ui_font_size_changed),
            cx.subscribe_in(
                &agent_server_store,
                window,
                Self::handle_agent_servers_updated,
            ),
        ];

        cx.on_release(|this, cx| {
            if let Some(connected) = this.as_connected() {
                connected.close_all_sessions(cx).detach();
            }
            for window in this.notifications.drain(..) {
                window
                    .update(cx, |_, window, _| {
                        window.remove_window();
                    })
                    .ok();
            }
        })
        .detach();

        let thread_id = thread_id.unwrap_or_else(ThreadId::new);

        Self {
            agent: agent.clone(),
            connection_store: connection_store.clone(),
            connection_key: connection_key.clone(),
            agent_server_store,
            workspace,
            project: project.clone(),
            prompt_store,
            thread_id,
            root_session_id: resume_session_id.clone(),
            server_state: Self::initial_state(
                agent.clone(),
                connection_store,
                connection_key,
                resume_session_id,
                work_dirs,
                title,
                project,
                initial_content,
                source,
                window,
                cx,
            ),
            notifications: Vec::new(),
            notification_subscriptions: HashMap::default(),
            auth_task: None,
            _subscriptions: subscriptions,
            focus_handle: cx.focus_handle(),
        }
    }

    fn set_server_state(&mut self, state: ServerState, cx: &mut Context<Self>) {
        if let Some(connected) = self.as_connected() {
            connected.close_all_sessions(cx).detach();
        }

        self.server_state = state;
        cx.emit(AcpServerViewEvent::ActiveThreadChanged);
        cx.notify();
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (resume_session_id, work_dirs, title) = self
            .root_thread_view()
            .map(|thread_view| {
                let tv = thread_view.read(cx);
                let thread = tv.thread.read(cx);
                (
                    Some(thread.session_id().clone()),
                    thread.work_dirs().cloned(),
                    thread.title(),
                )
            })
            .unwrap_or_else(|| {
                let session_id = self.root_session_id.clone();
                let (work_dirs, title) = session_id
                    .as_ref()
                    // Upstream looked the session's folders and title up in its saved
                    // thread metadata. Nothing is saved here, so a resumed session
                    // starts from the project's own paths.
                    .and_then(|_id| None)
                    .unwrap_or((None, None));
                (session_id, work_dirs, title)
            });

        let state = Self::initial_state(
            self.agent.clone(),
            self.connection_store.clone(),
            self.connection_key.clone(),
            resume_session_id,
            work_dirs,
            title,
            self.project.clone(),
            None,
            "agent_panel",
            window,
            cx,
        );
        self.set_server_state(state, cx);

        if let Some(view) = self.root_thread_view() {
            view.update(cx, |this, cx| {
                this.message_editor.update(cx, |editor, cx| {
                    editor.set_session_capabilities(this.session_capabilities.clone(), cx);
                });
            });
        }
        cx.notify();
    }

    fn initial_state(
        agent: Rc<dyn AgentServer>,
        connection_store: Entity<AgentConnectionStore>,
        connection_key: Agent,
        resume_session_id: Option<acp::SessionId>,
        work_dirs: Option<PathList>,
        title: Option<SharedString>,
        project: Entity<Project>,
        initial_content: Option<AgentInitialContent>,
        source: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ServerState {
        // Upstream refused external agents in a shared project unless the agent was
        // its own. Collaboration went with the hard fork, so no project is shared.
        let session_work_dirs = work_dirs.unwrap_or_else(|| project.read(cx).default_path_list(cx));

        let connection_entry = connection_store.update(cx, |store, cx| {
            store.request_connection(connection_key, agent.clone(), cx)
        });

        let connection_entry_subscription =
            cx.subscribe(&connection_entry, |this, _entry, event, cx| match event {
                AgentConnectionEntryEvent::NewVersionAvailable(version) => {
                    if let Some(thread) = this.root_thread_view() {
                        thread.update(cx, |thread, cx| {
                            thread.new_server_version_available = Some(version.clone());
                            cx.notify();
                        });
                    }
                }
            });

        let connect_result = connection_entry.read(cx).wait_for_connection();

        let side = match AgentSettings::get_global(cx).sidebar_side() {
            SidebarSide::Left => "left",
            SidebarSide::Right => "right",
        };
        let thread_location = match AgentSettings::get_global(cx).new_thread_location {
            NewThreadLocation::LocalProject => "current_worktree",
            NewThreadLocation::NewWorktree => "new_worktree",
        };

        let load_task = cx.spawn_in(window, async move |this, cx| {
            let connection = match connect_result.await {
                Ok(AgentConnectedState { connection, .. }) => connection,
                Err(err) => {
                    this.update_in(cx, |this, window, cx| {
                        this.handle_load_error(err, window, cx);
                        cx.notify();
                    })
                    .log_err();
                    return;
                }
            };

            telemetry::event!(
                "Agent Thread Started",
                agent = connection.telemetry_id(),
                source = source,
                side = side,
                thread_location = thread_location
            );

            let mut resumed_without_history = false;
            let result = if let Some(session_id) = resume_session_id.clone() {
                cx.update(|_, cx| {
                    if connection.supports_load_session() {
                        connection.clone().load_session(
                            session_id,
                            project.clone(),
                            session_work_dirs,
                            title,
                            cx,
                        )
                    } else if connection.supports_resume_session() {
                        resumed_without_history = true;
                        connection.clone().resume_session(
                            session_id,
                            project.clone(),
                            session_work_dirs,
                            title,
                            cx,
                        )
                    } else {
                        Task::ready(Err(anyhow!(LoadError::Other(
                            "Loading or resuming sessions is not supported by this agent.".into()
                        ))))
                    }
                })
                .log_err()
            } else {
                cx.update(|_, cx| {
                    connection
                        .clone()
                        .new_session(project.clone(), session_work_dirs, cx)
                })
                .log_err()
            };

            let Some(result) = result else {
                return;
            };

            let result = match result.await {
                Err(e) => match e.downcast::<acp_thread::AuthRequired>() {
                    Ok(err) => {
                        cx.update(|window, cx| {
                            Self::handle_auth_required(
                                this,
                                err,
                                agent.agent_id(),
                                connection,
                                window,
                                cx,
                            )
                        })
                        .log_err();
                        return;
                    }
                    Err(err) => Err(err),
                },
                Ok(thread) => Ok(thread),
            };

            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(thread) => {
                        let root_session_id = thread.read(cx).session_id().clone();

                        let conversation = cx.new(|cx| {
                            let mut conversation = Conversation::default();
                            conversation.register_thread(thread.clone(), cx);
                            conversation
                        });

                        let current = this.new_thread_view(
                            thread,
                            conversation.clone(),
                            resumed_without_history,
                            initial_content,
                            window,
                            cx,
                        );

                        if this.focus_handle.contains_focused(window, cx) {
                            current
                                .read(cx)
                                .message_editor
                                .focus_handle(cx)
                                .focus(window, cx);
                        }

                        this.root_session_id = Some(root_session_id.clone());
                        this.set_server_state(
                            ServerState::Connected(ConnectedServerState {
                                connection,
                                auth_state: AuthState::Ok,
                                active_id: Some(root_session_id.clone()),
                                threads: HashMap::from_iter([(root_session_id, current)]),
                                conversation,
                                _connection_entry_subscription: connection_entry_subscription,
                            }),
                            cx,
                        );
                    }
                    Err(err) => {
                        this.handle_load_error(
                            LoadError::Other(err.to_string().into()),
                            window,
                            cx,
                        );
                    }
                };
            })
            .log_err();
        });

        let loading_view = cx.new(|_cx| LoadingView {
            _load_task: load_task,
        });

        ServerState::Loading {
            _loading: loading_view,
        }
    }

    fn new_thread_view(
        &self,
        thread: Entity<AcpThread>,
        conversation: Entity<Conversation>,
        resumed_without_history: bool,
        initial_content: Option<AgentInitialContent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ThreadView> {
        let agent_id = self.agent.agent_id();
        let session_capabilities = Arc::new(RwLock::new(SessionCapabilities::new(
            thread.read(cx).prompt_capabilities(),
            thread.read(cx).available_commands().to_vec(),
        )));

        let action_log = thread.read(cx).action_log().clone();

        let entry_view_state = cx.new(|_| {
            EntryViewState::new(
                self.workspace.clone(),
                self.project.downgrade(),
                self.prompt_store.clone(),
                session_capabilities.clone(),
                self.agent.agent_id(),
            )
        });

        let count = thread.read(cx).entries().len();
        let list_state = ListState::new(0, gpui::ListAlignment::Top, px(2048.0));
        list_state.set_follow_mode(gpui::FollowMode::Tail);

        entry_view_state.update(cx, |view_state, cx| {
            for ix in 0..count {
                view_state.sync_entry(ix, &thread, window, cx);
            }
            list_state.splice_focusable(
                0..0,
                (0..count).map(|ix| view_state.entry(ix)?.focus_handle(cx)),
            );
        });

        if let Some(scroll_position) = thread.read(cx).ui_scroll_position() {
            list_state.scroll_to(scroll_position);
        } else {
            list_state.scroll_to_end();
        }

        crate::agent_diff::AgentDiff::set_active_thread(&self.workspace, thread.clone(), window, cx);

        let connection = thread.read(cx).connection().clone();
        let session_id = thread.read(cx).session_id().clone();

        // Check for config options first
        // Config options take precedence over legacy mode/model selectors
        // (feature flag gating happens at the data layer)
        let config_options_provider = connection.session_config_options(&session_id, cx);

        let config_options_view;
        let mode_selector;
        let model_selector;
        if let Some(config_options) = config_options_provider {
            // Use config options - don't create mode_selector or model_selector
            let agent_server = self.agent.clone();
            let fs = self.project.read(cx).fs().clone();
            config_options_view =
                Some(cx.new(|cx| {
                    ConfigOptionsView::new(config_options, agent_server, fs, window, cx)
                }));
            model_selector = None;
            mode_selector = None;
        } else {
            // Fall back to legacy mode/model selectors
            config_options_view = None;
            model_selector = connection.model_selector(&session_id).map(|selector| {
                let agent_server = self.agent.clone();
                let fs = self.project.read(cx).fs().clone();
                cx.new(|cx| {
                    ModelSelectorPopover::new(
                        selector,
                        agent_server,
                        fs,
                        PopoverMenuHandle::default(),
                        self.focus_handle(cx),
                        window,
                        cx,
                    )
                })
            });

            mode_selector = connection
                .session_modes(&session_id, cx)
                .map(|session_modes| {
                    let fs = self.project.read(cx).fs().clone();
                    cx.new(|_cx| ModeSelector::new(session_modes, self.agent.clone(), fs))
                });
        }

        let subscriptions = vec![
            cx.subscribe_in(&thread, window, Self::handle_thread_event),
            cx.observe(&action_log, |_, _, cx| cx.notify()),
        ];

        let subagent_sessions = thread
            .read(cx)
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                AgentThreadEntry::ToolCall(call) => call
                    .subagent_session_info
                    .as_ref()
                    .map(|i| i.session_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();

        if !subagent_sessions.is_empty() {
            let parent_session_id = thread.read(cx).session_id().clone();
            cx.spawn_in(window, async move |this, cx| {
                this.update_in(cx, |this, window, cx| {
                    for subagent_id in subagent_sessions {
                        this.load_subagent_session(
                            subagent_id,
                            parent_session_id.clone(),
                            window,
                            cx,
                        );
                    }
                })
            })
            .detach();
        }

        // Upstream built a profile selector here, driven by its native agent's
        // thread. Profiles are a native-agent concept — tool sets Zed itself
        // enforces — and an external agent governs its own tools.

        let agent_display_name = self
            .agent_server_store
            .read(cx)
            .agent_display_name(&agent_id.clone())
            .unwrap_or_else(|| agent_id.0.clone());

        let agent_icon = self.agent.logo();
        let agent_icon_from_external_svg = self
            .agent_server_store
            .read(cx)
            .agent_icon(&self.agent.agent_id())
            .or_else(|| {
                project::AgentRegistryStore::try_global(cx).and_then(|store| {
                    store
                        .read(cx)
                        .agent(&self.agent.agent_id())
                        .and_then(|a| a.icon_path().cloned())
                })
            });

        let weak = cx.weak_entity();
        cx.new(|cx| {
            ThreadView::new(
                thread,
                conversation,
                weak,
                agent_icon,
                agent_icon_from_external_svg,
                agent_id,
                agent_display_name,
                self.workspace.clone(),
                entry_view_state,
                config_options_view,
                mode_selector,
                model_selector,
                list_state,
                session_capabilities,
                resumed_without_history,
                self.project.downgrade(),
                self.prompt_store.clone(),
                initial_content,
                subscriptions,
                window,
                cx,
            )
        })
    }

    fn handle_auth_required(
        this: WeakEntity<Self>,
        err: AuthRequired,
        _agent_id: AgentId,
        connection: Rc<dyn AgentConnection>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Upstream read `err.provider_id` here and opened that model provider's
        // configuration view inside the agent panel. `AuthRequired` no longer
        // carries a provider — external agents authenticate through their own CLI —
        // so there is no in-editor configuration surface to open.
        let (configuration_view, subscription) = (None, None);

        this.update(cx, |this, cx| {
            let description = err
                .description
                .map(|desc| cx.new(|cx| Markdown::new(desc.into(), None, None, cx)));
            let auth_state = AuthState::Unauthenticated {
                pending_auth_method: None,
                configuration_view,
                description,
                _subscription: subscription,
            };
            if let Some(connected) = this.as_connected_mut() {
                connected.auth_state = auth_state;
                if let Some(view) = connected.active_view()
                    && view
                        .read(cx)
                        .message_editor
                        .focus_handle(cx)
                        .is_focused(window)
                {
                    this.focus_handle.focus(window, cx)
                }
            } else {
                this.set_server_state(
                    ServerState::Connected(ConnectedServerState {
                        auth_state,
                        active_id: None,
                        threads: HashMap::default(),
                        connection,
                        conversation: cx.new(|_cx| Conversation::default()),
                        _connection_entry_subscription: Subscription::new(|| {}),
                    }),
                    cx,
                );
            }
            cx.notify();
        })
        .ok();
    }

    fn handle_load_error(&mut self, err: LoadError, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.root_thread_view() {
            if view
                .read(cx)
                .message_editor
                .focus_handle(cx)
                .is_focused(window)
            {
                self.focus_handle.focus(window, cx)
            }
        }
        self.emit_load_error_telemetry(&err);
        self.set_server_state(ServerState::LoadError { error: err }, cx);
    }

    fn handle_agent_servers_updated(
        &mut self,
        _agent_server_store: &Entity<project::AgentServerStore>,
        _event: &project::AgentServersUpdated,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // If we're in a LoadError state OR have a thread_error set (which can happen
        // when agent.connect() fails during loading), retry loading the thread.
        // This handles the case where a thread is restored before authentication completes.
        let should_retry = match &self.server_state {
            ServerState::Loading { .. } => false,
            ServerState::LoadError { .. } => true,
            ServerState::Connected(connected) => {
                connected.auth_state.is_ok() && connected.has_thread_error(cx)
            }
        };

        if should_retry {
            if let Some(active) = self.root_thread_view() {
                active.update(cx, |active, cx| {
                    active.clear_thread_error(cx);
                });
            }
            self.reset(window, cx);
        }
    }

    pub fn workspace(&self) -> &WeakEntity<Workspace> {
        &self.workspace
    }

    pub fn agent_key(&self) -> &Agent {
        &self.connection_key
    }

    pub fn title(&self, cx: &App) -> SharedString {
        match &self.server_state {
            ServerState::Connected(view) => view
                .active_view()
                .and_then(|v| v.read(cx).thread.read(cx).title())
                .unwrap_or_else(|| DEFAULT_THREAD_TITLE.into()),
            ServerState::Loading { .. } => "Loading…".into(),
            ServerState::LoadError { error, .. } => match error {
                LoadError::Unsupported { .. } => {
                    format!("Upgrade {}", self.agent.agent_id()).into()
                }
                LoadError::FailedToInstall(_) => {
                    format!("Failed to Install {}", self.agent.agent_id()).into()
                }
                LoadError::Exited { .. } => format!("{} Exited", self.agent.agent_id()).into(),
                LoadError::Other(_) => format!("Error Loading {}", self.agent.agent_id()).into(),
            },
        }
    }

    pub fn cancel_generation(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = self.active_thread() {
            active.update(cx, |active, cx| {
                active.cancel_generation(cx);
            });
        }
    }

    pub fn parent_id(&self) -> ThreadId {
        self.thread_id
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.server_state, ServerState::Loading { .. })
    }

    fn send_queued_message_at_index(
        &mut self,
        index: usize,
        is_send_now: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(active) = self.root_thread_view() {
            active.update(cx, |active, cx| {
                active.send_queued_message_at_index(index, is_send_now, window, cx);
            });
        }
    }

    fn move_queued_message_to_main_editor(
        &mut self,
        index: usize,
        inserted_text: Option<&str>,
        cursor_offset: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(active) = self.root_thread_view() {
            active.update(cx, |active, cx| {
                active.move_queued_message_to_main_editor(
                    index,
                    inserted_text,
                    cursor_offset,
                    window,
                    cx,
                );
            });
        }
    }

    fn handle_thread_event(
        &mut self,
        thread: &Entity<AcpThread>,
        event: &AcpThreadEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let session_id = thread.read(cx).session_id().clone();
        let has_thread = self
            .as_connected()
            .is_some_and(|connected| connected.threads.contains_key(&session_id));
        if !has_thread {
            return;
        };
        let is_subagent = thread.read(cx).parent_session_id().is_some();
        if !is_subagent && affects_thread_metadata(event) {
            cx.emit(RootThreadUpdated);
        }
        match event {
            AcpThreadEvent::NewEntry => {
                let len = thread.read(cx).entries().len();
                let index = len - 1;
                if let Some(active) = self.thread_view(&session_id) {
                    let entry_view_state = active.read(cx).entry_view_state.clone();
                    let list_state = active.read(cx).list_state.clone();
                    entry_view_state.update(cx, |view_state, cx| {
                        view_state.sync_entry(index, thread, window, cx);
                        list_state.splice_focusable(
                            index..index,
                            [view_state
                                .entry(index)
                                .and_then(|entry| entry.focus_handle(cx))],
                        );
                    });
                    active.update(cx, |active, cx| {
                        active.sync_editor_mode_for_empty_state(cx);
                    });
                }
            }
            AcpThreadEvent::EntryUpdated(index) => {
                if let Some(active) = self.thread_view(&session_id) {
                    let entry_view_state = active.read(cx).entry_view_state.clone();
                    let list_state = active.read(cx).list_state.clone();
                    entry_view_state.update(cx, |view_state, cx| {
                        view_state.sync_entry(*index, thread, window, cx);
                    });
                    list_state.remeasure_items(*index..*index + 1);
                    active.update(cx, |active, cx| {
                        active.auto_expand_streaming_thought(cx);
                    });
                }
            }
            AcpThreadEvent::EntriesRemoved(range) => {
                if let Some(active) = self.thread_view(&session_id) {
                    let entry_view_state = active.read(cx).entry_view_state.clone();
                    let list_state = active.read(cx).list_state.clone();
                    entry_view_state.update(cx, |view_state, _cx| view_state.remove(range.clone()));
                    list_state.splice(range.clone(), 0);
                    active.update(cx, |active, cx| {
                        active.sync_editor_mode_for_empty_state(cx);
                    });
                }
            }
            AcpThreadEvent::SubagentSpawned(subagent_session_id) => {
                self.load_subagent_session(subagent_session_id.clone(), session_id, window, cx)
            }
            AcpThreadEvent::ToolAuthorizationRequested(_) => {
                self.notify_with_sound("Waiting for tool confirmation", IconName::Info, window, cx);
            }
            AcpThreadEvent::ToolAuthorizationReceived(_) => {}
            AcpThreadEvent::Retry(retry) => {
                if let Some(active) = self.thread_view(&session_id) {
                    active.update(cx, |active, _cx| {
                        active.thread_retry_status = Some(retry.clone());
                    });
                }
            }
            AcpThreadEvent::Stopped(stop_reason) => {
                if let Some(active) = self.thread_view(&session_id) {
                    let is_generating =
                        matches!(thread.read(cx).status(), ThreadStatus::Generating);
                    active.update(cx, |active, cx| {
                        if !is_generating {
                            active.thread_retry_status.take();
                            active.clear_auto_expand_tracking();
                            if active.list_state.is_following_tail() {
                                active.list_state.scroll_to_end();
                            }
                        }
                        active.sync_generating_indicator(cx);
                    });
                }
                if is_subagent {
                    if *stop_reason == acp::StopReason::EndTurn {
                        thread.update(cx, |thread, cx| {
                            thread.mark_as_subagent_output(cx);
                        });
                    }
                    return;
                }

                let used_tools = thread.read(cx).used_tools_since_last_user_message();
                self.notify_with_sound(
                    if used_tools {
                        "Finished running tools"
                    } else {
                        "New message"
                    },
                    IconName::ZedAssistant,
                    window,
                    cx,
                );

                let should_send_queued = if let Some(active) = self.root_thread_view() {
                    active.update(cx, |active, cx| {
                        if active.skip_queue_processing_count > 0 {
                            active.skip_queue_processing_count -= 1;
                            false
                        } else if active.user_interrupted_generation {
                            // Manual interruption: don't auto-process queue.
                            // Reset the flag so future completions can process normally.
                            active.user_interrupted_generation = false;
                            false
                        } else {
                            let has_queued = !active.local_queued_messages.is_empty();
                            // Don't auto-send if the first message editor is currently focused
                            let is_first_editor_focused = active
                                .queued_message_editors
                                .first()
                                .is_some_and(|editor| editor.focus_handle(cx).is_focused(window));
                            has_queued && !is_first_editor_focused
                        }
                    })
                } else {
                    false
                };
                if should_send_queued {
                    self.send_queued_message_at_index(0, false, window, cx);
                }
            }
            AcpThreadEvent::Refusal => {
                let error = ThreadError::Refusal;
                if let Some(active) = self.thread_view(&session_id) {
                    active.update(cx, |active, cx| {
                        active.handle_thread_error(error, cx);
                        active.thread_retry_status.take();
                    });
                }
                if !is_subagent {
                    let model_or_agent_name = self.current_model_name(cx);
                    let notification_message =
                        format!("{} refused to respond to this request", model_or_agent_name);
                    self.notify_with_sound(&notification_message, IconName::Warning, window, cx);
                }
            }
            AcpThreadEvent::Error => {
                if let Some(active) = self.thread_view(&session_id) {
                    let is_generating =
                        matches!(thread.read(cx).status(), ThreadStatus::Generating);
                    active.update(cx, |active, cx| {
                        if !is_generating {
                            active.thread_retry_status.take();
                            if active.list_state.is_following_tail() {
                                active.list_state.scroll_to_end();
                            }
                        }
                        active.sync_generating_indicator(cx);
                    });
                }
                if !is_subagent {
                    self.notify_with_sound(
                        "Agent stopped due to an error",
                        IconName::Warning,
                        window,
                        cx,
                    );
                }
            }
            AcpThreadEvent::LoadError(error) => {
                if let Some(view) = self.root_thread_view() {
                    if view
                        .read(cx)
                        .message_editor
                        .focus_handle(cx)
                        .is_focused(window)
                    {
                        self.focus_handle.focus(window, cx)
                    }
                }
                self.set_server_state(
                    ServerState::LoadError {
                        error: error.clone(),
                    },
                    cx,
                );
            }
            AcpThreadEvent::TitleUpdated => {
                if let Some(title) = thread.read(cx).title()
                    && let Some(active_thread) = self.thread_view(&session_id)
                {
                    let title_editor = active_thread.read(cx).title_editor.clone();
                    title_editor.update(cx, |editor, cx| {
                        if editor.text(cx) != title {
                            editor.set_text(title, window, cx);
                        }
                    });
                }
                cx.notify();
            }
            AcpThreadEvent::PromptCapabilitiesUpdated => {
                if let Some(active) = self.thread_view(&session_id) {
                    active.update(cx, |active, _cx| {
                        active
                            .session_capabilities
                            .write()
                            .set_prompt_capabilities(thread.read(_cx).prompt_capabilities());
                    });
                }
            }
            AcpThreadEvent::TokenUsageUpdated => {
                if let Some(active) = self.thread_view(&session_id) {
                    active.update(cx, |active, cx| {
                        active.update_turn_tokens(cx);
                    });
                }
            }
            AcpThreadEvent::AvailableCommandsUpdated(available_commands) => {
                if let Some(thread_view) = self.thread_view(&session_id) {
                    let has_commands = !available_commands.is_empty();

                    let agent_display_name = self
                        .agent_server_store
                        .read(cx)
                        .agent_display_name(&self.agent.agent_id())
                        .unwrap_or_else(|| self.agent.agent_id().0.to_string().into());

                    let new_placeholder =
                        placeholder_text(agent_display_name.as_ref(), has_commands);

                    thread_view.update(cx, |thread_view, cx| {
                        thread_view
                            .session_capabilities
                            .write()
                            .set_available_commands(available_commands.clone());
                        thread_view.message_editor.update(cx, |editor, cx| {
                            editor.set_placeholder_text(&new_placeholder, window, cx);
                        });
                    });
                }
            }
            AcpThreadEvent::ModeUpdated(_mode) => {
                // The connection keeps track of the mode
                cx.notify();
            }
            AcpThreadEvent::ConfigOptionsUpdated(_) => {
                // The watch task in ConfigOptionsView handles rebuilding selectors
                cx.notify();
            }
            AcpThreadEvent::WorkingDirectoriesUpdated => {
                cx.notify();
            }
            AcpThreadEvent::PromptUpdated => {
                cx.notify();
            }
        }
        cx.notify();
    }

    fn authenticate(
        &mut self,
        method: acp::AuthMethodId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let Some(connected) = self.as_connected_mut() else {
            return;
        };
        let connection = connected.connection.clone();

        let AuthState::Unauthenticated {
            configuration_view,
            pending_auth_method,
            ..
        } = &mut connected.auth_state
        else {
            return;
        };

        let agent_telemetry_id = connection.telemetry_id();

        if let Some(login_task) = connection.terminal_auth_task(&method, cx) {
            configuration_view.take();
            pending_auth_method.replace(method.clone());

            let project = self.project.clone();
            cx.notify();
            self.auth_task = Some(cx.spawn_in(window, {
                async move |this, cx| {
                    let result = async {
                        let login = login_task.await?;
                        this.update_in(cx, |_this, window, cx| {
                            Self::spawn_external_agent_login(
                                login,
                                workspace,
                                project,
                                method.clone(),
                                false,
                                window,
                                cx,
                            )
                        })?
                        .await
                    }
                    .await;

                    match &result {
                        Ok(_) => telemetry::event!(
                            "Authenticate Agent Succeeded",
                            agent = agent_telemetry_id
                        ),
                        Err(_) => {
                            telemetry::event!(
                                "Authenticate Agent Failed",
                                agent = agent_telemetry_id,
                            )
                        }
                    }

                    this.update_in(cx, |this, window, cx| {
                        if let Err(err) = result {
                            if let Some(ConnectedServerState {
                                auth_state:
                                    AuthState::Unauthenticated {
                                        pending_auth_method,
                                        ..
                                    },
                                ..
                            }) = this.as_connected_mut()
                            {
                                pending_auth_method.take();
                            }
                            if let Some(active) = this.root_thread_view() {
                                active.update(cx, |active, cx| {
                                    active.handle_thread_error(err, cx);
                                })
                            }
                        } else {
                            this.reset(window, cx);
                        }
                        this.auth_task.take()
                    })
                    .ok();
                }
            }));
            return;
        }

        configuration_view.take();
        pending_auth_method.replace(method.clone());

        let authenticate = connection.authenticate(method, cx);
        cx.notify();
        self.auth_task = Some(cx.spawn_in(window, {
            async move |this, cx| {
                let result = authenticate.await;

                match &result {
                    Ok(_) => telemetry::event!(
                        "Authenticate Agent Succeeded",
                        agent = agent_telemetry_id
                    ),
                    Err(_) => {
                        telemetry::event!("Authenticate Agent Failed", agent = agent_telemetry_id,)
                    }
                }

                this.update_in(cx, |this, window, cx| {
                    if let Err(err) = result {
                        if let Some(ConnectedServerState {
                            auth_state:
                                AuthState::Unauthenticated {
                                    pending_auth_method,
                                    ..
                                },
                            ..
                        }) = this.as_connected_mut()
                        {
                            pending_auth_method.take();
                        }
                        if let Some(active) = this.root_thread_view() {
                            active.update(cx, |active, cx| active.handle_thread_error(err, cx));
                        }
                    } else {
                        this.reset(window, cx);
                    }
                    this.auth_task.take()
                })
                .ok();
            }
        }));
    }

    fn load_subagent_session(
        &mut self,
        subagent_id: acp::SessionId,
        parent_session_id: acp::SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(connected) = self.as_connected() else {
            return;
        };
        if connected.threads.contains_key(&subagent_id)
            || !connected.connection.supports_load_session()
        {
            return;
        }
        let Some(parent_thread) = connected.threads.get(&parent_session_id) else {
            return;
        };
        let work_dirs = parent_thread
            .read(cx)
            .thread
            .read(cx)
            .work_dirs()
            .cloned()
            .unwrap_or_else(|| self.project.read(cx).default_path_list(cx));

        let subagent_thread_task = connected.connection.clone().load_session(
            subagent_id,
            self.project.clone(),
            work_dirs,
            None,
            cx,
        );

        cx.spawn_in(window, async move |this, cx| {
            let subagent_thread = subagent_thread_task.await?;
            this.update_in(cx, |this, window, cx| {
                let Some(conversation) = this
                    .as_connected()
                    .map(|connected| connected.conversation.clone())
                else {
                    return;
                };
                let subagent_session_id = subagent_thread.read(cx).session_id().clone();
                conversation.update(cx, |conversation, cx| {
                    conversation.register_thread(subagent_thread.clone(), cx);
                });
                let view =
                    this.new_thread_view(subagent_thread, conversation, false, None, window, cx);
                let Some(connected) = this.as_connected_mut() else {
                    return;
                };
                connected.threads.insert(subagent_session_id, view);
            })
        })
        .detach();
    }

    fn spawn_external_agent_login(
        login: task::SpawnInTerminal,
        workspace: Entity<Workspace>,
        project: Entity<Project>,
        method: acp::AuthMethodId,
        previous_attempt: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<()>> {
        let Some(terminal_panel) = workspace.read(cx).panel::<TerminalPanel>(cx) else {
            return Task::ready(Err(anyhow!("Terminal panel is unavailable")));
        };

        window.spawn(cx, async move |cx| {
            let mut task = login.clone();
            if let Some(cmd) = &task.command {
                // Have "node" command use Zed's managed Node runtime by default
                if cmd == "node" {
                    let resolved_node_runtime = project.update(cx, |project, cx| {
                        let agent_server_store = project.agent_server_store().clone();
                        agent_server_store.update(cx, |store, cx| {
                            store.node_runtime().map(|node_runtime| {
                                cx.background_spawn(async move { node_runtime.binary_path().await })
                            })
                        })
                    });

                    if let Some(resolve_task) = resolved_node_runtime {
                        if let Ok(node_path) = resolve_task.await {
                            task.command = Some(node_path.to_string_lossy().to_string());
                        }
                    }
                }
            }
            task.shell = task::Shell::WithArguments {
                program: task.command.take().expect("login command should be set"),
                args: std::mem::take(&mut task.args),
                title_override: None,
            };

            let terminal = terminal_panel
                .update_in(cx, |terminal_panel, window, cx| {
                    terminal_panel.spawn_task(&task, window, cx)
                })?
                .await?;

            let success_patterns = match method.0.as_ref() {
                "claude-login" | GEMINI_TERMINAL_AUTH_METHOD_ID => vec![
                    "Login successful".to_string(),
                    "Type your message".to_string(),
                ],
                _ => Vec::new(),
            };
            if success_patterns.is_empty() {
                // No success patterns specified: wait for the process to exit and check exit code
                let exit_status = terminal
                    .read_with(cx, |terminal, cx| terminal.wait_for_completed_task(cx))?
                    .await;

                match exit_status {
                    Some(status) if status.success() => Ok(()),
                    Some(status) => Err(anyhow!(
                        "Login command failed with exit code: {:?}",
                        status.code()
                    )),
                    None => Err(anyhow!("Login command terminated without exit status")),
                }
            } else {
                // Look for specific output patterns to detect successful login
                let mut exit_status = terminal
                    .read_with(cx, |terminal, cx| terminal.wait_for_completed_task(cx))?
                    .fuse();

                let logged_in = cx
                    .spawn({
                        let terminal = terminal.clone();
                        async move |cx| {
                            loop {
                                cx.background_executor().timer(Duration::from_secs(1)).await;
                                let content =
                                    terminal.update(cx, |terminal, _cx| terminal.get_content())?;
                                if success_patterns
                                    .iter()
                                    .any(|pattern| content.contains(pattern))
                                {
                                    return anyhow::Ok(());
                                }
                            }
                        }
                    })
                    .fuse();
                futures::pin_mut!(logged_in);
                futures::select_biased! {
                    result = logged_in => {
                        if let Err(e) = result {
                            log::error!("{e}");
                            return Err(anyhow!("exited before logging in"));
                        }
                    }
                    _ = exit_status => {
                        if !previous_attempt
                            && project.read_with(cx, |project, _| project.is_via_remote_server())
                            && method.0.as_ref() == GEMINI_TERMINAL_AUTH_METHOD_ID
                        {
                            return cx
                                .update(|window, cx| {
                                    Self::spawn_external_agent_login(
                                        login,
                                        workspace,
                                        project.clone(),
                                        method,
                                        true,
                                        window,
                                        cx,
                                    )
                                })?
                                .await;
                        }
                        return Err(anyhow!("exited before logging in"));
                    }
                }
                terminal.update(cx, |terminal, _| terminal.kill_active_task())?;
                Ok(())
            }
        })
    }

    pub fn has_user_submitted_prompt(&self, cx: &App) -> bool {
        self.root_thread_view().is_some_and(|active| {
            active
                .read(cx)
                .thread
                .read(cx)
                .entries()
                .iter()
                .any(|entry| matches!(entry, AgentThreadEntry::UserMessage(_)))
        })
    }

    fn render_auth_required_state(
        &self,
        connection: &Rc<dyn AgentConnection>,
        description: Option<&Entity<Markdown>>,
        configuration_view: Option<&AnyView>,
        pending_auth_method: Option<&acp::AuthMethodId>,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let auth_methods = connection.auth_methods();

        let agent_display_name = self
            .agent_server_store
            .read(cx)
            .agent_display_name(&self.agent.agent_id())
            .unwrap_or_else(|| self.agent.agent_id().0);

        let show_fallback_description = auth_methods.len() > 1
            && configuration_view.is_none()
            && description.is_none()
            && pending_auth_method.is_none();

        let auth_buttons = || {
            h_flex().justify_end().flex_wrap().gap_1().children(
                connection
                    .auth_methods()
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(ix, method)| {
                        let (method_id, name) = (method.id().0.clone(), method.name().to_string());
                        let agent_telemetry_id = connection.telemetry_id();

                        Button::new(method_id.clone(), name)
                            .label_size(LabelSize::Small)
                            .map(|this| {
                                if ix == 0 {
                                    this.style(ButtonStyle::Tinted(TintColor::Accent))
                                } else {
                                    this.style(ButtonStyle::Outlined)
                                }
                            })
                            .when_some(method.description(), |this, description| {
                                this.tooltip(Tooltip::text(description.to_string()))
                            })
                            .on_click({
                                cx.listener(move |this, _, window, cx| {
                                    telemetry::event!(
                                        "Authenticate Agent Started",
                                        agent = agent_telemetry_id,
                                        method = method_id
                                    );

                                    this.authenticate(
                                        acp::AuthMethodId::new(method_id.clone()),
                                        window,
                                        cx,
                                    )
                                })
                            })
                    }),
            )
        };

        if pending_auth_method.is_some() {
            return Callout::new()
                .icon(IconName::Info)
                .title(format!("Authenticating to {}…", agent_display_name))
                .actions_slot(
                    Icon::new(IconName::ArrowCircle)
                        .size(IconSize::Small)
                        .color(Color::Muted)
                        .with_rotate_animation(2)
                        .into_any_element(),
                )
                .into_any_element();
        }

        Callout::new()
            .icon(IconName::Info)
            .title(format!("Authenticate to {}", agent_display_name))
            .when(auth_methods.len() == 1, |this| {
                this.actions_slot(auth_buttons())
            })
            .description_slot(
                v_flex()
                    .text_ui(cx)
                    .map(|this| {
                        if show_fallback_description {
                            this.child(
                                Label::new("Choose one of the following authentication options:")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                        } else {
                            this.children(
                                configuration_view
                                    .cloned()
                                    .map(|view| div().w_full().child(view)),
                            )
                            .children(description.map(|desc| {
                                self.render_markdown(
                                    desc.clone(),
                                    MarkdownStyle::themed(MarkdownFont::Editor, window, cx),
                                )
                            }))
                        }
                    })
                    .when(auth_methods.len() > 1, |this| {
                        this.gap_1().child(auth_buttons())
                    }),
            )
            .into_any_element()
    }

    fn emit_load_error_telemetry(&self, error: &LoadError) {
        let error_kind = match error {
            LoadError::Unsupported { .. } => "unsupported",
            LoadError::FailedToInstall(_) => "failed_to_install",
            LoadError::Exited { .. } => "exited",
            LoadError::Other(_) => "other",
        };

        let agent_name = self.agent.agent_id();

        telemetry::event!(
            "Agent Panel Error Shown",
            agent = agent_name,
            kind = error_kind,
            message = error.to_string(),
        );
    }

    fn render_load_error(
        &self,
        e: &LoadError,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (title, message, action_slot): (_, SharedString, _) = match e {
            LoadError::Unsupported {
                command: path,
                current_version,
                minimum_version,
            } => {
                return self.render_unsupported(path, current_version, minimum_version, window, cx);
            }
            LoadError::FailedToInstall(msg) => (
                "Failed to Install",
                msg.into(),
                Some(self.create_copy_button(msg.to_string()).into_any_element()),
            ),
            LoadError::Exited { status } => (
                "Failed to Launch",
                format!("Server exited with status {status}").into(),
                None,
            ),
            LoadError::Other(msg) => (
                "Failed to Launch",
                msg.into(),
                Some(self.create_copy_button(msg.to_string()).into_any_element()),
            ),
        };

        Callout::new()
            .severity(Severity::Error)
            .icon(IconName::XCircleFilled)
            .title(title)
            .description(message)
            .actions_slot(div().children(action_slot))
            .into_any_element()
    }

    fn render_unsupported(
        &self,
        path: &SharedString,
        version: &SharedString,
        minimum_version: &SharedString,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (heading_label, description_label) = (
            format!("Upgrade {} to work with Zed", self.agent.agent_id()),
            if version.is_empty() {
                format!(
                    "Currently using {}, which does not report a valid --version",
                    path,
                )
            } else {
                format!(
                    "Currently using {}, which is only version {} (need at least {minimum_version})",
                    path, version
                )
            },
        );

        v_flex()
            .w_full()
            .p_3p5()
            .gap_2p5()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .bg(linear_gradient(
                180.,
                linear_color_stop(cx.theme().colors().editor_background.opacity(0.4), 4.),
                linear_color_stop(cx.theme().status().info_background.opacity(0.), 0.),
            ))
            .child(
                v_flex().gap_0p5().child(Label::new(heading_label)).child(
                    Label::new(description_label)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
            )
            .into_any_element()
    }



    // `as_native_connection` / `as_native_thread` downcast the ACP connection to
    // Zed's in-process agent. Every agent here is an external CLI, so the downcast
    // could only ever return None and every caller took the other branch.

    fn queued_messages_len(&self, cx: &App) -> usize {
        self.root_thread_view()
            .map(|thread| thread.read(cx).local_queued_messages.len())
            .unwrap_or_default()
    }

    fn update_queued_message(
        &mut self,
        index: usize,
        content: Vec<acp::ContentBlock>,
        tracked_buffers: Vec<Entity<Buffer>>,
        cx: &mut Context<Self>,
    ) -> bool {
        match self.root_thread_view() {
            Some(thread) => thread.update(cx, |thread, _cx| {
                if index < thread.local_queued_messages.len() {
                    thread.local_queued_messages[index] = QueuedMessage {
                        content,
                        tracked_buffers,
                    };
                    true
                } else {
                    false
                }
            }),
            None => false,
        }
    }

    fn queued_message_contents(&self, cx: &App) -> Vec<Vec<acp::ContentBlock>> {
        match self.root_thread_view() {
            None => Vec::new(),
            Some(thread) => thread
                .read(cx)
                .local_queued_messages
                .iter()
                .map(|q| q.content.clone())
                .collect(),
        }
    }

    fn save_queued_message_at_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let editor = match self.root_thread_view() {
            Some(thread) => thread.read(cx).queued_message_editors.get(index).cloned(),
            None => None,
        };
        let Some(editor) = editor else {
            return;
        };

        let contents_task = editor.update(cx, |editor, cx| editor.contents(false, cx));

        cx.spawn(async move |this, cx| {
            let Ok((content, tracked_buffers)) = contents_task.await else {
                return Ok::<(), anyhow::Error>(());
            };

            this.update(cx, |this, cx| {
                this.update_queued_message(index, content, tracked_buffers, cx);
                cx.notify();
            })?;

            Ok(())
        })
        .detach_and_log_err(cx);
    }

    fn sync_queued_message_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let needed_count = self.queued_messages_len(cx);
        let queued_messages = self.queued_message_contents(cx);

        let agent_name = self.agent.agent_id();
        let workspace = self.workspace.clone();
        let project = self.project.downgrade();
        let Some(connected) = self.as_connected() else {
            return;
        };
        let Some(thread) = connected.active_view() else {
            return;
        };
        let session_capabilities = thread.read(cx).session_capabilities.clone();

        let current_count = thread.read(cx).queued_message_editors.len();
        let last_synced = thread.read(cx).last_synced_queue_length;

        if current_count == needed_count && needed_count == last_synced {
            return;
        }

        if current_count > needed_count {
            thread.update(cx, |thread, _cx| {
                thread.queued_message_editors.truncate(needed_count);
                thread
                    .queued_message_editor_subscriptions
                    .truncate(needed_count);
            });

            let editors = thread.read(cx).queued_message_editors.clone();
            for (index, editor) in editors.into_iter().enumerate() {
                if let Some(content) = queued_messages.get(index) {
                    editor.update(cx, |editor, cx| {
                        editor.set_read_only(true, cx);
                        editor.set_message(content.clone(), window, cx);
                    });
                }
            }
        }

        while thread.read(cx).queued_message_editors.len() < needed_count {
            let index = thread.read(cx).queued_message_editors.len();
            let content = queued_messages.get(index).cloned().unwrap_or_default();

            let editor = cx.new(|cx| {
                let mut editor = MessageEditor::new(
                    workspace.clone(),
                    project.clone(),
                    None,
                    session_capabilities.clone(),
                    agent_name.clone(),
                    "",
                    EditorMode::AutoHeight {
                        min_lines: 1,
                        max_lines: Some(10),
                    },
                    window,
                    cx,
                );
                editor.set_read_only(true, cx);
                editor.set_message(content, window, cx);
                editor
            });

            let subscription = cx.subscribe_in(
                &editor,
                window,
                move |this, _editor, event, window, cx| match event {
                    MessageEditorEvent::InputAttempted {
                        text,
                        cursor_offset,
                    } => this.move_queued_message_to_main_editor(
                        index,
                        Some(text.as_ref()),
                        Some(*cursor_offset),
                        window,
                        cx,
                    ),
                    MessageEditorEvent::LostFocus => {
                        this.save_queued_message_at_index(index, cx);
                    }
                    MessageEditorEvent::Cancel => {
                        window.focus(&this.focus_handle(cx), cx);
                    }
                    MessageEditorEvent::Send => {
                        window.focus(&this.focus_handle(cx), cx);
                    }
                    MessageEditorEvent::SendImmediately => {
                        this.send_queued_message_at_index(index, true, window, cx);
                    }
                    _ => {}
                },
            );

            thread.update(cx, |thread, _cx| {
                thread.queued_message_editors.push(editor);
                thread
                    .queued_message_editor_subscriptions
                    .push(subscription);
            });
        }

        if let Some(active) = self.root_thread_view() {
            active.update(cx, |active, _cx| {
                active.last_synced_queue_length = needed_count;
            });
        }
    }

    fn render_markdown(&self, markdown: Entity<Markdown>, style: MarkdownStyle) -> MarkdownElement {
        let workspace = self.workspace.clone();
        MarkdownElement::new(markdown, style).on_url_click(move |text, window, cx| {
            crate::conversation_view::thread_view::open_link(text, &workspace, window, cx);
        })
    }

    fn notify_with_sound(
        &mut self,
        caption: impl Into<SharedString>,
        icon: IconName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(feature = "audio")]
        self.play_notification_sound(window, cx);
        self.show_notification(caption, icon, window, cx);
    }

    /// Whether this conversation is the one the user is actually looking at.
    ///
    /// The answer decides whether a finished turn is worth a notification, so it has
    /// to mean "on screen", not merely "open". Upstream asked its dock panel; agents
    /// live in the centre pane here, so it asks the pane instead: is some `AgentView`
    /// the active item of its pane, and is this the conversation it holds.
    fn is_visible(&self, cx: &Context<Self>) -> bool {
        let Some(workspace) = self.workspace.upgrade() else {
            return false;
        };
        let this = cx.entity_id();
        workspace.read(cx).panes().iter().any(|pane| {
            pane.read(cx)
                .active_item()
                .and_then(|item| item.downcast::<crate::AgentView>())
                .and_then(|view| view.read(cx).conversation_view().cloned())
                .is_some_and(|conversation| conversation.entity_id() == this)
        })
    }

    fn agent_status_visible(&self, window: &Window, cx: &Context<Self>) -> bool {
        window.is_window_active() && self.is_visible(cx)
    }

    #[cfg(feature = "audio")]
    fn play_notification_sound(&self, window: &Window, cx: &mut Context<Self>) {
        let visible = window.is_window_active() && self.is_visible(cx);
        let settings = AgentSettings::get_global(cx);
        if settings.play_sound_when_agent_done.should_play(visible) {
            Audio::play_sound(Sound::AgentDone, cx);
        }
    }

    fn show_notification(
        &mut self,
        caption: impl Into<SharedString>,
        icon: IconName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.notifications.is_empty() {
            return;
        }

        let settings = AgentSettings::get_global(cx);

        let should_notify = !self.agent_status_visible(window, cx);

        if !should_notify {
            return;
        }

        let Some(root_thread) = self.root_thread_view() else {
            return;
        };
        let root_thread = root_thread.read(cx).thread.read(cx);
        let root_session_id = root_thread.session_id().clone();
        let root_work_dirs = root_thread.work_dirs().cloned();
        let root_title = root_thread.title();

        // TODO: Change this once we have title summarization for external agents.
        let title = self.agent.agent_id().0;

        match settings.notify_when_agent_waiting {
            NotifyWhenAgentWaiting::PrimaryScreen => {
                if let Some(primary) = cx.primary_display() {
                    self.pop_up(
                        icon,
                        caption.into(),
                        title,
                        root_session_id,
                        root_work_dirs,
                        root_title,
                        window,
                        primary,
                        cx,
                    );
                }
            }
            NotifyWhenAgentWaiting::AllScreens => {
                let caption = caption.into();
                for screen in cx.displays() {
                    self.pop_up(
                        icon,
                        caption.clone(),
                        title.clone(),
                        root_session_id.clone(),
                        root_work_dirs.clone(),
                        root_title.clone(),
                        window,
                        screen,
                        cx,
                    );
                }
            }
            NotifyWhenAgentWaiting::Never => {
                // Don't show anything
            }
        }
    }

    fn pop_up(
        &mut self,
        icon: IconName,
        caption: SharedString,
        title: SharedString,
        root_session_id: acp::SessionId,
        root_work_dirs: Option<PathList>,
        root_title: Option<SharedString>,
        window: &mut Window,
        screen: Rc<dyn PlatformDisplay>,
        cx: &mut Context<Self>,
    ) {
        let options = AgentNotification::window_options(screen, cx);

        let project_name = self.workspace.upgrade().and_then(|workspace| {
            workspace
                .read(cx)
                .project()
                .read(cx)
                .visible_worktrees(cx)
                .next()
                .map(|worktree| worktree.read(cx).root_name_str().to_string())
        });

        if let Some(screen_window) = cx
            .open_window(options, |_window, cx| {
                cx.new(|_cx| {
                    AgentNotification::new(title.clone(), caption.clone(), icon, project_name)
                })
            })
            .log_err()
            && let Some(pop_up) = screen_window.entity(cx).log_err()
        {
            self.notification_subscriptions
                .entry(screen_window)
                .or_insert_with(Vec::new)
                .push(cx.subscribe_in(&pop_up, window, {
                    move |this, _, event, window, cx| match event {
                        AgentNotificationEvent::Accepted => {
                            let Some(handle) = window.window_handle().downcast::<MultiWorkspace>()
                            else {
                                log::error!("root view should be a MultiWorkspace");
                                return;
                            };
                            cx.activate(true);

                            let workspace_handle = this.workspace.clone();
                            let agent = this.connection_key.clone();
                            let root_session_id = root_session_id.clone();
                            let root_work_dirs = root_work_dirs.clone();
                            let root_title = root_title.clone();

                            cx.defer(move |cx| {
                                handle
                                    .update(cx, |multi_workspace, window, cx| {
                                        window.activate_window();
                                        if let Some(workspace) = workspace_handle.upgrade() {
                                            multi_workspace.activate(
                                                workspace.clone(),
                                                None,
                                                window,
                                                cx,
                                            );
                                            // Upstream revealed its dock panel and asked
                                            // it to load the notified thread. Agents are
                                            // pane items here, so accepting the
                                            // notification brings the agent's own tab
                                            // forward instead.
                                            let _ = (
                                                &agent,
                                                &root_session_id,
                                                &root_work_dirs,
                                                &root_title,
                                            );
                                            crate::AgentView::activate_for_agent(
                                                workspace, window, cx,
                                            );
                                        }
                                    })
                                    .log_err();
                            });

                            this.dismiss_notifications(cx);
                        }
                        AgentNotificationEvent::Dismissed => {
                            this.dismiss_notifications(cx);
                        }
                    }
                }));

            self.notifications.push(screen_window);

            // If the user manually refocuses the original window, dismiss the popup.
            self.notification_subscriptions
                .entry(screen_window)
                .or_insert_with(Vec::new)
                .push({
                    let pop_up_weak = pop_up.downgrade();

                    cx.observe_window_activation(window, move |this, window, cx| {
                        if this.agent_status_visible(window, cx)
                            && let Some(pop_up) = pop_up_weak.upgrade()
                        {
                            pop_up.update(cx, |notification, cx| {
                                notification.dismiss(cx);
                            });
                        }
                    })
                });
        }
    }

    fn dismiss_notifications(&mut self, cx: &mut Context<Self>) {
        for window in self.notifications.drain(..) {
            window
                .update(cx, |_, window, _| {
                    window.remove_window();
                })
                .ok();

            self.notification_subscriptions.remove(&window);
        }
    }

    fn agent_ui_font_size_changed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry_view_state) = self
            .active_thread()
            .map(|active| active.read(cx).entry_view_state.clone())
        {
            entry_view_state.update(cx, |entry_view_state, cx| {
                entry_view_state.agent_ui_font_size_changed(cx);
            });
        }
    }

    pub(crate) fn insert_dragged_files(
        &self,
        paths: Vec<project::ProjectPath>,
        added_worktrees: Vec<Entity<project::Worktree>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(active_thread) = self.active_thread() {
            active_thread.update(cx, |thread, cx| {
                thread.message_editor.update(cx, |editor, cx| {
                    editor.insert_dragged_files(paths, added_worktrees, window, cx);
                    editor.focus_handle(cx).focus(window, cx);
                })
            });
        }
    }

    /// Inserts the selected text into the message editor or the message being
    /// edited, if any.
    pub(crate) fn insert_selections(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(active_thread) = self.active_thread() {
            active_thread.update(cx, |thread, cx| {
                thread.active_editor(cx).update(cx, |editor, cx| {
                    editor.insert_selections(window, cx);
                })
            });
        }
    }

    fn current_model_name(&self, cx: &App) -> SharedString {
        // For native agent (Zed Agent), use the specific model name (e.g., "Claude 3.5 Sonnet")
        // For ACP agents, use the agent name (e.g., "Claude Agent", "Gemini CLI")
        // This provides better clarity about what refused the request
        if false {
            self.root_thread_view()
                .and_then(|active| active.read(cx).model_selector.clone())
                .and_then(|selector| selector.read(cx).active_model(cx))
                .map(|model| model.name.clone())
                .unwrap_or_else(|| SharedString::from("The model"))
        } else {
            // ACP agent - use the agent name (e.g., "Claude Agent", "Gemini CLI")
            self.agent.agent_id().0
        }
    }

    fn create_copy_button(&self, message: impl Into<String>) -> impl IntoElement {
        let message = message.into();

        CopyButton::new("copy-error-message", message).tooltip_label("Copy Error Message")
    }

    pub(crate) fn reauthenticate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agent_id = self.agent.agent_id();
        if let Some(active) = self.root_thread_view() {
            active.update(cx, |active, cx| active.clear_thread_error(cx));
        }
        let this = cx.weak_entity();
        let Some(connection) = self.as_connected().map(|c| c.connection.clone()) else {
            debug_panic!("This should not be possible");
            return;
        };
        window.defer(cx, |window, cx| {
            Self::handle_auth_required(this, AuthRequired::new(), agent_id, connection, window, cx);
        })
    }
}

fn loading_contents_spinner(size: IconSize) -> AnyElement {
    Icon::new(IconName::LoadCircle)
        .size(size)
        .color(Color::Accent)
        .with_rotate_animation(3)
        .into_any_element()
}

fn placeholder_text(agent_name: &str, has_commands: bool) -> String {
    if false {
        format!("Message the {} — @ to include context", agent_name)
    } else if has_commands {
        format!(
            "Message {} — @ to include context, / for commands",
            agent_name
        )
    } else {
        format!("Message {} — @ to include context", agent_name)
    }
}

impl Focusable for ConversationView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.active_thread() {
            Some(thread) => thread.read(cx).focus_handle(cx),
            None => self.focus_handle.clone(),
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ConversationView {
    /// Expands a tool call so its content is visible.
    /// This is primarily useful for visual testing.
    pub fn expand_tool_call(&mut self, tool_call_id: acp::ToolCallId, cx: &mut Context<Self>) {
        if let Some(active) = self.active_thread() {
            active.update(cx, |active, _cx| {
                active.expanded_tool_calls.insert(tool_call_id);
            });
            cx.notify();
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_updated_at(&mut self, updated_at: Instant, cx: &mut Context<Self>) {
        let Some(connected) = self.as_connected_mut() else {
            return;
        };

        connected.conversation.update(cx, |conversation, _cx| {
            conversation.updated_at = Some(updated_at);
        });
    }
}

impl Render for ConversationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_queued_message_editors(window, cx);

        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().colors().panel_background)
            .child(match &self.server_state {
                ServerState::Loading { .. } => v_flex()
                    .flex_1()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .child(
                        Label::new("Loading…").color(Color::Muted).with_animation(
                            "loading-agent-label",
                            Animation::new(Duration::from_secs(2))
                                .repeat()
                                .with_easing(pulsating_between(0.3, 0.7)),
                            |label, delta| label.alpha(delta),
                        ),
                    )
                    .into_any(),
                ServerState::LoadError { error: e, .. } => v_flex()
                    .flex_1()
                    .size_full()
                    .items_center()
                    .justify_end()
                    .child(self.render_load_error(e, window, cx))
                    .into_any(),
                ServerState::Connected(ConnectedServerState {
                    connection,
                    auth_state:
                        AuthState::Unauthenticated {
                            description,
                            configuration_view,
                            pending_auth_method,
                            _subscription,
                        },
                    ..
                }) => v_flex()
                    .flex_1()
                    .size_full()
                    .justify_end()
                    .child(self.render_auth_required_state(
                        connection,
                        description.as_ref(),
                        configuration_view.as_ref(),
                        pending_auth_method.as_ref(),
                        window,
                        cx,
                    ))
                    .into_any_element(),
                ServerState::Connected(connected) => {
                    if let Some(view) = connected.active_view() {
                        view.clone().into_any_element()
                    } else {
                        debug_panic!("This state should never be reached");
                        div().into_any_element()
                    }
                }
            })
    }
}

fn plan_label_markdown_style(
    status: &acp::PlanEntryStatus,
    window: &Window,
    cx: &App,
) -> MarkdownStyle {
    let default_md_style = MarkdownStyle::themed(MarkdownFont::Editor, window, cx);

    MarkdownStyle {
        base_text_style: TextStyle {
            color: cx.theme().colors().text_muted,
            strikethrough: if matches!(status, acp::PlanEntryStatus::Completed) {
                Some(gpui::StrikethroughStyle {
                    thickness: px(1.),
                    color: Some(cx.theme().colors().text_muted.opacity(0.8)),
                })
            } else {
                None
            },
            ..default_md_style.base_text_style
        },
        ..default_md_style
    }
}

// Upstream's tests for this view drove it through `AgentPanel` — constructing the
// dock panel, asserting its visibility, loading native threads into it. None of
// that survives a centre-pane port, and rewriting them was not part of getting
// the view to compile. The view is therefore untested here: phase 04's own draw
// tests and phase 05's end-to-end pass are where that coverage has to come from.
