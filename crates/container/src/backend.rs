use async_trait::async_trait;
use futures::stream::BoxStream;

use crate::resource::{Resource, ResourceAction, ResourceKind};

/// Which engine is answering.
///
/// Docker and Podman share a command line and so share a backend; they are told
/// apart here only so the UI can label them, and so Podman can offer the one
/// kind Docker has no word for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Docker,
    Podman,
    Kubernetes,
}

impl BackendKind {
    pub fn label(self) -> &'static str {
        match self {
            BackendKind::Docker => "Docker",
            BackendKind::Podman => "Podman",
            BackendKind::Kubernetes => "Kubernetes",
        }
    }
}

/// Why a backend could not answer.
///
/// `EngineMissing` and `EngineNotRunning` are separate on purpose: "not
/// installed" and "installed but the daemon is down" need different words in
/// front of the user, and collapsing them into one error is how a panel ends up
/// telling somebody to install what they already have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerError {
    /// The command-line tool is not on `PATH`.
    EngineMissing { program: String },
    /// The tool is there, but it cannot reach its daemon or cluster.
    EngineNotRunning { program: String },
    /// The tool is there and usable, but nothing has been chosen for it to look
    /// at -- a kubeconfig with no context, say.
    ///
    /// Separate from `EngineNotRunning` because the two are cured differently
    /// and because `kubectl` cannot tell them apart on its own: with no context
    /// it falls back to `localhost:8080` and reports a refused connection, which
    /// would send somebody looking for a cluster that was never the problem.
    /// Docker never returns this.
    NotConfigured { detail: String },
    /// The tool answered, and the answer was "you may not".
    NotAuthorized { detail: String },
    /// Asked about something that is no longer there.
    NotFound { id: String },
    /// The command ran and failed for some other reason. `stderr` is the
    /// engine's own words and is untrusted text -- show it, never act on it.
    CommandFailed { program: String, stderr: String },
    /// The command succeeded and its output could not be read.
    Parse { detail: String },
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerError::EngineMissing { program } => {
                write!(f, "{program} is not installed")
            }
            ContainerError::EngineNotRunning { program } => {
                write!(f, "{program} is installed but not running")
            }
            ContainerError::NotConfigured { detail } => write!(f, "not configured: {detail}"),
            ContainerError::NotAuthorized { detail } => write!(f, "not authorized: {detail}"),
            ContainerError::NotFound { id } => write!(f, "no longer there: {id}"),
            ContainerError::CommandFailed { program, stderr } => {
                write!(f, "{program} failed: {stderr}")
            }
            ContainerError::Parse { detail } => write!(f, "could not read the output: {detail}"),
        }
    }
}

impl std::error::Error for ContainerError {}

/// Something the panel learned about a resource without asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendEvent {
    /// Something changed under this kind; the list for it is now stale.
    Stale { kind: ResourceKind },
}

/// One engine the panel can talk to.
///
/// `supported_kinds` and `supported_actions` are what keep this trait honest: a
/// Kubernetes pod is not a Docker container, and the view is written to ask what
/// a backend offers rather than to branch on which backend it is. The day a
/// `pod_phase` field appears on a Docker type, this seam has been breached.
#[async_trait]
pub trait ContainerBackend: Send + Sync {
    fn kind(&self) -> BackendKind;

    /// The kinds this engine has a word for. Docker has no pods; Kubernetes has
    /// no volumes in the sense this panel means.
    fn supported_kinds(&self) -> &'static [ResourceKind];

    /// What may be done to a resource of this kind. An image does not start; a
    /// pod does not restart.
    fn supported_actions(&self, kind: ResourceKind) -> &'static [ResourceAction];

    async fn list(&self, kind: ResourceKind) -> Result<Vec<Resource>, ContainerError>;

    /// Does `action` to the resource `id` of kind `kind`.
    ///
    /// The kind is a parameter and not inferred from the id, because the command
    /// differs by kind on the same engine: `podman start` and `podman pod start`
    /// are different verbs for the same word. Callers must ask
    /// `supported_actions(kind)` first; anything outside that answer is an error,
    /// never a silent success.
    async fn act(
        &self,
        kind: ResourceKind,
        action: ResourceAction,
        id: &str,
    ) -> Result<(), ContainerError>;

    /// A stream that says when a list went stale, if this engine has one.
    /// `None` means the panel must poll instead.
    fn watch(&self) -> Option<BoxStream<'static, BackendEvent>>;

    /// The command that follows this resource's log, if it has one.
    ///
    /// Handed back as a program and a list of arguments, never as one string:
    /// the id comes from the engine but the *name* inside it was chosen by
    /// whoever created the resource, and a name with a space or a semicolon in it
    /// must not be able to become two commands. The caller runs it in a terminal
    /// and never through a shell.
    ///
    /// The backend builds it because only the backend knows the syntax --
    /// `docker logs -f x` and `kubectl logs -f -n ns x` are the same intention in
    /// two vocabularies, and the view must not learn either.
    fn logs_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)>;

    /// Everything a prune of this scope would delete, listed one by one.
    ///
    /// `None` for an engine with no such notion. For Docker this is the whole
    /// reason [`crate::DestructivePlan`] exists: `docker system prune` has no
    /// `--dry-run`, so the only way to tell somebody what they are about to lose
    /// is to go and find out first.
    async fn prune_targets(
        &self,
        scope: crate::PruneScope,
    ) -> Option<Result<Vec<Resource>, ContainerError>>;

    /// Carries out a plan whose confirmation has already been given.
    ///
    /// Takes the plan and not a kind and an id, so there is no way to remove
    /// something that was never enumerated and shown.
    async fn destroy(&self, plan: &crate::DestructivePlan) -> Result<(), ContainerError>;

    /// The command that opens a shell inside this resource, if it has one.
    ///
    /// Same argument-splitting rule as [`Self::logs_command`], and it matters
    /// more here: this one is interactive and long-lived.
    fn exec_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)>;
}
