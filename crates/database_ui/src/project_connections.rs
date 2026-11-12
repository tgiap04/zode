//! The databases a project names for itself: the data sources DataGrip and the
//! other JetBrains IDEs keep in `.idea/`, listed in a "Project" group and
//! connected as soon as the project is open.
//!
//! Read, never written. A data source here holds no secret -- DataGrip keeps
//! its passwords in its own store -- so one that needs a password asks for it
//! the first time it is clicked, and the keychain keeps it from then on under
//! the connection's URL, as for any other connection.

use crate::connection_store::ConnectionConfig;
use crate::database_panel::{ConnectionNode, DatabasePanel, NodeOrigin};
use gpui::{App, Context, Entity, Task, Window};
use project::{Project, ProjectPath, WorktreeId};
use util::rel_path::RelPath;

/// Where the shared half of the data sources lives: names and JDBC URLs.
pub(crate) const DATA_SOURCES_PATH: &str = ".idea/dataSources.xml";
/// Where the per-machine half lives: the user each data source signs in as.
pub(crate) const LOCAL_DATA_SOURCES_PATH: &str = ".idea/dataSources.local.xml";

/// One data source as the IDE wrote it down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DataSource {
    pub(crate) name: String,
    pub(crate) jdbc_url: String,
    pub(crate) user: Option<String>,
}

/// The data sources in `shared`, each with the user `local` gives it.
///
/// The user is looked for in both files: the IDE writes it to the local one,
/// which is never committed, but a hand-written shared file may carry it too.
/// A file that does not parse lists nothing rather than failing the project.
pub(crate) fn read_data_sources(shared: &str, local: Option<&str>) -> Vec<DataSource> {
    let Ok(shared) = roxmltree::Document::parse(shared) else {
        return Vec::new();
    };
    let local = local.and_then(|local| roxmltree::Document::parse(local).ok());
    let local_user = |uuid: Option<&str>, name: &str| {
        local.as_ref().and_then(|local| {
            data_source_nodes(local)
                .find(|node| match (uuid, node.attribute("uuid")) {
                    (Some(wanted), Some(found)) => wanted == found,
                    _ => node.attribute("name") == Some(name),
                })
                .and_then(|node| child_text(node, "user-name"))
        })
    };

    data_source_nodes(&shared)
        .filter_map(|node| {
            let name = node.attribute("name")?.to_string();
            let jdbc_url = child_text(node, "jdbc-url")?;
            let user =
                local_user(node.attribute("uuid"), &name).or_else(|| child_text(node, "user-name"));
            Some(DataSource {
                name,
                jdbc_url,
                user,
            })
        })
        .collect()
}

fn data_source_nodes<'a>(
    document: &'a roxmltree::Document<'a>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'a>> {
    document
        .descendants()
        .filter(|node| node.has_tag_name("data-source"))
}

