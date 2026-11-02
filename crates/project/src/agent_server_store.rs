use std::{any::Any, path::PathBuf, time::Duration};

use anyhow::{Context as _, Result, bail};
use collections::HashMap;
use gpui::{
    AppContext as _, AsyncApp, Context, Entity, EventEmitter, SharedString, Subscription, Task,
};
use remote::RemoteClient;
use rpc::{
    AnyProtoClient, TypedEnvelope,
    proto::{self},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::{RegisterSetting, SettingsStore};
use util::{ResultExt as _, debug_panic, paths::PathStyle};

use crate::ProjectEnvironment;
use crate::agent_bypass::{BypassCheck, CacheKey, PROBE_TIMEOUT};

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
    /// The flag this CLI takes to stop asking before it acts, or `None` if it
    /// has none.
    ///
    /// Deliberately has no `Default`: adding a sixth agent must not compile
    /// until somebody has decided what its auto-approve posture is. The five
    /// values below are not equivalent to each other -- see the two that say so.
    pub bypass_flag: Option<&'static str>,
}

impl BuiltinAgent {
    pub fn install_command(&self) -> &'static str {
        self.install_command_for(PathStyle::local())
    }

    /// The install line for a host of `path_style`, which for a remote project
    /// is the remote's rather than this machine's: a Windows app connected to a
    /// WSL distro must offer the Linux installer.
    pub fn install_command_for(&self, path_style: PathStyle) -> &'static str {
        match path_style {
            PathStyle::Windows => self.install_windows,
            PathStyle::Posix => self.install_unix,
        }
    }

    fn missing(&self, path_style: PathStyle) -> AgentBinaryMissing {
        AgentBinaryMissing {
            agent: AgentId(self.id.into()),
            binary: self.binary,
            install_command: self.install_command_for(path_style),
            docs_url: self.docs_url,
        }
    }
}

pub const CLAUDE_CODE_AGENT_ID: &str = "claude-acp";
pub const CODEX_AGENT_ID: &str = "codex-acp";
/// Ids match the ACP registry's own, so a user who points `agent_servers` at the
/// registry entry lands on the same agent rather than a second copy of it.
pub const ANTIGRAVITY_AGENT_ID: &str = "antigravity-acp";
pub const COPILOT_AGENT_ID: &str = "github-copilot-cli";
/// opencode ships an `opencode acp` subcommand, but this editor no longer speaks
/// ACP, so matching that registry's ids like the three above would be following
/// a constraint that no longer applies here.
pub const OPENCODE_AGENT_ID: &str = "opencode";

pub const BUILTIN_AGENTS: &[BuiltinAgent] = &[
    BuiltinAgent {
        id: CLAUDE_CODE_AGENT_ID,
        display_name: "Claude Code",
        binary: "claude",
        docs_url: "https://code.claude.com/docs/en/setup",
        install_unix: "curl -fsSL https://claude.ai/install.sh | bash",
        install_windows: "irm https://claude.ai/install.ps1 | iex",
        bypass_flag: Some("--dangerously-skip-permissions"),
    },
    BuiltinAgent {
        id: CODEX_AGENT_ID,
        display_name: "Codex",
        binary: "codex",
        docs_url: "https://developers.openai.com/codex/cli",
        install_unix: "curl -fsSL https://chatgpt.com/codex/install.sh | sh",
        install_windows: "powershell -ExecutionPolicy ByPass -c \"irm https://chatgpt.com/codex/install.ps1 | iex\"",
        // Not `--dangerously-bypass-approvals-and-sandbox`, which Codex's own
        // help calls EXTREMELY DANGEROUS: this one stops the asking but keeps
        // the workspace-write sandbox, which is the nearest thing to what the
        // other four do, since none of them has a sandbox to drop.
        bypass_flag: Some("--approve-for-me"),
    },
    BuiltinAgent {
        id: ANTIGRAVITY_AGENT_ID,
        display_name: "Antigravity",
        binary: "agy",
        docs_url: "https://antigravity.google/docs/cli/getting-started/",
        install_unix: "curl -fsSL https://antigravity.google/cli/install.sh | bash",
        install_windows: "irm https://antigravity.google/cli/install.ps1 | iex",
        bypass_flag: Some("--dangerously-skip-permissions"),
    },
    BuiltinAgent {
        id: COPILOT_AGENT_ID,
        display_name: "Copilot",
        binary: "copilot",
        docs_url: "https://docs.github.com/copilot/how-tos/copilot-cli",
        install_unix: "curl -fsSL https://gh.io/copilot-install | bash",
        install_windows: "winget install GitHub.Copilot",
        bypass_flag: Some("--allow-all"),
    },
    BuiltinAgent {
        id: OPENCODE_AGENT_ID,
        display_name: "opencode",
        binary: "opencode",
        docs_url: "https://opencode.ai/docs/",
        install_unix: "curl -fsSL https://opencode.ai/install | bash",
        // opencode publishes no PowerShell or winget one-liner; its docs lead
        // Windows users to WSL and then list choco, scoop and npm. npm is the one
        // most likely to already work on a machine running this editor.
        install_windows: "npm install -g opencode-ai",
        // Not the same posture as the other four, and the odd one out in this
        // table: `--auto` auto-approves everything *except* what the user has
        // explicitly denied, so a deny-list still holds under it.
        bypass_flag: Some("--auto"),
    },
];

