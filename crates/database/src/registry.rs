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
    pub binary: DriverBinary,
    pub source: DriverSource,
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
        executable: PathBuf,
    ) {
        let id = id.into();
        self.drivers.insert(
            id.clone(),
            DriverDescriptor {
                id,
                name: name.into(),
                binary: DriverBinary {
                    executable,
                    args: Vec::new(),
                    env: HashMap::default().into_iter().collect(),
                },
                source: DriverSource::BuiltIn,
            },
        );
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
            binary: DriverBinary {
                executable: PathBuf::from(id),
                args: Vec::new(),
                env: Default::default(),
            },
            source,
        }
    }

    /// A shipped driver is what every stored connection was made against.
    /// Letting an extension take its name over would change what `postgres`
    /// means without anything on screen saying so.
    #[test]
    fn a_built_in_driver_survives_an_extension_claiming_its_name() {
        let mut registry = DriverRegistry::new();
        registry.register_built_in("postgres", "PostgreSQL", PathBuf::from("zode-db-postgres"));

        let accepted = registry.register_extension(descriptor("postgres", DriverSource::Extension));

        assert!(!accepted, "the extension's claim must be refused");
        assert_eq!(
            registry.get("postgres").map(|driver| driver.source),
            Some(DriverSource::BuiltIn),
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