fn child_text(node: roxmltree::Node, tag: &str) -> Option<String> {
    node.children()
        .find(|child| child.has_tag_name(tag))
        .and_then(|child| child.text())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// The connection a data source becomes, or `None` for an engine Zode has no
/// driver for.
///
/// A JDBC URL is the engine's own URL behind a `jdbc:` prefix. Which driver
/// runs it is `driver_for_url`'s call, the same one a pasted URL gets. The
/// scheme is kept when it is the driver's own or an extension of it, and
/// replaced by the driver's when it names a fork the driver speaks for but
/// whose scheme it does not read.
pub(crate) fn connection_for(source: &DataSource, project_dir: &str) -> Option<ConnectionConfig> {
    let trimmed = source.jdbc_url.trim();
    let prefix = trimmed.get(..5)?;
    if !prefix.eq_ignore_ascii_case("jdbc:") {
        return None;
    }
    let engine_url = trimmed[5..]
        .replace("$PROJECT_DIR$", project_dir)
        .replace("$ProjectFileDir$", project_dir);
    let driver = crate::driver_registry::driver_for_url(&engine_url)?;

    let url = match engine_url.split_once("://") {
        Some((scheme, rest)) => {
            let scheme = if scheme.to_ascii_lowercase().starts_with(driver) {
                scheme
            } else {
                driver
            };
            let authority = rest.split(['/', '?']).next().unwrap_or_default();
            match source.user.as_deref() {
                Some(user) if !authority.contains('@') => {
                    format!("{scheme}://{}@{rest}", encode_user(user))
                }
                _ => format!("{scheme}://{rest}"),
            }
        }
        // No authority: what follows the scheme is a file path.
        None => engine_url
            .split_once(':')
            .map_or(engine_url.as_str(), |(_, path)| path)
            .to_string(),
    };

    Some(ConnectionConfig {
        name: source.name.clone(),
        driver: driver.to_string(),
        url,
    })
}

/// A user name made safe to stand before the `@` of a URL.
fn encode_user(user: &str) -> String {
    let mut encoded = String::with_capacity(user.len());
    for byte in user.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

impl DatabasePanel {
    /// Reads the data sources of every folder in this panel's project and
    /// lists them, opening each new one on its own.
    ///
    /// Through the project rather than the file system, so a folder on a remote
    /// host -- WSL, SSH -- is read where it lives.
    pub(crate) fn load_project_connections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let project = workspace.read(cx).project().clone();
        let folders: Vec<(WorktreeId, String)> = project
            .read(cx)
            .visible_worktrees(cx)
            .map(|worktree| {
                let worktree = worktree.read(cx);
                (
                    worktree.id(),
                    worktree.abs_path().to_string_lossy().into_owned(),
                )
            })
            .collect();
        let reads: Vec<_> = folders
            .into_iter()
            .map(|(worktree_id, folder)| {
                let shared = read_project_file(&project, worktree_id, DATA_SOURCES_PATH, cx);
                let local = read_project_file(&project, worktree_id, LOCAL_DATA_SOURCES_PATH, cx);
                (folder, shared, local)
            })
            .collect();

        self.tasks.push(cx.spawn_in(window, async move |this, cx| {
            let mut configs = Vec::new();
            for (folder, shared, local) in reads {
                let Some(shared) = shared.await else {
                    continue;
                };
                let local = local.await;
                configs.extend(
                    read_data_sources(&shared, local.as_deref())
                        .iter()
                        .filter_map(|source| connection_for(source, &folder)),
                );
            }
            this.update_in(cx, |this, window, cx| {
                this.set_project_connections(configs, window, cx)
            })
            .ok();
        }));
    }

    /// Replaces the project's connections with `configs`, keeping any already
    /// open to the same database, and opens the new ones.
    fn set_project_connections(
        &mut self,
        configs: Vec<ConnectionConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let places = self.remember_places();
        let mut personal = Vec::new();
        let mut previous = Vec::new();
        let mut team = Vec::new();
        for node in std::mem::take(&mut self.connections) {
            match node.origin() {
                NodeOrigin::Personal => personal.push(node),
                NodeOrigin::Project => previous.push(node),
                NodeOrigin::Team => team.push(node),
            }
        }

        let mut project: Vec<ConnectionNode> = Vec::with_capacity(configs.len());
        for config in configs {
            // Two folders may name the same database; the first is kept.
            if project.iter().any(|node| node.config.name == config.name) {
                continue;
            }
            let kept = previous
                .iter()
                .position(|node| node.config.name == config.name && node.config.url == config.url);
            project.push(match kept {
                Some(index) => previous.remove(index),
                None => {
                    let mut node = ConnectionNode::idle(config);
                    node.from_project = true;
                    node
                }
            });
        }

        let first_project = personal.len();
        let project_count = project.len();
        self.connections = personal;
        self.connections.extend(project);
        self.connections.extend(team);
        self.restore_places(places);

        for index in first_project..first_project + project_count {
            self.connect_unattended(index, window, cx);
        }
        cx.notify();
    }
}

/// The text of `path` in one of the project's folders, `None` when it cannot be
/// read. A file that is not there opens as an empty buffer, which lists nothing.
fn read_project_file(
    project: &Entity<Project>,
    worktree_id: WorktreeId,
    path: &str,
    cx: &mut App,
) -> Task<Option<String>> {
    let Ok(path) = RelPath::unix(path) else {
        return Task::ready(None);
    };
    let open = project.update(cx, |project, cx| {
        project.open_buffer(
            ProjectPath {
                worktree_id,
                path: path.into(),
            },
            cx,
        )
    });
    cx.spawn(async move |cx| {
        let buffer = open.await.ok()?;
        Some(buffer.read_with(cx, |buffer, _| buffer.text()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="DataSourceManagerImpl" format="xml" multifile-model="true">
    <data-source source="LOCAL" name="auth" uuid="6183b463-142b-5827-b4c6-7f046924c065">
      <driver-ref>postgresql</driver-ref>
      <jdbc-driver>org.postgresql.Driver</jdbc-driver>
      <jdbc-url>jdbc:postgresql://127.0.0.1:15441/auth</jdbc-url>
      <working-dir>$ProjectFileDir$</working-dir>
    </data-source>
    <data-source source="LOCAL" name="cache" uuid="1">
      <jdbc-url>jdbc:sqlite:$PROJECT_DIR$/data/cache.db</jdbc-url>
    </data-source>
    <data-source source="LOCAL" name="legacy" uuid="2">
      <jdbc-url>jdbc:mariadb://db.internal:3307/legacy?useSSL=false</jdbc-url>
      <user-name>shared-user</user-name>
    </data-source>
    <data-source source="LOCAL" name="warehouse" uuid="3">
      <jdbc-url>jdbc:snowflake://acme.snowflakecomputing.com</jdbc-url>
    </data-source>
  </component>
</project>"#;

    const LOCAL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="dataSourceStorageLocal" created-in="DB-253.30387.92">
    <data-source name="auth" uuid="6183b463-142b-5827-b4c6-7f046924c065">
      <secret-storage>master_key</secret-storage>
      <user-name>auth</user-name>
    </data-source>
  </component>
</project>"#;

    #[test]
    fn every_data_source_is_read_with_its_local_user() {
        let sources = read_data_sources(SHARED, Some(LOCAL));
        assert_eq!(
            sources,
            vec![
                DataSource {
                    name: "auth".into(),
                    jdbc_url: "jdbc:postgresql://127.0.0.1:15441/auth".into(),
                    user: Some("auth".into()),
                },
                DataSource {
                    name: "cache".into(),
                    jdbc_url: "jdbc:sqlite:$PROJECT_DIR$/data/cache.db".into(),
                    user: None,
                },
                DataSource {
                    name: "legacy".into(),
                    jdbc_url: "jdbc:mariadb://db.internal:3307/legacy?useSSL=false".into(),
                    user: Some("shared-user".into()),
                },
                DataSource {
                    name: "warehouse".into(),
                    jdbc_url: "jdbc:snowflake://acme.snowflakecomputing.com".into(),
                    user: None,
                },
            ]
        );
    }

    #[test]
    fn a_file_that_is_not_xml_lists_nothing() {
        assert!(read_data_sources("not xml at all", None).is_empty());
        assert!(read_data_sources(SHARED, Some("<broken")).len() == 4);
    }

    fn connection(name: &str, url: &str) -> Option<ConnectionConfig> {
        let sources = read_data_sources(SHARED, Some(LOCAL));
        let source = sources
            .iter()
            .find(|source| source.name == name)
            .expect("the fixture has it");
        let mut expected = connection_for(source, "/work/proxy");
        if let Some(config) = expected.as_mut() {
            assert_eq!(config.url, url, "{name}");
        }
        expected.take()
    }

    #[test]
    fn a_server_url_signs_in_as_its_user() {
        let config = connection("auth", "postgresql://auth@127.0.0.1:15441/auth")
            .expect("a postgres data source has a driver");
        assert_eq!(config.name, "auth");
        assert_eq!(config.driver, "postgres");

        let legacy = connection(
            "legacy",
            "mysql://shared-user@db.internal:3307/legacy?useSSL=false",
        )
        .expect("a mariadb data source runs on the mysql driver");
        assert_eq!(legacy.driver, "mysql");
    }

    #[test]
    fn a_file_url_is_a_path_inside_the_project() {
        let config = connection("cache", "/work/proxy/data/cache.db").expect("sqlite has a driver");
        assert_eq!(config.driver, "sqlite");
    }

    #[test]
    fn an_engine_without_a_driver_is_left_out() {
        assert_eq!(connection("warehouse", ""), None);
    }
}
