use anyhow::{Result, anyhow};
use collections::HashMap;
use database::install::{DriverEndpoint, DriverInstaller, store};
use database::registry::{
    DriverDescriptor, DriverOrigin, DriverRegistry, DriverSource, DriverState,
};
use database::transport::DriverBinary;
use extension::{ExtensionDatabaseDriver, ExtensionDatabaseDriverProxy, ExtensionHostProxy};
use gpui::{App, AppContext as _, Entity, Global, SharedString, Task, WeakEntity};
use project::Project;
use rpc::{AnyProtoClient, proto};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use util::command::CommandHost;

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
const BUILT_IN: &[(&str, &str)] = store::PUBLISHED_DRIVERS;

/// Where a driver binary lives on this machine, if it lives anywhere: beside
/// the executable, in the download store, or wherever `ZODE_DB_<ID>` names.
/// See `store::locate_in` for the order and why.
///
/// A fourth answer, "nowhere", is the point. `driver_path` used to always
/// return a `PathBuf`, so every driver read as installed, and a bare name was
/// handed to a shell that wrote `command not found` to what the client read as
/// the driver's stderr. The one fact worth knowing -- the driver was never
/// installed -- appeared nowhere.
fn resolve_driver(id: &str, store_root: &Path) -> DriverState {
    match store::locate_in(store_root, id, |key| std::env::var_os(key)) {
        Some((path, location)) => DriverState::installed(path, origin_of(location)),
        None => DriverState::NotInstalled,
    }
}

fn origin_of(location: store::DriverLocation) -> DriverOrigin {
    match location {
        store::DriverLocation::BesideExecutable => DriverOrigin::BesideExecutable,
        store::DriverLocation::Store => DriverOrigin::Store,
        store::DriverLocation::Manual => DriverOrigin::Manual,
        store::DriverLocation::Override => DriverOrigin::Path,
    }
}

