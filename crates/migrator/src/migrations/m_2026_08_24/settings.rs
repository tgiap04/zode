use anyhow::Result;
use serde_json::Value;

use crate::migrations::migrate_settings;

const AGENT_SERVERS_KEY: &str = "agent_servers";

/// The `agent_servers` entry types whose consumers were removed with the ACP
/// registry and the extension agent API.
///
/// `custom` is deliberately absent: an entry naming a command to run still has a
/// consumer and stays exactly as the user wrote it.
const REMOVED_TYPES: &[&str] = &["registry", "extension"];

/// Drops `agent_servers` entries of a type that can no longer be honoured.
///
/// Leaving them in place is not an option: `CustomAgentServerSettings` is tagged
/// by `type`, so a tag with no variant fails deserialization of the whole
/// `agent_servers` map — one stale entry would cost the user their entire
/// settings file rather than just that agent.
///
/// This matters more than the usual removal because a *previous* migration put
/// these entries there. `m_2026_02_25::migrate_builtin_agent_servers_to_registry`
/// rewrote every builtin agent entry to `"type": "registry"`, so a user who has
/// simply kept their settings across upgrades is holding exactly the entries this
/// pass now has to clear.
pub fn remove_registry_and_extension_agent_servers(value: &mut Value) -> Result<()> {
    migrate_settings(value, &mut migrate_one)
}

fn migrate_one(obj: &mut serde_json::Map<String, Value>) -> Result<()> {
    let Some(agent_servers) = obj.get_mut(AGENT_SERVERS_KEY) else {
        return Ok(());
    };
    let Some(servers_map) = agent_servers.as_object_mut() else {
        return Ok(());
    };

    servers_map.retain(|_, entry| {
        let removed = entry
            .as_object()
            .and_then(|entry| entry.get("type"))
            .and_then(|kind| kind.as_str())
            .is_some_and(|kind| REMOVED_TYPES.contains(&kind));
        !removed
    });

    // An `agent_servers: {}` left behind is not wrong, but it is a key the user
    // never wrote asking to be explained later.
    if servers_map.is_empty() {
        obj.remove(AGENT_SERVERS_KEY);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migrate(input: &str) -> Value {
        let mut value: Value = serde_json::from_str(input).unwrap();
        remove_registry_and_extension_agent_servers(&mut value).unwrap();
        value
    }

    #[test]
    fn a_registry_entry_and_an_extension_entry_are_dropped() {
        let after = migrate(
            r#"{"agent_servers":{
                "codex-acp":{"type":"registry","env":{"A":"1"}},
                "my-ext-agent":{"type":"extension"},
                "my-script":{"type":"custom","command":"/bin/agent","args":["--acp"]}
            }}"#,
        );
        let servers = after["agent_servers"].as_object().unwrap();
        assert_eq!(
            servers.len(),
            1,
            "only the custom entry survives: {servers:?}"
        );
        assert_eq!(servers["my-script"]["command"], "/bin/agent");
    }

    /// The whole point: an entry the editor cannot honour must not be left where
    /// it would fail to parse.
    #[test]
    fn a_settings_file_of_nothing_but_removed_entries_loses_the_key() {
        let after =
            migrate(r#"{"agent_servers":{"codex-acp":{"type":"registry"}},"theme":"One Dark"}"#);
        assert!(after.get("agent_servers").is_none());
        assert_eq!(
            after["theme"], "One Dark",
            "unrelated settings are untouched"
        );
    }

    /// A pre-`m_2026_02_25` entry has no `type` at all. It is not one of the
    /// removed kinds, so it is left for that older migration to deal with rather
    /// than deleted here.
    #[test]
    fn an_entry_with_no_type_is_left_alone() {
        let after = migrate(r#"{"agent_servers":{"gemini":{"command":"/bin/gemini"}}}"#);
        assert_eq!(after["agent_servers"]["gemini"]["command"], "/bin/gemini");
    }

    #[test]
    fn settings_without_agent_servers_are_untouched() {
        let after = migrate(r#"{"theme":"One Dark"}"#);
        assert_eq!(after, serde_json::json!({"theme":"One Dark"}));
    }

    /// `agent_servers` set to something that is not an object is a user error
    /// this pass must not turn into a crash.
    #[test]
    fn a_non_object_agent_servers_value_is_not_a_panic() {
        let after = migrate(r#"{"agent_servers":"nonsense"}"#);
        assert_eq!(after["agent_servers"], "nonsense");
    }
}
