use collections::{HashMap, IndexMap};
use schemars::{JsonSchema, json_schema};
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};
use std::sync::Arc;
use std::{borrow::Cow, path::PathBuf};

use crate::ExtendingVec;


/// Where new threads should start by default.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum NewThreadLocation {
    /// Start threads in the current project.
    #[default]
    LocalProject,
    /// Start threads in a new worktree.
    NewWorktree,
}

/// Where to position the threads sidebar.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum SidebarDockPosition {
    /// Always show the sidebar on the left side.
    #[default]
    Left,
    /// Always show the sidebar on the right side.
    Right,
}

/// How thinking blocks should be displayed by default in the agent panel.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingBlockDisplay {
    /// Thinking blocks fully expand during streaming, then auto-collapse
    /// when the model finishes thinking. Users can re-expand after collapse.
    #[default]
    Auto,
    /// Thinking blocks auto-expand with a height constraint during streaming,
    /// then remain in their constrained state when complete. Users can click
    /// to fully expand or collapse.
    Preview,
    /// Thinking blocks are always fully expanded by default (no height constraint).
    AlwaysExpanded,
    /// Thinking blocks are always collapsed by default.
    AlwaysCollapsed,
}

#[with_fallible_options]
#[derive(Clone, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom, Debug, Default)]
pub struct AgentSettingsContent {
    /// Whether the Agent is enabled.
    ///
    /// Default: true
    pub enabled: Option<bool>,
    /// Where to position the threads sidebar.
    ///
    /// Default: left
    pub sidebar_side: Option<SidebarDockPosition>,
    /// Whether to limit the content width in the agent panel. When enabled,
    /// content will be constrained to `max_content_width` and centered when
    /// the panel is wider than that value, for optimal readability.
    ///
    /// Default: true
    pub limit_content_width: Option<bool>,
    /// Maximum content width in pixels for the agent panel. Content will be
    /// centered when the panel is wider than this value.
    ///
    /// Default: 850
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub max_content_width: Option<f32>,
    /// Where new threads should start by default.
    ///
    /// Default: "local_project"
    pub new_thread_location: Option<NewThreadLocation>,
    /// Where to show a popup notification when the agent is waiting for user input.
    ///
    /// Default: "primary_screen"
    pub notify_when_agent_waiting: Option<NotifyWhenAgentWaiting>,
    /// When to play a sound when the agent has either completed its response, or needs user input.
    ///
    /// Default: never
    pub play_sound_when_agent_done: Option<PlaySoundWhenAgentDone>,
    /// Whether to show thumb buttons for feedback in the agent panel.
    ///
    /// Default: true
    pub enable_feedback: Option<bool>,
    /// Whether to have edit cards in the agent panel expanded, showing a preview of the full diff.
    ///
    /// Default: true
    pub expand_edit_card: Option<bool>,
    /// Whether to have terminal cards in the agent panel expanded, showing the whole command output.
    ///
    /// Default: true
    pub expand_terminal_card: Option<bool>,
    /// How thinking blocks should be displayed by default in the agent panel.
    ///
    /// Default: automatic
    pub thinking_display: Option<ThinkingBlockDisplay>,
    /// Whether clicking the stop button on a running terminal tool should also cancel the agent's generation.
    /// Note that this only applies to the stop button, not to ctrl+c inside the terminal.
    ///
    /// Default: true
    pub cancel_generation_on_terminal_stop: Option<bool>,
    /// Whether to always use cmd-enter (or ctrl-enter on Linux or Windows) to send messages in the agent panel.
    ///
    /// Default: false
    pub use_modifier_to_send: Option<bool>,
    /// Minimum number of lines of height the agent message editor should have.
    ///
    /// Default: 4
    pub message_editor_min_lines: Option<usize>,
    /// Whether to show turn statistics (elapsed time during generation, final turn duration).
    ///
    /// Default: false
    pub show_turn_stats: Option<bool>,
}

impl AgentSettingsContent {
    pub fn set_sidebar_side(&mut self, position: SidebarDockPosition) {
        self.sidebar_side = Some(position);
    }

    pub fn set_new_thread_location(&mut self, value: NewThreadLocation) {
        self.new_thread_location = Some(value);
    }
}

#[with_fallible_options]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct AgentProfileContent {
    pub name: Arc<str>,
    #[serde(default)]
    pub tools: IndexMap<Arc<str>, bool>,
    /// Whether all context servers are enabled by default.
    pub enable_all_context_servers: Option<bool>,
    #[serde(default)]
    pub context_servers: IndexMap<Arc<str>, ContextServerPresetContent>,
    /// The default language model selected when using this profile.
    pub default_model: Option<LanguageModelSelection>,
}