pub fn built_in_drivers(store_root: &Path) -> DriverRegistry {
    let mut registry = DriverRegistry::new();
    for (id, name) in BUILT_IN {
        registry.register_built_in(*id, *name, resolve_driver(id, store_root));
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
    /// Whether that driver sits in the download store, and so is one Zode may
    /// offer to delete.
    ///
    /// A driver beside the executable belongs to the bundle or the checkout,
    /// and one named by `ZODE_DB_<ID>` to whoever named it -- neither is Zode's
    /// to remove. What is in the store is, but see [`Self::hand_placed`] for how
    /// sure Zode is that a click brings it back.
    pub downloaded: bool,
    /// Whether that driver in the store carries nothing saying Zode put it
    /// there.
    ///
    /// Either somebody copied it in by hand -- which is what the install error
    /// tells someone with no route to the API to do -- or an earlier Zode
    /// downloaded it, before there was anything to leave. Zode cannot tell the
    /// two apart, so it offers to remove it all the same and says plainly that
    /// the driver may not come back.
    pub hand_placed: bool,
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

/// Every engine, with the driver each one needs and whether it is on `host`.
///
/// A driver an extension added that no catalogue row names is appended: an
/// engine Zode has never heard of must still be reachable, or the plugin
/// architecture only works for engines someone already thought of. Not for a
/// remote host, though: an extension's driver is a path on this machine.
pub fn catalogue(host: &DriverHost, cx: &mut App) -> Vec<CatalogueEntry> {
    let registry = host.registry(cx);
    let registry = registry.read(cx);
    // Removing a driver from a remote host has no request of its own, and the
    // only removal there is deletes this machine's copy instead.
    let removable = matches!(host, DriverHost::Local);

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
            downloaded: removable
                && registry.get(driver).is_some_and(|driver| {
                    matches!(
                        driver.origin(),
                        Some(DriverOrigin::Store | DriverOrigin::Manual)
                    )
                }),
            hand_placed: removable
                && registry
                    .get(driver)
                    .is_some_and(|driver| driver.origin() == Some(DriverOrigin::Manual)),
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
            downloaded: false,
            hand_placed: false,
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
    store::is_published(id)
}

/// Re-resolves one driver, after it has been downloaded.
///
/// Notifies only on a real change, so a re-resolve that found what it already
/// had does not redraw every window. Returns whether it is now installed.
pub fn refresh(id: &str, cx: &mut App) -> bool {
    let root = store_root(cx);
    global(cx).update(cx, |registry, cx| {
        if registry.set_state(id, resolve_driver(id, &root)) {
            cx.notify();
        }
        registry.get(id).is_some_and(|driver| driver.is_installed())
    })
}

/// Deletes a downloaded driver and re-resolves it, so every engine that
/// speaks through it reads as not downloaded again.
///
/// The deletion runs off the main thread: a driver is tens of megabytes. A
/// connection already running the driver keeps its process; the next one to
/// start is offered the download again.
///
/// Does nothing about a download of the same driver that is still running, and
/// cannot: it is a shared future that other windows are waiting on, and one
/// caller has no business stopping it for the rest. It is not reachable from
/// the dialog either, which offers Remove only for a driver that is installed
/// and Download only for one that is not.
pub fn remove_download(id: SharedString, cx: &mut App) -> gpui::Task<anyhow::Result<()>> {
    let root = store_root(cx);
    let removal = cx.background_spawn({
        let id = id.clone();
        async move { store::remove_in(&root, &id) }
    });
    cx.spawn(async move |cx| {
        let outcome = removal.await.map(|_| ());
        // Re-resolved whichever way it went: a removal that failed halfway --
        // Windows will not delete a running executable -- may still have taken
        // the driver, and the registry must say what is on disk now.
        cx.update(|cx| {
            refresh(&id, cx);
        });
        outcome
    })
}

/// The one installer, shared by every window.
///
/// One rather than one per modal for the same reason the registry is one: two
/// windows asking for the same driver must join a single download rather than
/// fetch tens of megabytes twice, and that de-duplication lives inside the
/// installer's own in-flight map.
///
/// The base comes from `zode_account::api_url()` -- Zode's own API, not a
/// GitHub release -- because `crates/database` is deliberately kept ignorant
/// of accounts and gpui, and something in the gpui world has to resolve it.
/// `api_url()` is a free function that needs no signed-in `Account`, which is
/// the point: a driver is a public asset, and someone who has not logged in
/// yet must still be able to fetch one.
pub fn installer(cx: &mut App) -> Arc<DriverInstaller> {
    if !cx.has_global::<GlobalDriverInstaller>() {
        let endpoint = DriverEndpoint::new(zode_account::api_url());
        let root = store_root(cx);
        let installer = Arc::new(DriverInstaller::with_root(cx.http_client(), endpoint, root));
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

/// A base a test's `DriverEndpoint` can be built on without ever reaching the
/// network. `.invalid` is a reserved TLD that never resolves, so a test that
/// somehow escapes its fake http client fails loudly instead of quietly
/// calling out to the real world.
#[cfg(test)]
const TEST_API_BASE: &str = "https://api.test.invalid/api";

/// Points the installer at a base a test serves, and a store it owns.
///
/// All three matter. Without the root, a test would download into the running
/// user's real data directory; without the http client it would reach for
/// `TEST_API_BASE` for real; and without setting the root *before* the registry
/// reads it, the resolver would keep looking somewhere the installer never
/// wrote. The installer trusts the key the test fixtures are signed with, and
/// no other, and accepts the low versions those fixtures publish.
#[cfg(test)]
pub(crate) fn set_installer_for_test(root: PathBuf, cx: &mut App) {
    cx.set_global(GlobalDriverStore(root.clone()));
    let endpoint = DriverEndpoint::new(TEST_API_BASE);
    let installer = Arc::new(
        DriverInstaller::with_root(cx.http_client(), endpoint, root)
            .trusting(database::signature::testing::public_key())
            .accepting_versions_from("0.0.0"),
    );
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
        let root = store_root(cx);
        let registry = cx.new(|_| built_in_drivers(&root));
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

/// What a remote server says when it has no handler for a driver request.
pub const REMOTE_SERVER_OUT_OF_DATE: &str = "Update the remote server to use database drivers here";

/// How long a remote host may take to say whether it has a driver.
///
/// A current server answers a request it has no handler for. Only one built
/// before unknown requests were answered drops it without a word, and without
/// a deadline that question would never be answered.
const REMOTE_RESOLVE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a remote host may take to download and install a driver.
///
/// One request rather than a start and a poll: the host's installer gives up
/// by itself on a download that goes quiet for 30s, so this bounds only a
/// download that keeps moving, slowly. It is asked only after a resolve has
/// shown the server answers these requests at all.
const REMOTE_INSTALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Where a project's database drivers are found, installed and run.
#[derive(Clone)]
pub enum DriverHost {
    /// This machine: the global registry, its download store and extensions.
    Local,
    /// The host a WSL or SSH project lives on. Nothing on this machine stands
    /// in for a driver there, not even one of the same name.
    Remote(RemoteDrivers),
}

impl DriverHost {
    pub fn registry(&self, cx: &mut App) -> Entity<DriverRegistry> {
        match self {
            Self::Local => global(cx),
            Self::Remote(remote) => remote.registry.clone(),
        }
    }

    /// The remote host's name for messages, `None` for this machine.
    pub fn host_name(&self) -> Option<SharedString> {
        match self {
            Self::Local => None,
            Self::Remote(remote) => Some(remote.host_name.clone()),
        }
    }

    /// Asks a remote host again which drivers it has. This machine's are
    /// resolved where they are installed, so there is nothing to ask.
    pub fn refresh_all(&self, cx: &mut App) -> Task<Result<()>> {
        let Self::Remote(remote) = self else {
            return Task::ready(Ok(()));
        };
        let answers: Vec<_> = BUILT_IN
            .iter()
            .map(|(id, _)| remote.ensure(id, false, cx))
            .collect();
        // All awaited, not each in turn: returning at the first failure would
        // drop, and so cancel, the questions still waiting on the host.
        cx.spawn(async move |_| {
            futures::future::join_all(answers)
                .await
                .into_iter()
                .find_map(Result::err)
                .map_or(Ok(()), Err)
        })
    }
}

/// Asks a remote host about its drivers, and reaches it to run one.
///
/// A trait so a test can stand in for the remote server and the wrapper
/// command, and see exactly what this machine asked of each.
pub trait RemoteDriverService {
    fn ensure(&self, driver: &str, install: bool, cx: &mut App) -> Task<Result<DriverState>>;
    fn command_host(&self, cx: &mut App) -> Task<Result<Arc<dyn CommandHost>>>;
}

#[derive(Clone)]
pub struct RemoteDrivers {
    host_name: SharedString,
    registry: Entity<DriverRegistry>,
    service: Rc<dyn RemoteDriverService>,
}

impl RemoteDrivers {
    #[cfg(test)]
    pub(crate) fn with_service(
        host_name: &str,
        service: Rc<dyn RemoteDriverService>,
        cx: &mut App,
    ) -> Self {
        Self {
            host_name: host_name.to_string().into(),
            registry: cx.new(|_| remote_built_ins()),
            service,
        }
    }

    /// The driver as the host has it now, after asking it. With `install`, a
    /// driver the host lacks is downloaded there first.
    pub fn ensure(
        &self,
        id: &str,
        install: bool,
        cx: &mut App,
    ) -> Task<Result<Option<DriverDescriptor>>> {
        let answer = self.service.ensure(id, install, cx);
        let registry = self.registry.clone();
        let id = id.to_string();
        cx.spawn(async move |cx| {
            let state = answer.await?;
            Ok(cx.update(|cx| {
                registry.update(cx, |registry, cx| {
                    if registry.set_state(&id, state) {
                        cx.notify();
                    }
                    registry.get(&id).cloned()
                })
            }))
        })
    }

    /// Installs a driver on the host, unless it is there already.
    ///
    /// Resolved first under the short deadline, so a server too old to know
    /// the request is found out in seconds rather than at the install's.
    pub fn install(&self, id: &str, cx: &mut App) -> Task<Result<()>> {
        let resolved = self.ensure(id, false, cx);
        let this = self.clone();
        let id = id.to_string();
        cx.spawn(async move |cx| {
            if resolved.await?.is_some_and(|driver| driver.is_installed()) {
                return Ok(());
            }
            let installed = cx.update(|cx| this.ensure(&id, true, cx)).await?;
            anyhow::ensure!(
                installed.is_some_and(|driver| driver.is_installed()),
                "{} did not install the `{id}` driver",
                this.host_name
            );
            Ok(())
        })
    }

    pub fn command_host(&self, cx: &mut App) -> Task<Result<Arc<dyn CommandHost>>> {
        self.service.command_host(cx)
    }

    pub fn host_name(&self) -> &SharedString {
        &self.host_name
    }
}

/// Every published driver, none of them known to be installed until the host
/// says so -- and no extension's, which names a path on this machine.
fn remote_built_ins() -> DriverRegistry {
    let mut registry = DriverRegistry::new();
    for (id, name) in BUILT_IN {
        registry.register_built_in(*id, *name, DriverState::NotInstalled);
    }
    registry
}

/// The drivers of the machine `project` lives on.
///
/// A project opened over a remote server gets that host's drivers, in a
/// registry shared by every window on the same host. Anything else -- a local
/// project, or one joined from a collaborator -- keeps this machine's.
pub fn for_project(project: &Entity<Project>, cx: &mut App) -> DriverHost {
    let Some(remote_client) = project
        .read(cx)
        .remote_client()
        .filter(|_| project.read(cx).is_via_remote_server())
    else {
        return DriverHost::Local;
    };
    let (options, client) = {
        let remote_client = remote_client.read(cx);
        (
            remote_client.connection_options(),
            remote_client.proto_client(),
        )
    };
    let host_name: SharedString = options.display_name().into();
    let key = remote::remote_connection_identity(&options).stable_key();
    if !cx.has_global::<RemoteDriverRegistries>() {
        cx.set_global(RemoteDriverRegistries(HashMap::default()));
    }
    let existing = cx.global::<RemoteDriverRegistries>().0.get(&key).cloned();
    let registry = match existing {
        Some(registry) => registry,
        None => {
            let registry = cx.new(|_| remote_built_ins());
            cx.global_mut::<RemoteDriverRegistries>()
                .0
                .insert(key, registry.clone());
            registry
        }
    };
    DriverHost::Remote(RemoteDrivers {
        host_name: host_name.clone(),
        registry,
        service: Rc::new(ProjectDriverService {
            project: project.downgrade(),
            client,
            host_name,
        }),
    })
}

/// One registry per remote host, by `RemoteConnectionIdentity::stable_key`.
/// Holds one entry per host connected to in this run, which is a handful.
struct RemoteDriverRegistries(HashMap<String, Entity<DriverRegistry>>);

impl Global for RemoteDriverRegistries {}

struct ProjectDriverService {
    project: WeakEntity<Project>,
    client: AnyProtoClient,
    host_name: SharedString,
}

impl RemoteDriverService for ProjectDriverService {
    fn ensure(&self, driver: &str, install: bool, cx: &mut App) -> Task<Result<DriverState>> {
        let request = self.client.request(proto::EnsureDatabaseDriver {
            project_id: proto::REMOTE_SERVER_PROJECT_ID,
            driver_id: driver.to_string(),
            install,
            api_url: zode_account::api_url(),
        });
        let timeout = if install {
            REMOTE_INSTALL_TIMEOUT
        } else {
            REMOTE_RESOLVE_TIMEOUT
        };
        let timer = cx.background_executor().timer(timeout);
        let host_name = self.host_name.clone();
        cx.spawn(async move |_| {
            let response = within_deadline(request, timer, timeout).await?;
            state_from_answer(response, &host_name)
        })
    }

    fn command_host(&self, cx: &mut App) -> Task<Result<Arc<dyn CommandHost>>> {
        match self.project.upgrade() {
            Some(project) => project.update(cx, |project, cx| project.command_host(cx)),
            None => Task::ready(Err(anyhow!("the project on {} was closed", self.host_name))),
        }
    }
}

/// `request`, unless `timer` fires first, with a server that does not know
/// the request named as one to update.
async fn within_deadline<T>(
    request: impl Future<Output = Result<T>>,
    timer: impl Future<Output = ()>,
    timeout: Duration,
) -> Result<T> {
    match futures::future::select(std::pin::pin!(request), std::pin::pin!(timer)).await {
        futures::future::Either::Left((Ok(response), _)) => Ok(response),
        futures::future::Either::Left((Err(error), _)) => {
            if format!("{error:#}").contains("no handler registered") {
                Err(anyhow!("{REMOTE_SERVER_OUT_OF_DATE}"))
            } else {
                Err(error)
            }
        }
        // Not called out of date: a current server that is merely slow ends
        // the same way.
        futures::future::Either::Right(_) => Err(anyhow!(
            "the remote server did not answer within {}. If it is older than this Zode: \
             {REMOTE_SERVER_OUT_OF_DATE}",
            spoken_duration(timeout)
        )),
    }
}

fn spoken_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds == 0 || !seconds.is_multiple_of(60) {
        return format!("{seconds}s");
    }
    match seconds / 60 {
        1 => "1 minute".to_string(),
        minutes => format!("{minutes} minutes"),
    }
}

fn state_from_answer(
    response: proto::EnsureDatabaseDriverResponse,
    host_name: &str,
) -> Result<DriverState> {
    use proto::ensure_database_driver_response::State;
    match response.state {
        Some(State::Installed(installed)) => {
            let origin = match proto::DatabaseDriverOrigin::from_i32(installed.origin) {
                Some(proto::DatabaseDriverOrigin::BesideExecutable) => {
                    DriverOrigin::BesideExecutable
                }
                Some(proto::DatabaseDriverOrigin::Store) => DriverOrigin::Store,
                Some(proto::DatabaseDriverOrigin::Override) => DriverOrigin::Path,
                // Left unset, or a newer server's origin: shown as placed by
                // hand, which claims least.
                Some(
                    proto::DatabaseDriverOrigin::Manual | proto::DatabaseDriverOrigin::Unspecified,
                )
                | None => DriverOrigin::Manual,
            };
            Ok(DriverState::installed(
                PathBuf::from(installed.executable),
                origin,
            ))
        }
        Some(State::NotInstalled(_)) => Ok(DriverState::NotInstalled),
        Some(State::Failed(failed)) => Err(anyhow!("{host_name}: {}", failed.reason)),
        None => Err(anyhow!("{host_name} answered with no driver state")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::install::test_support::plant_driver;

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
        let store = tempfile::tempdir().unwrap();
        let registry = built_in_drivers(store.path());
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
        // A store of its own rather than the real one: a driver of any version
        // resolves, so one the developer running this had downloaded would be
        // found and read as installed.
        let store = tempfile::tempdir().unwrap();
        let registry = built_in_drivers(store.path());
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

    /// The store's version directory is the published driver set's, which is no
    /// longer anything the running app can name -- so whichever one was
    /// installed is the one a driver resolves to.
    #[gpui::test]
    fn a_driver_installed_under_any_version_is_one_the_registry_finds(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = tempfile::tempdir().unwrap();
        plant_driver(store.path(), "postgres", "0.9.9", true);

        cx.update(|cx| {
            set_installer_for_test(store.path().to_path_buf(), cx);

            assert!(refresh("postgres", cx));

            let registry = global(cx);
            let origin = registry
                .read(cx)
                .get("postgres")
                .and_then(|driver| driver.origin());
            assert_eq!(origin, Some(DriverOrigin::Store));
        });
    }

    /// What the install error tells an offline user to do -- and what a driver
    /// downloaded by an earlier Zode, which left no marker, looks like. It
    /// resolves and runs like any other, and is offered for removal, flagged as
    /// one that may not come back.
    #[gpui::test]
    fn a_driver_with_no_marker_is_found_and_offered_for_removal_as_hand_placed(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = tempfile::tempdir().unwrap();
        plant_driver(store.path(), "postgres", "0.9.9", false);

        cx.update(|cx| {
            set_installer_for_test(store.path().to_path_buf(), cx);
            assert!(refresh("postgres", cx), "it is installed and usable");

            let registry = global(cx);
            assert_eq!(
                registry
                    .read(cx)
                    .get("postgres")
                    .and_then(|driver| driver.origin()),
                Some(DriverOrigin::Manual)
            );
            for entry in catalogue(&DriverHost::Local, cx)
                .into_iter()
                .filter(|entry| entry.driver == "postgres")
            {
                assert!(entry.installed, "{entry:?}");
                assert!(
                    entry.downloaded,
                    "{} sits in the store, so it can be removed",
                    entry.name
                );
                assert!(
                    entry.hand_placed,
                    "{} carries no marker, so removing it must warn",
                    entry.name
                );
            }
        });
    }

    #[gpui::test]
    fn a_driver_zode_installed_is_offered_for_removal(cx: &mut gpui::TestAppContext) {
        let store = tempfile::tempdir().unwrap();
        plant_driver(store.path(), "postgres", "0.9.9", true);

        cx.update(|cx| {
            set_installer_for_test(store.path().to_path_buf(), cx);
            refresh("postgres", cx);

            assert!(
                catalogue(&DriverHost::Local, cx)
                    .into_iter()
                    .filter(|entry| entry.driver == "postgres")
                    .all(|entry| entry.downloaded && !entry.hand_placed)
            );
        });
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

/// A remote host for tests: answers driver questions from what it was told it
/// has, and records what this machine asked of it and what it tried to run.
#[cfg(test)]
pub(crate) mod remote_test_support {
    use super::*;
    use std::cell::RefCell;
    use std::sync::Mutex;

    pub(crate) const HOST_NAME: &str = "WSL: Ubuntu";

    /// Where the host keeps its `postgres` driver: a path of the host's, which
    /// exists nowhere on this machine.
    pub(crate) const REMOTE_POSTGRES: &str =
        "/home/me/.local/share/zode/database_drivers/postgres/0.9.0/zode-db-postgres";

    #[derive(Default)]
    pub(crate) struct RecordingHost {
        /// Every `(driver, install)` this machine asked, in order.
        pub(crate) asked: RefCell<Vec<(String, bool)>>,
        postgres_installed: RefCell<bool>,
        pub(crate) command_host: Arc<RecordingCommandHost>,
    }

    impl RecordingHost {
        pub(crate) fn with_postgres() -> Self {
            let host = Self::default();
            host.postgres_installed.replace(true);
            host
        }
    }

    impl RemoteDriverService for RecordingHost {
        fn ensure(&self, driver: &str, install: bool, _cx: &mut App) -> Task<Result<DriverState>> {
            self.asked.borrow_mut().push((driver.to_string(), install));
            if driver == "postgres" && install {
                self.postgres_installed.replace(true);
            }
            let state = if driver == "postgres" && *self.postgres_installed.borrow() {
                DriverState::installed(PathBuf::from(REMOTE_POSTGRES), DriverOrigin::Store)
            } else {
                DriverState::NotInstalled
            };
            Task::ready(Ok(state))
        }

        fn command_host(&self, _cx: &mut App) -> Task<Result<Arc<dyn CommandHost>>> {
            Task::ready(Ok(self.command_host.clone()))
        }
    }

    /// Refuses to build any command, so nothing is ever spawned, and keeps
    /// the program it was asked for.
    #[derive(Default)]
    pub(crate) struct RecordingCommandHost {
        pub(crate) programs: Mutex<Vec<String>>,
        /// Says it is this machine's rather than the host's.
        pub(crate) local: bool,
    }

    impl CommandHost for RecordingCommandHost {
        fn command(
            &self,
            program: &str,
            _args: &[&str],
            _env: &[(&str, &str)],
            _cwd: Option<&Path>,
        ) -> Result<util::command::Command> {
            self.programs
                .lock()
                .map_err(|_| anyhow!("poisoned"))?
                .push(program.to_string());
            Err(anyhow!("the test host runs nothing"))
        }

        fn path_style(&self) -> util::paths::PathStyle {
            util::paths::PathStyle::Posix
        }

        fn remote_name(&self) -> Option<String> {
            (!self.local).then(|| HOST_NAME.to_string())
        }

        fn environment_variable(&self, _name: &str) -> Option<String> {
            None
        }
    }

    pub(crate) fn remote_host(service: Rc<RecordingHost>, cx: &mut App) -> DriverHost {
        DriverHost::Remote(RemoteDrivers::with_service(HOST_NAME, service, cx))
    }
}

#[cfg(test)]
mod remote_driver_tests {
    use super::remote_test_support::*;
    use super::*;
    use database::install::test_support::{directory_entries, plant_driver};

    fn postgres_rows(host: &DriverHost, cx: &mut App) -> Vec<CatalogueEntry> {
        catalogue(host, cx)
            .into_iter()
            .filter(|entry| entry.driver == "postgres")
            .collect()
    }

    /// This machine has the driver; the host does not until it says so, and
    /// once it does, the driver started is the host's, through the host.
    #[gpui::test]
    async fn a_remote_host_answers_for_its_drivers_and_this_machine_never_does(
        cx: &mut gpui::TestAppContext,
    ) {
        let local_store = tempfile::tempdir().unwrap();
        let local_postgres = plant_driver(local_store.path(), "postgres", "0.9.9", true);
        let service = Rc::new(RecordingHost::with_postgres());
        let host = cx.update(|cx| {
            set_installer_for_test(local_store.path().to_path_buf(), cx);
            assert!(refresh("postgres", cx), "this machine has the driver");
            remote_host(service.clone(), cx)
        });

        cx.update(|cx| {
            let rows = postgres_rows(&host, cx);
            assert!(!rows.is_empty());
            assert!(
                rows.iter().all(|entry| !entry.installed),
                "this machine's driver was offered for the host: {rows:?}"
            );
        });

        cx.update(|cx| host.refresh_all(cx)).await.unwrap();
        assert_eq!(
            service.asked.borrow().clone(),
            BUILT_IN
                .iter()
                .map(|(id, _)| (id.to_string(), false))
                .collect::<Vec<_>>(),
            "the host is asked about every published driver, and asked to install none"
        );

        let descriptor = cx.update(|cx| {
            let rows = postgres_rows(&host, cx);
            assert!(rows.iter().all(|entry| entry.installed), "{rows:?}");
            assert!(
                rows.iter()
                    .all(|entry| !entry.downloaded && !entry.hand_placed),
                "removal would delete this machine's copy, so a host's is not offered: {rows:?}"
            );
            host.registry(cx).read(cx).get("postgres").cloned()
        });
        let descriptor = descriptor.expect("the host's registry names postgres");
        assert_eq!(
            descriptor.binary().map(|binary| binary.executable.clone()),
            Some(PathBuf::from(REMOTE_POSTGRES))
        );

        let async_cx = cx.to_async();
        let started = crate::session::start_driver(&host, &descriptor, &async_cx).await;
        let error = match started {
            Ok(_) => panic!("the test host runs nothing"),
            Err(error) => format!("{error:#}"),
        };
        assert!(error.contains(&format!("on {HOST_NAME}")), "{error}");
        assert_eq!(
            service.command_host.programs.lock().unwrap().clone(),
            vec![REMOTE_POSTGRES.to_string()],
            "the host's path, run through the host"
        );

        cx.update(|cx| {
            assert!(
                postgres_rows(&DriverHost::Local, cx)
                    .iter()
                    .all(|entry| entry.installed),
                "this machine's own catalogue is unchanged"
            );
            assert_eq!(
                global(cx)
                    .read(cx)
                    .get("postgres")
                    .and_then(|driver| driver.binary())
                    .map(|binary| binary.executable.clone()),
                Some(local_postgres)
            );
        });
        assert_eq!(
            directory_entries(local_store.path(), "postgres").len(),
            1,
            "nothing was added to this machine's store"
        );
    }

    /// A server from before this request answers "no handler registered".
    #[gpui::test]
    async fn an_older_remote_server_is_named_as_one_to_update_and_nothing_runs_here(
        cx: &mut gpui::TestAppContext,
        server_cx: &mut gpui::TestAppContext,
    ) {
        let fs = fs::FakeFs::new(cx.executor());
        let (project, _server) = project::Project::test_remote(fs, cx, server_cx).await;
        let local_store = tempfile::tempdir().unwrap();
        let host = cx.update(|cx| {
            set_installer_for_test(local_store.path().to_path_buf(), cx);
            for_project(&project, cx)
        });
        let DriverHost::Remote(remote) = host else {
            panic!("a project on a remote server got this machine's drivers");
        };

        let install = cx.update(|cx| remote.install("postgres", cx));
        cx.run_until_parked();
        let error = install
            .await
            .expect_err("a server with no handler cannot install");
        assert_eq!(format!("{error:#}"), REMOTE_SERVER_OUT_OF_DATE);

        cx.update(|cx| {
            assert_eq!(installer(cx).waiters("postgres"), 0);
            assert!(
                !global(cx)
                    .read(cx)
                    .get("postgres")
                    .is_some_and(|driver| driver.is_installed()),
                "no driver of this machine's stood in"
            );
        });
        assert!(directory_entries(local_store.path(), "postgres").is_empty());
    }

    /// Answers `sqlite` with an error at once, and every other driver a
    /// moment later, `postgres` as installed.
    struct PartlyFailingHost;

    impl RemoteDriverService for PartlyFailingHost {
        fn ensure(&self, driver: &str, _install: bool, cx: &mut App) -> Task<Result<DriverState>> {
            if driver == "sqlite" {
                return Task::ready(Err(anyhow!("the sqlite driver is broken on the host")));
            }
            let state = if driver == "postgres" {
                DriverState::installed(PathBuf::from(REMOTE_POSTGRES), DriverOrigin::Store)
            } else {
                DriverState::NotInstalled
            };
            let answered = cx.background_executor().timer(Duration::from_millis(10));
            cx.background_spawn(async move {
                answered.await;
                Ok(state)
            })
        }

        fn command_host(&self, _cx: &mut App) -> Task<Result<Arc<dyn CommandHost>>> {
            Task::ready(Err(anyhow!("the test host runs nothing")))
        }
    }

    #[gpui::test]
    async fn one_driver_failing_on_the_host_leaves_the_answers_for_the_others(
        cx: &mut gpui::TestAppContext,
    ) {
        let host = cx.update(|cx| {
            DriverHost::Remote(RemoteDrivers::with_service(
                HOST_NAME,
                Rc::new(PartlyFailingHost),
                cx,
            ))
        });

        let refreshed = cx.update(|cx| host.refresh_all(cx));
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(10));
        cx.run_until_parked();

        let error = format!("{:#}", refreshed.await.expect_err("sqlite failed"));
        assert!(error.contains("the sqlite driver is broken"), "{error}");
        cx.update(|cx| {
            assert!(
                host.registry(cx)
                    .read(cx)
                    .get("postgres")
                    .is_some_and(|driver| driver.is_installed()),
                "the host said it has postgres, and one failure elsewhere must not hide that"
            );
        });
    }

    /// A remote project's driver must run through something that reaches the
    /// host. A command host of this machine's would run the host's path here.
    #[gpui::test]
    async fn a_remote_driver_is_never_run_through_this_machine(cx: &mut gpui::TestAppContext) {
        let mut service = RecordingHost::with_postgres();
        service.command_host = Arc::new(RecordingCommandHost {
            local: true,
            ..Default::default()
        });
        let service = Rc::new(service);
        let host = cx.update(|cx| remote_host(service.clone(), cx));
        let DriverHost::Remote(remote) = &host else {
            panic!("a remote host was built");
        };
        let descriptor = cx
            .update(|cx| remote.ensure("postgres", false, cx))
            .await
            .unwrap()
            .expect("the host names postgres");

        let started = crate::session::start_driver(&host, &descriptor, &cx.to_async()).await;
        let error = match started {
            Ok(_) => panic!("nothing may start through this machine"),
            Err(error) => format!("{error:#}"),
        };
        assert!(
            service.command_host.programs.lock().unwrap().is_empty(),
            "the host's path was handed to this machine to run"
        );
        assert!(error.contains("does not reach"), "{error}");
    }

    #[test]
    fn a_long_deadline_is_said_in_minutes() {
        let answer = futures::executor::block_on(within_deadline(
            futures::future::pending::<Result<()>>(),
            futures::future::ready(()),
            REMOTE_INSTALL_TIMEOUT,
        ));
        let error = format!("{:#}", answer.expect_err("the deadline passed"));
        assert!(
            error.starts_with("the remote server did not answer within 30 minutes."),
            "{error}"
        );
    }

    /// An origin this machine cannot name, or one the host left unset, is
    /// shown as the one that claims least.
    #[test]
    fn an_origin_the_host_does_not_name_claims_the_least() {
        use proto::ensure_database_driver_response::{Installed, State};
        for origin in [0, 99] {
            let answer = state_from_answer(
                proto::EnsureDatabaseDriverResponse {
                    state: Some(State::Installed(Installed {
                        executable: REMOTE_POSTGRES.to_string(),
                        origin,
                    })),
                },
                HOST_NAME,
            );
            match answer {
                Ok(DriverState::Installed { origin: found, .. }) => assert_eq!(
                    found,
                    DriverOrigin::Manual,
                    "origin {origin} must read as placed by hand"
                ),
                other => panic!("an installed driver, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_host_that_never_answers_is_not_called_out_of_date() {
        let answer = futures::executor::block_on(within_deadline(
            futures::future::pending::<Result<()>>(),
            futures::future::ready(()),
            REMOTE_RESOLVE_TIMEOUT,
        ));
        let error = format!("{:#}", answer.expect_err("the deadline passed"));
        assert!(
            error.starts_with("the remote server did not answer within 30s"),
            "{error}"
        );
    }

    #[test]
    fn a_failure_the_host_reports_is_said_with_its_name() {
        use proto::ensure_database_driver_response::{Failed, State};
        let answer = state_from_answer(
            proto::EnsureDatabaseDriverResponse {
                state: Some(State::Failed(Failed {
                    reason: "the archive does not match the checksum".to_string(),
                })),
            },
            HOST_NAME,
        );
        let error = format!("{:#}", answer.expect_err("a failure is an error"));
        assert_eq!(
            error,
            format!("{HOST_NAME}: the archive does not match the checksum")
        );
    }
}