pub fn builtin_agent(id: &str) -> Option<&'static BuiltinAgent> {
    BUILTIN_AGENTS.iter().find(|agent| agent.id == id)
}

/// A remote server's answer to `LocateAgentCli`, as the launch path wants it.
///
/// The path is only ever stringified back into the terminal command, never
/// resolved here: on a Windows client a POSIX path is not `Path::is_absolute`,
/// so it is checked by the remote's own path style instead.
fn agent_binary_from_remote(
    builtin: &'static BuiltinAgent,
    path_style: PathStyle,
    path: Option<String>,
) -> Result<AgentBinary> {
    match path {
        None => Ok(AgentBinary::Missing(builtin.missing(path_style))),
        Some(path) if path_style.is_absolute(&path) => Ok(AgentBinary::Found(PathBuf::from(path))),
        Some(path) => bail!(
            "the remote server reported `{path}` for `{}`, which is not an absolute path",
            builtin.binary
        ),
    }
}

/// A remote server's answer to `ProbeAgentBypassFlag`. An answer missing its
/// verdict is a server this client does not understand, and fails closed.
fn bypass_check_from_remote(
    flag: &'static str,
    binary: PathBuf,
    check: Option<proto::probe_agent_bypass_flag_response::Check>,
) -> BypassCheck {
    use proto::probe_agent_bypass_flag_response::Check;
    match check {
        Some(Check::Present(_)) => BypassCheck::Present,
        Some(Check::Absent(_)) => BypassCheck::Absent { flag, binary },
        Some(Check::Unprobed(unprobed)) => BypassCheck::Unprobed {
            reason: unprobed.reason,
        },
        None => BypassCheck::Unprobed {
            reason: "the remote server sent no answer for the probe".to_string(),
        },
    }
}

const REMOTE_SERVER_OUT_OF_DATE: &str =
    "the remote server does not support agent terminals yet; reconnect to update it";

/// How long a client waits for `LocateAgentCli`. The server may have to load a
/// login shell before it can search that shell's `PATH`.
const REMOTE_LOCATE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a client waits for `ProbeAgentBypassFlag`: the server's own probe
/// budget, plus room for the round trip.
const REMOTE_PROBE_TIMEOUT: Duration = Duration::from_secs(PROBE_TIMEOUT.as_secs() + 5);

/// A server built before these requests existed answers them with "no
/// handler registered". Said in terms the user can act on, since the fix is a
/// reconnect, which fetches the server matching this app.
fn remote_agent_error(error: anyhow::Error) -> anyhow::Error {
    if format!("{error:#}").contains("no handler registered") {
        anyhow::anyhow!(REMOTE_SERVER_OUT_OF_DATE)
    } else {
        error
    }
}

/// `request`, unless `timer` fires first.
///
/// A server built before these requests existed drops them without a word --
/// its message loop has no type to decode them into -- so without a deadline
/// the launch would wait on it forever.
async fn remote_agent_request<T>(
    request: impl Future<Output = Result<T>>,
    timer: impl Future<Output = ()>,
    timeout: Duration,
) -> Result<T> {
    match futures::future::select(std::pin::pin!(request), std::pin::pin!(timer)).await {
        futures::future::Either::Left((response, _)) => response.map_err(remote_agent_error),
        futures::future::Either::Right(_) => Err(anyhow::anyhow!(
            "the remote server did not answer within {}s; if it predates agent terminals, \
             reconnect to update it",
            timeout.as_secs()
        )),
    }
}

