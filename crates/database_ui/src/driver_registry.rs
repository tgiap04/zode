use database::install::{DriverInstaller, ReleaseCoordinates, store};
use database::registry::{
    DriverDescriptor, DriverOrigin, DriverRegistry, DriverSource, DriverState,
};
use database::transport::DriverBinary;
use extension::{ExtensionDatabaseDriver, ExtensionDatabaseDriverProxy, ExtensionHostProxy};
use gpui::{App, AppContext as _, Entity, Global, SharedString};
use release_channel::{AppVersion, RELEASE_REPO};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The drivers Zode knows how to fetch.
///
/// Hard-coded rather than discovered: each is a binary built from this
/// repository and published with the release, and a driver that is *supposed*
/// to be fetchable but is missing should say so plainly rather than quietly not
/// existing. Extensions add to this same registry -- see
/// `DriverRegistry::register_extension`.
///
/// Being listed here no longer means being present. Zode bundles none of these:
/// each arrives when the engine it speaks for is first connected to, so an
/// install carries only the drivers its user actually asked for. The binary
/// name each one has is `store::executable_name`, which is also the name the
/// release publishes -- one spelling, not two.
const BUILT_IN: &[(&str, &str)] = &[
    ("sqlite", "SQLite"),
    ("postgres", "PostgreSQL"),
    ("mysql", "MySQL"),
    ("mongodb", "MongoDB"),
];

/// Where a driver binary lives, if it lives anywhere.
///
/// Four places, in order, and the fourth is the point:
///
/// 1. **Beside the running executable** -- true of a development build and of
///    any bundle that still carries one. First deliberately: a checkout that has
///    run `make drivers` must use the driver it just built, never a downloaded
///    one from an older release. It is also the rollback: putting the drivers
///    back into the bundle restores the old behaviour with no code change.
/// 2. **The download store**, under `paths::database_drivers_dir()`, keyed by
///    the running app's version. This is where a driver fetched on demand lands,
///    and where someone with no route to GitHub can put one by hand.
/// 3. **A bare name on `PATH`**, for anyone who put a driver there deliberately.
/// 4. **Nowhere** -- [`DriverState::NotInstalled`].
///
/// That fourth answer did not exist before. `driver_path` always returned a
/// `PathBuf`, so `registry.get(id).is_some()` was always true, so every driver
/// read as installed and the "not installed" path through the UI was
/// unreachable code. Worse, the path it returned was a bare name handed to a
/// *shell* (`StdioTransport::new` builds the command through `ShellBuilder`):
/// `spawn` succeeded, the shell wrote `command not found` to what the client
/// read as the driver's stderr, and the reconnect loop repeated it. The one
/// fact worth knowing -- the driver was never installed -- appeared nowhere,
/// and the message that did appear looked like the driver talking.
fn resolve_driver(id: &str, version: &str, store_root: &Path) -> DriverState {
    let executable = store::executable_name(id);

    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&executable)));
    if let Some(path) = beside_exe.as_ref().filter(|path| path.is_file()) {
        return DriverState::installed(path.clone(), DriverOrigin::BesideExecutable);
    }

    if let Some(path) = store::installed_path_in(store_root, id, version) {
        return DriverState::installed(path, DriverOrigin::Store);
    }

    // `PATH` is consulted only when someone has said, by setting an
    // environment variable, that they mean it. Left implicit it was
    // indistinguishable from "not installed", which is the whole defect above.
    if let Some(path) = path_override(id) {
        return DriverState::installed(path, DriverOrigin::Path);
    }

    log::info!(
        "database driver `{executable}` is not installed{}; it will be offered for download",
        beside_exe
            .as_deref()
            .map(|path| format!(" (looked beside the executable at {})", path.display()))
            .unwrap_or_default()
    );
    DriverState::NotInstalled
}

