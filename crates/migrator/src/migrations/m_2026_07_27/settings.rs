use anyhow::Result;
use serde_json::Value;

use crate::migrations::migrate_settings;

/// Keys whose consumers were removed in the fork that dropped auth, cloud,
/// AI/agent, and auto-update: `agent`, `language_models`,
/// `collaboration_panel`, `calls`, `message_editor`, `auto_update`,
/// `show_call_status_icon`, `agent_ui_font_size`, `agent_buffer_font_size`.
const REMOVED_KEYS: &[&str] = &[
    "agent",
    "language_models",
    "collaboration_panel",
    "calls",
    "message_editor",
    "auto_update",
    "show_call_status_icon",
    "agent_ui_font_size",
    "agent_buffer_font_size",
];

pub fn remove_dead_subsystem_settings(value: &mut Value) -> Result<()> {
    migrate_settings(value, &mut migrate_one)
}

fn migrate_one(obj: &mut serde_json::Map<String, Value>) -> Result<()> {
    for key in REMOVED_KEYS {
        obj.remove(*key);
    }
    Ok(())
}
