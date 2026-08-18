use anyhow::Result;
use credentials_provider::CredentialsProvider;
use gpui::{App, AsyncApp, Task};
use settings::{RegisterSetting, Settings};
use std::collections::BTreeSet;
use std::sync::Arc;

/// How many rows one page holds when the settings say nothing.
///
/// Mirrors `assets/settings/default.json`, which is what actually decides the
/// default -- a `#[default]` here would be overridden by that file, so this
/// exists only for the path where settings are absent altogether (tests).
const DEFAULT_PAGE_SIZE: u32 = 200;

/// One database, as configured. Holds no secret and no live connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionConfig {
    pub name: String,
    pub driver: String,
    pub url: String,
}

impl ConnectionConfig {
    /// Where this connection's password lives in the OS keychain.
    ///
    /// The URL, because that is what `CredentialsProvider` is keyed by, and
    /// because it is the thing that actually identifies the server -- renaming
    /// a connection in settings should not lose its password, while pointing it
    /// at a different host should.
    pub fn credential_key(&self) -> &str {
        &self.url
    }
}

#[derive(Clone, Debug, RegisterSetting)]
pub struct DatabaseSettings {
    pub connections: Vec<ConnectionConfig>,
    pub page_size: u32,
}

impl Settings for DatabaseSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let database = content.database.clone();
        Self {
            connections: database
                .as_ref()
                .and_then(|database| database.connections.clone())
                .unwrap_or_default()
                .into_iter()
                // A connection missing any of the three cannot be reached, and
                // dropping it beats drawing a node that fails on every click.
                // The settings file is hand-written, so this happens.
                .filter_map(|connection| {
                    Some(ConnectionConfig {
                        name: connection.name?,
                        driver: connection.driver?,
                        url: connection.url?,
                    })
                })
                .collect(),
            page_size: database
                .and_then(|database| database.page_size)
                .unwrap_or(DEFAULT_PAGE_SIZE)
                // A page of zero rows would page forever without ever showing
                // anything, and settings are hand-written.
                .max(1),
        }
    }
}

/// Which connections this project cares about.
///
/// Kept in the key-value store rather than in settings or in a column on
/// `workspaces`: it is per machine *and* per project, it is nobody's business
/// to commit, and `workspaces` is read positionally -- a new column there
/// silently shifts every field after it on real user data.
pub const PINNED_CONNECTIONS_KEY: &str = "database-pinned-connections";

/// Reads and writes the pins for one workspace.
#[derive(Clone, Debug, Default)]
pub struct PinnedConnections {
    names: BTreeSet<String>,
}

impl PinnedConnections {
    pub fn is_pinned(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn toggle(&mut self, name: &str) -> bool {
        if self.names.remove(name) {
            false
        } else {
            self.names.insert(name.to_string());
            true
        }
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn from_names(names: impl IntoIterator<Item = String>) -> Self {
        Self {
            names: names.into_iter().collect(),
        }
    }

    pub fn to_names(&self) -> Vec<String> {
        self.names.iter().cloned().collect()
    }
}

/// The connections a project should show.
///
/// With nothing pinned, everything -- a fresh project showing an empty list
/// would look broken rather than unconfigured. Once anything is pinned, the
/// pins are the answer, which is the whole point of having them: a machine with
/// thirty databases should not list thirty in every project.
pub fn visible_connections(
    all: &[ConnectionConfig],
    pinned: &PinnedConnections,
) -> Vec<ConnectionConfig> {
    if pinned.is_empty() {
        return all.to_vec();
    }
    all.iter()
        .filter(|connection| pinned.is_pinned(&connection.name))
        .cloned()
        .collect()
}

/// Fetches a connection's password from the OS keychain.
///
/// Returns `None` when there is none stored, which is the ordinary case for
/// SQLite and for servers that trust the local user -- not an error.
pub fn read_secret(
    credentials: Arc<dyn CredentialsProvider>,
    key: String,
    cx: &App,
) -> Task<Result<Option<String>>> {
    cx.spawn(async move |cx: &mut AsyncApp| {
        let Some((_username, password)) = credentials.read_credentials(&key, cx).await? else {
            return Ok(None);
        };
        Ok(Some(String::from_utf8(password)?))
    })
}

/// Stores a connection's password in the OS keychain.
///
/// The username is stored alongside it but never read back: the URL already
/// carries whichever user the driver should connect as, and a second copy here
/// would be one more thing that can disagree with settings. It is written
/// because the keychain's own UI shows it, and an entry with a blank account
/// name is one nobody can identify later.
pub fn write_secret(
    credentials: Arc<dyn CredentialsProvider>,
    key: String,
    username: String,
    password: String,
    cx: &App,
) -> Task<Result<()>> {
    cx.spawn(async move |cx: &mut AsyncApp| {
        credentials
            .write_credentials(&key, &username, password.as_bytes(), cx)
            .await
    })
}

/// Forgets a connection's password.
pub fn delete_secret(
    credentials: Arc<dyn CredentialsProvider>,
    key: String,
    cx: &App,
) -> Task<Result<()>> {
    cx.spawn(async move |cx: &mut AsyncApp| credentials.delete_credentials(&key, cx).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(name: &str) -> ConnectionConfig {
        ConnectionConfig {
            name: name.into(),
            driver: "sqlite".into(),
            url: format!("/tmp/{name}.sqlite"),
        }
    }

    /// A project that has pinned nothing has not said "show me nothing" -- it
    /// has said nothing at all, and an empty list would read as broken.
    #[test]
    fn nothing_pinned_shows_everything() {
        let all = [config("a"), config("b")];
        let visible = visible_connections(&all, &PinnedConnections::default());
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn pinning_one_narrows_the_list_to_it() {
        let all = [config("a"), config("b")];
        let pinned = PinnedConnections::from_names(["b".to_string()]);
        let visible = visible_connections(&all, &pinned);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "b");
    }

    /// A pin naming a connection that has since been removed from settings must
    /// not resurrect it, and must not hide the rest either.
    #[test]
    fn a_pin_for_a_connection_that_no_longer_exists_is_simply_ignored() {
        let all = [config("a")];
        let pinned = PinnedConnections::from_names(["gone".to_string(), "a".to_string()]);
        let visible = visible_connections(&all, &pinned);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "a");
    }

    #[test]
    fn toggling_a_pin_reports_which_way_it_went() {
        let mut pinned = PinnedConnections::default();
        assert!(pinned.toggle("a"), "first toggle pins");
        assert!(pinned.is_pinned("a"));
        assert!(!pinned.toggle("a"), "second toggle unpins");
        assert!(!pinned.is_pinned("a"));
    }

    /// The password follows the server, not the label: renaming a connection
    /// must not lose it, pointing it elsewhere must not carry it over.
    #[test]
    fn the_credential_key_is_the_url_not_the_name() {
        let mut renamed = config("a");
        renamed.name = "a renamed".into();
        assert_eq!(config("a").credential_key(), renamed.credential_key());

        let mut moved = config("a");
        moved.url = "/tmp/elsewhere.sqlite".into();
        assert_ne!(config("a").credential_key(), moved.credential_key());
    }
}
