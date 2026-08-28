use std::{any::Any, path::PathBuf, time::Duration};

use anyhow::{Context as _, Result, bail};
use collections::HashMap;
use gpui::{AsyncApp, Context, Entity, EventEmitter, SharedString, Subscription, Task};
use remote::RemoteClient;
use rpc::{
    AnyProtoClient, TypedEnvelope,
    proto::{self},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::{RegisterSetting, SettingsStore};
use util::{ResultExt as _, debug_panic};

use crate::ProjectEnvironment;

use crate::worktree_store::WorktreeStore;

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, JsonSchema)]
pub struct AgentServerCommand {
    #[serde(rename = "command")]
    pub path: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    pub env: Option<HashMap<String, String>>,
}

impl std::fmt::Debug for AgentServerCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let filtered_env = self.env.as_ref().map(|env| {
            env.iter()
                .map(|(k, v)| {
                    (
                        k,
                        if util::redact::should_redact(k) {
                            "[REDACTED]"
                        } else {
                            v
                        },
                    )
                })
                .collect::<Vec<_>>()
        });

        f.debug_struct("AgentServerCommand")
            .field("path", &self.path)
            .field("args", &self.args)
            .field("env", &filtered_env)
            .finish()
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct AgentId(pub SharedString);

impl AgentId {
    pub fn new(id: impl Into<SharedString>) -> Self {
        AgentId(id.into())
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&'static str> for AgentId {
    fn from(value: &'static str) -> Self {
        AgentId(value.into())
    }
}

impl From<AgentId> for SharedString {
    fn from(value: AgentId) -> Self {
        value.0
    }
}

impl AsRef<str> for AgentId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for AgentId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExternalAgentSource {
    /// An `agent_servers` entry naming a command the user chose themselves.
    #[default]
    Custom,
    /// Ships with the editor — see [`BuiltinAgent`].
    Builtin,
}

/// An agent that ships with the editor.
///
/// Everything here is what running the agent as a terminal session needs, and
/// nothing more. There is deliberately no package or version to fetch: this build
/// runs the CLI the user installed and never downloads an agent, which is what
/// keeps any network call off the cold-start path of someone who never opens an
/// agent at all.
pub struct BuiltinAgent {
    pub id: &'static str,
    pub display_name: &'static str,
    /// The CLI the user installs themselves. Terminal mode runs this directly, and
    /// its presence is what the "not installed" prompt tests for.
    pub binary: &'static str,
    pub docs_url: &'static str,
    /// Install line for macOS/Linux and for Windows, taken from each vendor's docs.
    install_unix: &'static str,
    install_windows: &'static str,
}

impl BuiltinAgent {
    pub fn install_command(&self) -> &'static str {
        if cfg!(target_os = "windows") {
            self.install_windows
        } else {
            self.install_unix
        }
    }
}

pub const CLAUDE_CODE_AGENT_ID: &str = "claude-acp";
pub const CODEX_AGENT_ID: &str = "codex-acp";
/// Ids match the ACP registry's own, so a user who points `agent_servers` at the
/// registry entry lands on the same agent rather than a second copy of it.
pub const ANTIGRAVITY_AGENT_ID: &str = "antigravity-acp";
pub const COPILOT_AGENT_ID: &str = "github-copilot-cli";

pub const BUILTIN_AGENTS: &[BuiltinAgent] = &[
    BuiltinAgent {
        id: CLAUDE_CODE_AGENT_ID,
        display_name: "Claude Code",
        binary: "claude",
        docs_url: "https://code.claude.com/docs/en/setup",
        install_unix: "curl -fsSL https://claude.ai/install.sh | bash",
        install_windows: "irm https://claude.ai/install.ps1 | iex",
    },
    BuiltinAgent {
        id: CODEX_AGENT_ID,
        display_name: "Codex",
        binary: "codex",
        docs_url: "https://developers.openai.com/codex/cli",
        install_unix: "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
        install_windows: "powershell -ExecutionPolicy ByPass -c \"irm https://chatgpt.com/codex/install.ps1 | iex\"",
    },
    BuiltinAgent {
        id: ANTIGRAVITY_AGENT_ID,
        display_name: "Antigravity",
        binary: "agy",
        docs_url: "https://antigravity.google/docs/cli/getting-started/",
        install_unix: "curl -fsSL https://antigravity.google/cli/install.sh | bash",
        install_windows: "irm https://antigravity.google/cli/install.ps1 | iex",
    },
    BuiltinAgent {
        id: COPILOT_AGENT_ID,
        display_name: "Copilot",
        binary: "copilot",
        docs_url: "https://docs.github.com/copilot/how-tos/copilot-cli",
        install_unix: "curl -fsSL https://gh.io/copilot-install | bash",
        install_windows: "winget install GitHub.Copilot",
    },
];