/// Looks `binary` up in `search_path`, which is meant to be the `PATH` of the
/// user's own shell rather than the one this process was started with.
///
/// The process `PATH` is only a fallback for when the shell environment could not
/// be loaded at all: reporting "not installed" because a login shell failed is a
/// worse answer than searching a narrower `PATH`.
fn locate_binary(
    binary: &str,
    search_path: Option<String>,
    skip_windows_drives: bool,
) -> Option<PathBuf> {
    let search_path = search_path
        .filter(|path| !path.is_empty())
        .or_else(|| std::env::var("PATH").ok())?;
    // Applied after the fallback, so the process `PATH` of a server running
    // inside WSL is held to the same rule as the shell's.
    let search_path = if skip_windows_drives {
        without_windows_drive_mounts(&search_path)
    } else {
        search_path
    };
    // `cwd` only affects relative `PATH` entries, which are vanishingly rare; an
    // unreadable working directory must not fail the lookup.
    let cwd = std::env::current_dir().unwrap_or_default();
    which::which_in(binary, Some(search_path), cwd).ok()
}

/// `search_path` without the Windows drives WSL mounts under `/mnt/<letter>/`.
///
/// WSL appends the Windows `PATH` to the distro's by default, so a Windows-side
/// `claude.cmd` shim or npm global would otherwise satisfy the lookup and the
/// terminal would run a Windows program against Linux paths.
fn without_windows_drive_mounts(search_path: &str) -> String {
    search_path
        .split(':')
        .filter(|entry| !is_windows_drive_mount(entry))
        .collect::<Vec<_>>()
        .join(":")
}

