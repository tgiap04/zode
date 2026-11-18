//! The write side's vocabulary: what a caller may ask an engine to do, and the
//! checks every value passes before it can become part of a command.
//!
//! Names, references, keys and paths here come from people -- whoever created
//! the resource, or whoever typed into a form -- and end up as arguments of a
//! process run with the user's own privileges. Commands are always a program and
//! an argument list, so a space or a semicolon cannot split one into two; what
//! the checks below add is that a value cannot be read as an *option* either.

use crate::backend::ContainerError;
use crate::resource::{ComposeLabels, Resource, ResourceKind};

/// A write-side capability the view asks about before drawing its entry point,
/// so it never has to call something and read the refusal as "no".
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// The compose verbs and `compose down`. Whether the tool is actually
    /// installed is a separate, asynchronous question:
    /// [`crate::ContainerBackend::compose_available`].
    Compose,
    ImagePull,
    ImageTag,
    ImageHistory,
    ImageRun,
    VolumeCreate,
    NetworkCreate,
    /// Connecting and disconnecting a container to a network.
    NetworkConnect,
}

/// A compose verb that runs in a terminal so its output can stream. `down` is
/// not here: it loses things, so it goes through
/// [`crate::DestructivePlan::compose_down`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ComposeOp {
    Up,
    Restart,
    Stop,
    Start,
    Pull,
    Build,
    Logs,
}

/// Which compose project an operation is aimed at, and where its files are.
///
/// Not [`ComposeLabels`] itself: one value that says everything `docker
/// compose` needs, from either source -- a container's labels or a project
/// row of `docker compose ls`. Both carry the config files; a target that has
/// none is refused by the operations that need them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposeTarget {
    pub project: String,
    /// Passed as `--project-directory`.
    pub working_dir: Option<String>,
    /// Each passed as its own `-f`, in order.
    pub config_files: Vec<String>,
}

impl ComposeTarget {
    pub fn new(project: impl Into<String>) -> Self {
        Self {
            project: project.into(),
            ..Self::default()
        }
    }

    pub fn from_labels(labels: &ComposeLabels) -> Self {
        Self {
            project: labels.project.clone(),
            working_dir: labels.working_dir.clone(),
            config_files: labels.config_files.clone(),
        }
    }

    /// Whether the compose files are known.
    ///
    /// Without them `docker compose` looks for a default file in its working
    /// directory and reads a different model than the one that created the
    /// containers -- for `down --volumes` that is a different set of volumes
    /// removed than the plan listed. So an operation is refused rather than
    /// run against a guess.
    pub(crate) fn require_config_files(&self) -> Result<(), ContainerError> {
        if self.config_files.is_empty() {
            return Err(ContainerError::Invalid {
                detail: format!(
                    "the compose files of project {:?} are not known, and without them compose \
                     would read a different configuration",
                    self.project
                ),
            });
        }
        Ok(())
    }

    /// From a `ComposeProject` row, which carries the config files
    /// `docker compose ls` printed. `None` for any other kind.
    pub fn from_project(resource: &Resource) -> Option<Self> {
        if resource.kind != ResourceKind::ComposeProject {
            return None;
        }
        let config = resource
            .detail
            .iter()
            .find(|(label, _)| *label == "Config")
            .map(|(_, value)| value.as_str())
            .unwrap_or_default();
        Some(Self::new(resource.id.clone()).with_config_text(config))
    }

    pub fn with_working_dir(mut self, working_dir: Option<String>) -> Self {
        self.working_dir = working_dir.filter(|dir| !dir.is_empty());
        self
    }

    /// Config files as `docker compose ls` prints them: comma-separated.
    pub fn with_config_text(mut self, text: &str) -> Self {
        self.config_files = split_config_files(text);
        self
    }
}

/// `a.yml,b.yml` as the files it names. Compose joins them with a bare comma,
/// so a path with a comma in it cannot be told apart from two.
pub(crate) fn split_config_files(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|file| !file.is_empty())
        .map(String::from)
        .collect()
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum PortProtocol {
    #[default]
    Tcp,
    Udp,
}