pub fn builtin_agent(id: &str) -> Option<&'static BuiltinAgent> {
    BUILTIN_AGENTS.iter().find(|agent| agent.id == id)
}

/// Looks `binary` up in `search_path`, which is meant to be the `PATH` of the
/// user's own shell rather than the one this process was started with.
///
/// The process `PATH` is only a fallback for when the shell environment could not
/// be loaded at all: reporting "not installed" because a login shell failed is a
/// worse answer than searching a narrower `PATH`.
fn locate_binary(binary: &str, search_path: Option<String>) -> Option<PathBuf> {
    let search_path = search_path
        .filter(|path| !path.is_empty())
        .or_else(|| std::env::var("PATH").ok())?;
    // `cwd` only affects relative `PATH` entries, which are vanishingly rare; an
    // unreadable working directory must not fail the lookup.
    let cwd = std::env::current_dir().unwrap_or_default();
    which::which_in(binary, Some(search_path), cwd).ok()
}

/// Everything the UI needs to tell the user how to install a missing CLI, without
/// having to know which agent it is asking about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentBinaryMissing {
    pub agent: AgentId,
    pub binary: &'static str,
    pub install_command: &'static str,
    pub docs_url: &'static str,
}

/// A missing CLI is an expected state with a screen of its own, not a failure — so
/// it rides back as a value. The surrounding `Result` is reserved for a resolution
/// that genuinely could not be performed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentBinary {
    Found(PathBuf),
    Missing(AgentBinaryMissing),
}

pub trait ExternalAgentServer {
    fn get_command(
        &self,
        extra_args: Vec<String>,
        extra_env: HashMap<String, String>,
        cx: &mut AsyncApp,
    ) -> Task<Result<AgentServerCommand>>;

    fn version(&self) -> Option<&SharedString> {
        None
    }

    fn take_new_version_available_tx(&mut self) -> Option<watch::Sender<Option<String>>> {
        None
    }

    fn set_new_version_available_tx(&mut self, _tx: watch::Sender<Option<String>>) {}

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

enum AgentServerStoreState {
    Local {
        project_environment: Entity<ProjectEnvironment>,
        downstream_client: Option<(u64, AnyProtoClient)>,
        settings: Option<AllAgentServersSettings>,
        _subscriptions: Vec<Subscription>,
    },
    Remote {
        project_id: u64,
        upstream_client: Entity<RemoteClient>,
        worktree_store: Entity<WorktreeStore>,
    },
    Collab,
}

pub struct ExternalAgentEntry {
    server: Box<dyn ExternalAgentServer>,
    icon: Option<SharedString>,
    display_name: Option<SharedString>,
    pub source: ExternalAgentSource,
}

impl ExternalAgentEntry {
    pub fn new(
        server: Box<dyn ExternalAgentServer>,
        source: ExternalAgentSource,
        icon: Option<SharedString>,
        display_name: Option<SharedString>,
    ) -> Self {
        Self {
            server,
            icon,
            display_name,
            source,
        }
    }
}

pub struct AgentServerStore {
    state: AgentServerStoreState,
    pub external_agents: HashMap<AgentId, ExternalAgentEntry>,
}

pub struct AgentServersUpdated;

impl EventEmitter<AgentServersUpdated> for AgentServerStore {}

impl AgentServerStore {
    pub fn agent_icon(&self, id: &AgentId) -> Option<SharedString> {
        self.external_agents
            .get(id)
            .and_then(|entry| entry.icon.clone())
    }

    pub fn agent_source(&self, name: &AgentId) -> Option<ExternalAgentSource> {
        self.external_agents.get(name).map(|entry| entry.source)
    }
}

impl AgentServerStore {
    pub fn agent_display_name(&self, name: &AgentId) -> Option<SharedString> {
        self.external_agents
            .get(name)
            .and_then(|entry| entry.display_name.clone())
    }

