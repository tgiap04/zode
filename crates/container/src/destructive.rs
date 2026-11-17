//! Removing things, and the one rule that makes it safe.
//!
//! A [`DestructivePlan`] cannot be built without a list of what will actually be
//! lost. That is a structural guarantee rather than a convention: no enumeration,
//! no plan; no plan, no confirmation dialog; no dialog, nothing removed. A future
//! caller cannot skip the listing step by forgetting to, because there is no
//! constructor that lets them.
//!
//! The specific thing being guarded against: `docker system prune` has **no
//! `--dry-run`**. There is no way to ask Docker what it is about to delete, so
//! the only honest confirmation is one built from lists this crate gathered
//! itself.

use crate::operations::ComposeTarget;
use crate::resource::{Resource, ResourceKind};

/// What a person has to type, on top of the name, to remove something that is
/// still in use.
pub const FORCE_CONFIRMATION: &str = "force";

/// The containers that still depend on one removal target: mount it, sit on
/// it, or were created from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependents {
    pub target_id: String,
    pub containers: Vec<Resource>,
}

/// How far a prune reaches.
///
/// `Volumes` is separate and not a boolean field on the plan, so that reading a
/// call site tells you whether data can be lost. `docker system prune --volumes`
/// deletes volumes no *running* container uses -- and a database that is merely
/// stopped has exactly such a volume. That is how people lose databases.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum PruneScope {
    /// Dangling images, stopped containers, unused networks. Reclaims space;
    /// loses nothing that was holding data.
    #[default]
    Reclaimable,
    /// The above, plus unused volumes. **Can destroy real data.**
    IncludingVolumes,
}

impl PruneScope {
    pub fn includes_volumes(self) -> bool {
        self == PruneScope::IncludingVolumes
    }
}

/// What a destructive action will do, with the losses already enumerated.
///
/// Construct only through [`DestructivePlan::remove`] or
/// [`DestructivePlan::prune`]. Both require the targets up front; there is no
/// way to describe a removal without listing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestructivePlan {
    /// Each thing that will be lost, as the engine listed it. Not a description
    /// -- the actual rows, so the dialog shows what the panel showed.
    targets: Vec<Resource>,
    /// What the person has to type, exactly.
    confirmation: String,
    /// A sentence about consequences, when there are consequences beyond the
    /// targets themselves.
    warning: Option<String>,
    intent: Intent,
    /// What depends on the targets, as far as the engine could say.
    dependents: Vec<Dependents>,
    /// Set only through [`DestructivePlan::forced`].
    force: bool,
    /// Which project a `compose down` is aimed at.
    compose: Option<ComposeTarget>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Remove {
        kind: ResourceKind,
    },
    Prune {
        scope: PruneScope,
    },
    /// `compose down`: the project's containers and networks, and its volumes
    /// only when `with_volumes`.
    ComposeDown {
        with_volumes: bool,
    },
}

impl DestructivePlan {
    /// Removing named things.
    ///
    /// The confirmation is the resource's own name, because typing it is what
    /// makes somebody read *which* one they are removing. A count would not.
    /// Returns `None` for an empty selection: there is nothing to confirm.
    pub fn remove(kind: ResourceKind, targets: Vec<Resource>) -> Option<Self> {
        let first = targets.first()?;
        let confirmation = if targets.len() == 1 {
            first.name.clone()
        } else {
            // Several at once: a single name would be misleading about the rest,
            // so the count is what must be typed -- alongside the full list.
            targets.len().to_string()
        };
        Some(Self {
            confirmation,
            warning: (kind == ResourceKind::Volume)
                .then(|| "A volume holds data. Removing it cannot be undone.".to_string()),
            targets,
            intent: Intent::Remove { kind },
            dependents: Vec::new(),
            force: false,
            compose: None,
        })
    }