impl PortProtocol {
    pub(crate) fn word(self) -> &'static str {
        match self {
            PortProtocol::Tcp => "tcp",
            PortProtocol::Udp => "udp",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PortMapping {
    pub host: u16,
    pub container: u16,
    pub protocol: PortProtocol,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeMount {
    /// A named volume or a host path.
    pub source: String,
    pub target: String,
    pub read_only: bool,
}

/// What `docker run` is asked to start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunSpec {
    pub image: String,
    pub name: Option<String>,
    pub ports: Vec<PortMapping>,
    pub env: Vec<(String, String)>,
    pub volumes: Vec<VolumeMount>,
    pub network: Option<String>,
    pub detach: bool,
    pub remove_on_exit: bool,
    /// Replaces the image's own command. Placed after the image, where the
    /// engine no longer reads options, so it is not validated.
    pub command: Vec<String>,
}

/// One row of an image's history, as the engine printed it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageLayer {
    pub created_by: String,
    pub size: String,
    pub created: String,
}

fn refuse(field: &str, reason: &str, value: &str) -> ContainerError {
    ContainerError::Invalid {
        detail: format!("{field} {reason}: {value:?}"),
    }
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

/// `[a-zA-Z0-9][a-zA-Z0-9_.-]*`: what an engine accepts as a container, volume,
/// network or compose project name, and so also the one shape that cannot start
/// with a dash.
pub(crate) fn check_name(field: &str, value: &str) -> Result<(), ContainerError> {
    let mut chars = value.chars();
    let starts_well = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric());
    if starts_well && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        Ok(())
    } else {
        Err(refuse(
            field,
            "must be letters, digits, `_`, `.` or `-`, starting with a letter or digit",
            value,
        ))
    }
}

/// An image reference (`repo/name:tag@sha256:..`): no whitespace, no control
/// characters, and not a dash first.
pub(crate) fn check_reference(field: &str, value: &str) -> Result<(), ContainerError> {
    if value.is_empty() {
        return Err(refuse(field, "is empty", value));
    }
    if value.starts_with('-') {
        return Err(refuse(field, "must not start with `-`", value));
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(refuse(field, "must not contain whitespace", value));
    }
    Ok(())
}

/// A file or directory handed to the engine as a value.
pub(crate) fn check_path(field: &str, value: &str) -> Result<(), ContainerError> {
    if value.is_empty() {
        return Err(refuse(field, "is empty", value));
    }
    if value.starts_with('-') {
        return Err(refuse(field, "must not start with `-`", value));
    }
    if has_control(value) {
        return Err(refuse(field, "must not contain control characters", value));
    }
    Ok(())
}

/// `[A-Za-z_][A-Za-z0-9_]*`.
pub(crate) fn check_env_key(value: &str) -> Result<(), ContainerError> {
    let mut chars = value.chars();
    let starts_well = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_');
    if starts_well && chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(())
    } else {
        Err(refuse(
            "environment variable name",
            "must be letters, digits and `_`, not starting with a digit",
            value,
        ))
    }
}

/// A label key: dotted, slashed names are normal (`com.example/owner`), `=` is
/// the separator and so is not allowed.
pub(crate) fn check_label_key(value: &str) -> Result<(), ContainerError> {
    let mut chars = value.chars();
    let starts_well = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric());
    if starts_well && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '/'))
    {
        Ok(())
    } else {
        Err(refuse(
            "label name",
            "must start with a letter or digit and hold no spaces or `=`",
            value,
        ))
    }
}

/// A free-form value that is joined to a key (`KEY=value`), so a leading dash
/// is harmless; only a control character, which no process accepts, is not.
pub(crate) fn check_text(field: &str, value: &str) -> Result<(), ContainerError> {
    if value.contains('\0') {
        Err(refuse(field, "must not contain a NUL", value))
    } else {
        Ok(())
    }
}

/// `10.5.0.0/16`, `fd00::/64`.
pub(crate) fn check_subnet(value: &str) -> Result<(), ContainerError> {
    let valid = value.contains('/')
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() || matches!(c, '.' | ':' | '/'));
    if valid {
        Ok(())
    } else {
        Err(refuse("subnet", "must look like `10.5.0.0/16`", value))
    }
}

/// Port 0 means "pick one" to the engine, which is not a mapping.
pub(crate) fn check_port(mapping: &PortMapping) -> Result<(), ContainerError> {
    if mapping.host == 0 || mapping.container == 0 {
        return Err(refuse(
            "port",
            "must be between 1 and 65535",
            &format!("{}:{}", mapping.host, mapping.container),
        ));
    }
    Ok(())
}

/// A bind source is a path; a volume source is a name. Either may be given, but
/// a `:` would split `source:target` in the wrong place -- except in a Windows
/// drive letter, which is how a bind source there starts.
pub(crate) fn check_mount(mount: &VolumeMount) -> Result<(), ContainerError> {
    check_path("volume source", &mount.source)?;
    check_path("volume target", &mount.target)?;
    let drive = matches!(
        mount.source.as_bytes(),
        [letter, b':', b'\\' | b'/', ..] if letter.is_ascii_alphabetic()
    );
    let source = if drive {
        mount.source.get(2..).unwrap_or_default()
    } else {
        mount.source.as_str()
    };
    if source.contains(':') {
        return Err(refuse(
            "volume source",
            "must not contain `:`",
            &mount.source,
        ));
    }
    if mount.target.contains(':') {
        return Err(refuse(
            "volume target",
            "must not contain `:`",
            &mount.target,
        ));
    }
    Ok(())
}