    pub fn init_remote(session: &AnyProtoClient) {
        session.add_entity_message_handler(Self::handle_external_agents_updated);
        session.add_entity_message_handler(Self::handle_new_version_available);
    }

    pub fn init_headless(session: &AnyProtoClient) {
        session.add_entity_request_handler(Self::handle_get_agent_server_command);
    }

    fn agent_servers_settings_changed(&mut self, cx: &mut Context<Self>) {
        let AgentServerStoreState::Local {
            settings: old_settings,
            ..
        } = &mut self.state
        else {
            debug_panic!(
                "should not be subscribed to agent server settings changes in non-local project"
            );
            return;
        };

        let new_settings = cx
            .global::<SettingsStore>()
            .get::<AllAgentServersSettings>(None)
            .clone();
        if Some(&new_settings) == old_settings.as_ref() {
            return;
        }

        self.reregister_agents(cx);
    }

    fn reregister_agents(&mut self, cx: &mut Context<Self>) {
        let AgentServerStoreState::Local {
            project_environment,
            downstream_client,
            settings: old_settings,
            ..
        } = &mut self.state
        else {
            debug_panic!("Non-local projects should never attempt to reregister. This is a bug!");

            return;
        };

        let new_settings = cx
            .global::<SettingsStore>()
            .get::<AllAgentServersSettings>(None)
            .clone();

        // Drain the existing versioned agents, extracting reconnect state
        // from any active connection so we can preserve it or trigger a
        // reconnect when the version changes.
        let mut old_versioned_agents: HashMap<
            AgentId,
            (SharedString, watch::Sender<Option<String>>),
        > = HashMap::default();
        for (name, mut entry) in self.external_agents.drain() {
            if let Some(version) = entry.server.version().cloned() {
                if let Some(tx) = entry.server.take_new_version_available_tx() {
                    old_versioned_agents.insert(name, (version, tx));
                }
            }
        }

        for (name, settings) in new_settings.iter() {
            match settings {
                CustomAgentServerSettings::Custom { command, .. } => {
                    let agent_name = AgentId(name.clone().into());
                    self.external_agents.insert(
                        agent_name.clone(),
                        ExternalAgentEntry::new(
                            Box::new(LocalCustomAgent {
                                command: command.clone(),
                                project_environment: project_environment.clone(),
                            }) as Box<dyn ExternalAgentServer>,
                            ExternalAgentSource::Custom,
                            None,
                            None,
                        ),
                    );
                }
            }
        }

        // For each rebuilt versioned agent, compare the version. If it
        // changed, notify the active connection to reconnect. Otherwise,
        // transfer the channel to the new entry so future updates can use it.
        for (name, entry) in &mut self.external_agents {
            let Some((old_version, mut tx)) = old_versioned_agents.remove(name) else {
                continue;
            };
            let Some(new_version) = entry.server.version() else {
                continue;
            };

            if new_version != &old_version {
                tx.send(Some(new_version.to_string())).ok();
            } else {
                entry.server.set_new_version_available_tx(tx);
            }
        }

        *old_settings = Some(new_settings);

        if let Some((project_id, downstream_client)) = downstream_client {
            downstream_client
                .send(proto::ExternalAgentsUpdated {
                    project_id: *project_id,
                    names: self
                        .external_agents
                        .keys()
                        .map(|name| name.to_string())
                        .collect(),
                })
                .log_err();
        }
        cx.emit(AgentServersUpdated);
    }

    pub fn local(project_environment: Entity<ProjectEnvironment>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe_global::<SettingsStore>(|this, cx| {
            this.agent_servers_settings_changed(cx);
        })];
        let mut this = Self {
            state: AgentServerStoreState::Local {
                project_environment,
                downstream_client: None,
                settings: None,
                _subscriptions: subscriptions,
            },
            external_agents: HashMap::default(),
        };
        if let Some(_events) = extension::ExtensionEvents::try_global(cx) {}
        this.agent_servers_settings_changed(cx);
        this
    }

    pub(crate) fn remote(
        project_id: u64,
        upstream_client: Entity<RemoteClient>,
        worktree_store: Entity<WorktreeStore>,
    ) -> Self {
        Self {
            state: AgentServerStoreState::Remote {
                project_id,
                upstream_client,
                worktree_store,
            },
            external_agents: HashMap::default(),
        }
    }

    pub fn collab() -> Self {
        Self {
            state: AgentServerStoreState::Collab,
            external_agents: HashMap::default(),
        }
    }

