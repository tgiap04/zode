//! Fetching the most recently published driver.
//!
//! The order matters more than any single step: the manifest's signature is
//! checked before anything in it is believed, the bytes are hashed while they
//! stream, compared against that manifest, and only then unpacked. Nothing
//! unverified is ever handed to an archive reader, and nothing half-installed
//! is ever visible where the resolver looks.

use anyhow::{Context as _, Result};
use collections::HashMap;
use futures::future::{BoxFuture, FutureExt as _, Shared};
use futures::{AsyncReadExt as _, AsyncWriteExt as _, channel::mpsc};
use http_client::{AsyncBody, HttpClient, Response, StatusCode};
use parking_lot::Mutex;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::install::endpoint::DriverEndpoint;
use crate::install::manifest::{
    DriverAsset, DriverManifest, MANIFEST_ASSET, current_target, parse_sha256,
};
use crate::install::{store, unpack};
use crate::protocol::{PROTOCOL_VERSION, version_is_compatible};
use crate::signature::{self, DRIVER_MANIFEST_PUBLIC_KEY, SIGNED_MANIFEST_ASSET};

/// The most a manifest may weigh. A real one is a few kilobytes -- a line per
/// driver per platform -- so this is generous, and exists only so that a server
/// that answers with something endless cannot fill memory.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// The most an archive may weigh, whatever its manifest says it does. A driver
/// is a few tens of megabytes; this is what a manifest cannot talk Zode past.
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

/// How long a connection may go without delivering a byte before the install is
/// given up on. Per read rather than overall: a slow link is fine, a stalled
/// one never finishes.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// What to say to someone building Zode from source, who is the one person for
/// whom "download it" is the wrong remedy.
const DEVELOPMENT_REMEDY: &str = "In a development build, run `script/build-database-drivers` \
     (or `make drivers`) instead";

/// How far along an install is, for a progress bar that means something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallProgress {
    FetchingManifest,
    /// `total` is absent only if the release omitted the size.
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    Verifying,
    Unpacking,
}

/// `Arc` rather than `anyhow::Error` because the future is shared: two windows
/// asking for the same driver await one download and must both see why it
/// failed.
pub type InstallResult = Result<PathBuf, Arc<anyhow::Error>>;

type SharedInstall = Shared<BoxFuture<'static, InstallResult>>;

/// One install of one driver, however many callers are waiting on it.
struct Running {
    /// Which install this is, so that when it finishes it removes its own entry
    /// and not that of a later one that replaced it.
    run: u64,
    install: SharedInstall,
    /// Callers that asked and have neither been answered nor given up.
    waiters: usize,
}

pub struct DriverInstaller {
    http: Arc<dyn HttpClient>,
    endpoint: DriverEndpoint,
    root: PathBuf,
    trusted_key: [u8; 32],
    minimum_version: String,
    idle_timeout: Duration,
    next_run: AtomicU64,
    in_flight: Mutex<HashMap<String, Running>>,
    watchers: Mutex<HashMap<String, Vec<mpsc::UnboundedSender<InstallProgress>>>>,
}

impl DriverInstaller {
    pub fn new(http: Arc<dyn HttpClient>, endpoint: DriverEndpoint) -> Self {
        Self::with_root(http, endpoint, store::root().to_path_buf())
    }

    pub fn with_root(http: Arc<dyn HttpClient>, endpoint: DriverEndpoint, root: PathBuf) -> Self {
        Self {
            http,
            endpoint,
            root,
            trusted_key: DRIVER_MANIFEST_PUBLIC_KEY,
            minimum_version: store::MINIMUM_DRIVER_SET_VERSION.to_string(),
            idle_timeout: IDLE_TIMEOUT,
            next_run: AtomicU64::new(0),
            in_flight: Mutex::default(),
            watchers: Mutex::default(),
        }
    }

    /// Trusts `public_key` in place of the one built into the app.
    ///
    /// For tests, which sign with a key of their own. Not available to a
    /// shipped build, so nothing at runtime can swap the trust anchor.
    #[cfg(any(test, feature = "test-support"))]
    pub fn trusting(mut self, public_key: [u8; 32]) -> Self {
        self.trusted_key = public_key;
        self
    }

    /// Accepts driver sets from `version` up, in place of the minimum built into
    /// the app, which no test fixture is old enough to be above.
    #[cfg(any(test, feature = "test-support"))]
    pub fn accepting_versions_from(mut self, version: &str) -> Self {
        self.minimum_version = version.to_string();
        self
    }

    /// How many callers are waiting on the install of `id` right now.
    #[cfg(any(test, feature = "test-support"))]
    pub fn waiters(&self, id: &str) -> usize {
        self.in_flight
            .lock()
            .get(id)
            .map_or(0, |running| running.waiters)
    }

