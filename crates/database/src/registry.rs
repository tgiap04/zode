use crate::transport::DriverBinary;
use collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// What a driver is called on the wire and in settings. Stable across
/// versions -- a connection stored last month names its driver by this.
pub type DriverId = Arc<str>;

/// Everything Zode knows about a driver before it has spoken to it.
#[derive(Clone, Debug)]
pub struct DriverDescriptor {
    pub id: DriverId,
    /// Shown when choosing an engine for a new connection.
    pub name: String,
    pub state: DriverState,
    pub source: DriverSource,
}

impl DriverDescriptor {
    /// How to start this driver, or `None` when it is not on the machine.
    ///
    /// Every spawn goes through here. That it can answer `None` is the point:
    /// a registered driver used to be a runnable one by construction, which
    /// made "not installed" a state nothing could represent and the UI built
    /// for it unreachable.
    pub fn binary(&self) -> Option<&DriverBinary> {
        match &self.state {
            DriverState::Installed { binary, .. } => Some(binary),
            DriverState::NotInstalled => None,
        }
    }

    pub fn is_installed(&self) -> bool {
        matches!(self.state, DriverState::Installed { .. })
    }

    pub fn origin(&self) -> Option<DriverOrigin> {
        match &self.state {
            DriverState::Installed { origin, .. } => Some(*origin),
            DriverState::NotInstalled => None,
        }
    }
}

/// Whether a driver is actually on this machine.
#[derive(Clone, Debug)]
pub enum DriverState {
    Installed {
        binary: DriverBinary,
        origin: DriverOrigin,
    },
    /// Zode knows this driver by name, but it has not been downloaded.
    NotInstalled,
}

impl DriverState {
    /// The common case: a driver found at a path, with no arguments or
    /// environment of its own.
    pub fn installed(executable: PathBuf, origin: DriverOrigin) -> Self {
        Self::Installed {
            binary: DriverBinary {
                executable,
                args: Vec::new(),
                env: std::collections::HashMap::new(),
            },
            origin,
        }
    }
}

/// Where the binary being run came from.
///
/// Carried so that a failure can say which of the three a driver was started
/// from. A driver misbehaving because it is a stale build beside the executable
/// looks identical, from the error alone, to one that downloaded badly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverOrigin {
    /// Beside the running executable: a development build, or a bundle.
    BesideExecutable,
    /// Downloaded, under `paths::database_drivers_dir()`.
    Store,
    /// A bare name left to `PATH`, put there deliberately by someone.
    Path,
    /// Declared by an extension, which names its own path.
    Extension,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverSource {
    /// Shipped with Zode.
    BuiltIn,
    /// Declared by an extension's `database_drivers` manifest entry.
    Extension,
}

/// Which drivers exist.
///
/// Built-ins are registered at startup; extensions add to the same map later
/// (see the `database_drivers` manifest entry). Kept apart from `DriverClient`
/// so that "which drivers exist" and "talking to one" never share a lifetime --
/// the registry outlives every connection made through it.
#[derive(Default)]
pub struct DriverRegistry {
    drivers: HashMap<DriverId, DriverDescriptor>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_built_in(
        &mut self,
        id: impl Into<DriverId>,
        name: impl Into<String>,
        state: DriverState,
    ) {
        let id = id.into();
        self.drivers.insert(
            id.clone(),
            DriverDescriptor {
                id,
                name: name.into(),
                state,
                source: DriverSource::BuiltIn,
            },
        );
    }

    /// Replaces one driver's state after it has been installed.
    ///
    /// Returns whether anything changed, so a caller can skip notifying every
    /// window over a re-resolve that found what it already had.
    pub fn set_state(&mut self, id: &str, state: DriverState) -> bool {
        let Some(driver) = self.drivers.get_mut(id) else {
            return false;
        };
        let changed = driver.is_installed() != matches!(state, DriverState::Installed { .. });
        driver.state = state;
        changed
    }

