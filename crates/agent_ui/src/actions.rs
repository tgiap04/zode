//! Actions, constants and small helpers lifted from upstream's `agent_ui.rs`.
//!
//! Upstream kept these beside a 900-line `init` that registered the agent dock
//! panel, the inline assistant and the native agent. None of that applies here,
//! so only the declarations the ported views actually reference come across.

use agent_client_protocol::schema as acp;
use gpui::{Action, SharedString, actions};
use project::AgentId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use ui::IconName;

pub const DEFAULT_THREAD_TITLE: &str = "New Agent Thread";
// `PARALLEL_AGENT_LAYOUT_BACKFILL_KEY` keyed a one-off migration that moved users
// onto the parallel-agent window layout. There is no agent dock here to lay out.
actions!(
    agent,
    [
        /// Renames the agent tab, so two sessions of one agent can be told apart.
        RenameAgent,
        /// Toggles the menu to create new agent threads.
        ToggleNewThreadMenu,
        /// Toggles the options menu for agent settings and preferences.
        ToggleOptionsMenu,
        /// Toggles the profile or mode selector for switching between agent profiles.
        ToggleProfileSelector,
        /// Cycles through available session modes.
        CycleModeSelector,
        /// Cycles through favorited models in the ACP model selector.
        CycleFavoriteModels,
        /// Expands the message editor to full size.
        ExpandMessageEditor,
        /// Adds a context server to the configuration.
        AddContextServer,
        /// Archives the currently selected thread.
        ArchiveSelectedThread,
        /// Removes the currently selected thread.
        RemoveSelectedThread,
        /// Starts a chat conversation with follow-up enabled.
        ChatWithFollow,
        /// Cycles to the next inline assist suggestion.
        CycleNextInlineAssist,
        /// Cycles to the previous inline assist suggestion.
        CyclePreviousInlineAssist,
        /// Moves focus up in the interface.
        FocusUp,
        /// Moves focus down in the interface.
        FocusDown,
        /// Moves focus left in the interface.
        FocusLeft,
        /// Moves focus right in the interface.
        FocusRight,
        /// Opens the active thread as a markdown file.
        OpenActiveThreadAsMarkdown,
        /// Opens the agent diff view to review changes.
        OpenAgentDiff,
        /// Copies the current thread to the clipboard as JSON for debugging.
        CopyThreadToClipboard,
        /// Loads a thread from the clipboard JSON for debugging.
        LoadThreadFromClipboard,
        /// Keeps the current suggestion or change.
        Keep,
        /// Rejects the current suggestion or change.
        Reject,
        /// Rejects all suggestions or changes.
        RejectAll,
        /// Undoes the most recent reject operation, restoring the rejected changes.
        UndoLastReject,
        /// Keeps all suggestions or changes.
        KeepAll,
        /// Allow this operation only this time.
        AllowOnce,
        /// Allow this operation and remember the choice.
        AllowAlways,
        /// Reject this operation only this time.
        RejectOnce,
        /// Follows the agent's suggestions.
        Follow,
        /// Resets the trial upsell notification.
        ResetTrialUpsell,
        /// Resets the trial end upsell notification.
        ResetTrialEndUpsell,
        /// Opens the "Add Context" menu in the message editor.
        OpenAddContextMenu,
        /// Continues the current thread.
        ContinueThread,
        /// Interrupts the current generation and sends the message immediately.
        SendImmediately,
        /// Sends the next queued message immediately.
        SendNextQueuedMessage,
        /// Removes the first message from the queue (the next one to be sent).
        RemoveFirstQueuedMessage,
        /// Edits the first message in the queue (the next one to be sent).
        EditFirstQueuedMessage,
        /// Clears all messages from the queue.
        ClearMessageQueue,
        /// Opens the permission granularity dropdown for the current tool call.
        OpenPermissionDropdown,
        /// Toggles thinking mode for models that support extended thinking.
        ToggleThinkingMode,
        /// Cycles through available thinking effort levels for the current model.
        CycleThinkingEffort,
        /// Toggles the thinking effort selector menu open or closed.
        ToggleThinkingEffortMenu,
        /// Toggles fast mode for models that support it.
        ToggleFastMode,
        /// Scroll the output by one page up.
        ScrollOutputPageUp,
        /// Scroll the output by one page down.
        ScrollOutputPageDown,
        /// Scroll the output up by three lines.
        ScrollOutputLineUp,
        /// Scroll the output down by three lines.
        ScrollOutputLineDown,
        /// Scroll the output to the top.
        ScrollOutputToTop,
        /// Scroll the output to the bottom.
        ScrollOutputToBottom,
        /// Scroll the output to the previous user message.
        ScrollOutputToPreviousMessage,
        /// Scroll the output to the next user message.
        ScrollOutputToNextMessage,
        /// Import agent threads from other Zed release channels (e.g. Preview, Nightly).
        ImportThreadsFromOtherChannels,
    ]
);

actions!(
    dev,
    [
        /// Shows metadata for the currently active thread.
        ShowThreadMetadata,
        /// Shows metadata for all threads in the sidebar.
        ShowAllSidebarThreadMetadata,
    ]
);

