/// A kind of thing an engine can list.
///
/// Deliberately not a flat union of every engine's vocabulary: each backend
/// declares which of these it answers for through
/// [`crate::backend::ContainerBackend::supported_kinds`], and the view asks that
/// rather than asking which engine it is holding.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Container,
    Image,
    Volume,
    Network,
    ComposeProject,
    /// Podman's pod, and Kubernetes' pod. The same word for two different
    /// things, which is why nothing but the backend interprets it.
    Pod,
}

impl ResourceKind {
    /// What the group is called in the tree.
    pub fn label(self) -> &'static str {
        match self {
            ResourceKind::Container => "Containers",
            ResourceKind::Image => "Images",
            ResourceKind::Volume => "Volumes",
            ResourceKind::Network => "Networks",
            ResourceKind::ComposeProject => "Compose",
            ResourceKind::Pod => "Pods",
        }
    }
}

/// Whether a resource is up, and how confidently we know.
///
/// `Unknown` is not a failure: `docker images` has no running state to report,
/// and inventing "stopped" for an image would put a misleading dot beside it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RunState {
    Running,
    /// Up, but frozen: every process inside is suspended and the container can
    /// be resumed exactly where it was.
    ///
    /// Its own state rather than a flavour of `Running` or `Stopped`, because it
    /// is the only thing that decides which button a row gets: a paused
    /// container resumes, it does not start. Docker reports it as
    /// `State: "paused"` -- observed on 29.4.3, where `Status` also gains a
    /// "(Paused)" suffix that is text and never parsed.
    Paused,
    Stopped,
    Unknown,
}

/// One row in the panel.
///
/// `detail` carries the engine's own words for the columns this kind has --
/// image and ports for a container, tag and size for an image -- rather than a
/// struct per kind. The panel renders them as text and never interprets them,
/// so a new column costs a string, not a type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    pub kind: ResourceKind,
    /// Stable handle the engine accepts back in `act`.
    pub id: String,
    /// What a person calls it.
    pub name: String,
    pub state: RunState,
    /// Ordered label/value pairs, already formatted by the backend.
    pub detail: Vec<(&'static str, String)>,
    /// The resource this one sits under, if any -- a container inside a pod, a
    /// service inside a compose project.
    pub parent: Option<String>,
    /// The compose labels the engine stamped on this container, if it has any.
    pub compose: Option<ComposeLabels>,
}

/// What `docker compose` records on every container it creates.
///
/// Read from `com.docker.compose.*` labels and carried as data so the view can
/// group containers by project and find the checkout they were built from.
/// Podman's `podman-compose` writes the same labels, which is the only reason
/// this is not Docker-specific.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposeLabels {
    pub project: String,
    pub service: String,
    /// The directory `docker compose up` was run from, on the machine that ran
    /// it. `None` when the container carries compose labels but not this one.
    pub working_dir: Option<String>,
}

impl ComposeLabels {
    /// Whether `other` was brought up by the same compose project from the same
    /// directory -- two projects of one name in different directories are not
    /// the same checkout.
    pub fn same_project(&self, other: &ComposeLabels) -> bool {
        self.project == other.project && self.working_dir == other.working_dir
    }
}

/// Something the panel can ask an engine to do, without losing anything.
///
/// Removal is deliberately **not** here. It goes through
/// [`crate::DestructivePlan`], which cannot be built without a list of what will
/// be lost -- so there is no window in which a removal exists and its gate does
/// not. Adding a `Remove` variant here would open exactly that window.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ResourceAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
}

impl ResourceAction {
    pub fn label(self) -> &'static str {
        match self {
            ResourceAction::Start => "Start",
            ResourceAction::Stop => "Stop",
            ResourceAction::Restart => "Restart",
            ResourceAction::Pause => "Pause",
            // "Resume" rather than "Unpause": it is the word for what the person
            // wants, and the engine's own verb is nobody's business but the
            // backend's.
            ResourceAction::Unpause => "Resume",
        }
    }
}

/// Whether a kind can be removed at all.
///
/// Read by the view to decide whether to draw the button that *starts* the
/// confirmation flow. It never runs anything itself.
pub fn is_removable(kind: ResourceKind) -> bool {
    !matches!(kind, ResourceKind::ComposeProject)
}