    /// Adds a driver an extension declared.
    ///
    /// A built-in wins a name clash and the extension's is dropped with a
    /// warning: the shipped driver is the one whose behaviour every stored
    /// connection was made against, and silently swapping it for a third
    /// party's would change what `postgres` means underneath the user.
    pub fn register_extension(&mut self, descriptor: DriverDescriptor) -> bool {
        if let Some(existing) = self.drivers.get(&descriptor.id)
            && existing.source == DriverSource::BuiltIn
        {
            log::warn!(
                "an extension declares database driver `{}`, which is built in; keeping the built-in",
                descriptor.id
            );
            return false;
        }
        self.drivers.insert(descriptor.id.clone(), descriptor);
        true
    }

    /// Drops a driver an extension declared, when that extension is removed.
    ///
    /// A built-in of the same name stays: it is there because Zode ships it,
    /// not because the extension asked, and uninstalling an extension that
    /// tried and failed to claim the name must not take the real one with it.
    pub fn unregister_extension(&mut self, id: &str) {
        if self
            .drivers
            .get(id)
            .is_some_and(|driver| driver.source == DriverSource::Extension)
        {
            self.drivers.remove(id);
        }
    }

    pub fn get(&self, id: &str) -> Option<&DriverDescriptor> {
        self.drivers.get(id)
    }

    /// Every driver, sorted by display name so the engine picker does not
    /// reshuffle itself between launches.
    pub fn all(&self) -> Vec<&DriverDescriptor> {
        let mut drivers: Vec<_> = self.drivers.values().collect();
        drivers.sort_by(|a, b| a.name.cmp(&b.name));
        drivers
    }

    pub fn is_empty(&self) -> bool {
        self.drivers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(id: &str, source: DriverSource) -> DriverDescriptor {
        DriverDescriptor {
            id: id.into(),
            name: id.to_string(),
            state: DriverState::installed(PathBuf::from(id), DriverOrigin::Extension),
            source,
        }
    }

    /// A shipped driver is what every stored connection was made against.
    /// Letting an extension take its name over would change what `postgres`
    /// means without anything on screen saying so.
    #[test]
    fn a_built_in_driver_survives_an_extension_claiming_its_name() {
        let mut registry = DriverRegistry::new();
        registry.register_built_in(
            "postgres",
            "PostgreSQL",
            DriverState::installed(
                PathBuf::from("zode-db-postgres"),
                DriverOrigin::BesideExecutable,
            ),
        );

        let accepted = registry.register_extension(descriptor("postgres", DriverSource::Extension));

        assert!(!accepted, "the extension's claim must be refused");
        assert_eq!(
            registry.get("postgres").map(|driver| driver.source),
            Some(DriverSource::BuiltIn),
        );
    }

    /// The state the whole on-demand path hangs off. Before it existed a
    /// registered driver was a runnable one by construction, so `installed`
    /// was true for every driver Zode had heard of and the UI written for the
    /// other case could not be reached.
    #[test]
    fn a_driver_that_is_not_on_the_machine_hands_back_no_binary() {
        let mut registry = DriverRegistry::new();
        registry.register_built_in("mongodb", "MongoDB", DriverState::NotInstalled);

        let driver = registry.get("mongodb").expect("still a driver Zode knows");
        assert!(!driver.is_installed());
        assert!(
            driver.binary().is_none(),
            "a driver that is not installed must not hand back something to spawn"
        );
    }

    /// What the modal does the moment a download finishes: the driver it
    /// already knew about becomes one it can start.
    #[test]
    fn installing_a_driver_reports_the_change() {
        let mut registry = DriverRegistry::new();
        registry.register_built_in("mongodb", "MongoDB", DriverState::NotInstalled);

        let changed = registry.set_state(
            "mongodb",
            DriverState::installed(PathBuf::from("/store/zode-db-mongodb"), DriverOrigin::Store),
        );

        assert!(
            changed,
            "not-installed to installed is a change worth notifying over"
        );
        assert!(
            registry
                .get("mongodb")
                .is_some_and(|driver| driver.is_installed())
        );
        assert_eq!(
            registry.get("mongodb").and_then(|driver| driver.origin()),
            Some(DriverOrigin::Store)
        );
    }

    #[test]
    fn an_extension_may_add_a_driver_zode_does_not_ship() {
        let mut registry = DriverRegistry::new();
        assert!(registry.register_extension(descriptor("duckdb", DriverSource::Extension)));
        assert_eq!(
            registry.get("duckdb").map(|driver| driver.source),
            Some(DriverSource::Extension),
        );
    }
}