/// Action to authorize a tool call with a specific permission option.
/// This is used by the permission granularity dropdown to authorize tool calls.
#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = agent)]
#[serde(deny_unknown_fields)]
pub struct AuthorizeToolCall {
    /// The tool call ID to authorize.
    pub tool_call_id: String,
    /// The permission option ID to use.
    pub option_id: String,
    /// The kind of permission option (serialized as string).
    pub option_kind: String,
}

/// Action to select a permission granularity option from the dropdown.
/// This updates the selected granularity without triggering authorization.
#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = agent)]
#[serde(deny_unknown_fields)]
pub struct SelectPermissionGranularity {
    /// The tool call ID for which to select the granularity.
    pub tool_call_id: String,
    /// The index of the selected granularity option.
    pub index: usize,
}

/// Action to toggle a command pattern checkbox in the permission dropdown.
#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = agent)]
#[serde(deny_unknown_fields)]
pub struct ToggleCommandPattern {
    /// The tool call ID for which to toggle the pattern.
    pub tool_call_id: String,
    /// The index of the command pattern to toggle.
    pub pattern_index: usize,
}

/// Creates a new conversation thread, optionally based on an existing thread.
#[derive(Default, Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = agent)]
#[serde(deny_unknown_fields)]
pub struct NewThread;

/// Creates a new external agent conversation thread.
#[derive(Default, Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = agent)]
#[serde(deny_unknown_fields)]
pub struct NewExternalAgentThread {
    /// Which agent to use for the conversation.
    agent: Option<Agent>,
}

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = agent)]
#[serde(deny_unknown_fields)]
pub struct NewNativeAgentThreadFromSummary {
    pub from_session_id: acp::SessionId,
}

/// Which agent a thread belongs to.
///
/// Upstream had a `NativeAgent` variant for the agent Zed ran in-process. This
/// fork has none — every agent is an external CLI speaking ACP — so only the
/// external case survives. `is_native` stays, answering `false`, so the ported
/// views keep working without an edit at each call site.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Agent {
    #[serde(alias = "Custom")]
    Custom {
        #[serde(rename = "name")]
        id: AgentId,
    },
}

impl From<AgentId> for Agent {
    fn from(id: AgentId) -> Self {
        Self::Custom { id }
    }
}

impl Agent {
    pub fn id(&self) -> AgentId {
        match self {
            Self::Custom { id } => id.clone(),
        }
    }

    pub fn is_native(&self) -> bool {
        false
    }

    pub fn label(&self) -> SharedString {
        match self {
            Self::Custom { id, .. } => project::builtin_agent(id.as_ref())
                .map(|builtin| SharedString::from(builtin.display_name))
                .unwrap_or_else(|| id.0.clone()),
        }
    }

    pub fn icon(&self) -> Option<IconName> {
        match self {
            Self::Custom { id } => Some(crate::agent_icon(id.as_ref())),
        }
    }

    pub fn server(&self) -> Rc<dyn agent_servers::AgentServer> {
        match self {
            Self::Custom { id } => Rc::new(agent_servers::CustomAgentServer::new(id.clone())),
        }
    }
}

/// Content to initialize new external agent with.
pub enum AgentInitialContent {
    ThreadSummary {
        session_id: acp::SessionId,
        title: Option<SharedString>,
    },
    ContentBlock {
        blocks: Vec<acp::ContentBlock>,
        auto_submit: bool,
    },
    FromExternalSource(crate::external_source_prompt::ExternalSourcePrompt),
}

impl From<crate::external_source_prompt::ExternalSourcePrompt> for AgentInitialContent {
    fn from(prompt: crate::external_source_prompt::ExternalSourcePrompt) -> Self {
        Self::FromExternalSource(prompt)
    }
}

pub(crate) fn humanize_token_count(count: u64) -> String {
    match count {
        0..=999 => count.to_string(),
        1000..=9999 => {
            let thousands = count / 1000;
            let hundreds = (count % 1000 + 50) / 100;
            if hundreds == 0 {
                format!("{}k", thousands)
            } else if hundreds == 10 {
                format!("{}k", thousands + 1)
            } else {
                format!("{}.{}k", thousands, hundreds)
            }
        }
        10_000..=999_999 => format!("{}k", (count + 500) / 1000),
        1_000_000..=9_999_999 => {
            let millions = count / 1_000_000;
            let hundred_thousands = (count % 1_000_000 + 50_000) / 100_000;
            if hundred_thousands == 0 {
                format!("{}M", millions)
            } else if hundred_thousands == 10 {
                format!("{}M", millions + 1)
            } else {
                format!("{}.{}M", millions, hundred_thousands)
            }
        }
        10_000_000.. => format!("{}M", (count + 500_000) / 1_000_000),
    }
}

/// Identity for a conversation, so a subagent can name the thread it branched from.
///
/// Upstream kept this in `thread_metadata_store`, alongside the sqlite persistence
/// for saved threads. Threads are not persisted here, but the identity still earns
/// its place: it is how a parent thread and its subagents refer to each other
/// within a session.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct ThreadId(uuid::Uuid);

impl ThreadId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for ThreadId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
