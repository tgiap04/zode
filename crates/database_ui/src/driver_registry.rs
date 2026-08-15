use database::registry::{DriverDescriptor, DriverRegistry, DriverSource};
use database::transport::DriverBinary;
use extension::{ExtensionDatabaseDriver, ExtensionDatabaseDriverProxy, ExtensionHostProxy};
use gpui::{App, AppContext as _, Entity, Global, SharedString};
use std::path::PathBuf;
use std::sync::Arc;

/// The drivers Zode ships.
///
/// Hard-coded rather than discovered: each is a binary built from this
/// repository, and a driver that is *supposed* to be there but is missing
/// should say so plainly rather than quietly not existing. Extensions add to
/// this same registry -- see `DriverRegistry::register_extension`.
const BUILT_IN: &[(&str, &str, &str)] = &[
    ("sqlite", "SQLite", "zode-db-sqlite"),
    ("postgres", "PostgreSQL", "zode-db-postgres"),
    ("mysql", "MySQL", "zode-db-mysql"),
];

/// Where a driver binary lives.
///
/// Beside the running executable, which is true both of an installed app
/// bundle and of `target/debug` during development -- so there is no separate
/// "am I in a checkout" path to get wrong. Falls back to the bare name, which
/// lets `PATH` answer for anyone who put one there deliberately.
fn driver_path(executable: &str) -> PathBuf {
    let executable = if cfg!(windows) {
        format!("{executable}.exe")
    } else {
        executable.to_string()
    };

    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&executable)))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from(executable))
}

pub fn built_in_drivers() -> DriverRegistry {
    let mut registry = DriverRegistry::new();
    for (id, name, executable) in BUILT_IN {
        registry.register_built_in(*id, *name, driver_path(executable));
    }
    registry
}

/// An engine someone might want to connect to, whether or not a driver for it
/// is installed.
///
/// Separate from the registry on purpose. The registry answers "what can I
/// talk to"; this answers "what is there", which is the question the picker
/// asks -- and an engine missing from the picker is one nobody can discover is
/// missing. Listing an uninstalled engine and saying so beats pretending it
/// does not exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogueEntry {
    /// The driver that speaks to this engine. Several engines share one: a
    /// wire protocol is what a driver implements, and CockroachDB's is
    /// PostgreSQL's.
    pub driver: SharedString,
    pub name: SharedString,
    pub description: SharedString,
    pub group: SharedString,
    /// Whether that driver is actually in the registry.
    pub installed: bool,
}

/// The engines Zode knows the names of.
///
/// This list and `BUILT_IN` are the only two places in this crate that may name
/// an engine -- and this one names them as *data* shown to a person, never as a
/// branch in behaviour. Everything a connection does still comes from whatever
/// its driver answers.
///
/// The aliases are not a convenience: CockroachDB and PGlite speak the
/// PostgreSQL wire protocol and MariaDB speaks MySQL's, so the shipped drivers
/// genuinely serve them. Oracle and SQL Server do not share a protocol with
/// anything here, so they are listed as what they are -- absent, and waiting
/// for an extension.
const CATALOGUE: &[(&str, &str, &str, &str)] = &[
    (
        "postgres",
        "CockroachDB",
        "Distributed SQL, PostgreSQL-compatible",
        "Relational",
    ),
    (
        "mysql",
        "MariaDB",
        "Open-source fork of MySQL",
        "Relational",
    ),
    (
        "mysql",
        "MySQL",
        "Most popular open-source SQL database",
        "Relational",
    ),
    (
        "oracle",
        "Oracle",
        "Enterprise SQL with PL/SQL",
        "Relational",
    ),
    (
        "postgres",
        "PGlite",
        "Embedded WASM Postgres over a socket server",
        "Relational",
    ),
    (
        "postgres",
        "PostgreSQL",
        "Advanced object-relational SQL",
        "Relational",
    ),
    (
        "sqlserver",
        "SQL Server",
        "Microsoft's enterprise SQL database",
        "Relational",
    ),
    (
        "sqlite",
        "SQLite",
        "A whole database in a single file",
        "Relational",
    ),
];

/// Every engine, with the driver each one needs and whether it is here.
///
/// A driver an extension added that no catalogue row names is appended: an
/// engine Zode has never heard of must still be reachable, or the plugin
/// architecture only works for engines someone already thought of.
pub fn catalogue(cx: &mut App) -> Vec<CatalogueEntry> {
    let registry = global(cx);
    let registry = registry.read(cx);

    let mut entries: Vec<CatalogueEntry> = CATALOGUE
        .iter()
        .map(|(driver, name, description, group)| CatalogueEntry {
            driver: SharedString::new_static(driver),
            name: SharedString::new_static(name),
            description: SharedString::new_static(description),
            group: SharedString::new_static(group),
            installed: registry.get(driver).is_some(),
        })
        .collect();

    for driver in registry.all() {
        if entries
            .iter()
            .any(|entry| entry.driver == driver.id.as_ref())
        {
            continue;
        }
        entries.push(CatalogueEntry {
            driver: driver.id.to_string().into(),
            name: driver.name.clone().into(),
            description: SharedString::new_static("Provided by an extension"),
            group: SharedString::new_static("Relational"),
            installed: true,
        });
    }
    entries
}