    /// Taking a compose project down.
    ///
    /// `volumes` is `Some` exactly when `--volumes` was chosen, and then holds
    /// the project's volumes -- so the flag and the list that justifies it
    /// cannot disagree. `None` when there is nothing at all to remove.
    pub fn compose_down(
        project: ComposeTarget,
        containers: Vec<Resource>,
        networks: Vec<Resource>,
        volumes: Option<Vec<Resource>>,
    ) -> Option<Self> {
        let with_volumes = volumes.is_some();
        let mut targets = containers;
        targets.extend(networks);
        targets.extend(volumes.unwrap_or_default());
        if targets.is_empty() {
            return None;
        }
        let warning = if with_volumes {
            "This removes the project's volumes with it. Their data cannot be recovered."
        } else {
            "Containers and networks are removed. Volumes are kept."
        };
        Some(Self {
            confirmation: project.project.clone(),
            warning: Some(warning.to_string()),
            targets,
            intent: Intent::ComposeDown { with_volumes },
            dependents: Vec::new(),
            force: false,
            compose: Some(project),
        })
    }

    /// The same plan, saying what still depends on its targets. Whoever builds
    /// the plan has asked the engine; this only carries the answer to the
    /// dialog.
    pub fn with_dependents(mut self, dependents: Vec<Dependents>) -> Self {
        self.dependents = dependents;
        self
    }

    /// The same removal, forced: the engine is told to remove what is in use.
    ///
    /// Takes what was typed and gives nothing back unless it is exactly
    /// [`FORCE_CONFIRMATION`], so a forced plan cannot exist without a second,
    /// deliberate confirmation. Only a removal can be forced; anything else
    /// is `None`.
    ///
    /// The warning it sets is engine-neutral, and so says little. Prefer
    /// [`Self::forced_with_effect`] with
    /// [`crate::ContainerBackend::force_effect`]: what `--force` does differs
    /// by engine and by kind.
    pub fn forced(self, typed: &str) -> Option<Self> {
        self.forced_with_effect(
            typed,
            "Forced: the engine removes this even where it is in use, and anything \
             depending on it loses it.",
        )
    }

    /// [`Self::forced`], with the warning the dialog shows being `effect`: the
    /// engine's own account of what forcing this kind does, from
    /// [`crate::ContainerBackend::force_effect`].
    pub fn forced_with_effect(mut self, typed: &str, effect: &str) -> Option<Self> {
        if typed.trim() != FORCE_CONFIRMATION || !matches!(self.intent, Intent::Remove { .. }) {
            return None;
        }
        self.force = true;
        self.warning = Some(effect.to_string());
        Some(self)
    }

    /// Pruning: everything the engine considers unused.
    ///
    /// `targets` must be the list this crate gathered, never a guess. Empty is
    /// `None`: a prune with nothing to prune should not open a dialog.
    pub fn prune(scope: PruneScope, targets: Vec<Resource>) -> Option<Self> {
        if targets.is_empty() {
            return None;
        }
        Some(Self {
            targets,
            // Nothing here has one name to type, so the word itself is the
            // confirmation -- paired with the enumerated list above it, which is
            // the part that actually informs.
            confirmation: "prune".to_string(),
            warning: scope.includes_volumes().then(|| {
                "This includes volumes. A stopped database's volume counts as \
                 unused, and its data will be gone."
                    .to_string()
            }),
            intent: Intent::Prune { scope },
            dependents: Vec::new(),
            force: false,
            compose: None,
        })
    }

    pub fn targets(&self) -> &[Resource] {
        &self.targets
    }

    pub fn confirmation(&self) -> &str {
        &self.confirmation
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    pub fn intent(&self) -> Intent {
        self.intent
    }

    pub fn dependents(&self) -> &[Dependents] {
        &self.dependents
    }

    /// Whether anything is still using a target -- the cue to offer
    /// [`Self::forced`].
    pub fn has_dependents(&self) -> bool {
        self.dependents
            .iter()
            .any(|entry| !entry.containers.is_empty())
    }

    pub fn is_forced(&self) -> bool {
        self.force
    }

    pub fn compose_target(&self) -> Option<&ComposeTarget> {
        self.compose.as_ref()
    }

    /// Whether what was typed matches.
    ///
    /// Exact, including case: this is the last gate before something is gone,
    /// and a forgiving comparison here buys convenience with the one thing that
    /// must not be convenient. Surrounding whitespace is trimmed, because a
    /// trailing space is a typing artefact and not a different answer.
    pub fn is_confirmed_by(&self, typed: &str) -> bool {
        typed.trim() == self.confirmation
    }
}