fn is_windows_drive_mount(entry: &str) -> bool {
    let Some(rest) = entry.strip_prefix("/mnt/") else {
        return false;
    };
    let mut chars = rest.chars();
    chars
        .next()
        .is_some_and(|drive| drive.is_ascii_alphabetic())
        && chars.next().is_none_or(|separator| separator == '/')
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
    /// Probe answers for this run. See `agent_bypass` for why this is keyed by
    /// the binary on disk rather than by a version.
    bypass_flags: crate::agent_bypass::BypassFlagCache,
    /// Stands in for the shell `PATH` when the shell environment carries none,
    /// which under test is always.
    #[cfg(any(test, feature = "test-support"))]
    search_path_override: Option<String>,
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
        session.add_entity_request_handler(Self::handle_locate_agent_cli);
        session.add_entity_request_handler(Self::handle_probe_agent_bypass_flag);
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
            bypass_flags: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            search_path_override: None,
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
            bypass_flags: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            search_path_override: None,
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
            bypass_flags: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            search_path_override: None,
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

    /// Whether `agent`'s installed binary still takes the flag this editor would
    /// pass it to stop the permission prompts.
    ///
    /// Fails closed. Only `Present` may be read as permission to add the flag:
    /// an agent with no mapping, an unreadable binary, a spawn that fails and a
    /// probe that runs long all answer something else. See `agent_bypass` for
    /// why a mapping that has gone stale has to surface here, as a refusal,
    /// rather than as a launch that quietly goes on asking.
    pub fn verify_bypass_flag(
        &mut self,
        agent: &AgentId,
        binary: PathBuf,
        cx: &mut Context<Self>,
    ) -> Task<BypassCheck> {
        let Some(flag) = builtin_agent(agent.as_ref()).and_then(|builtin| builtin.bypass_flag)
        else {
            return Task::ready(BypassCheck::Unprobed {
                reason: format!("`{agent}` has no auto-approve flag recorded"),
            });
        };
        let environment =
            match &self.state {
                AgentServerStoreState::Remote {
                    project_id,
                    upstream_client,
                    ..
                } => {
                    // Forwarded before the cache key is built: that key stats
                    // `binary` on this machine, where a remote path names nothing
                    // (or worse, something else). The server keeps its own cache.
                    let request = upstream_client.read(cx).proto_client().request(
                        proto::ProbeAgentBypassFlag {
                            project_id: *project_id,
                            agent_id: agent.to_string(),
                            binary: binary.to_string_lossy().into_owned(),
                        },
                    );
                    let timer = cx.background_executor().timer(REMOTE_PROBE_TIMEOUT);
                    return cx.spawn(async move |_, _| {
                        match remote_agent_request(request, timer, REMOTE_PROBE_TIMEOUT).await {
                            Ok(response) => bypass_check_from_remote(flag, binary, response.check),
                            Err(error) => BypassCheck::Unprobed {
                                reason: format!("{error:#}"),
                            },
                        }
                    });
                }
                AgentServerStoreState::Local {
                    project_environment,
                    ..
                } => Some(project_environment.downgrade()),
                AgentServerStoreState::Collab => None,
            };
        let Some(key) = CacheKey::for_binary(&binary) else {
            return Task::ready(BypassCheck::Unprobed {
                reason: format!("could not read `{}`", binary.display()),
            });
        };
        if let Some(present) = self.bypass_flags.get(&key) {
            return Task::ready(Self::decide(present, flag, binary));
        }

        let search_path_override = self.search_path_override();
        cx.spawn(async move |this, cx| {
            // The `PATH` the CLI was found on, not this process's. An npm or nvm
            // install is a `#!/usr/bin/env node` script, and `env` finds no
            // `node` on the minimal `PATH` a GUI app (or a server started by
            // `wsl.exe`) inherits.
            let environment = environment.and_then(|environment| {
                environment
                    .update(cx, |environment, cx| environment.default_environment(cx))
                    .log_err()
            });
            let environment = match environment {
                Some(environment) => environment.await.unwrap_or_default(),
                None => HashMap::default(),
            };
            let search_path = environment
                .get("PATH")
                .filter(|path| !path.is_empty())
                .cloned()
                .or(search_path_override);

            let probe = cx.background_spawn({
                let binary = binary.clone();
                async move {
                    let mut command = util::command::new_command(&binary);
                    // Without this the 5s timer below only stops *waiting*: the
                    // dropped future drops the `Child`, and a wedged `--help`
                    // outlives the app, one per launch. `kill_on_drop` defaults
                    // to false (`util::command`), which is why every other
                    // spawn site in this tree sets it.
                    command.arg("--help").kill_on_drop(true);
                    if let Some(search_path) = search_path {
                        command.env("PATH", search_path);
                    }
                    command.output().await
                }
            });
            let timer = cx.background_executor().timer(PROBE_TIMEOUT);

            let output = match futures::future::select(Box::pin(probe), Box::pin(timer)).await {
                futures::future::Either::Left((output, _)) => output,
                // The probe loses the race. Refusing beats holding a launch open
                // on a binary that is not answering.
                futures::future::Either::Right(_) => {
                    return BypassCheck::Unprobed {
                        reason: format!(
                            "`{} --help` did not answer within {}s",
                            binary.display(),
                            PROBE_TIMEOUT.as_secs()
                        ),
                    };
                }
            };

            let output = match output {
                Ok(output) => output,
                // A spawn that failed is not evidence the flag is gone, and it is
                // certainly not evidence it is there.
                Err(error) => {
                    return BypassCheck::Unprobed {
                        reason: format!("could not run `{} --help`: {error}", binary.display()),
                    };
                }
            };

            let help = crate::agent_bypass::help_text(&output.stdout, &output.stderr);
            // A failure that printed no option line at all -- `env: node: No
            // such file or directory` and exit 127, say -- never reached the
            // CLI's own help, so it says nothing about the flag. Reading it as
            // `Absent` would tell the reader the flag was renamed. Option lines
            // are what `flag_present` reads, so a help page that exits non-zero
            // is still read as one.
            let printed_options = help.lines().any(|line| line.trim_start().starts_with('-'));
            if !output.status.success() && !printed_options {
                return BypassCheck::Unprobed {
                    reason: format!(
                        "`{} --help` failed ({}) without printing any help",
                        binary.display(),
                        output.status
                    ),
                };
            }
            let present = crate::agent_bypass::flag_present(&help, flag);
            this.update(cx, |this, _| this.bypass_flags.insert(key, present))
                .log_err();
            Self::decide(present, flag, binary)
        })
    }

    fn decide(present: bool, flag: &'static str, binary: PathBuf) -> BypassCheck {
        if present {
            BypassCheck::Present
        } else {
            BypassCheck::Absent { flag, binary }
        }
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

        let project_environment = match &self.state {
            AgentServerStoreState::Local {
                project_environment,
                ..
            } => project_environment,
            AgentServerStoreState::Remote {
                project_id,
                upstream_client,
                ..
            } => {
                let upstream_client = upstream_client.read(cx);
                // The CLI lives on the remote host, so its install line has to
                // be that host's, whatever this machine is.
                let path_style = upstream_client.path_style();
                let request = upstream_client
                    .proto_client()
                    .request(proto::LocateAgentCli {
                        project_id: *project_id,
                        agent_id: agent.to_string(),
                    });
                let timer = cx.background_executor().timer(REMOTE_LOCATE_TIMEOUT);
                return cx.spawn(async move |_, _| {
                    let response =
                        remote_agent_request(request, timer, REMOTE_LOCATE_TIMEOUT).await?;
                    agent_binary_from_remote(builtin, path_style, response.path)
                });
            }
            AgentServerStoreState::Collab => {
                return Task::ready(Err(anyhow::anyhow!(
                    "locating an agent CLI is only supported for local projects"
                )));
            }
        };

        let environment = project_environment.downgrade();
        let search_path_override = self.search_path_override();
        cx.spawn(async move |_, cx| {
            let environment = environment
                .update(cx, |environment, cx| environment.default_environment(cx))?
                .await
                .unwrap_or_default();

            let search_path = environment.get("PATH").cloned().or(search_path_override);
            // Read from both: the shell environment is what the user sees, and
            // the process one is what a server started by `wsl.exe` inherits
            // when that shell could not be loaded.
            let inside_wsl = environment.contains_key("WSL_DISTRO_NAME")
                || std::env::var_os("WSL_DISTRO_NAME").is_some();

            // Under test the project environment is empty, and `locate_binary`
            // would fall back to the process `PATH` — which makes the answer
            // depend on which agent CLIs the developer happens to have
            // installed, and then spawns that real CLI in a pty whose reader
            // thread the test scheduler does not own ("Your test is not
            // deterministic"). A test that wants a binary found supplies a
            // `PATH` of its own; one that supplies none is asking about the
            // not-installed path.
            if cfg!(any(test, feature = "test-support")) && search_path.is_none() {
                return Ok(AgentBinary::Missing(builtin.missing(PathStyle::local())));
            }

            // `which_in` stats every `PATH` entry; keep that off the foreground
            // thread so a click never costs a frame.
            let found = cx
                .background_executor()
                .spawn(async move { locate_binary(builtin.binary, search_path, inside_wsl) })
                .await;

            Ok(match found {
                Some(path) => AgentBinary::Found(path),
                None => AgentBinary::Missing(builtin.missing(PathStyle::local())),
            })
        })
    }

    /// Points a store's CLI lookup and `--help` probe at `search_path` when its
    /// shell environment has no `PATH`, which under test is always: see
    /// `resolve_agent_binary`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_search_path_for_test(&mut self, search_path: Option<String>) {
        self.search_path_override = search_path;
    }

    #[cfg(any(test, feature = "test-support"))]
    fn search_path_override(&self) -> Option<String> {
        self.search_path_override.clone()
    }

    #[cfg(not(any(test, feature = "test-support")))]
    fn search_path_override(&self) -> Option<String> {
        None
    }

    /// This store's cached probe answer for `binary`, if it has one.
    #[cfg(any(test, feature = "test-support"))]
    pub fn cached_bypass_flag_for_test(&self, binary: &std::path::Path) -> Option<bool> {
        CacheKey::for_binary(binary).and_then(|key| self.bypass_flags.get(&key))
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

    async fn handle_locate_agent_cli(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::LocateAgentCli>,
        mut cx: AsyncApp,
    ) -> Result<proto::LocateAgentCliResponse> {
        let agent = AgentId::new(envelope.payload.agent_id);
        let binary = this
            .update(&mut cx, |this, cx| this.resolve_agent_binary(&agent, cx))
            .await?;
        Ok(proto::LocateAgentCliResponse {
            path: match binary {
                AgentBinary::Found(path) => Some(path.to_string_lossy().into_owned()),
                AgentBinary::Missing(_) => None,
            },
        })
    }

    /// Probes the CLI this host resolves for the agent, never a path the
    /// request names: a request is input, and this is where it would turn into
    /// a program the server runs. The request's path only has to agree with the
    /// one found, since that is the binary the client is about to launch.
    async fn handle_probe_agent_bypass_flag(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::ProbeAgentBypassFlag>,
        mut cx: AsyncApp,
    ) -> Result<proto::ProbeAgentBypassFlagResponse> {
        use proto::probe_agent_bypass_flag_response::{Absent, Check, Present, Unprobed};

        let agent = AgentId::new(envelope.payload.agent_id);
        let requested = envelope.payload.binary;
        let located = this
            .update(&mut cx, |this, cx| this.resolve_agent_binary(&agent, cx))
            .await;
        let check = match located {
            Ok(AgentBinary::Found(binary)) if binary.to_string_lossy() == requested.as_str() => {
                this.update(&mut cx, |this, cx| {
                    this.verify_bypass_flag(&agent, binary, cx)
                })
                .await
            }
            Ok(AgentBinary::Found(binary)) => BypassCheck::Unprobed {
                reason: format!(
                    "`{requested}` is not the `{agent}` CLI on this host's PATH, which is `{}`",
                    binary.display()
                ),
            },
            Ok(AgentBinary::Missing(missing)) => BypassCheck::Unprobed {
                reason: format!("`{}` is not installed on this host", missing.binary),
            },
            Err(error) => BypassCheck::Unprobed {
                reason: format!("{error:#}"),
            },
        };
        Ok(proto::ProbeAgentBypassFlagResponse {
            check: Some(match check {
                BypassCheck::Present => Check::Present(Present {}),
                BypassCheck::Absent { .. } => Check::Absent(Absent {}),
                BypassCheck::Unprobed { reason } => Check::Unprobed(Unprobed { reason }),
            }),
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
    use gpui::TestAppContext;
    use settings::Settings as _;
    use std::sync::Arc;
    use tempfile::TempDir;

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

    /// One executable in a directory that is on nobody's `PATH`.
    ///
    /// The lookups below used to probe with `std::env::current_exe`, on the
    /// reasoning that the process `PATH` could not possibly see the test binary.
    /// That holds on Unix and is false on Windows: cargo puts the test binary's
    /// own directory on `PATH` there so its DLLs resolve, so both lookups found
    /// the test binary itself and the two assertions that the process `PATH`
    /// finds nothing failed. A file under the system temp directory is
    /// unreachable from `PATH` on every platform, which is what makes the
    /// negative half of these tests mean the same thing everywhere.
    struct Probe {
        dir: TempDir,
        name: String,
    }

    impl Probe {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("a temp directory for the probe");
            // `which` accepts a name on Windows only when its extension is in
            // PATHEXT, and on Unix only when the executable bit is set.
            let name = format!("probe{}", std::env::consts::EXE_SUFFIX);
            let path = dir.path().join(&name);
            std::fs::write(&path, b"").expect("the probe is written");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                    .expect("the probe is executable");
            }
            Self { dir, name }
        }

        fn path(&self) -> PathBuf {
            self.dir.path().join(&self.name)
        }

        fn search_path(&self) -> String {
            self.dir.path().to_string_lossy().into_owned()
        }
    }

    /// The bug this guards against: a GUI application on macOS inherits a minimal
    /// `PATH`, so the native Claude Code installer's `~/.local/bin/claude` is
    /// invisible unless the user's shell environment is consulted first. Both
    /// halves matter — finding it through the supplied path proves nothing unless
    /// the process `PATH` alone demonstrably fails to find the same binary.
    #[test]
    fn locates_a_binary_that_the_process_path_cannot_see() {
        let probe = Probe::new();

        let found = locate_binary(&probe.name, Some(probe.search_path()), false)
            .expect("a binary in the supplied PATH must be found");
        // Canonicalised on both sides: the macOS temp directory is a symlink
        // (`/var/folders/...` onto `/private/var/folders/...`), and which end of
        // it comes back is not what this test is about.
        assert_eq!(
            found.canonicalize().ok(),
            probe.path().canonicalize().ok(),
            "a binary in the supplied PATH must be found where it was put"
        );

        assert_eq!(
            locate_binary(&probe.name, None, false),
            None,
            "the same binary must be invisible to the process PATH, or this test \
             proves nothing about reading the shell environment"
        );
    }

    #[test]
    fn empty_shell_path_falls_back_rather_than_reporting_missing() {
        // A login shell that fails to report a PATH must not be turned into a
        // "not installed" verdict.
        let probe = Probe::new();
        assert_eq!(
            locate_binary(&probe.name, Some(String::new()), false),
            None,
            "an empty shell PATH falls back to the process PATH, which cannot \
             reach a temp directory"
        );
        // The other half of the fallback: it has to actually search that PATH.
        // Windows ships no `sh`, and there is no name it is guaranteed to
        // resolve, so the claim is made where it can be made.
        assert!(locate_binary("sh", Some(String::new()), false).is_some() || cfg!(windows));
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
        assert!(builtin_agent(OPENCODE_AGENT_ID).is_some());
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

    /// The install line follows the host the CLI will run on, not the one this
    /// app runs on: a Windows app connected to a WSL distro offering
    /// `irm ... | iex` would hand a Linux shell a PowerShell command.
    #[test]
    fn install_command_follows_the_hosts_path_style() {
        let claude = builtin_agent(CLAUDE_CODE_AGENT_ID).expect("claude is a builtin");
        assert_eq!(
            claude.install_command_for(PathStyle::Posix),
            "curl -fsSL https://claude.ai/install.sh | bash"
        );
        assert_eq!(
            claude.install_command_for(PathStyle::Windows),
            "irm https://claude.ai/install.ps1 | iex"
        );
        assert_eq!(
            claude.install_command(),
            claude.install_command_for(PathStyle::local()),
            "a local project keeps the line it always had"
        );
    }

    /// What the remote said is input: a path that is not absolute on the
    /// remote's own terms is refused rather than handed to a terminal, and a
    /// missing CLI is described in the remote's terms whatever this host is.
    #[test]
    fn a_remote_answer_is_checked_by_the_remotes_path_style() {
        let claude = builtin_agent(CLAUDE_CODE_AGENT_ID).expect("claude is a builtin");
        assert_eq!(
            agent_binary_from_remote(
                claude,
                PathStyle::Posix,
                Some("/home/me/.local/bin/claude".into())
            )
            .ok(),
            Some(AgentBinary::Found(PathBuf::from(
                "/home/me/.local/bin/claude"
            )))
        );
        assert!(
            agent_binary_from_remote(claude, PathStyle::Posix, Some("bin/claude".into())).is_err(),
            "a relative path names nothing the terminal can run"
        );
        assert!(agent_binary_from_remote(claude, PathStyle::Posix, Some(String::new())).is_err());
        let Ok(AgentBinary::Missing(missing)) =
            agent_binary_from_remote(claude, PathStyle::Posix, None)
        else {
            panic!("no path is a missing CLI");
        };
        assert_eq!(missing.install_command, claude.install_unix);
    }

    /// The remote's path style picks the installer, whichever this host is: a
    /// Windows remote is offered PowerShell even from a Unix client.
    #[test]
    fn a_missing_cli_on_a_windows_remote_offers_the_windows_installer() {
        let claude = builtin_agent(CLAUDE_CODE_AGENT_ID).expect("claude is a builtin");
        let Ok(AgentBinary::Missing(missing)) =
            agent_binary_from_remote(claude, PathStyle::Windows, None)
        else {
            panic!("no path is a missing CLI");
        };
        assert_eq!(missing.install_command, claude.install_windows);
    }

    /// Executable scripts in a directory on nobody's `PATH`.
    #[cfg(unix)]
    fn scripts(files: &[(&str, &str)]) -> TempDir {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().expect("a temp directory for the scripts");
        for (name, body) in files {
            let path = dir.path().join(name);
            std::fs::write(&path, body).expect("the script is written");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("the script is executable");
        }
        dir
    }

    #[cfg(unix)]
    async fn probe_claude(
        store: &gpui::Entity<AgentServerStore>,
        binary: PathBuf,
        cx: &mut TestAppContext,
    ) -> BypassCheck {
        store
            .update(cx, |store, cx| {
                store.verify_bypass_flag(&AgentId::new(CLAUDE_CODE_AGENT_ID), binary, cx)
            })
            .await
    }

    /// An npm or nvm install is a `#!/usr/bin/env node` script. Probed with
    /// this process's `PATH`, `env` finds no interpreter, and a CLI that
    /// offers the flag would read as one that does not.
    #[cfg(unix)]
    #[gpui::test]
    async fn the_probe_runs_with_the_shell_path(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        init_test_settings(cx);
        let interpreter = "zode-test-interpreter";
        let bin = scripts(&[
            (
                interpreter,
                "#!/bin/sh\nprintf '  --dangerously-skip-permissions  Skip prompts\\n'\n",
            ),
            ("claude", &format!("#!/usr/bin/env {interpreter}\n")),
        ]);
        let store = create_agent_server_store(cx);
        store.update(cx, |store, _| {
            store.set_search_path_for_test(Some(bin.path().to_string_lossy().into_owned()))
        });

        let check = probe_claude(&store, bin.path().join("claude"), cx).await;

        assert_eq!(check, BypassCheck::Present);
    }

    /// A probe that never reached the CLI's help says nothing about the flag;
    /// `Absent` would tell the reader it had been renamed.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_failed_probe_with_no_help_is_unprobed(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        init_test_settings(cx);
        let bin = scripts(&[(
            "claude",
            "#!/bin/sh\necho 'env: node: No such file or directory' >&2\nexit 127\n",
        )]);
        let store = create_agent_server_store(cx);

        let check = probe_claude(&store, bin.path().join("claude"), cx).await;

        assert!(
            matches!(check, BypassCheck::Unprobed { .. }),
            "got {check:?}"
        );
        assert_eq!(
            store.read_with(cx, |store, _| store
                .cached_bypass_flag_for_test(&bin.path().join("claude"))),
            None,
            "no answer is not an answer to remember"
        );
    }

    /// A help page is read whatever the exit code: only a failure that printed
    /// no option at all is set aside.
    #[cfg(unix)]
    #[gpui::test]
    async fn a_help_page_that_exits_non_zero_is_still_read(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        init_test_settings(cx);
        let bin = scripts(&[(
            "claude",
            "#!/bin/sh\nprintf '  --some-other-flag  Something\\n'\nexit 1\n",
        )]);
        let store = create_agent_server_store(cx);
        let binary = bin.path().join("claude");

        let check = probe_claude(&store, binary.clone(), cx).await;

        assert_eq!(
            check,
            BypassCheck::Absent {
                flag: "--dangerously-skip-permissions",
                binary,
            }
        );
    }

    #[test]
    fn a_remote_probe_without_a_verdict_fails_closed() {
        use proto::probe_agent_bypass_flag_response::{Absent, Check, Present, Unprobed};
        let flag = "--dangerously-skip-permissions";
        let binary = PathBuf::from("/usr/bin/claude");
        assert_eq!(
            bypass_check_from_remote(flag, binary.clone(), Some(Check::Present(Present {}))),
            BypassCheck::Present
        );
        assert_eq!(
            bypass_check_from_remote(flag, binary.clone(), Some(Check::Absent(Absent {}))),
            BypassCheck::Absent {
                flag,
                binary: binary.clone()
            }
        );
        assert_eq!(
            bypass_check_from_remote(
                flag,
                binary.clone(),
                Some(Check::Unprobed(Unprobed {
                    reason: "timed out".into()
                }))
            ),
            BypassCheck::Unprobed {
                reason: "timed out".into()
            }
        );
        assert!(matches!(
            bypass_check_from_remote(flag, binary, None),
            BypassCheck::Unprobed { .. }
        ));
    }

    #[test]
    fn an_old_server_is_named_as_out_of_date() {
        let mapped = remote_agent_error(anyhow::anyhow!(
            "RPC request LocateAgentCli failed: no handler registered for LocateAgentCli"
        ));
        assert_eq!(
            mapped.to_string(),
            "the remote server does not support agent terminals yet; reconnect to update it"
        );
        let other = remote_agent_error(anyhow::anyhow!("connection lost"));
        assert_eq!(
            other.to_string(),
            "connection lost",
            "other errors pass through"
        );
    }

    #[test]
    fn windows_drive_mounts_are_dropped_from_a_wsl_path() {
        assert_eq!(
            without_windows_drive_mounts(
                "/home/me/.local/bin:/mnt/c/Users/me/AppData/Roaming/npm:/usr/bin:/mnt/d:/mnt/wsl/bin:/mnt/cd/bin"
            ),
            "/home/me/.local/bin:/usr/bin:/mnt/wsl/bin:/mnt/cd/bin",
            "only `/mnt/<drive letter>` entries are Windows drives; other mounts stay"
        );
        assert_eq!(without_windows_drive_mounts("/mnt/c/Windows"), "");
        assert_eq!(without_windows_drive_mounts(""), "");
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

    /// The sealed decision, made mechanical.
    ///
    /// Codex ships two auto-approve flags and they are not interchangeable: this
    /// one keeps the workspace-write sandbox, the other drops it and Codex's own
    /// help calls that EXTREMELY DANGEROUS. Pointing this mapping at the second
    /// would hand every Codex user the more dangerous posture from a toggle that
    /// says nothing about sandboxes.
    #[test]
    fn codex_maps_to_the_sandboxed_variant() {
        let codex = builtin_agent(CODEX_AGENT_ID).expect("codex is a builtin");
        assert_eq!(codex.bypass_flag, Some("--approve-for-me"));
        assert_ne!(
            codex.bypass_flag,
            Some("--dangerously-bypass-approvals-and-sandbox"),
            "the no-sandbox variant is deliberately not offered"
        );
    }

    /// opencode is the odd one out and must stay visibly so.
    ///
    /// `--auto` auto-approves everything *except* what the user explicitly
    /// denied, so a deny-list still holds under it. The other four drop the
    /// asking outright. Giving opencode one of their spellings would be a claim
    /// that the five postures are the same, which they are not.
    #[test]
    fn opencode_is_not_flattened_into_the_others_posture() {
        let opencode = builtin_agent(OPENCODE_AGENT_ID).expect("opencode is a builtin");
        assert_eq!(opencode.bypass_flag, Some("--auto"));
        for other in [
            CLAUDE_CODE_AGENT_ID,
            ANTIGRAVITY_AGENT_ID,
            COPILOT_AGENT_ID,
            CODEX_AGENT_ID,
        ] {
            assert_ne!(
                builtin_agent(other).and_then(|agent| agent.bypass_flag),
                opencode.bypass_flag,
                "`{other}` must not share opencode's flag"
            );
        }
    }

    /// A sixth agent cannot compile without a value, but it can compile with
    /// `None`. This is what stops `None` being used as "decide later".
    #[test]
    fn every_builtin_has_a_decided_posture() {
        for agent in BUILTIN_AGENTS {
            assert!(
                agent.bypass_flag.is_some(),
                "`{}` has no auto-approve posture recorded; if it genuinely has \
                 no such flag, say so here rather than leaving it undecided",
                agent.id
            );
        }
    }
}