/// An explicit escape hatch: `ZODE_DB_<ID>` naming a driver to run instead.
///
/// Deliberately explicit rather than a silent `PATH` search. Someone developing
/// a driver, or running one Zode does not ship, needs a way to point at it --
/// but guessing that a matching name on `PATH` was meant is how the old
/// fallback turned "never built" into an error that read like the driver's own.
fn path_override(id: &str) -> Option<PathBuf> {
    let key = format!("ZODE_DB_{}", id.to_ascii_uppercase());
    let value = std::env::var_os(&key)?;
    let path = PathBuf::from(value);
    if path.is_file() {
        log::info!("using the `{id}` driver named by {key}: {}", path.display());
        Some(path)
    } else {
        log::warn!(
            "{key} names {}, which is not a file; ignoring it",
            path.display()
        );
        None
    }
}

/// The version whose drivers this build of Zode will run.
///
/// Part of the store path, so an updated app does not pick up the driver its
/// previous version downloaded. A driver speaks a pinned protocol
/// (`database::PROTOCOL_VERSION`), and the release that shipped this app is the
/// one whose drivers were built against it.
pub fn driver_version(cx: &App) -> String {
    AppVersion::global(cx).to_string()
}

pub fn built_in_drivers(version: &str, store_root: &Path) -> DriverRegistry {
    let mut registry = DriverRegistry::new();
    for (id, name) in BUILT_IN {
        registry.register_built_in(*id, *name, resolve_driver(id, version, store_root));
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
    /// Whether that driver is on this machine *now*.
    ///
    /// Not "does Zode know about it": all four shipped drivers are always
    /// known, and none of them is bundled. False here means it has yet to be
    /// downloaded, which is an offer, not a dead end.
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
    (
        "mongodb",
        "MongoDB",
        "Document store, queried with JSON commands",
        "Document",
    ),
];

/// Which driver a URL's scheme names.
///
/// The third and last place in this crate that may name an engine, and like
/// `CATALOGUE` it names them as *data*: nothing here branches on which engine it
/// matched. It exists because the import-a-URL path has no engine list to pick
/// from -- the scheme is the only thing that says what the URL is for, and
/// without reading it a pasted URL was saved against no driver at all, which is
/// a connection nothing can ever open.
///
/// The aliases are the same ones `CATALOGUE` justifies: MariaDB speaks MySQL's
/// wire protocol, and `postgresql://` is the spelling libpq documents.
const URL_SCHEMES: &[(&str, &str)] = &[
    ("mongodb", "mongodb"),
    ("mongodb+srv", "mongodb"),
    ("mysql", "mysql"),
    ("mariadb", "mysql"),
    ("postgres", "postgres"),
    ("postgresql", "postgres"),
    ("sqlite", "sqlite"),
];

/// The driver a pasted URL is for, or `None` when its scheme names none.
///
/// `None` rather than a guess: saving a URL against the wrong driver produces a
/// connection that fails somewhere far less obvious than here, and the person
/// who pasted it can always pick the engine from the list instead.
pub fn driver_for_url(url: &str) -> Option<&'static str> {
    let url = url.trim();
    let lookup = |scheme: &str| {
        let scheme = scheme.to_ascii_lowercase();
        URL_SCHEMES
            .iter()
            .find(|(prefix, _)| *prefix == scheme)
            .map(|(_, driver)| *driver)
    };

    if let Some((scheme, _)) = url.split_once("://") {
        return lookup(scheme);
    }
    // `sqlite:app.db` -- the one scheme here that is ordinarily written without
    // an authority, since what follows it is a path and not a host.
    if let Some((scheme, _)) = url.split_once(':')
        && let Some(driver) = lookup(scheme)
    {
        return Some(driver);
    }
    // No scheme at all is a bare filesystem path, which is what SQLite takes --
    // and `sqlite` is the only shipped driver whose connection string is one.
    if url.starts_with('/') || url.starts_with('.') || url.starts_with('~') {
        return Some("sqlite");
    }
    None
}

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
            installed: registry
                .get(driver)
                .is_some_and(|driver| driver.is_installed()),
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
            // Not "Relational": that was the only shape shipped when this line
            // was written, and an extension's engine is precisely the one whose
            // shape nobody here knows.
            group: SharedString::new_static("Other"),
            installed: true,
        });
    }
    entries
}