    pub fn shared(&mut self, project_id: u64, client: AnyProtoClient, cx: &mut Context<Self>) {
        match &mut self.state {
            AgentServerStoreState::Local {
                downstream_client, ..
            } => {
                *downstream_client = Some((project_id, client.clone()));
                // Send the current list of external agents downstream, but only after a delay,
                // to avoid having the message arrive before the downstream project's agent server store
                // sets up its handlers.
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    let names = this.update(cx, |this, _| {
                        this.external_agents()
                            .map(|name| name.to_string())
                            .collect()
                    })?;
                    client
                        .send(proto::ExternalAgentsUpdated { project_id, names })
                        .log_err();
                    anyhow::Ok(())
                })
                .detach();
            }
            AgentServerStoreState::Remote { .. } => {
                debug_panic!(
                    "external agents over collab not implemented, remote project should not be shared"
                );
            }
            AgentServerStoreState::Collab => {
                debug_panic!("external agents over collab not implemented, should not be shared");
            }
        }
    }

    pub fn get_external_agent(
        &mut self,
        name: &AgentId,
    ) -> Option<&mut (dyn ExternalAgentServer + 'static)> {
        self.external_agents
            .get_mut(name)
            .map(|entry| entry.server.as_mut())
    }

    /// Locates the agent's own CLI — the one terminal mode runs, and the one the
    /// "not installed" screen asks about.
    ///
    /// The lookup goes through [`ProjectEnvironment`] rather than the process
    /// environment on purpose. A GUI application on macOS starts with a minimal
    /// `PATH`, so the native Claude Code installer's `~/.local/bin/claude` is
    /// invisible to `which` unless the user's real shell environment is loaded
    /// first — checking the process `PATH` reports "not installed" on a machine
    /// where the CLI is installed and working.
    ///
    /// The environment task is shared and cached by `ProjectEnvironment`, so the
    /// login shell is spawned at most once; this is still called on demand rather
    /// than at startup, since nothing before the first click needs the answer.
    pub fn resolve_agent_binary(
        &self,
        agent: &AgentId,
        cx: &mut Context<Self>,
    ) -> Task<Result<AgentBinary>> {
        let Some(builtin) = builtin_agent(agent.as_ref()) else {
            return Task::ready(Err(anyhow::anyhow!(
                "agent `{agent}` has no CLI of its own to locate"
            )));
        };

        let AgentServerStoreState::Local {
            project_environment,
            ..
        } = &self.state
        else {
            return Task::ready(Err(anyhow::anyhow!(
                "locating an agent CLI is only supported for local projects"
            )));
        };

        let environment = project_environment.downgrade();
        cx.spawn(async move |_, cx| {
            let environment = environment
                .update(cx, |environment, cx| environment.default_environment(cx))?
                .await
                .unwrap_or_default();

            let search_path = environment.get("PATH").cloned();

            // Under test the project environment is empty, and `locate_binary`
            // would fall back to the process `PATH` — which makes the answer
            // depend on which agent CLIs the developer happens to have
            // installed, and then spawns that real CLI in a pty whose reader
            // thread the test scheduler does not own ("Your test is not
            // deterministic"). A test that wants a binary found supplies a
            // `PATH` of its own; one that supplies none is asking about the
            // not-installed path.
            if cfg!(any(test, feature = "test-support")) && search_path.is_none() {
                return Ok(AgentBinary::Missing(AgentBinaryMissing {
                    agent: AgentId(builtin.id.into()),
                    binary: builtin.binary,
                    install_command: builtin.install_command(),
                    docs_url: builtin.docs_url,
                }));
            }

            // `which_in` stats every `PATH` entry; keep that off the foreground
            // thread so a click never costs a frame.
            let found = cx
                .background_executor()
                .spawn(async move { locate_binary(builtin.binary, search_path) })
                .await;

            Ok(match found {
                Some(path) => AgentBinary::Found(path),
                None => AgentBinary::Missing(AgentBinaryMissing {
                    agent: AgentId(builtin.id.into()),
                    binary: builtin.binary,
                    install_command: builtin.install_command(),
                    docs_url: builtin.docs_url,
                }),
            })
        })
    }

    pub fn no_browser(&self) -> bool {
        match &self.state {
            AgentServerStoreState::Local {
                downstream_client, ..
            } => downstream_client
                .as_ref()
                .is_some_and(|(_, client)| !client.has_wsl_interop()),
            _ => false,
        }
    }

    pub fn has_external_agents(&self) -> bool {
        !self.external_agents.is_empty()
    }

    pub fn external_agents(&self) -> impl Iterator<Item = &AgentId> {
        self.external_agents.keys()
    }

    async fn handle_get_agent_server_command(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::GetAgentServerCommand>,
        mut cx: AsyncApp,
    ) -> Result<proto::AgentServerCommand> {
        let command = this
            .update(&mut cx, |this, cx| {
                let AgentServerStoreState::Local {
                    downstream_client, ..
                } = &this.state
                else {
                    debug_panic!("should not receive GetAgentServerCommand in a non-local project");
                    bail!("unexpected GetAgentServerCommand request in a non-local project");
                };
                let no_browser = this.no_browser();
                let agent = this
                    .external_agents
                    .get_mut(&*envelope.payload.name)
                    .map(|entry| entry.server.as_mut())
                    .with_context(|| format!("agent `{}` not found", envelope.payload.name))?;
                let new_version_available_tx =
                    downstream_client
                        .clone()
                        .map(|(project_id, downstream_client)| {
                            let (new_version_available_tx, mut new_version_available_rx) =
                                watch::channel(None);
                            cx.spawn({
                                let name = envelope.payload.name.clone();
                                async move |_, _| {
                                    if let Some(version) =
                                        new_version_available_rx.recv().await.ok().flatten()
                                    {
                                        downstream_client.send(
                                            proto::NewExternalAgentVersionAvailable {
                                                project_id,
                                                name: name.clone(),
                                                version,
                                            },
                                        )?;
                                    }
                                    anyhow::Ok(())
                                }
                            })
                            .detach_and_log_err(cx);
                            new_version_available_tx
                        });
                let mut extra_env = HashMap::default();
                if no_browser {
                    extra_env.insert("NO_BROWSER".to_owned(), "1".to_owned());
                }
                if let Some(new_version_available_tx) = new_version_available_tx {
                    agent.set_new_version_available_tx(new_version_available_tx);
                }
                anyhow::Ok(agent.get_command(vec![], extra_env, &mut cx.to_async()))
            })?
            .await?;
        Ok(proto::AgentServerCommand {
            path: command.path.to_string_lossy().into_owned(),
            args: command.args,
            env: command
                .env
                .map(|env| env.into_iter().collect())
                .unwrap_or_default(),
            root_dir: envelope
                .payload
                .root_dir
                .unwrap_or_else(|| paths::home_dir().to_string_lossy().to_string()),
            login: None,
        })
    }

    async fn handle_external_agents_updated(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::ExternalAgentsUpdated>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |this, cx| {
            let AgentServerStoreState::Remote {
                project_id,
                upstream_client,
                worktree_store,
            } = &this.state
            else {
                debug_panic!(
                    "handle_external_agents_updated should not be called for a non-remote project"
                );
                bail!("unexpected ExternalAgentsUpdated message")
            };

            let mut previous_entries = std::mem::take(&mut this.external_agents);
            let mut new_version_available_txs = HashMap::default();
            let mut metadata = HashMap::default();

            for (name, mut entry) in previous_entries.drain() {
                if let Some(tx) = entry.server.take_new_version_available_tx() {
                    new_version_available_txs.insert(name.clone(), tx);
                }

                metadata.insert(name, (entry.icon, entry.display_name, entry.source));
            }

            this.external_agents = envelope
                .payload
                .names
                .into_iter()
                .map(|name| {
                    let agent_id = AgentId(name.into());
                    let (icon, display_name, source) = metadata.remove(&agent_id).unwrap_or((
                        None,
                        None,
                        ExternalAgentSource::default(),
                    ));
                    let agent = RemoteExternalAgentServer {
                        project_id: *project_id,
                        upstream_client: upstream_client.clone(),
                        worktree_store: worktree_store.clone(),
                        name: agent_id.clone(),
                        new_version_available_tx: new_version_available_txs.remove(&agent_id),
                    };
                    (
                        agent_id,
                        ExternalAgentEntry::new(
                            Box::new(agent) as Box<dyn ExternalAgentServer>,
                            source,
                            icon,
                            display_name,
                        ),
                    )
                })
                .collect();
            cx.emit(AgentServersUpdated);
            Ok(())
        })
    }

    async fn handle_new_version_available(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::NewExternalAgentVersionAvailable>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |this, _| {
            if let Some(entry) = this.external_agents.get_mut(&*envelope.payload.name)
                && let Some(mut tx) = entry.server.take_new_version_available_tx()
            {
                tx.send(Some(envelope.payload.version)).ok();
                entry.server.set_new_version_available_tx(tx);
            }
        });
        Ok(())
    }
}