#[with_fallible_options]
#[derive(Debug, PartialEq, Clone, Default, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct ContextServerPresetContent {
    pub tools: IndexMap<Arc<str>, bool>,
}

#[derive(
    Copy,
    Clone,
    Default,
    Debug,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    PartialEq,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum NotifyWhenAgentWaiting {
    #[default]
    PrimaryScreen,
    AllScreens,
    Never,
}

#[derive(
    Copy,
    Clone,
    Default,
    Debug,
    Serialize,
    Deserialize,
    JsonSchema,
    MergeFrom,
    PartialEq,
    strum::VariantArray,
    strum::VariantNames,
)]
#[serde(rename_all = "snake_case")]
pub enum PlaySoundWhenAgentDone {
    #[default]
    Never,
    WhenHidden,
    Always,
}

impl PlaySoundWhenAgentDone {
    pub fn should_play(&self, visible: bool) -> bool {
        match self {
            PlaySoundWhenAgentDone::Never => false,
            PlaySoundWhenAgentDone::WhenHidden => !visible,
            PlaySoundWhenAgentDone::Always => true,
        }
    }
}

#[with_fallible_options]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq)]
pub struct LanguageModelSelection {
    pub provider: LanguageModelProviderSetting,
    pub model: String,
    #[serde(default)]
    pub enable_thinking: bool,
    pub effort: Option<String>,
    pub speed: Option<crate::Speed>,
}

#[with_fallible_options]
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, MergeFrom, PartialEq)]
pub struct LanguageModelParameters {
    pub provider: Option<LanguageModelProviderSetting>,
    pub model: Option<String>,
    #[serde(serialize_with = "crate::serialize_optional_f32_with_two_decimal_places")]
    pub temperature: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, MergeFrom)]
pub struct LanguageModelProviderSetting(pub String);

impl JsonSchema for LanguageModelProviderSetting {
    fn schema_name() -> Cow<'static, str> {
        "LanguageModelProviderSetting".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // list the builtin providers as a subset so that we still auto complete them in the settings
        json_schema!({
            "anyOf": [
                {
                    "type": "string",
                    "enum": [
                        "amazon-bedrock",
                        "anthropic",
                        "copilot_chat",
                        "deepseek",
                        "google",
                        "lmstudio",
                        "mistral",
                        "ollama",
                        "openai",
                        "openrouter",
                        "vercel",
                        "vercel_ai_gateway",
                        "x_ai",
                        "zed.dev"
                    ]
                },
                {
                    "type": "string",
                }
            ]
        })
    }
}

impl From<String> for LanguageModelProviderSetting {
    fn from(provider: String) -> Self {
        Self(provider)
    }
}

impl From<&str> for LanguageModelProviderSetting {
    fn from(provider: &str) -> Self {
        Self(provider.to_string())
    }
}

#[with_fallible_options]
#[derive(Default, PartialEq, Deserialize, Serialize, Clone, JsonSchema, MergeFrom, Debug)]
#[serde(transparent)]
pub struct AllAgentServersSettings(pub HashMap<String, CustomAgentServerSettings>);