struct GlobalDriverRegistry(Entity<DriverRegistry>);

impl Global for GlobalDriverRegistry {}

/// The one registry, shared by every window.
///
/// One rather than one per panel because extensions add to it: a driver
/// installed while a second window is open must be usable from both, and a
/// per-panel copy would leave whichever panel was built first speaking for an
/// older list.
///
/// Fills itself with the built-ins on first ask, so a panel built in a test
/// that never called [`init`] still finds the shipped drivers.
pub fn global(cx: &mut App) -> Entity<DriverRegistry> {
    if !cx.has_global::<GlobalDriverRegistry>() {
        let registry = cx.new(|_| built_in_drivers());
        cx.set_global(GlobalDriverRegistry(registry));
    }
    cx.global::<GlobalDriverRegistry>().0.clone()
}

/// Starts listening for drivers declared by extensions.
pub fn init(cx: &mut App) {
    global(cx);
    ExtensionHostProxy::default_global(cx).register_database_driver_proxy(ExtensionDrivers);
}

struct ExtensionDrivers;

impl ExtensionDatabaseDriverProxy for ExtensionDrivers {
    fn register_database_driver(&self, driver: ExtensionDatabaseDriver, cx: &mut App) {
        let descriptor = DriverDescriptor {
            id: driver.id,
            name: driver.name,
            binary: DriverBinary {
                executable: driver.executable,
                args: driver.args,
                env: driver.env.into_iter().collect(),
            },
            source: DriverSource::Extension,
        };
        global(cx).update(cx, |registry, _cx| {
            registry.register_extension(descriptor);
        });
    }

    fn unregister_database_driver(&self, driver_id: Arc<str>, cx: &mut App) {
        global(cx).update(cx, |registry, _cx| {
            registry.unregister_extension(&driver_id);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(id: &str) -> ExtensionDatabaseDriver {
        ExtensionDatabaseDriver {
            id: id.into(),
            name: id.to_string(),
            executable: PathBuf::from("/extensions").join(id),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    /// The whole point of the manifest entry: a driver Zode does not ship must
    /// become one the tree can open, without Zode being rebuilt.
    #[gpui::test]
    fn a_driver_an_extension_declares_becomes_one_the_panel_can_open(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            init(cx);
            let proxy = ExtensionHostProxy::default_global(cx);

            proxy.register_database_driver(declared("duckdb"), cx);
            let registry = global(cx);
            assert!(
                registry.read(cx).get("duckdb").is_some(),
                "an extension's driver must reach the registry the panel reads"
            );

            proxy.unregister_database_driver("duckdb".into(), cx);
            let registry = global(cx);
            assert!(
                registry.read(cx).get("duckdb").is_none(),
                "and must leave it when the extension does"
            );
        });
    }

    /// Asserted through the proxy rather than only on the registry: this is the
    /// path an installed extension actually takes, and a stored connection
    /// naming a shipped driver must keep meaning the shipped one.
    #[gpui::test]
    fn an_extension_cannot_take_over_a_shipped_driver(cx: &mut gpui::TestAppContext) {
        let claimed = BUILT_IN[0].0;
        cx.update(|cx| {
            init(cx);
            let proxy = ExtensionHostProxy::default_global(cx);
            proxy.register_database_driver(declared(claimed), cx);

            let registry = global(cx);
            let driver = registry.read(cx);
            let driver = driver.get(claimed).expect("the built-in is still there");
            assert_eq!(driver.source, DriverSource::BuiltIn);
        });
    }

    /// Uninstalling an extension that tried and failed to claim a shipped name
    /// must not take the shipped driver down with it.
    #[gpui::test]
    fn removing_such_an_extension_leaves_the_shipped_driver(cx: &mut gpui::TestAppContext) {
        let claimed = BUILT_IN[0].0;
        cx.update(|cx| {
            init(cx);
            let proxy = ExtensionHostProxy::default_global(cx);
            proxy.register_database_driver(declared(claimed), cx);
            proxy.unregister_database_driver(claimed.into(), cx);

            let registry = global(cx);
            assert!(
                registry.read(cx).get(claimed).is_some(),
                "`{claimed}` is shipped, so nothing an extension does may remove it"
            );
        });
    }

    /// Every id here is one a settings file may name, and one the tree will
    /// look up on click. A duplicate would make one of them unreachable.
    #[test]
    fn the_built_in_drivers_are_distinct_and_all_registered() {
        let registry = built_in_drivers();
        for (id, _, _) in BUILT_IN {
            assert!(
                registry.get(id).is_some(),
                "`{id}` is listed as built in but did not register"
            );
        }
        assert_eq!(
            registry.all().len(),
            BUILT_IN.len(),
            "two built-in drivers share an id, so one of them is unreachable"
        );
    }
}