/// Puts a driver into the global registry as though it had been downloaded.
///
/// Tests run beside a test binary, not beside Zode, so no driver resolves and
/// every shipped engine is correctly `NotInstalled`. Any test about what
/// happens *after* a driver is present has to say so; before drivers were
/// fetched on demand it got that for free, and the tests that did are exactly
/// the ones this exists for.
#[cfg(test)]
pub(crate) fn install_for_test(id: &str, cx: &mut App) {
    let path = std::env::temp_dir().join(store::executable_name(id));
    global(cx).update(cx, |registry, cx| {
        registry.set_state(id, DriverState::installed(path, DriverOrigin::Store));
        cx.notify();
    });
}

/// Whether Zode publishes a driver for this id, and so can fetch one.
///
/// The catalogue lists engines Zode knows the *name* of, which is a wider set
/// than the drivers it builds: Oracle and SQL Server share a wire protocol with
/// nothing here and wait on an extension. Offering to download a driver that no
/// release contains would be a button that can only fail.
pub fn is_publishable(id: &str) -> bool {
    BUILT_IN.iter().any(|(built_in, _)| *built_in == id)
}

/// Re-resolves one driver, after it has been downloaded.
///
/// Notifies only on a real change, so a re-resolve that found what it already
/// had does not redraw every window. Returns whether it is now installed.
pub fn refresh(id: &str, cx: &mut App) -> bool {
    let version = driver_version(cx);
    let root = store_root(cx);
    global(cx).update(cx, |registry, cx| {
        if registry.set_state(id, resolve_driver(id, &version, &root)) {
            cx.notify();
        }
        registry.get(id).is_some_and(|driver| driver.is_installed())
    })
}

/// The one installer, shared by every window.
///
/// One rather than one per modal for the same reason the registry is one: two
/// windows asking for the same driver must join a single download rather than
/// fetch tens of megabytes twice, and that de-duplication lives inside the
/// installer's own in-flight map.
pub fn installer(cx: &mut App) -> Arc<DriverInstaller> {
    if !cx.has_global::<GlobalDriverInstaller>() {
        let release = ReleaseCoordinates::new(RELEASE_REPO, driver_version(cx));
        let root = store_root(cx);
        let installer = Arc::new(DriverInstaller::with_root(cx.http_client(), release, root));
        cx.set_global(GlobalDriverInstaller(installer));
    }
    cx.global::<GlobalDriverInstaller>().0.clone()
}

/// Where downloaded drivers live.
///
/// Read by *both* the installer, which writes there, and the resolver, which
/// looks there -- from one global, so the two can never be pointed at different
/// directories. They were briefly two lookups, and a test that installed into
/// one and resolved from the other is what said so.
pub fn store_root(cx: &mut App) -> PathBuf {
    if !cx.has_global::<GlobalDriverStore>() {
        cx.set_global(GlobalDriverStore(store::root().to_path_buf()));
    }
    cx.global::<GlobalDriverStore>().0.clone()
}

struct GlobalDriverStore(PathBuf);

impl Global for GlobalDriverStore {}

/// Points the installer at a release a test serves, and a store it owns.
///
/// All three matter. Without the root, a test would download into the running
/// user's real data directory; without the http client it would reach for
/// github.com; and without setting the root *before* the registry reads it, the
/// resolver would keep looking somewhere the installer never wrote.
#[cfg(test)]
pub(crate) fn set_installer_for_test(root: PathBuf, version: &str, cx: &mut App) {
    cx.set_global(GlobalDriverStore(root.clone()));
    let release = ReleaseCoordinates::new(RELEASE_REPO, version);
    let installer = Arc::new(DriverInstaller::with_root(cx.http_client(), release, root));
    cx.set_global(GlobalDriverInstaller(installer));
}