impl std::ops::Deref for AllAgentServersSettings {
    type Target = HashMap<String, CustomAgentServerSettings>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for AllAgentServersSettings {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[with_fallible_options]
#[derive(Deserialize, Serialize, Clone, JsonSchema, MergeFrom, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CustomAgentServerSettings {
    Custom {
        #[serde(rename = "command")]
        path: PathBuf,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        /// Default: {}
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
        /// The default mode to use for this agent.
        ///
        /// Note: Not only all agents support modes.
        ///
        /// Default: None
        default_mode: Option<String>,
        /// The default model to use for this agent.
        ///
        /// This should be the model ID as reported by the agent.
        ///
        /// Default: None
        default_model: Option<String>,
        /// The favorite models for this agent.
        ///
        /// These are the model IDs as reported by the agent.
        ///
        /// Default: []
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        favorite_models: Vec<String>,
        /// Default values for session config options.
        ///
        /// This is a map from config option ID to value ID.
        ///
        /// Default: {}
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        default_config_options: HashMap<String, String>,
        /// Favorited values for session config options.
        ///
        /// This is a map from config option ID to a list of favorited value IDs.
        ///
        /// Default: {}
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        favorite_config_option_values: HashMap<String, Vec<String>>,
    },
    Extension {
        /// Additional environment variables to pass to the agent.
        ///
        /// Default: {}
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
        /// The default mode to use for this agent.
        ///
        /// Note: Not only all agents support modes.
        ///
        /// Default: None
        default_mode: Option<String>,
        /// The default model to use for this agent.
        ///
        /// This should be the model ID as reported by the agent.
        ///
        /// Default: None
        default_model: Option<String>,
        /// The favorite models for this agent.
        ///
        /// These are the model IDs as reported by the agent.
        ///
        /// Default: []
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        favorite_models: Vec<String>,
        /// Default values for session config options.
        ///
        /// This is a map from config option ID to value ID.
        ///
        /// Default: {}
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        default_config_options: HashMap<String, String>,
        /// Favorited values for session config options.
        ///
        /// This is a map from config option ID to a list of favorited value IDs.
        ///
        /// Default: {}
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        favorite_config_option_values: HashMap<String, Vec<String>>,
    },
    Registry {
        /// Additional environment variables to pass to the agent.
        ///
        /// Default: {}
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
        /// The default mode to use for this agent.
        ///
        /// Note: Not only all agents support modes.
        ///
        /// Default: None
        default_mode: Option<String>,
        /// The default model to use for this agent.
        ///
        /// This should be the model ID as reported by the agent.
        ///
        /// Default: None
        default_model: Option<String>,
        /// The favorite models for this agent.
        ///
        /// These are the model IDs as reported by the agent.
        ///
        /// Default: []
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        favorite_models: Vec<String>,
        /// Default values for session config options.
        ///
        /// This is a map from config option ID to value ID.
        ///
        /// Default: {}
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        default_config_options: HashMap<String, String>,
        /// Favorited values for session config options.
        ///
        /// This is a map from config option ID to a list of favorited value IDs.
        ///
        /// Default: {}
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        favorite_config_option_values: HashMap<String, Vec<String>>,
    },
}

#[with_fallible_options]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct ToolPermissionsContent {
    /// Global default permission when no tool-specific rules match.
    /// Individual tools can override this with their own default.
    /// Default: confirm
    #[serde(alias = "default_mode")]
    pub default: Option<ToolPermissionMode>,

    /// Per-tool permission rules.
    /// Keys are tool names (e.g. terminal, edit_file, fetch) including MCP
    /// tools (e.g. mcp:server_name:tool_name). Any tool name is accepted;
    /// even tools without meaningful text input can have a `default` set.
    #[serde(default)]
    pub tools: HashMap<Arc<str>, ToolRulesContent>,
}

#[with_fallible_options]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct ToolRulesContent {
    /// Default mode when no regex rules match.
    /// When unset, inherits from the global `tool_permissions.default`.
    #[serde(alias = "default_mode")]
    pub default: Option<ToolPermissionMode>,

    /// Regexes for inputs to auto-approve.
    /// For terminal: matches command. For file tools: matches path. For fetch: matches URL.
    /// For `copy_path` and `move_path`, patterns are matched independently against each
    /// path (source and destination).
    /// Patterns accumulate across settings layers (user, project, profile) and cannot be
    /// removed by a higher-priority layer—only new patterns can be added.
    /// Default: []
    pub always_allow: Option<ExtendingVec<ToolRegexRule>>,

    /// Regexes for inputs to auto-reject.
    /// **SECURITY**: These take precedence over ALL other rules, across ALL settings layers.
    /// For `copy_path` and `move_path`, patterns are matched independently against each
    /// path (source and destination).
    /// Patterns accumulate across settings layers (user, project, profile) and cannot be
    /// removed by a higher-priority layer—only new patterns can be added.
    /// Default: []
    pub always_deny: Option<ExtendingVec<ToolRegexRule>>,

    /// Regexes for inputs that must always prompt.
    /// Takes precedence over always_allow but not always_deny.
    /// For `copy_path` and `move_path`, patterns are matched independently against each
    /// path (source and destination).
    /// Patterns accumulate across settings layers (user, project, profile) and cannot be
    /// removed by a higher-priority layer—only new patterns can be added.
    /// Default: []
    pub always_confirm: Option<ExtendingVec<ToolRegexRule>>,
}

#[with_fallible_options]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct ToolRegexRule {
    /// The regex pattern to match.
    #[serde(default)]
    pub pattern: String,

    /// Whether the regex is case-sensitive.
    /// Default: false (case-insensitive)
    pub case_sensitive: Option<bool>,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, MergeFrom,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermissionMode {
    /// Auto-approve without prompting.
    Allow,
    /// Auto-reject with an error.
    Deny,
    /// Always prompt for confirmation (default behavior).
    #[default]
    Confirm,
}

impl std::fmt::Display for ToolPermissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolPermissionMode::Allow => write!(f, "Allow"),
            ToolPermissionMode::Deny => write!(f, "Deny"),
            ToolPermissionMode::Confirm => write!(f, "Confirm"),
        }
    }
}

// Upstream tested the `tool_permissions` setters here. That surface described
// Zed's own allow/deny/confirm pre-filter over tool calls; external agents ask
// over ACP instead, so both the setters and their tests went with it.