struct RemoteExternalAgentServer {
    project_id: u64,
    upstream_client: Entity<RemoteClient>,
    worktree_store: Entity<WorktreeStore>,
    name: AgentId,
    new_version_available_tx: Option<watch::Sender<Option<String>>>,
}

impl ExternalAgentServer for RemoteExternalAgentServer {
    fn take_new_version_available_tx(&mut self) -> Option<watch::Sender<Option<String>>> {
        self.new_version_available_tx.take()
    }

    fn set_new_version_available_tx(&mut self, tx: watch::Sender<Option<String>>) {
        self.new_version_available_tx = Some(tx);
    }

    fn get_command(
        &self,
        extra_args: Vec<String>,
        extra_env: HashMap<String, String>,
        cx: &mut AsyncApp,
    ) -> Task<Result<AgentServerCommand>> {
        let project_id = self.project_id;
        let name = self.name.to_string();
        let upstream_client = self.upstream_client.downgrade();
        let worktree_store = self.worktree_store.clone();
        cx.spawn(async move |cx| {
            let root_dir = worktree_store.read_with(cx, |worktree_store, cx| {
                crate::Project::default_visible_worktree_paths(worktree_store, cx)
                    .into_iter()
                    .next()
                    .map(|path| path.display().to_string())
            });

            let mut response = upstream_client
                .update(cx, |upstream_client, _| {
                    upstream_client
                        .proto_client()
                        .request(proto::GetAgentServerCommand {
                            project_id,
                            name,
                            root_dir,
                        })
                })?
                .await?;
            response.args.extend(extra_args);
            response.env.extend(extra_env);

            Ok(AgentServerCommand {
                path: response.path.into(),
                args: response.args,
                env: Some(response.env.into_iter().collect()),
            })
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

struct LocalCustomAgent {
    project_environment: Entity<ProjectEnvironment>,
    command: AgentServerCommand,
}

impl ExternalAgentServer for LocalCustomAgent {
    fn get_command(
        &self,
        extra_args: Vec<String>,
        extra_env: HashMap<String, String>,
        cx: &mut AsyncApp,
    ) -> Task<Result<AgentServerCommand>> {
        let mut command = self.command.clone();
        let project_environment = self.project_environment.downgrade();
        cx.spawn(async move |cx| {
            let mut env = project_environment
                .update(cx, |project_environment, cx| {
                    project_environment.default_environment(cx)
                })?
                .await
                .unwrap_or_default();
            env.extend(command.env.unwrap_or_default());
            env.extend(extra_env);
            command.env = Some(env);
            command.args.extend(extra_args);
            Ok(command)
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Default, Clone, JsonSchema, Debug, PartialEq, RegisterSetting)]
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

#[derive(Clone, JsonSchema, Debug, PartialEq)]
pub enum CustomAgentServerSettings {
    Custom {
        command: AgentServerCommand,
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
        /// Default: []
        favorite_models: Vec<String>,
        /// Default values for session config options.
        ///
        /// This is a map from config option ID to value ID.
        ///
        /// Default: {}
        default_config_options: HashMap<String, String>,
        /// Favorited values for session config options.
        ///
        /// This is a map from config option ID to a list of favorited value IDs.
        ///
        /// Default: {}
        favorite_config_option_values: HashMap<String, Vec<String>>,
    },
}

impl CustomAgentServerSettings {
    pub fn command(&self) -> Option<&AgentServerCommand> {
        match self {
            CustomAgentServerSettings::Custom { command, .. } => Some(command),
        }
    }

    pub fn default_mode(&self) -> Option<&str> {
        match self {
            CustomAgentServerSettings::Custom { default_mode, .. } => default_mode.as_deref(),
        }
    }

    pub fn default_model(&self) -> Option<&str> {
        match self {
            CustomAgentServerSettings::Custom { default_model, .. } => default_model.as_deref(),
        }
    }

    pub fn favorite_models(&self) -> &[String] {
        match self {
            CustomAgentServerSettings::Custom {
                favorite_models, ..
            } => favorite_models,
        }
    }

    pub fn default_config_option(&self, config_id: &str) -> Option<&str> {
        match self {
            CustomAgentServerSettings::Custom {
                default_config_options,
                ..
            } => default_config_options.get(config_id).map(|s| s.as_str()),
        }
    }

    pub fn favorite_config_option_values(&self, config_id: &str) -> Option<&[String]> {
        match self {
            CustomAgentServerSettings::Custom {
                favorite_config_option_values,
                ..
            } => favorite_config_option_values
                .get(config_id)
                .map(|v| v.as_slice()),
        }
    }
}

impl From<settings::CustomAgentServerSettings> for CustomAgentServerSettings {
    fn from(value: settings::CustomAgentServerSettings) -> Self {
        match value {
            settings::CustomAgentServerSettings::Custom {
                path,
                args,
                env,
                default_mode,
                default_model,
                favorite_models,
                default_config_options,
                favorite_config_option_values,
            } => CustomAgentServerSettings::Custom {
                command: AgentServerCommand {
                    path: PathBuf::from(shellexpand::tilde(&path.to_string_lossy()).as_ref()),
                    args,
                    env: Some(env),
                },
                default_mode,
                default_model,
                favorite_models,
                default_config_options,
                favorite_config_option_values,
            },
        }
    }
}

impl settings::Settings for AllAgentServersSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let agent_settings = content.agent_servers.clone().unwrap();
        Self(
            agent_settings
                .0
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree_store::{WorktreeIdCounter, WorktreeStore};
    use fs::Fs;
    use gpui::{AppContext as _, TestAppContext};
    use settings::Settings as _;
    use std::sync::Arc;

    fn init_test_settings(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = SettingsStore::test(cx);
            cx.set_global(settings_store);
        });
    }

    fn create_agent_server_store(cx: &mut TestAppContext) -> gpui::Entity<AgentServerStore> {
        cx.update(|cx| {
            let fs: Arc<dyn Fs> = fs::FakeFs::new(cx.background_executor().clone());
            let worktree_store =
                cx.new(|cx| WorktreeStore::local(false, fs.clone(), WorktreeIdCounter::get(cx)));
            let project_environment = cx.new(|cx| {
                crate::ProjectEnvironment::new(None, worktree_store.downgrade(), None, false, cx)
            });
            cx.new(|cx| AgentServerStore::local(project_environment, cx))
        })
    }

    /// The bug this guards against: a GUI application on macOS inherits a minimal
    /// `PATH`, so the native Claude Code installer's `~/.local/bin/claude` is
    /// invisible unless the user's shell environment is consulted first. Both
    /// halves matter — finding it through the supplied path proves nothing unless
    /// the process `PATH` alone demonstrably fails to find the same binary.
    #[test]
    fn locates_a_binary_that_the_process_path_cannot_see() {
        let exe = std::env::current_exe().expect("test binary has a path");
        let dir = exe.parent().expect("test binary lives in a directory");
        let name = exe
            .file_name()
            .and_then(|name| name.to_str())
            .expect("test binary has a name");

        assert_eq!(
            locate_binary(name, Some(dir.to_string_lossy().into_owned())).as_deref(),
            Some(exe.as_path()),
            "a binary in the supplied PATH must be found"
        );

        assert_eq!(
            locate_binary(name, None),
            None,
            "the same binary must be invisible to the process PATH, or this test \
             proves nothing about reading the shell environment"
        );
    }

    #[test]
    fn empty_shell_path_falls_back_rather_than_reporting_missing() {
        // A login shell that fails to report a PATH must not be turned into a
        // "not installed" verdict.
        let exe = std::env::current_exe().expect("test binary has a path");
        let name = exe.file_name().and_then(|name| name.to_str()).unwrap();
        assert_eq!(locate_binary(name, Some(String::new())), None);
        assert!(locate_binary("sh", Some(String::new())).is_some() || cfg!(windows));
    }

    /// What a built-in must carry to be usable at all: a binary to look for, an
    /// install command to offer when it is missing, and docs to point at.
    #[test]
    fn every_builtin_can_answer_the_not_installed_screen() {
        for builtin in BUILTIN_AGENTS {
            assert!(!builtin.binary.is_empty(), "{} has no binary", builtin.id);
            assert!(!builtin.display_name.is_empty());
            assert!(!builtin.install_command().is_empty());
            assert!(
                builtin.docs_url.starts_with("https://"),
                "{} docs url is not https: {}",
                builtin.id,
                builtin.docs_url
            );
        }

        assert!(builtin_agent(CLAUDE_CODE_AGENT_ID).is_some());
        assert!(builtin_agent(CODEX_AGENT_ID).is_some());
        assert!(builtin_agent(ANTIGRAVITY_AGENT_ID).is_some());
        assert!(builtin_agent(COPILOT_AGENT_ID).is_some());
        assert!(builtin_agent("nope").is_none());
    }

    /// Every builtin id must be unique: `builtin_agent` returns the first match,
    /// so a duplicate would make one of them permanently unreachable while the
    /// rail happily drew a button for it.
    #[test]
    fn builtin_agent_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for builtin in BUILTIN_AGENTS {
            assert!(
                seen.insert(builtin.id),
                "duplicate builtin id {}",
                builtin.id
            );
        }
    }

    /// "Install Now" types the command into a terminal and deliberately stops
    /// short of pressing return. A newline anywhere in these strings would run it
    /// instead — turning an offer into an unrequested `curl … | bash`.
    #[test]
    fn install_commands_never_carry_a_newline() {
        for builtin in BUILTIN_AGENTS {
            for command in [builtin.install_unix, builtin.install_windows] {
                assert!(
                    !command.contains('\n') && !command.contains('\r'),
                    "{} would execute on its own: {command:?}",
                    builtin.id
                );
                assert!(!command.trim().is_empty());
            }
        }
    }

    /// Nothing registers a built-in into `external_agents` any more, and nothing
    /// needs to: the rail reaches an agent through `builtin_agent` and
    /// `resolve_agent_binary`, neither of which reads that map. The map exists
    /// only for an `agent_servers` entry the user wrote themselves.
    ///
    /// This is the test that would fail if a future change re-introduced a
    /// registration path — and with it a network fetch on the cold-start path,
    /// which is the thing the fork removed.
    #[gpui::test]
    async fn a_store_with_no_settings_registers_no_agents(cx: &mut TestAppContext) {
        init_test_settings(cx);
        let store = create_agent_server_store(cx);

        store.read_with(cx, |store, _| {
            assert!(
                !store.has_external_agents(),
                "a default store registered {:?}",
                store.external_agents().collect::<Vec<_>>()
            );
        });

        // Every built-in is still reachable, just not through that map.
        for builtin in BUILTIN_AGENTS {
            assert!(builtin_agent(builtin.id).is_some());
        }
    }

    #[gpui::test]
    async fn a_user_entry_naming_a_builtin_id_does_not_displace_it(cx: &mut TestAppContext) {
        init_test_settings(cx);
        let store = create_agent_server_store(cx);

        cx.update(|cx| {
            AllAgentServersSettings::override_global(
                AllAgentServersSettings(
                    [(
                        CLAUDE_CODE_AGENT_ID.to_string(),
                        settings::CustomAgentServerSettings::Custom {
                            path: PathBuf::from("/usr/local/bin/my-claude"),
                            args: Vec::new(),
                            env: HashMap::default(),
                            default_mode: None,
                            default_model: None,
                            favorite_models: Vec::new(),
                            default_config_options: HashMap::default(),
                            favorite_config_option_values: HashMap::default(),
                        }
                        .into(),
                    )]
                    .into_iter()
                    .collect(),
                ),
                cx,
            );
        });
        cx.run_until_parked();

        store.read_with(cx, |store, _| {
            assert_eq!(
                store.agent_source(&AgentId::new(CLAUDE_CODE_AGENT_ID)),
                Some(ExternalAgentSource::Custom),
                "the entry the user wrote is what the map holds"
            );
            assert_eq!(
                store.agent_source(&AgentId::new(CODEX_AGENT_ID)),
                None,
                "an agent nobody configured is absent, not a built-in entry"
            );
        });

        // And the point of the whole thing: naming a built-in id in
        // `agent_servers` must not cost the user that agent's rail button, which
        // resolves through `builtin_agent` and never looks at the map above.
        assert_eq!(
            builtin_agent(CLAUDE_CODE_AGENT_ID).map(|builtin| builtin.binary),
            Some("claude")
        );
    }
}