    #[cfg(test)]
    fn giving_up_after(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    /// Progress reports for whichever install of `id` is running or about to.
    ///
    /// Registered before awaiting the install, so a second caller joining an
    /// download already in flight still sees the bar move.
    pub fn watch(&self, id: &str) -> mpsc::UnboundedReceiver<InstallProgress> {
        let (sender, receiver) = mpsc::unbounded();
        self.watchers
            .lock()
            .entry(id.to_string())
            .or_default()
            .push(sender);
        receiver
    }

    /// Downloads and installs a driver, or joins the download already running.
    ///
    /// One download per driver, however many windows ask: the archives run to
    /// tens of megabytes, and two windows opened on the same project asking at
    /// once is the ordinary case rather than a rare one.
    ///
    /// Every caller is expected to end with either the result or [`forget`]; the
    /// count of those still waiting is what decides when a download may stop.
    ///
    /// [`forget`]: Self::forget
    pub fn install(self: &Arc<Self>, id: &str) -> SharedInstall {
        let mut in_flight = self.in_flight.lock();
        if let Some(running) = in_flight.get_mut(id) {
            running.waiters += 1;
            return running.install.clone();
        }
        let run = self.next_run.fetch_add(1, Ordering::Relaxed);
        let install = {
            let installer = self.clone();
            let id = id.to_string();
            async move { installer.run(&id, run).await.map_err(Arc::new) }
                .boxed()
                .shared()
        };
        in_flight.insert(
            id.to_string(),
            Running {
                run,
                install: install.clone(),
                waiters: 1,
            },
        );
        install
    }

    /// One caller giving up on its [`install`](Self::install).
    ///
    /// The download goes on for whoever else is still waiting, and is dropped
    /// only when the last one leaves: a shared future advances only while
    /// something awaits it, so a download nobody wants is left parked in the
    /// map, and the next Download button would resume one the person had
    /// already walked away from. Dropping it is also what cancels it -- the
    /// callers' own clones went with their tasks.
    pub fn forget(&self, id: &str) {
        let mut in_flight = self.in_flight.lock();
        let Some(running) = in_flight.get_mut(id) else {
            return;
        };
        running.waiters = running.waiters.saturating_sub(1);
        if running.waiters > 0 {
            return;
        }
        in_flight.remove(id);
        drop(in_flight);
        self.watchers.lock().remove(id);
    }

    async fn run(self: Arc<Self>, id: &str, run: u64) -> Result<PathBuf> {
        let installed = self.install_once(id, run).await;
        // Cleared whether it succeeded or failed: a failure that stays cached
        // would make Retry a button that replays the same error forever. Only
        // this run's own entry, though: one that gave up and was followed by a
        // fresh install has an entry that is not ours to take.
        let mut in_flight = self.in_flight.lock();
        if in_flight.get(id).is_some_and(|running| running.run == run) {
            in_flight.remove(id);
            drop(in_flight);
            self.watchers.lock().remove(id);
        }
        installed
    }

    async fn install_once(&self, id: &str, run: u64) -> Result<PathBuf> {
        let target = current_target().with_context(|| {
            format!(
                "Zode does not publish database drivers for {}-{}",
                std::env::consts::ARCH,
                std::env::consts::OS
            )
        })?;

        self.report(id, InstallProgress::FetchingManifest);
        let manifest = self.fetch_signed_manifest().await?;
        let version = manifest.version.as_str();
        let asset = manifest.find(id, target).with_context(|| {
            let published = manifest.targets_for(id);
            if published.is_empty() {
                format!(
                    "version {version} publishes no `{id}` driver. {DEVELOPMENT_REMEDY}, or \
                     install the driver by hand into {}",
                    self.root.display()
                )
            } else {
                format!(
                    "version {version} has no `{id}` driver for {target} (it publishes: {}). \
                     {DEVELOPMENT_REMEDY}, or install the driver by hand into {}",
                    published.join(", "),
                    self.root.display()
                )
            }
        })?;
        let expected = parse_sha256(&asset.asset, &asset.sha256)?;

        // Everything that can be refused without downloading is, because the
        // archive is tens of megabytes and the person is watching a bar.
        store::ensure_not_a_rollback_in(&self.root, version, &self.minimum_version)?;
        ensure_driver_speaks_our_protocol(id, asset)?;
        anyhow::ensure!(
            asset.size <= MAX_ARCHIVE_BYTES,
            "the manifest says the `{id}` driver is {} bytes, more than the {MAX_ARCHIVE_BYTES} \
             Zode will download",
            asset.size
        );

        let directory = store::version_dir_in(&self.root, id, version);
        smol::fs::create_dir_all(directory.parent().unwrap_or(&self.root))
            .await
            .with_context(|| format!("creating {}", self.root.display()))?;

        // Downloaded beside where it will be unpacked, so nothing crosses a
        // filesystem boundary and the final move stays a rename. Named per
        // install, so two never write the same file, and held by guards so that
        // one dropped partway leaves neither behind.
        let scratch = format!("{}-{run}", std::process::id());
        let archive = store::Scratch::at(store::hidden_sibling(
            &directory,
            &format!("{scratch}.download"),
        ));
        let staging = store::Scratch::at(store::hidden_sibling(
            &directory,
            &format!("{scratch}.incoming"),
        ));

        let actual = self
            .download_to(
                id,
                &self.endpoint.asset_url(version, &asset.asset),
                archive.path(),
                asset.size,
            )
            .await?;
        self.report(id, InstallProgress::Verifying);
        anyhow::ensure!(
            actual == expected,
            "the downloaded `{id}` driver does not match the checksum the manifest publishes \
             (expected {expected}, got {actual}); it has been discarded"
        );

        // Recorded before anything is unpacked, so that no driver of a version
        // is ever on disk without the machine remembering it has seen it.
        store::record_install_in(&self.root, version)?;
        self.report(id, InstallProgress::Unpacking);
        let installed = unpack::install_archive(
            archive.path(),
            &asset.entry,
            staging.path(),
            &directory,
            &store::executable_name(id),
        )
        .await?;

        // Off the calling thread: this removes the previous driver set, which
        // on Windows can be slow while a scanner reads the old executable.
        let (root, id, keep) = (self.root.clone(), id.to_string(), version.to_string());
        smol::unblock(move || store::prune_other_versions_in(&root, &id, &keep)).await?;
        Ok(installed)
    }

    /// Streams an asset to `destination`, returning its SHA-256.
    ///
    /// Hashed as it arrives rather than by re-reading the file: the bytes are
    /// already in hand, and re-reading leaves a window in which what was
    /// checked and what gets unpacked are not provably the same file.
    ///
    /// Bounded both ways: it stops the moment more arrives than the manifest
    /// said would, and it gives up on a connection that goes quiet. Without the
    /// first a server that never stops fills the disk, and without the second
    /// the bar sits there until someone cancels.
    async fn download_to(
        &self,
        id: &str,
        url: &str,
        destination: &std::path::Path,
        expected_size: u64,
    ) -> Result<String> {
        let mut response = self
            .get(url, &format!("downloading the `{id}` driver from {url}"))
            .await?;
        // A publish deletes the version a manifest names, so an archive the
        // manifest just promised can be gone by the time it is asked for.
        anyhow::ensure!(
            response.status() != StatusCode::NOT_FOUND,
            "the published `{id}` driver set changed while it was being downloaded (the server \
             no longer has {url}); try again to install the current one"
        );
        anyhow::ensure!(
            response.status().is_success(),
            "downloading the `{id}` driver failed with status {}",
            response.status()
        );

        let mut file = smol::fs::File::create(destination)
            .await
            .with_context(|| format!("creating {}", destination.display()))?;
        let total = (expected_size > 0).then_some(expected_size);
        let limit = total.unwrap_or(MAX_ARCHIVE_BYTES);
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];
        let stalled = format!("the `{id}` driver download");

        loop {
            let read = self
                .within_idle_timeout(
                    &stalled,
                    response.body_mut().read(&mut buffer).map(|read| {
                        read.with_context(|| format!("reading the `{id}` driver from {url}"))
                    }),
                )
                .await?;
            if read == 0 {
                break;
            }
            received += read as u64;
            anyhow::ensure!(
                received <= limit,
                "the `{id}` driver download is larger than the {limit} bytes it was published \
                 as; stopped at {received}"
            );
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read])
                .await
                .with_context(|| format!("writing {}", destination.display()))?;
            self.report(id, InstallProgress::Downloading { received, total });
        }
        file.flush().await?;
        // Said as what it is: a short body is a connection that dropped, and
        // reporting it as a checksum failure sent people after the wrong cause.
        anyhow::ensure!(
            total.is_none_or(|total| received == total),
            "the download of the `{id}` driver was interrupted: {received} of {expected_size} \
             bytes arrived"
        );

        Ok(hex(hasher.finalize().as_slice()))
    }

    /// The manifest of the most recently published drivers, once its signature
    /// has been checked.
    ///
    /// `latest` is asked only which version is current. What is acted on is the
    /// signed manifest published under that version, because `latest` is the
    /// backend's own re-serialisation of the manifest it accepted and carries no
    /// signature at all. The two are not compared beyond the version: a signature
    /// either holds or it does not.
    ///
    /// Fetched afresh for every install and never kept: a publish deletes the
    /// version a held manifest names, so one remembered from an earlier install
    /// would send the next one to an archive that is no longer there.
    async fn fetch_signed_manifest(&self) -> Result<DriverManifest> {
        let latest_url = self.endpoint.latest_manifest_url();
        let mut response = self
            .get(
                &latest_url,
                &format!("fetching the driver manifest from {latest_url}"),
            )
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "no driver manifest is published at {latest_url} (the server answered {}). \
             {DEVELOPMENT_REMEDY}.",
            response.status()
        );
        let latest = self
            .read_limited(
                response.body_mut(),
                MAX_MANIFEST_BYTES,
                &format!("the driver manifest at {latest_url}"),
            )
            .await?;
        let version = serde_json::from_slice::<Advertised>(&latest)
            .context("the driver manifest is not the shape Zode expects")?
            .version;
        anyhow::ensure!(
            store::is_version(&version),
            "the driver manifest names {version:?} as its version, which is not a published \
             driver version (MAJOR.MINOR.PATCH, optionally followed by -prerelease)"
        );

        let signed_url = self.endpoint.asset_url(&version, SIGNED_MANIFEST_ASSET);
        let mut response = self
            .get(
                &signed_url,
                &format!("fetching the signed driver manifest from {signed_url}"),
            )
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(self
                .explain_missing_signed_manifest(&version, &signed_url)
                .await);
        }
        anyhow::ensure!(
            response.status().is_success(),
            "the signed driver manifest at {signed_url} could not be fetched (the server \
             answered {})",
            response.status()
        );
        let bundle = self
            .read_limited(
                response.body_mut(),
                MAX_MANIFEST_BYTES,
                &format!("the signed driver manifest at {signed_url}"),
            )
            .await?;

        let manifest =
            DriverManifest::parse(signature::verify_bundle(&bundle, &self.trusted_key)?)?;
        anyhow::ensure!(
            manifest.version == version,
            "the signed driver manifest is for version {}, not the {version} that is published",
            manifest.version
        );
        Ok(manifest)
    }

    /// Why there is no signed manifest under a version `latest` just named.
    ///
    /// Two things look the same from the signed manifest's own 404. A release
    /// published before manifests were signed has none, and says so for good. A
    /// publish that landed between the two requests deleted the whole version,
    /// signed manifest and all, and asking again will find the new one -- so
    /// calling that "not signed" sends someone after a problem that is not there.
    /// The version's own manifest tells them apart: it is there in the first case
    /// and gone in the second.
    async fn explain_missing_signed_manifest(
        &self,
        version: &str,
        signed_url: &str,
    ) -> anyhow::Error {
        let manifest_url = self.endpoint.asset_url(version, MANIFEST_ASSET);
        let still_published = self
            .get(
                &manifest_url,
                &format!("checking that version {version} is still published at {manifest_url}"),
            )
            .await
            .map(|response| response.status());
        match still_published {
            Ok(StatusCode::NOT_FOUND) => anyhow::anyhow!(
                "the published driver set changed while it was being read (version {version} is \
                 no longer on the server); try again to install the current one"
            ),
            Ok(status) if status.is_success() => anyhow::anyhow!(
                "the drivers published as version {version} are not signed, and Zode will not \
                 run drivers it cannot verify (nothing at {signed_url}); publishing that release \
                 again signs it. {DEVELOPMENT_REMEDY}."
            ),
            Ok(status) => anyhow::anyhow!(
                "there is no signed driver manifest at {signed_url}, and the server answered \
                 {status} when asked whether version {version} is still published; try again"
            ),
            Err(error) => anyhow::anyhow!(
                "there is no signed driver manifest at {signed_url}, and whether version \
                 {version} is still published could not be checked ({error:#}); try again"
            ),
        }
    }

    /// A GET against the configured origin, which gives up on a server that
    /// accepts the connection and then says nothing.
    async fn get(&self, url: &str, what: &str) -> Result<Response<AsyncBody>> {
        self.endpoint.ensure_trusted(url)?;
        self.within_idle_timeout(
            what,
            self.http
                .get(url, Default::default(), false)
                .map(|response| response.with_context(|| what.to_string())),
        )
        .await
    }

    /// The whole of `body`, refusing to hold more than `limit` bytes of it.
    ///
    /// One byte past the limit is read before refusing, so a body of exactly the
    /// limit is accepted and anything longer is told apart from it.
    async fn read_limited(&self, body: &mut AsyncBody, limit: u64, what: &str) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut buffer = vec![0u8; 16 * 1024];
        loop {
            let read = self
                .within_idle_timeout(
                    what,
                    body.read(&mut buffer)
                        .map(|read| read.with_context(|| format!("reading {what}"))),
                )
                .await?;
            if read == 0 {
                return Ok(bytes);
            }
            bytes.extend_from_slice(&buffer[..read]);
            anyhow::ensure!(
                bytes.len() as u64 <= limit,
                "{what} is larger than {limit} bytes, which no manifest Zode publishes comes \
                 close to; refusing to read it"
            );
        }
    }

    /// Runs `step`, failing instead of waiting on it for longer than the idle
    /// timeout.
    ///
    /// Real time on purpose, and not the executor's timer the lint points to:
    /// this crate does not depend on gpui when only `install` is enabled, and a
    /// test scheduler is free to fast-forward its own clock while a real read is
    /// still in flight, which would time out an install that was fine.
    #[allow(
        clippy::disallowed_methods,
        reason = "an idle timeout is wall-clock time; see above"
    )]
    async fn within_idle_timeout<T>(
        &self,
        what: &str,
        step: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let timeout = self.idle_timeout;
        smol::future::or(step, async move {
            smol::Timer::after(timeout).await;
            anyhow::bail!("{what} stalled: nothing arrived for {timeout:?}")
        })
        .await
    }

    fn report(&self, id: &str, progress: InstallProgress) {
        let mut watchers = self.watchers.lock();
        if let Some(senders) = watchers.get_mut(id) {
            senders.retain(|sender| sender.unbounded_send(progress).is_ok());
        }
    }
}

