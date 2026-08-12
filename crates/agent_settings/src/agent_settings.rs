

use gpui::{App, Pixels, px};
use project::DisableAiSettings;
use settings::{
    NewThreadLocation,
    NotifyWhenAgentWaiting, PlaySoundWhenAgentDone, Settings, SidebarDockPosition, SidebarSide, ThinkingBlockDisplay,
};


pub const SUMMARIZE_THREAD_PROMPT: &str = include_str!("prompts/summarize_thread_prompt.txt");
pub const SUMMARIZE_THREAD_DETAILED_PROMPT: &str =
    include_str!("prompts/summarize_thread_detailed_prompt.txt");

// Upstream carried `PanelLayout` and `WindowLayout` here, describing which edge
// each dock panel sits on and letting the agent panel drive a whole-window
// layout preset. Agents open in the centre pane in this fork, beside the editor,
// so there is no agent dock for a layout to be about.

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSettings {
    pub enabled: bool,
    pub sidebar_side: SidebarDockPosition,
    pub max_content_width: Option<Pixels>,
    pub notify_when_agent_waiting: NotifyWhenAgentWaiting,
    pub play_sound_when_agent_done: PlaySoundWhenAgentDone,
    pub enable_feedback: bool,
    pub expand_edit_card: bool,
    pub expand_terminal_card: bool,
    pub thinking_display: ThinkingBlockDisplay,
    pub cancel_generation_on_terminal_stop: bool,
    pub use_modifier_to_send: bool,
    pub message_editor_min_lines: usize,
    pub show_turn_stats: bool,
    pub new_thread_location: NewThreadLocation,
}

impl AgentSettings {
    pub fn enabled(&self, cx: &App) -> bool {
        self.enabled && !DisableAiSettings::get_global(cx).disable_ai
    }

    // `temperature_for_model` lived here, resolving a per-model temperature
    // override for Zed's own providers. External agents own their sampling
    // parameters, so it had no caller left once the native agent went.

    pub fn sidebar_side(&self) -> SidebarSide {
        match self.sidebar_side {
            SidebarDockPosition::Left => SidebarSide::Left,
            SidebarDockPosition::Right => SidebarSide::Right,
        }
    }

    pub fn set_message_editor_max_lines(&self) -> usize {
        self.message_editor_min_lines * 2
    }

}
// Upstream also carried `language_model_to_selection` here, mapping one of
// Zed's own models onto a stored selection. It served the native agent only —
// external agents report their models over ACP — and it was the last thing
// tying this crate to `language_model`.

impl Settings for AgentSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let agent = content.agent.clone().unwrap_or_default();
        Self {
            enabled: agent.enabled.unwrap_or(true),
            sidebar_side: agent.sidebar_side.unwrap_or_default(),
            max_content_width: if agent.limit_content_width.unwrap_or(true) {
                Some(px(agent.max_content_width.unwrap_or(850.)))
            } else {
                None
            },
            notify_when_agent_waiting: agent.notify_when_agent_waiting.unwrap_or_default(),
            play_sound_when_agent_done: agent.play_sound_when_agent_done.unwrap_or_default(),
            enable_feedback: agent.enable_feedback.unwrap_or(true),
            expand_edit_card: agent.expand_edit_card.unwrap_or(true),
            expand_terminal_card: agent.expand_terminal_card.unwrap_or(true),
            thinking_display: agent.thinking_display.unwrap_or_default(),
            cancel_generation_on_terminal_stop: agent
                .cancel_generation_on_terminal_stop
                .unwrap_or(true),
            use_modifier_to_send: agent.use_modifier_to_send.unwrap_or(false),
            message_editor_min_lines: agent.message_editor_min_lines.unwrap_or(3),
            show_turn_stats: agent.show_turn_stats.unwrap_or(false),
            new_thread_location: agent.new_thread_location.unwrap_or_default(),
        }
    }
}

// Upstream compiled `tool_permissions` here — Zed's own allow/deny/confirm
// pre-filter over tool calls, with per-tool regex rules. External agents run
// their own permission system and ask over ACP, and nothing in the ported views
// reads it. Phase 05 decides whether a second, editor-side filter earns its
// place in front of that.
#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream read every field with `.unwrap()`, which made a complete `agent`
    /// section in default.json load-bearing: a missing key was a panic at startup.
    /// The trimmed settings carry their own defaults instead, so the section is
    /// documentation rather than a requirement.
    #[test]
    fn settings_hold_their_defaults_without_a_default_json_section() {
        let settings = AgentSettings::from_settings(&settings::SettingsContent::default());

        assert!(settings.enabled);
        assert_eq!(settings.max_content_width, Some(px(850.)));
        assert_eq!(settings.message_editor_min_lines, 3);
        assert_eq!(settings.set_message_editor_max_lines(), 6);
        assert!(settings.expand_edit_card);
        assert!(settings.expand_terminal_card);
        assert!(settings.enable_feedback);
        assert!(settings.cancel_generation_on_terminal_stop);
        assert!(!settings.use_modifier_to_send);
        assert!(!settings.show_turn_stats);
    }

    /// `limit_content_width: false` is the one setting whose whole job is to turn
    /// another one off, so it earns a test of its own.
    #[test]
    fn clearing_the_width_limit_removes_the_maximum() {
        let mut content = settings::SettingsContent::default();
        content.agent = Some(settings::AgentSettingsContent {
            limit_content_width: Some(false),
            ..Default::default()
        });

        assert_eq!(
            AgentSettings::from_settings(&content).max_content_width,
            None
        );
    }
}