struct GlobalDriverInstaller(Arc<DriverInstaller>);

impl Global for GlobalDriverInstaller {}

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
        let version = driver_version(cx);
        let root = store_root(cx);
        let registry = cx.new(|_| built_in_drivers(&version, &root));
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
            state: DriverState::Installed {
                binary: DriverBinary {
                    executable: driver.executable,
                    args: driver.args,
                    env: driver.env.into_iter().collect(),
                },
                // An extension names its own path and is responsible for it
                // being there; nothing here downloads on its behalf.
                origin: DriverOrigin::Extension,
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
        let registry = built_in_drivers("0.0.0-test", store::root());
        for (id, _) in BUILT_IN {
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

    /// Zode bundles no drivers, so on a machine that has downloaded none, every
    /// shipped engine must still be *listed* -- and listed as absent. An engine
    /// missing from the picker is one nobody can discover is missing, and an
    /// engine claiming to be installed when it is not is a connection that
    /// fails somewhere far less obvious.
    #[test]
    fn a_driver_nobody_has_downloaded_is_still_a_driver_zode_knows() {
        let registry = built_in_drivers("0.0.0-no-such-release", store::root());
        for (id, _) in BUILT_IN {
            let driver = registry
                .get(id)
                .unwrap_or_else(|| panic!("`{id}` must be listed whether or not it is present"));
            // Beside-the-executable may legitimately find these in a checkout
            // that has run `make drivers`, so this asserts the invariant that
            // holds either way: installed and having a binary agree.
            assert_eq!(
                driver.is_installed(),
                driver.binary().is_some(),
                "`{id}` reports being installed and having a binary differently"
            );
        }
    }
}

#[cfg(test)]
mod url_scheme_tests {
    use super::*;

    /// The defect this exists for: a URL pasted into the import path was saved
    /// against no driver at all, which is a connection nothing can open -- and
    /// which made `Test Connection` return without a word.
    #[test]
    fn a_scheme_names_the_driver_that_speaks_it() {
        assert_eq!(
            driver_for_url("mongodb://user@host:27017/app?authSource=admin"),
            Some("mongodb")
        );
        assert_eq!(
            driver_for_url("mongodb+srv://user@cluster/app"),
            Some("mongodb")
        );
        assert_eq!(driver_for_url("postgres://user@host/app"), Some("postgres"));
        assert_eq!(
            driver_for_url("postgresql://user@host/app"),
            Some("postgres")
        );
        assert_eq!(driver_for_url("mysql://user@host/app"), Some("mysql"));
    }

    /// The aliases `CATALOGUE` already justifies: one wire protocol, several
    /// engines that speak it.
    #[test]
    fn an_alias_resolves_to_the_driver_that_serves_it() {
        assert_eq!(driver_for_url("mariadb://user@host/app"), Some("mysql"));
    }

    #[test]
    fn a_scheme_is_matched_regardless_of_case() {
        assert_eq!(driver_for_url("  MongoDB://host/app  "), Some("mongodb"));
    }

    /// SQLite is the one shipped engine whose connection string is a path, so
    /// it is the one a string with no scheme can only be.
    #[test]
    fn a_bare_path_is_sqlite() {
        assert_eq!(driver_for_url("/home/someone/app.sqlite"), Some("sqlite"));
        assert_eq!(driver_for_url("./app.db"), Some("sqlite"));
        assert_eq!(driver_for_url("sqlite:app.db"), Some("sqlite"));
    }

    /// `None`, not a guess: saving a URL against the wrong driver fails
    /// somewhere far less obvious than the dialog it was pasted into.
    #[test]
    fn an_unknown_scheme_names_nothing_rather_than_guessing() {
        assert_eq!(driver_for_url("oracle://user@host/app"), None);
        assert_eq!(driver_for_url("redis://host:6379"), None);
        assert_eq!(driver_for_url(""), None);
    }
}