/// All that is read of the manifest served under `latest`.
#[derive(Deserialize)]
struct Advertised {
    version: String,
}

/// Refuses a driver this build cannot talk to, before it is downloaded.
///
/// Otherwise the answer comes after the whole archive has been fetched,
/// verified and installed, at the handshake, in words that suggest downloading
/// it again -- which fetches the same driver. A row that says nothing about its
/// protocol is let through: the handshake still has the last word.
fn ensure_driver_speaks_our_protocol(id: &str, asset: &DriverAsset) -> Result<()> {
    let Some(theirs) = asset.protocol else {
        return Ok(());
    };
    if version_is_compatible(PROTOCOL_VERSION, theirs) {
        return Ok(());
    }
    if theirs > PROTOCOL_VERSION {
        anyhow::bail!(
            "the published `{id}` driver speaks database protocol version {theirs}, but this \
             build of Zode speaks {PROTOCOL_VERSION}. Update Zode to use it."
        );
    }
    anyhow::bail!(
        "the published `{id}` driver speaks database protocol version {theirs}, older than the \
         {PROTOCOL_VERSION} this build of Zode speaks, so no driver has been published for this \
         version of Zode yet. If you build Zode from source, run `make drivers`."
    );
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::test_support::{
        archive_asset_for, directory_entries, driver_archive, driver_manifest_for, manifest_for,
        sha256_of, signed,
    };
    use futures::StreamExt as _;
    use http_client::{AsyncBody, FakeHttpClient, Response};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const VERSION: &str = "0.1.1";

    fn manifest_json(archive: &[u8], sha256: String) -> Vec<u8> {
        manifest_for("postgres", archive, sha256, VERSION)
    }

    fn signed_manifest_for(archive: &[u8]) -> Vec<u8> {
        signed(&driver_manifest_for(
            "postgres",
            archive,
            sha256_of(archive),
            VERSION,
        ))
    }

    fn archive_path(version: &str) -> String {
        format!("/api/drivers/{version}/{}", archive_asset_for("postgres"))
    }

    /// What the server answers at the signed manifest's path.
    #[derive(Clone)]
    enum Signed {
        /// The legacy manifest it is asked about, signed with the test key, with
        /// the protocol this build speaks on every row -- what the release
        /// script publishes.
        Properly,
        /// Nothing there: a release published before manifests were signed.
        Nowhere,
        /// Nothing there, and nothing else of that version either: a publish
        /// landed after `latest` was read and deleted the whole version.
        Withdrawn,
        /// Exactly these bytes, whatever they are.
        Serving(Vec<u8>),
    }

    /// Every way the fake server can be told to behave, so that a test names the
    /// one thing it varies and gets the rest as a healthy release.
    struct Server {
        manifests: Vec<Vec<u8>>,
        archive: Vec<u8>,
        versions: Vec<String>,
        signed: Signed,
        archive_requests: Arc<AtomicUsize>,
        paths: Arc<Mutex<Vec<String>>>,
    }

    impl Server {
        fn publishing(manifests: Vec<Vec<u8>>, archive: Vec<u8>, versions: &[&str]) -> Self {
            Self {
                manifests,
                archive,
                versions: versions.iter().map(|version| version.to_string()).collect(),
                signed: Signed::Properly,
                archive_requests: Arc::default(),
                paths: Arc::default(),
            }
        }

        fn signed(mut self, signed: Signed) -> Self {
            self.signed = signed;
            self
        }

        /// The server's real URL contract and nothing looser: the manifest
        /// answers only at `/drivers/latest/`, a signed manifest and an archive
        /// only under one of the versions published, and every other path is a
        /// 404 -- so a client that builds a URL wrongly fails a test instead of
        /// passing against a fake that matched on a suffix.
        ///
        /// Each manifest request is answered with the next of `manifests`, the
        /// last repeating.
        fn start(&self) -> Arc<dyn HttpClient> {
            let manifest_path = format!("/api/drivers/latest/{MANIFEST_ASSET}");
            let archive_paths: Vec<String> = self
                .versions
                .iter()
                .map(|version| archive_path(version))
                .collect();
            let mut served_by_path: Vec<(String, Vec<u8>)> = self
                .manifests
                .iter()
                .filter_map(|bytes| DriverManifest::parse(bytes).ok())
                .filter_map(|manifest| {
                    let path = signed_path(&manifest.version);
                    match &self.signed {
                        Signed::Properly => {
                            let mut manifest = manifest;
                            for driver in &mut manifest.drivers {
                                driver.protocol = Some(PROTOCOL_VERSION);
                            }
                            Some((path, signed(&manifest)))
                        }
                        Signed::Nowhere | Signed::Withdrawn => None,
                        Signed::Serving(bytes) => Some((path, bytes.clone())),
                    }
                })
                .collect();
            // A published version carries its own manifest under its own name,
            // which is what tells a release that was never signed from one that
            // is gone.
            if !matches!(self.signed, Signed::Withdrawn) {
                served_by_path.extend(self.manifests.iter().filter_map(|bytes| {
                    let manifest = DriverManifest::parse(bytes).ok()?;
                    let path = format!("/api/drivers/{}/{MANIFEST_ASSET}", manifest.version);
                    Some((path, bytes.clone()))
                }));
            }
            let manifests = self.manifests.clone();
            let archive = self.archive.clone();
            let archive_requests = self.archive_requests.clone();
            let paths = self.paths.clone();
            let manifests_served = Arc::new(AtomicUsize::new(0));
            FakeHttpClient::create(move |request| {
                let manifests = manifests.clone();
                let archive = archive.clone();
                let archive_requests = archive_requests.clone();
                let paths = paths.clone();
                let manifest_path = manifest_path.clone();
                let archive_paths = archive_paths.clone();
                let served_by_path = served_by_path.clone();
                let manifests_served = manifests_served.clone();
                async move {
                    let path = request.uri().path().to_string();
                    paths.lock().push(path.clone());
                    let body = if path == manifest_path {
                        let turn = manifests_served.fetch_add(1, Ordering::SeqCst);
                        manifests[turn.min(manifests.len() - 1)].clone()
                    } else if let Some((_, bytes)) =
                        served_by_path.iter().find(|(signed, _)| *signed == path)
                    {
                        bytes.clone()
                    } else if archive_paths.contains(&path) {
                        archive_requests.fetch_add(1, Ordering::SeqCst);
                        archive
                    } else {
                        return Ok(Response::builder().status(404).body(Default::default())?);
                    };
                    Ok(Response::builder().status(200).body(body.into())?)
                }
            })
        }
    }

    fn signed_path(version: &str) -> String {
        format!("/api/drivers/{version}/{SIGNED_MANIFEST_ASSET}")
    }

    /// A server publishing `manifest` under `latest` and one archive under
    /// [`VERSION`], counting archive fetches so a test can prove a download
    /// happened once rather than twice.
    fn release_serving(
        manifest: Vec<u8>,
        archive: Vec<u8>,
        requests: Arc<AtomicUsize>,
    ) -> Arc<dyn HttpClient> {
        let mut server = Server::publishing(vec![manifest], archive, &[VERSION]);
        server.archive_requests = requests;
        server.start()
    }

    fn release_publishing(
        manifests: Vec<Vec<u8>>,
        archive: Vec<u8>,
        versions: &[&str],
        archive_requests: Arc<AtomicUsize>,
        paths: Arc<Mutex<Vec<String>>>,
    ) -> Arc<dyn HttpClient> {
        let mut server = Server::publishing(manifests, archive, versions);
        server.archive_requests = archive_requests;
        server.paths = paths;
        server.start()
    }

    fn installer(http: Arc<dyn HttpClient>, root: &std::path::Path) -> Arc<DriverInstaller> {
        Arc::new(
            DriverInstaller::with_root(
                http,
                DriverEndpoint::new("https://api.test.invalid/api"),
                root.to_path_buf(),
            )
            .trusting(signature::testing::public_key())
            .accepting_versions_from("0.0.0")
            .giving_up_after(Duration::from_secs(5)),
        )
    }

    #[test]
    fn a_verified_driver_lands_where_the_resolver_looks() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, sha256_of(&archive));
            let installer = installer(
                release_serving(manifest, archive, Arc::default()),
                root.path(),
            );

            let installed = installer.install("postgres").await.unwrap();

            assert_eq!(
                store::installed_path_in(root.path(), "postgres"),
                Some(installed)
            );
        });
    }

    /// The whole reason the hash is computed while the bytes stream. An asset
    /// that does not match must leave nothing behind -- not a partial install,
    /// and not the archive either, whose mere presence would let a later
    /// attempt skip the download.
    #[test]
    fn an_asset_that_fails_its_checksum_installs_nothing() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, "a".repeat(64));
            let installer = installer(
                release_serving(manifest, archive, Arc::default()),
                root.path(),
            );

            let error = installer
                .install("postgres")
                .await
                .expect_err("a driver that does not match its checksum must be refused");

            assert!(error.to_string().contains("checksum"), "{error}");
            assert_eq!(store::installed_path_in(root.path(), "postgres"), None);
            assert!(
                directory_entries(root.path(), "postgres").is_empty(),
                "the rejected archive must not be left to be trusted later: {:?}",
                directory_entries(root.path(), "postgres")
            );
        });
    }

    /// Two windows opened on one project asking at once is the ordinary case,
    /// and these archives run to tens of megabytes.
    #[test]
    fn two_callers_asking_at_once_share_one_download() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, sha256_of(&archive));
            let requests = Arc::new(AtomicUsize::new(0));
            let installer = installer(
                release_serving(manifest, archive, requests.clone()),
                root.path(),
            );

            let first = installer.install("postgres");
            let second = installer.install("postgres");
            let (first, second) = futures::join!(first, second);

            assert!(first.is_ok() && second.is_ok(), "{first:?} {second:?}");
            assert_eq!(
                requests.load(Ordering::SeqCst),
                1,
                "one driver asked for twice at once must be downloaded once"
            );
        });
    }

    /// A failure that stayed cached would make Retry a button that replays the
    /// same error forever.
    #[test]
    fn a_failed_install_can_be_retried() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let good = sha256_of(&archive);
            let failing = installer(
                release_serving(
                    manifest_json(&archive, "a".repeat(64)),
                    archive.clone(),
                    Arc::default(),
                ),
                root.path(),
            );
            failing.install("postgres").await.unwrap_err();
            assert!(
                failing.in_flight.lock().is_empty(),
                "a finished install must not stay in flight, or Retry replays it forever"
            );

            // A second installer stands in for the release being fixed; what is
            // asserted is that the first one left nothing behind to trip it.
            let fixed = installer(
                release_serving(manifest_json(&archive, good), archive, Arc::default()),
                root.path(),
            );
            fixed.install("postgres").await.unwrap();
        });
    }

    /// The manifest is what says which drivers a release published. Without it
    /// there is nothing to check an asset against, so there is nothing to do
    /// but say so -- and say the thing that fixes it in a checkout.
    #[test]
    fn a_release_with_no_manifest_says_so_and_names_the_remedy() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let installer = installer(FakeHttpClient::with_404_response(), root.path());

            let error = installer.install("postgres").await.unwrap_err();

            let message = error.to_string();
            assert!(message.contains("no driver manifest"), "{message}");
            assert!(message.contains("/drivers/latest/"), "{message}");
            assert!(message.contains("build-database-drivers"), "{message}");
        });
    }

    /// A configured base that cannot be trusted -- here, one that does not even
    /// parse -- must stop the install before the manifest is fetched at all.
    /// Fails closed the same way an origin mismatch would, and proves it with
    /// the fake client's request count staying at zero.
    #[test]
    fn an_untrusted_origin_is_refused_before_any_request() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let requests = Arc::new(AtomicUsize::new(0));
            let http = {
                let requests = requests.clone();
                FakeHttpClient::create(move |_| {
                    let requests = requests.clone();
                    async move {
                        requests.fetch_add(1, Ordering::SeqCst);
                        Ok(Response::builder().status(200).body(Default::default())?)
                    }
                })
            };
            let installer = Arc::new(DriverInstaller::with_root(
                http,
                DriverEndpoint::new("not a url"),
                root.path().to_path_buf(),
            ));

            installer.install("postgres").await.expect_err(
                "an unparsable configured base cannot be compared against, so it must be refused",
            );

            assert_eq!(
                requests.load(Ordering::SeqCst),
                0,
                "an untrusted origin must be refused before a request is made"
            );
        });
    }

    /// A driver the release does not carry for this platform. The 404 that a
    /// blind fetch would produce says nothing about why.
    #[test]
    fn a_driver_the_release_does_not_publish_is_named_before_anything_is_fetched() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, sha256_of(&archive));
            let installer = installer(
                release_serving(manifest, archive, Arc::default()),
                root.path(),
            );

            let error = installer.install("mongodb").await.unwrap_err();

            let message = error.to_string();
            assert!(message.contains("mongodb"), "{message}");
            assert!(message.contains("publishes no"), "{message}");
            assert!(message.contains(VERSION), "{message}");
        });
    }

    #[test]
    fn a_network_that_is_not_there_reports_the_network() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let http =
                FakeHttpClient::create(
                    |_| async move { anyhow::bail!("the network is unreachable") },
                );
            let installer = installer(http, root.path());

            let error = installer.install("postgres").await.unwrap_err();
            assert!(error.to_string().contains("driver manifest"), "{error}");
        });
    }

    /// What is installed, and from where, is whatever the manifest under
    /// `latest` names; nothing about the running app enters into it.
    #[test]
    fn the_archive_is_fetched_from_the_version_the_manifest_names() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_for("postgres", &archive, sha256_of(&archive), "0.2.0");
            let paths = Arc::new(Mutex::new(Vec::new()));
            let installer = installer(
                release_publishing(
                    vec![manifest],
                    archive,
                    &["0.2.0"],
                    Arc::default(),
                    paths.clone(),
                ),
                root.path(),
            );

            let installed = installer.install("postgres").await.unwrap();

            assert_eq!(
                paths.lock().as_slice(),
                [
                    format!("/api/drivers/latest/{MANIFEST_ASSET}"),
                    signed_path("0.2.0"),
                    archive_path("0.2.0"),
                ]
            );
            assert_eq!(
                installed,
                store::version_dir_in(root.path(), "postgres", "0.2.0")
                    .join(store::executable_name("postgres"))
            );
        });
    }

    /// A publish deletes the version a held manifest names, so a manifest kept
    /// from the last install would point the next one at a 404.
    #[test]
    fn each_install_reads_the_manifest_again() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest_at =
                |version| manifest_for("postgres", &archive, sha256_of(&archive), version);
            let paths = Arc::new(Mutex::new(Vec::new()));
            let installer = installer(
                release_publishing(
                    vec![manifest_at("0.1.0"), manifest_at("0.2.0")],
                    archive.clone(),
                    &["0.1.0", "0.2.0"],
                    Arc::default(),
                    paths.clone(),
                ),
                root.path(),
            );

            installer.install("postgres").await.unwrap();
            installer.install("postgres").await.unwrap();

            assert_eq!(
                paths.lock().as_slice(),
                [
                    format!("/api/drivers/latest/{MANIFEST_ASSET}"),
                    signed_path("0.1.0"),
                    archive_path("0.1.0"),
                    format!("/api/drivers/latest/{MANIFEST_ASSET}"),
                    signed_path("0.2.0"),
                    archive_path("0.2.0"),
                ]
            );
            assert_eq!(
                store::installed_path_in(root.path(), "postgres"),
                Some(
                    store::version_dir_in(root.path(), "postgres", "0.2.0")
                        .join(store::executable_name("postgres"))
                )
            );
            assert_eq!(
                store::installed_versions_in(root.path(), "postgres"),
                ["0.2.0"],
                "the version the new install replaced must be pruned"
            );
        });
    }

    /// The version becomes a URL segment and a directory name. One that is not
    /// a version must stop the install before either, however it got there.
    #[test]
    fn a_manifest_naming_no_usable_version_fetches_no_archive() {
        smol::block_on(async {
            let outer = tempfile::tempdir().unwrap();
            let root = outer.path().join("store");
            std::fs::create_dir_all(&root).unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_for("postgres", &archive, sha256_of(&archive), "../escape");
            let archive_requests = Arc::new(AtomicUsize::new(0));
            let installer = installer(
                release_publishing(
                    vec![manifest],
                    archive,
                    &["../escape"],
                    archive_requests.clone(),
                    Arc::default(),
                ),
                &root,
            );

            let error = installer.install("postgres").await.unwrap_err();

            assert!(error.to_string().contains("../escape"), "{error}");
            assert_eq!(
                archive_requests.load(Ordering::SeqCst),
                0,
                "nothing may be fetched under a version that is not one"
            );
            assert_eq!(
                std::fs::read_dir(&root).unwrap().count(),
                0,
                "nothing may be created in the store"
            );
            assert_eq!(
                std::fs::read_dir(outer.path()).unwrap().count(),
                1,
                "nothing may be created beside the store either"
            );
        });
    }

    /// Progress must reach a caller that registered before awaiting, or the
    /// bar the modal draws never moves.
    #[test]
    fn an_install_reports_its_progress() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, sha256_of(&archive));
            let installer = installer(
                release_serving(manifest, archive, Arc::default()),
                root.path(),
            );

            let progress = installer.watch("postgres");
            installer.install("postgres").await.unwrap();
            let reported: Vec<_> = progress.collect().await;

            assert!(
                reported.contains(&InstallProgress::FetchingManifest),
                "{reported:?}"
            );
            assert!(
                reported.contains(&InstallProgress::Verifying),
                "{reported:?}"
            );
            assert!(
                reported.contains(&InstallProgress::Unpacking),
                "{reported:?}"
            );
            assert!(
                reported.iter().any(|step| matches!(
                    step,
                    InstallProgress::Downloading { total: Some(_), .. }
                )),
                "a determinate bar needs a total: {reported:?}"
            );
        });
    }

    fn manifest_path() -> String {
        format!("/api/drivers/latest/{MANIFEST_ASSET}")
    }

    /// A publish deletes the installed version's archive, and the newer one's
    /// archive is not served either. The install fails, and what is installed
    /// stays exactly as it was.
    #[test]
    fn a_newer_version_whose_archive_is_withdrawn_leaves_the_installed_driver_in_place() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let paths = Arc::new(Mutex::new(Vec::new()));
            let installer = installer(
                release_publishing(
                    vec![
                        manifest_for("postgres", &archive, sha256_of(&archive), "0.1.0"),
                        manifest_for("postgres", &archive, sha256_of(&archive), "0.2.0"),
                    ],
                    archive,
                    &["0.1.0"],
                    Arc::default(),
                    paths.clone(),
                ),
                root.path(),
            );
            let installed = installer.install("postgres").await.unwrap();

            let error = installer.install("postgres").await.unwrap_err();

            let message = error.to_string();
            assert!(
                message.contains("changed while it was being downloaded"),
                "{message}"
            );
            assert!(message.contains("try again"), "{message}");
            assert!(
                !message.contains("status"),
                "a withdrawn archive is not a bare status code: {message}"
            );
            assert_eq!(
                store::installed_path_in(root.path(), "postgres"),
                Some(installed)
            );
            assert_eq!(directory_entries(root.path(), "postgres"), ["0.1.0"]);
            let asked = paths.lock().clone();
            assert_eq!(
                asked.last(),
                Some(&archive_path("0.2.0")),
                "the newer archive is what was asked for last"
            );
        });
    }

    #[test]
    fn a_newer_version_failing_its_checksum_leaves_the_installed_driver_in_place() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let installer = installer(
                release_publishing(
                    vec![
                        manifest_for("postgres", &archive, sha256_of(&archive), "0.1.0"),
                        manifest_for("postgres", &archive, "a".repeat(64), "0.2.0"),
                    ],
                    archive,
                    &["0.1.0", "0.2.0"],
                    Arc::default(),
                    Arc::default(),
                ),
                root.path(),
            );
            let installed = installer.install("postgres").await.unwrap();

            let error = installer
                .install("postgres")
                .await
                .expect_err("an archive that does not match its checksum must be refused");

            assert!(error.to_string().contains("checksum"), "{error}");
            assert_eq!(
                store::installed_path_in(root.path(), "postgres"),
                Some(installed)
            );
            assert_eq!(
                directory_entries(root.path(), "postgres"),
                ["0.1.0"],
                "the rejected archive must not be left beside the install"
            );
        });
    }

    /// A manifest naming `../escape` must be refused before any archive is asked
    /// for, and the install already on disk must not be touched.
    #[test]
    fn a_manifest_naming_no_usable_version_leaves_the_installed_driver_and_fetches_no_archive() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let paths = Arc::new(Mutex::new(Vec::new()));
            let installer = installer(
                release_publishing(
                    vec![
                        manifest_for("postgres", &archive, sha256_of(&archive), "0.1.0"),
                        manifest_for("postgres", &archive, sha256_of(&archive), "../escape"),
                    ],
                    archive,
                    &["0.1.0", "../escape"],
                    Arc::default(),
                    paths.clone(),
                ),
                root.path(),
            );
            let installed = installer.install("postgres").await.unwrap();
            let mark = paths.lock().len();

            let error = installer.install("postgres").await.unwrap_err();

            assert!(error.to_string().contains("../escape"), "{error}");
            assert_eq!(
                paths.lock()[mark..].to_vec(),
                [manifest_path()],
                "nothing may be fetched under a version that is not one"
            );
            assert_eq!(
                store::installed_path_in(root.path(), "postgres"),
                Some(installed)
            );
            assert_eq!(directory_entries(root.path(), "postgres"), ["0.1.0"]);
        });
    }

    #[test]
    fn a_latest_that_is_not_a_manifest_installs_nothing() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let paths = Arc::new(Mutex::new(Vec::new()));
            let installer = installer(
                release_publishing(
                    vec![b"<html>bad gateway</html>".to_vec()],
                    archive,
                    &["0.1.0"],
                    Arc::default(),
                    paths.clone(),
                ),
                root.path(),
            );

            let error = installer.install("postgres").await.unwrap_err();

            assert!(
                error.to_string().contains("not the shape Zode expects"),
                "{error}"
            );
            assert!(
                !root.path().join("postgres").exists(),
                "nothing may be created for a manifest that was refused"
            );
            assert_eq!(paths.lock().as_slice(), [manifest_path()]);
        });
    }

    /// Passes its checksum and then fails to unpack. The staging directory the
    /// unpack had already created must not outlive the failure.
    #[test]
    fn an_archive_that_fails_to_unpack_leaves_no_staging_directory_behind() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let installed = installer(
                release_serving(
                    manifest_for("postgres", &archive, sha256_of(&archive), VERSION),
                    archive,
                    Arc::default(),
                ),
                root.path(),
            )
            .install("postgres")
            .await
            .unwrap();

            let rubbish = b"this is not a gzipped tar archive".to_vec();
            let failing = installer(
                release_publishing(
                    vec![manifest_for(
                        "postgres",
                        &rubbish,
                        sha256_of(&rubbish),
                        "0.2.0",
                    )],
                    rubbish,
                    &["0.2.0"],
                    Arc::default(),
                    Arc::default(),
                ),
                root.path(),
            );

            let error = failing.install("postgres").await.unwrap_err();

            assert!(error.to_string().contains("unpacked"), "{error}");
            assert_eq!(
                store::installed_path_in(root.path(), "postgres"),
                Some(installed)
            );
            assert_eq!(
                directory_entries(root.path(), "postgres"),
                [VERSION],
                "a failed unpack must not leave its staging directory behind"
            );
        });
    }

    #[test]
    fn a_prerelease_version_installs_under_its_full_name_and_leaves_nothing_beside_it() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let installer = installer(
                release_publishing(
                    vec![manifest_for(
                        "postgres",
                        &archive,
                        sha256_of(&archive),
                        "0.1.5-rc.1",
                    )],
                    archive,
                    &["0.1.5-rc.1"],
                    Arc::default(),
                    Arc::default(),
                ),
                root.path(),
            );

            let installed = installer.install("postgres").await.unwrap();

            assert_eq!(
                installed,
                store::version_dir_in(root.path(), "postgres", "0.1.5-rc.1")
                    .join(store::executable_name("postgres"))
            );
            assert_eq!(directory_entries(root.path(), "postgres"), ["0.1.5-rc.1"]);
        });
    }

    #[test]
    fn installing_the_same_published_version_twice_leaves_one_install_in_place() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let installer = installer(
                release_publishing(
                    vec![manifest_for(
                        "postgres",
                        &archive,
                        sha256_of(&archive),
                        "0.1.0",
                    )],
                    archive,
                    &["0.1.0"],
                    Arc::default(),
                    Arc::default(),
                ),
                root.path(),
            );

            installer.install("postgres").await.unwrap();
            let installed = installer.install("postgres").await.unwrap();

            assert_eq!(
                installed,
                store::version_dir_in(root.path(), "postgres", "0.1.0")
                    .join(store::executable_name("postgres"))
            );
            assert_eq!(directory_entries(root.path(), "postgres"), ["0.1.0"]);
        });
    }

    /// A status other than 404 means something else went wrong, and saying the
    /// driver set changed would send the reader after the wrong cause.
    #[test]
    fn an_archive_refused_for_another_reason_still_reports_its_status() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, sha256_of(&archive));
            let signed_manifest = signed_manifest_for(&archive);
            let http = FakeHttpClient::create(move |request| {
                let manifest = manifest.clone();
                let signed_manifest = signed_manifest.clone();
                async move {
                    let path = request.uri().path().to_string();
                    if path == manifest_path() {
                        Ok(Response::builder().status(200).body(manifest.into())?)
                    } else if path == signed_path(VERSION) {
                        Ok(Response::builder()
                            .status(200)
                            .body(signed_manifest.into())?)
                    } else {
                        Ok(Response::builder().status(500).body(Default::default())?)
                    }
                }
            });

            let error = installer(http, root.path())
                .install("postgres")
                .await
                .unwrap_err();

            let message = error.to_string();
            assert!(message.contains("status 500"), "{message}");
            assert!(!message.contains("changed while"), "{message}");
            assert!(directory_entries(root.path(), "postgres").is_empty());
        });
    }

    /// Far larger than any manifest could be, so a client that reads the body to
    /// the end rather than stopping at the limit holds all of it in memory.
    #[test]
    fn a_manifest_far_larger_than_any_real_one_is_refused() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive_requests = Arc::new(AtomicUsize::new(0));
            let http = {
                let archive_requests = archive_requests.clone();
                FakeHttpClient::create(move |request| {
                    let archive_requests = archive_requests.clone();
                    async move {
                        if request.uri().path() != manifest_path() {
                            archive_requests.fetch_add(1, Ordering::SeqCst);
                        }
                        Ok(Response::builder()
                            .status(200)
                            .body(AsyncBody::from_reader(
                                futures::io::repeat(b' ').take(64 * 1024 * 1024),
                            ))?)
                    }
                })
            };

            let error = installer(http, root.path())
                .install("postgres")
                .await
                .unwrap_err();

            let message = error.to_string();
            assert!(message.contains("larger than"), "{message}");
            assert!(
                message.contains("1048576"),
                "the limit is what makes the refusal actionable: {message}"
            );
            assert_eq!(archive_requests.load(Ordering::SeqCst), 0);
            assert!(!root.path().join("postgres").exists());
        });
    }

    /// The asset becomes a URL segment, so one that is not an archive name must
    /// stop the install before any archive is asked for -- even when the
    /// manifest naming it carries a perfectly good signature, because the
    /// signature says who wrote it and not that it is sane.
    #[test]
    fn a_manifest_naming_an_asset_that_is_not_an_archive_fetches_no_archive() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let mut manifest =
                driver_manifest_for("postgres", &archive, sha256_of(&archive), VERSION);
            manifest.drivers[0].asset = "../../etc/passwd".to_string();
            let archive_requests = Arc::new(AtomicUsize::new(0));
            let paths = Arc::new(Mutex::new(Vec::new()));
            let mut server = Server::publishing(
                vec![manifest_json(&archive, sha256_of(&archive))],
                archive,
                &[VERSION],
            )
            .signed(Signed::Serving(signed(&manifest)));
            server.archive_requests = archive_requests.clone();
            server.paths = paths.clone();
            let installer = installer(server.start(), root.path());

            let error = installer.install("postgres").await.unwrap_err();

            assert!(error.to_string().contains("../../etc/passwd"), "{error}");
            assert_eq!(
                paths.lock().as_slice(),
                [manifest_path(), signed_path(VERSION)]
            );
            assert_eq!(archive_requests.load(Ordering::SeqCst), 0);
            assert!(!root.path().join("postgres").exists());
        });
    }

    /// Serves `bytes`, and reports what is on disk under `watched` the moment
    /// the first read is made -- which is while the archive is being written.
    struct ObservedBody {
        bytes: futures::io::Cursor<Vec<u8>>,
        observe: Option<Box<dyn FnOnce() + Send + Sync>>,
    }

    impl futures::AsyncRead for ObservedBody {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buffer: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            if let Some(observe) = self.observe.take() {
                observe();
            }
            std::pin::Pin::new(&mut self.bytes).poll_read(cx, buffer)
        }
    }

    /// The temporary archive is named after the version, and a pre-release
    /// version already ends in dot-separated identifiers: derived by swapping
    /// the last extension it became `0.1.5-rc.download`, which is itself a
    /// version.
    #[test]
    fn the_archive_in_flight_is_never_named_like_a_version() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_for("postgres", &archive, sha256_of(&archive), "0.1.5-rc.1");
            let signed_manifest = signed(&driver_manifest_for(
                "postgres",
                &archive,
                sha256_of(&archive),
                "0.1.5-rc.1",
            ));
            let seen = Arc::new(Mutex::new(None));
            let http = {
                let root = root.path().to_path_buf();
                let seen = seen.clone();
                FakeHttpClient::create(move |request| {
                    let manifest = manifest.clone();
                    let signed_manifest = signed_manifest.clone();
                    let archive = archive.clone();
                    let root = root.clone();
                    let seen = seen.clone();
                    async move {
                        if request.uri().path() == manifest_path() {
                            return Ok(Response::builder().status(200).body(manifest.into())?);
                        }
                        if request.uri().path() == signed_path("0.1.5-rc.1") {
                            return Ok(Response::builder()
                                .status(200)
                                .body(signed_manifest.into())?);
                        }
                        let body = ObservedBody {
                            bytes: futures::io::Cursor::new(archive),
                            observe: Some(Box::new(move || {
                                *seen.lock() = Some((
                                    directory_entries(&root, "postgres"),
                                    store::installed_path_in(&root, "postgres"),
                                ));
                            })),
                        };
                        Ok(Response::builder()
                            .status(200)
                            .body(AsyncBody::from_reader(body))?)
                    }
                })
            };

            installer(http, root.path())
                .install("postgres")
                .await
                .unwrap();

            let (entries, resolved) = seen
                .lock()
                .take()
                .expect("the archive body must have been read");
            assert_eq!(entries.len(), 1, "{entries:?}");
            assert!(
                entries[0].starts_with(".0.1.5-rc.1.") && entries[0].ends_with(".download"),
                "{entries:?}"
            );
            assert!(entries.iter().all(|name| !store::is_version(name)));
            assert_eq!(resolved, None);
            assert_eq!(directory_entries(root.path(), "postgres"), ["0.1.5-rc.1"]);
        });
    }

    // ---- The signature ----------------------------------------------------

    /// A healthy release of `version`, as the archive, the manifest value and the
    /// legacy manifest `latest` serves -- ready to be altered.
    async fn healthy(version: &str) -> (Vec<u8>, DriverManifest, Vec<u8>) {
        let archive = driver_archive("postgres").await;
        let checksum = sha256_of(&archive);
        let manifest = driver_manifest_for("postgres", &archive, checksum.clone(), version);
        let legacy = manifest_for("postgres", &archive, checksum, version);
        (archive, manifest, legacy)
    }

    /// Installs against a release of `version` whose signed manifest is whatever
    /// `signed_bytes` makes of the healthy one, into a root of the test's own.
    /// Returns the outcome, the root, and how many archives were fetched.
    async fn install_against(
        version: &str,
        signed_bytes: impl FnOnce(&DriverManifest) -> Signed,
    ) -> (InstallResult, tempfile::TempDir, Arc<AtomicUsize>) {
        let root = tempfile::tempdir().unwrap();
        let (archive, manifest, legacy) = healthy(version).await;
        let requests = Arc::new(AtomicUsize::new(0));
        let mut server =
            Server::publishing(vec![legacy], archive, &[version]).signed(signed_bytes(&manifest));
        server.archive_requests = requests.clone();
        let result = installer(server.start(), root.path())
            .install("postgres")
            .await;
        (result, root, requests)
    }

    /// What every manifest published until now looks like: nothing signed it.
    /// Refused before any archive is asked for, in words that say what is wrong
    /// and how to get a driver anyway.
    #[test]
    fn a_release_published_without_a_signature_is_refused_before_anything_is_downloaded() {
        smol::block_on(async {
            let (result, root, requests) = install_against("0.1.1", |_| Signed::Nowhere).await;

            let message = result.unwrap_err().to_string();
            assert!(message.contains("not signed"), "{message}");
            assert!(message.contains("0.1.1"), "{message}");
            assert!(message.contains("build-database-drivers"), "{message}");
            assert_eq!(requests.load(Ordering::SeqCst), 0);
            assert!(!root.path().join("postgres").exists());
        });
    }

    /// The case the whole change exists for: a manifest an attacker wrote, with
    /// a checksum that matches their own archive.
    #[test]
    fn a_manifest_signed_by_someone_else_is_refused_before_anything_is_downloaded() {
        smol::block_on(async {
            let (result, root, requests) = install_against("0.1.1", |manifest| {
                let other = ring::signature::Ed25519KeyPair::generate_pkcs8(
                    &ring::rand::SystemRandom::new(),
                )
                .unwrap();
                let json = serde_json::to_vec(manifest).unwrap();
                Signed::Serving(signature::sign_bundle(other.as_ref(), &json).unwrap())
            })
            .await;

            let message = result.unwrap_err().to_string();
            assert!(message.contains("does not match"), "{message}");
            assert_eq!(requests.load(Ordering::SeqCst), 0);
            assert!(!root.path().join("postgres").exists());
        });
    }

    /// A good signature over a manifest, with the checksum swapped afterwards
    /// for the one of an archive the attacker controls.
    #[test]
    fn a_signed_manifest_edited_after_signing_is_refused() {
        smol::block_on(async {
            let (result, root, requests) = install_against("0.1.1", |manifest| {
                let genuine = String::from_utf8(signed(manifest)).unwrap();
                let swapped = genuine.replacen(&manifest.drivers[0].sha256, &"b".repeat(64), 1);
                assert_ne!(genuine, swapped);
                Signed::Serving(swapped.into_bytes())
            })
            .await;

            let message = result.unwrap_err().to_string();
            assert!(message.contains("does not match"), "{message}");
            assert_eq!(requests.load(Ordering::SeqCst), 0);
            assert!(!root.path().join("postgres").exists());
        });
    }

    /// Every manifest ever published stays correctly signed. One for another
    /// version, served under this one's name, must not be taken for it.
    #[test]
    fn a_signed_manifest_for_another_version_than_the_one_published_is_refused() {
        smol::block_on(async {
            let (result, _root, requests) = install_against("0.2.0", |manifest| {
                let mut older = manifest.clone();
                older.version = "0.1.0".into();
                Signed::Serving(signed(&older))
            })
            .await;

            let message = result.unwrap_err().to_string();
            assert!(
                message.contains("0.1.0") && message.contains("0.2.0"),
                "{message}"
            );
            assert_eq!(requests.load(Ordering::SeqCst), 0);
        });
    }

    // ---- Rolling back ------------------------------------------------------

    /// A replayed older manifest verifies. What stops it is the machine
    /// remembering how far it has got.
    #[test]
    fn a_driver_set_older_than_the_one_installed_is_refused_without_a_download() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest_at =
                |version| manifest_for("postgres", &archive, sha256_of(&archive), version);
            let requests = Arc::new(AtomicUsize::new(0));
            let installer = installer(
                release_publishing(
                    vec![manifest_at("0.2.0"), manifest_at("0.1.0")],
                    archive.clone(),
                    &["0.1.0", "0.2.0"],
                    requests.clone(),
                    Arc::default(),
                ),
                root.path(),
            );
            let installed = installer.install("postgres").await.unwrap();

            let error = installer
                .install("postgres")
                .await
                .expect_err("an older set must not replace a newer one");

            let message = error.to_string();
            assert!(
                message.contains("a newer one, 0.2.0, was installed"),
                "{message}"
            );
            assert_eq!(requests.load(Ordering::SeqCst), 1, "no second download");
            assert_eq!(
                store::installed_path_in(root.path(), "postgres"),
                Some(installed)
            );
        });
    }

    /// Removing a driver is not a way to forget how far the machine has got.
    #[test]
    fn removing_the_driver_does_not_reopen_the_way_back() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest_at =
                |version| manifest_for("postgres", &archive, sha256_of(&archive), version);
            let installer = installer(
                release_publishing(
                    vec![manifest_at("0.2.0"), manifest_at("0.1.0")],
                    archive.clone(),
                    &["0.1.0", "0.2.0"],
                    Arc::default(),
                    Arc::default(),
                ),
                root.path(),
            );
            installer.install("postgres").await.unwrap();
            store::remove_in(root.path(), "postgres").unwrap();

            installer
                .install("postgres")
                .await
                .expect_err("the removal must not have cleared the record");
        });
    }

    // ---- The protocol ------------------------------------------------------

    fn signed_with_protocol(manifest: &DriverManifest, protocol: Option<u32>) -> Signed {
        let mut manifest = manifest.clone();
        manifest.drivers[0].protocol = protocol;
        Signed::Serving(signed(&manifest))
    }

    /// The loop this prevents: a driver the app can never talk to is downloaded,
    /// installed, refused at the handshake, and then offered again.
    #[test]
    fn a_driver_for_a_newer_protocol_is_refused_before_it_is_downloaded_and_says_to_update() {
        smol::block_on(async {
            let (result, root, requests) = install_against("0.1.1", |manifest| {
                signed_with_protocol(manifest, Some(PROTOCOL_VERSION + 1))
            })
            .await;

            let message = result.unwrap_err().to_string();
            assert!(message.contains("Update Zode"), "{message}");
            assert!(
                message.contains(&(PROTOCOL_VERSION + 1).to_string()),
                "{message}"
            );
            assert_eq!(requests.load(Ordering::SeqCst), 0, "nothing may be fetched");
            assert!(directory_entries(root.path(), "postgres").is_empty());
        });
    }

    /// When the app is the newer one, updating it is not the answer: the
    /// drivers have not been published for it yet.
    #[test]
    fn a_driver_for_an_older_protocol_is_refused_before_it_is_downloaded_without_telling_anyone_to_update()
     {
        smol::block_on(async {
            let (result, _root, requests) = install_against("0.1.1", |manifest| {
                signed_with_protocol(manifest, Some(PROTOCOL_VERSION - 1))
            })
            .await;

            let message = result.unwrap_err().to_string();
            assert!(!message.contains("Update Zode"), "{message}");
            assert!(message.contains("make drivers"), "{message}");
            assert_eq!(requests.load(Ordering::SeqCst), 0);
        });
    }

    #[test]
    fn a_row_that_says_nothing_about_its_protocol_is_let_through() {
        smol::block_on(async {
            let (result, root, requests) =
                install_against("0.1.1", |manifest| signed_with_protocol(manifest, None)).await;

            result.unwrap();
            assert_eq!(requests.load(Ordering::SeqCst), 1);
            assert!(store::installed_path_in(root.path(), "postgres").is_some());
        });
    }

    // ---- Limits ------------------------------------------------------------

    /// Counts what is read from `inner`, so a test can say how much of a body a
    /// client took before it stopped.
    struct Counting<Reader> {
        inner: Reader,
        taken: Arc<std::sync::atomic::AtomicU64>,
    }

    impl<Reader: futures::AsyncRead + Unpin> futures::AsyncRead for Counting<Reader> {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buffer: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            let poll = std::pin::Pin::new(&mut self.inner).poll_read(cx, buffer);
            if let std::task::Poll::Ready(Ok(read)) = &poll {
                self.taken.fetch_add(*read as u64, Ordering::SeqCst);
            }
            poll
        }
    }

    /// Hands over `bytes`, then goes quiet without ever waking its reader.
    struct StallsAfter {
        bytes: futures::io::Cursor<Vec<u8>>,
    }

    impl futures::AsyncRead for StallsAfter {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buffer: &mut [u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            if self.bytes.position() as usize >= self.bytes.get_ref().len() {
                return std::task::Poll::Pending;
            }
            std::pin::Pin::new(&mut self.bytes).poll_read(cx, buffer)
        }
    }

    /// A server whose archives are `archive_body`, over a healthy release of
    /// `archive`.
    fn serving_archive_as(
        archive: &[u8],
        archive_body: impl Fn() -> AsyncBody + Send + Sync + 'static,
    ) -> Arc<dyn HttpClient> {
        let manifest = manifest_json(archive, sha256_of(archive));
        let signed_manifest = signed_manifest_for(archive);
        FakeHttpClient::create(move |request| {
            let manifest = manifest.clone();
            let signed_manifest = signed_manifest.clone();
            let body = (request.uri().path() == archive_path(VERSION)).then(&archive_body);
            async move {
                let path = request.uri().path().to_string();
                if let Some(body) = body {
                    return Ok(Response::builder().status(200).body(body)?);
                }
                if path == manifest_path() {
                    return Ok(Response::builder().status(200).body(manifest.into())?);
                }
                if path == signed_path(VERSION) {
                    return Ok(Response::builder()
                        .status(200)
                        .body(signed_manifest.into())?);
                }
                Ok(Response::builder().status(404).body(Default::default())?)
            }
        })
    }

    /// Without a ceiling, a server that streams without end -- or a bug that
    /// does -- fills the disk until someone presses Cancel. Finite on purpose:
    /// the unfixed code must fail this by taking all of it, not by hanging.
    #[test]
    fn a_download_larger_than_the_manifest_said_is_stopped_early_and_leaves_nothing() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let taken = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let http = serving_archive_as(&archive, {
                let taken = taken.clone();
                move || {
                    AsyncBody::from_reader(Counting {
                        inner: futures::io::repeat(b'x').take(64 * 1024 * 1024),
                        taken: taken.clone(),
                    })
                }
            });

            let error = installer(http, root.path())
                .install("postgres")
                .await
                .expect_err("more bytes than advertised must be refused");

            let message = error.to_string();
            assert!(message.contains("larger than"), "{message}");
            assert!(
                taken.load(Ordering::SeqCst) < 1024 * 1024,
                "it must stop soon after the advertised {} bytes, not read the lot",
                archive.len()
            );
            assert!(directory_entries(root.path(), "postgres").is_empty());
            assert_eq!(store::installed_path_in(root.path(), "postgres"), None);
        });
    }

    /// A short body is a dropped connection, and "does not match the checksum"
    /// sends people after a tampered archive that was never there.
    #[test]
    fn a_download_that_ends_early_says_it_was_interrupted_and_leaves_nothing() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let truncated = archive[..archive.len() / 2].to_vec();
            let http = serving_archive_as(&archive, move || truncated.clone().into());

            let error = installer(http, root.path())
                .install("postgres")
                .await
                .expect_err("half an archive is not an install");

            let message = error.to_string();
            assert!(message.contains("interrupted"), "{message}");
            assert!(!message.contains("checksum"), "{message}");
            assert!(directory_entries(root.path(), "postgres").is_empty());
        });
    }

    /// Whatever the manifest claims, no archive is worth that much disk.
    #[test]
    fn a_manifest_advertising_an_enormous_archive_is_refused_before_it_is_fetched() {
        smol::block_on(async {
            let (result, root, requests) = install_against("0.1.1", |manifest| {
                let mut manifest = manifest.clone();
                manifest.drivers[0].size = u64::MAX;
                Signed::Serving(signed(&manifest))
            })
            .await;

            let message = result.unwrap_err().to_string();
            assert!(message.contains("more than"), "{message}");
            assert_eq!(requests.load(Ordering::SeqCst), 0);
            assert!(directory_entries(root.path(), "postgres").is_empty());
        });
    }

    /// A server that accepts the connection and then says nothing. Without a
    /// timeout the bar reads "Downloading..." until someone cancels.
    #[test]
    fn a_server_that_stops_sending_is_given_up_on_and_leaves_nothing() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let first = archive[..8].to_vec();
            let http = serving_archive_as(&archive, move || {
                AsyncBody::from_reader(StallsAfter {
                    bytes: futures::io::Cursor::new(first.clone()),
                })
            });
            let installer = Arc::new(
                DriverInstaller::with_root(
                    http,
                    DriverEndpoint::new("https://api.test.invalid/api"),
                    root.path().to_path_buf(),
                )
                .trusting(signature::testing::public_key())
                .accepting_versions_from("0.0.0")
                .giving_up_after(Duration::from_millis(100)),
            );

            let error = installer
                .install("postgres")
                .await
                .expect_err("a stalled download must end");

            let message = error.to_string();
            assert!(message.contains("stalled"), "{message}");
            assert!(directory_entries(root.path(), "postgres").is_empty());
        });
    }

    #[test]
    fn a_server_that_never_answers_is_given_up_on() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let http = FakeHttpClient::create(|_| async move {
                std::future::pending::<()>().await;
                anyhow::bail!("unreachable")
            });
            let installer = Arc::new(
                DriverInstaller::with_root(
                    http,
                    DriverEndpoint::new("https://api.test.invalid/api"),
                    root.path().to_path_buf(),
                )
                .trusting(signature::testing::public_key())
                .accepting_versions_from("0.0.0")
                .giving_up_after(Duration::from_millis(100)),
            );

            let error = installer.install("postgres").await.unwrap_err();

            assert!(error.to_string().contains("stalled"), "{error}");
        });
    }

    /// Walking away from a download drops it wherever it is, with no code of
    /// its own left to run -- so the partial file must belong to a destructor.
    #[allow(
        clippy::disallowed_methods,
        reason = "runs on `smol::block_on`, not a gpui executor, so there is no test clock to use"
    )]
    #[test]
    fn a_download_that_is_cancelled_leaves_no_partial_file_behind() {
        smol::block_on(async {
            use futures::future::{Either, select};

            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let first = archive[..8].to_vec();
            let http = serving_archive_as(&archive, move || {
                AsyncBody::from_reader(StallsAfter {
                    bytes: futures::io::Cursor::new(first.clone()),
                })
            });
            let installer = installer(http, root.path());

            let install = installer.install("postgres");
            let partial_exists = Box::pin(async {
                while directory_entries(root.path(), "postgres")
                    .iter()
                    .all(|name| !name.ends_with(".download"))
                {
                    smol::Timer::after(Duration::from_millis(5)).await;
                }
            });
            match select(install, partial_exists).await {
                Either::Right((_, install)) => drop(install),
                Either::Left((outcome, _)) => {
                    panic!("the download must still be going: {outcome:?}")
                }
            }
            installer.forget("postgres");

            assert!(
                directory_entries(root.path(), "postgres").is_empty(),
                "{:?}",
                directory_entries(root.path(), "postgres")
            );
        });
    }

    // ---- Who is waiting ------------------------------------------------------

    /// A second window joins the download the first started. Cancelling in the
    /// second must not take it from the first.
    #[test]
    fn a_caller_cancelling_leaves_the_download_to_the_ones_still_waiting() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, sha256_of(&archive));
            let requests = Arc::new(AtomicUsize::new(0));
            let installer = installer(
                release_serving(manifest, archive, requests.clone()),
                root.path(),
            );
            let first = installer.install("postgres");
            let second = installer.install("postgres");

            installer.forget("postgres");
            assert!(
                installer.in_flight.lock().contains_key("postgres"),
                "one caller is still waiting, so the download is still wanted"
            );
            drop(second);

            first.await.unwrap();
            assert_eq!(requests.load(Ordering::SeqCst), 1);
            assert!(installer.in_flight.lock().is_empty());
        });
    }

    #[test]
    fn the_last_caller_cancelling_clears_the_entry() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let installer = installer(FakeHttpClient::with_404_response(), root.path());
            let _first = installer.install("postgres");
            let _second = installer.install("postgres");

            installer.forget("postgres");
            installer.forget("postgres");

            assert!(installer.in_flight.lock().is_empty());
            installer.forget("postgres");
        });
    }

    /// An install that gave up and was followed by a new one has no entry of its
    /// own any more. When it finishes anyway, it must not take the new one's.
    #[test]
    fn a_finished_install_leaves_the_entry_of_a_newer_one_alone() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, sha256_of(&archive));
            let installer = installer(
                release_serving(manifest, archive, Arc::default()),
                root.path(),
            );
            let abandoned = installer.install("postgres");
            installer.forget("postgres");
            let newer = installer.install("postgres");

            abandoned.await.unwrap();

            assert!(
                installer.in_flight.lock().contains_key("postgres"),
                "the newer install is still running and must still be findable"
            );
            newer.await.unwrap();
            assert!(installer.in_flight.lock().is_empty());
        });
    }

    /// Two installs of one driver that overlap must not write the same file.
    #[test]
    fn overlapping_installs_of_one_driver_do_not_share_scratch_files() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, sha256_of(&archive));
            let requests = Arc::new(AtomicUsize::new(0));
            let installer = installer(
                release_serving(manifest, archive, requests.clone()),
                root.path(),
            );
            let abandoned = installer.install("postgres");
            installer.forget("postgres");
            let newer = installer.install("postgres");

            let (abandoned, newer) = futures::join!(abandoned, newer);

            assert!(
                abandoned.is_ok() && newer.is_ok(),
                "{abandoned:?} {newer:?}"
            );
            assert_eq!(requests.load(Ordering::SeqCst), 2);
            assert_eq!(directory_entries(root.path(), "postgres"), [VERSION]);
        });
    }

    /// An install leaves one thing that says Zode made it, because Remove goes
    /// by it.
    #[test]
    fn an_install_is_marked_as_zodes_own() {
        smol::block_on(async {
            let (result, _root, _requests) = install_against("0.1.1", |_| Signed::Properly).await;

            assert!(store::installed_by_zode(&result.unwrap()));
        });
    }

    // ---- A signed manifest that `latest` does not agree with ---------------

    /// Installs against a release where the unsigned copy under `latest` and
    /// the signed manifest are edited independently, so that a test can say
    /// which of the two an install believes.
    async fn install_where_they_disagree(
        edit_latest: impl FnOnce(&mut DriverManifest),
        edit_signed: impl FnOnce(&mut DriverManifest),
    ) -> (InstallResult, tempfile::TempDir, Arc<Mutex<Vec<String>>>) {
        let root = tempfile::tempdir().unwrap();
        let (archive, manifest, _) = healthy(VERSION).await;
        let mut latest = manifest.clone();
        for driver in &mut latest.drivers {
            driver.protocol = None;
        }
        edit_latest(&mut latest);
        let mut signed_manifest = manifest;
        edit_signed(&mut signed_manifest);
        let paths = Arc::new(Mutex::new(Vec::new()));
        let mut server = Server::publishing(
            vec![serde_json::to_vec(&latest).unwrap()],
            archive,
            &[VERSION],
        )
        .signed(Signed::Serving(signed(&signed_manifest)));
        server.paths = paths.clone();
        let result = installer(server.start(), root.path())
            .install("postgres")
            .await;
        (result, root, paths)
    }

    /// `latest` is the backend's own copy and nothing signed it: an attacker who
    /// can alter it can say anything. The rows acted on are the signed ones.
    #[test]
    fn the_signed_manifest_wins_over_a_latest_that_names_another_checksum() {
        smol::block_on(async {
            let (result, root, _) = install_where_they_disagree(
                |latest| latest.drivers[0].sha256 = "b".repeat(64),
                |_| {},
            )
            .await;

            result.expect("the signed checksum is the right one, and is what counts");
            assert!(store::installed_path_in(root.path(), "postgres").is_some());
        });
    }

    #[test]
    fn the_signed_manifest_wins_over_a_latest_that_names_another_size() {
        smol::block_on(async {
            let (result, _root, _) =
                install_where_they_disagree(|latest| latest.drivers[0].size = 1, |_| {}).await;

            result.expect("the signed size is the right one, and is what counts");
        });
    }

    #[test]
    fn the_signed_manifest_wins_over_a_latest_that_names_another_asset() {
        smol::block_on(async {
            let other = "zode-db-postgres-x86_64-unknown-linux-gnu.tar.gz";
            let (result, _root, paths) = install_where_they_disagree(
                |latest| latest.drivers[0].asset = other.to_string(),
                |_| {},
            )
            .await;

            result.expect("the signed asset is the right one, and is what counts");
            assert_eq!(
                paths.lock().last(),
                Some(&archive_path(VERSION)),
                "the archive fetched is the one the signed manifest names"
            );
            assert!(
                !paths.lock().iter().any(|path| path.ends_with(other)),
                "{:?}",
                paths.lock()
            );
        });
    }

    /// The other way round, so that "the signed one wins" cannot be satisfied by
    /// ignoring both: a wrong signed checksum fails whatever `latest` says.
    #[test]
    fn a_signed_checksum_that_does_not_match_the_archive_is_refused_whatever_latest_says() {
        smol::block_on(async {
            let (result, root, _) = install_where_they_disagree(
                |_| {},
                |signed| signed.drivers[0].sha256 = "c".repeat(64),
            )
            .await;

            let message = result.unwrap_err().to_string();
            assert!(message.contains("checksum"), "{message}");
            assert_eq!(store::installed_path_in(root.path(), "postgres"), None);
        });
    }

    #[test]
    fn a_signed_size_smaller_than_the_archive_is_refused_whatever_latest_says() {
        smol::block_on(async {
            let (result, root, _) =
                install_where_they_disagree(|_| {}, |signed| signed.drivers[0].size -= 1).await;

            let message = result.unwrap_err().to_string();
            assert!(message.contains("larger than"), "{message}");
            assert_eq!(store::installed_path_in(root.path(), "postgres"), None);
        });
    }

    // ---- When the signed manifest is not there -----------------------------

    /// A publish that lands between reading `latest` and reading what it names
    /// deletes the whole version. Calling that "not signed" sends someone after a
    /// problem that is not there; asking again would find the new set.
    #[test]
    fn a_version_that_vanished_while_being_read_says_so_and_does_not_say_unsigned() {
        smol::block_on(async {
            let (result, root, requests) = install_against("0.1.1", |_| Signed::Withdrawn).await;

            let message = result.unwrap_err().to_string();
            assert!(message.contains("changed while"), "{message}");
            assert!(message.contains("try again"), "{message}");
            assert!(!message.contains("not signed"), "{message}");
            assert_eq!(requests.load(Ordering::SeqCst), 0);
            assert!(!root.path().join("postgres").exists());
        });
    }

    // ---- The oldest set this build accepts -----------------------------------

    /// Without a record -- a fresh install, or an upgrade from a Zode that kept
    /// none -- the only thing between a machine and an older, vulnerable set a
    /// hostile server replays is the minimum built into the app. Built the way the
    /// app builds it: no relaxed floor.
    #[test]
    fn a_driver_set_older_than_the_minimum_built_in_is_refused_before_it_is_downloaded() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest_at =
                |version| manifest_for("postgres", &archive, sha256_of(&archive), version);
            let requests = Arc::new(AtomicUsize::new(0));
            let installer = Arc::new(
                DriverInstaller::with_root(
                    release_publishing(
                        vec![manifest_at("0.1.4"), manifest_at("0.1.5")],
                        archive.clone(),
                        &["0.1.4", "0.1.5"],
                        requests.clone(),
                        Arc::default(),
                    ),
                    DriverEndpoint::new("https://api.test.invalid/api"),
                    root.path().to_path_buf(),
                )
                .trusting(signature::testing::public_key()),
            );

            let error = installer
                .install("postgres")
                .await
                .expect_err("a set older than any ever signed must be refused");

            let message = error.to_string();
            assert!(message.contains("0.1.4"), "{message}");
            assert!(message.contains("wait for a newer set"), "{message}");
            assert_eq!(requests.load(Ordering::SeqCst), 0, "nothing may be fetched");
            assert!(directory_entries(root.path(), "postgres").is_empty());

            installer
                .install("postgres")
                .await
                .expect("the first signed set is accepted");
        });
    }

    /// What Esc leaves behind must not be able to outlive the dialog: the count
    /// of callers waiting is what decides whether a download is still wanted.
    #[test]
    fn the_callers_waiting_are_counted_until_they_are_answered() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let manifest = manifest_json(&archive, sha256_of(&archive));
            let installer = installer(
                release_serving(manifest, archive, Arc::default()),
                root.path(),
            );
            assert_eq!(installer.waiters("postgres"), 0);

            let first = installer.install("postgres");
            let second = installer.install("postgres");
            assert_eq!(installer.waiters("postgres"), 2);
            installer.forget("postgres");
            assert_eq!(installer.waiters("postgres"), 1);

            drop(second);
            first.await.unwrap();
            assert_eq!(installer.waiters("postgres"), 0);
        });
    }
}
