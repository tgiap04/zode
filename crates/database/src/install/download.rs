//! Fetching a driver from the release this build came from.
//!
//! The order matters more than any single step: the bytes are hashed while
//! they stream, compared against the manifest, and only then unpacked. Nothing
//! unverified is ever handed to an archive reader, and nothing half-installed
//! is ever visible where the resolver looks.

use anyhow::{Context as _, Result};
use collections::HashMap;
use futures::future::{BoxFuture, FutureExt as _, Shared};
use futures::{AsyncReadExt as _, AsyncWriteExt as _, channel::mpsc};
use http_client::HttpClient;
use http_client::github::ensure_release_host_is_trusted;
use parking_lot::Mutex;
use sha2::{Digest as _, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

use crate::install::manifest::{DriverManifest, ReleaseCoordinates, current_target, parse_sha256};
use crate::install::{store, unpack};

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

pub struct DriverInstaller {
    http: Arc<dyn HttpClient>,
    release: ReleaseCoordinates,
    root: PathBuf,
    manifest: Mutex<Option<Arc<DriverManifest>>>,
    in_flight: Mutex<HashMap<String, SharedInstall>>,
    watchers: Mutex<HashMap<String, Vec<mpsc::UnboundedSender<InstallProgress>>>>,
}

impl DriverInstaller {
    pub fn new(http: Arc<dyn HttpClient>, release: ReleaseCoordinates) -> Self {
        Self::with_root(http, release, store::root().to_path_buf())
    }

    pub fn with_root(
        http: Arc<dyn HttpClient>,
        release: ReleaseCoordinates,
        root: PathBuf,
    ) -> Self {
        Self {
            http,
            release,
            root,
            manifest: Mutex::default(),
            in_flight: Mutex::default(),
            watchers: Mutex::default(),
        }
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
    pub fn install(self: &Arc<Self>, id: &str) -> SharedInstall {
        let mut in_flight = self.in_flight.lock();
        if let Some(running) = in_flight.get(id) {
            return running.clone();
        }
        let shared = {
            let installer = self.clone();
            let id = id.to_string();
            async move { installer.run(&id).await.map_err(Arc::new) }
                .boxed()
                .shared()
        };
        in_flight.insert(id.to_string(), shared.clone());
        shared
    }

    /// Drops a paused install, so a cancelled download is not resumed by the
    /// next caller instead of being started afresh.
    ///
    /// A shared future only advances while something awaits it, so cancelling
    /// leaves one parked in the map rather than running. Harmless, but it would
    /// make the next Download button resume a download the person had already
    /// walked away from.
    pub fn forget(&self, id: &str) {
        self.in_flight.lock().remove(id);
        self.watchers.lock().remove(id);
    }

    async fn run(self: Arc<Self>, id: &str) -> Result<PathBuf> {
        let installed = self.install_once(id).await;
        // Cleared whether it succeeded or failed: a failure that stays cached
        // would make Retry a button that replays the same error forever.
        self.in_flight.lock().remove(id);
        self.watchers.lock().remove(id);
        installed
    }

    async fn install_once(&self, id: &str) -> Result<PathBuf> {
        let target = current_target().with_context(|| {
            format!(
                "Zode does not publish database drivers for {}-{}",
                std::env::consts::ARCH,
                std::env::consts::OS
            )
        })?;

        self.report(id, InstallProgress::FetchingManifest);
        let manifest = self.manifest().await?;
        let asset = manifest.find(id, target).with_context(|| {
            let published = manifest.targets_for(id);
            if published.is_empty() {
                format!(
                    "release v{} publishes no `{id}` driver ({})",
                    self.release.version,
                    self.release.release_url()
                )
            } else {
                format!(
                    "release v{} has no `{id}` driver for {target} (it publishes: {})",
                    self.release.version,
                    published.join(", ")
                )
            }
        })?;
        let expected = parse_sha256(&asset.asset, &asset.sha256)?;

        let directory = store::version_dir_in(&self.root, id, &self.release.version);
        smol::fs::create_dir_all(directory.parent().unwrap_or(&self.root))
            .await
            .with_context(|| format!("creating {}", self.root.display()))?;

        // Downloaded beside where it will be unpacked, so nothing crosses a
        // filesystem boundary and the final move stays a rename.
        let archive = directory.with_extension("download");
        let downloaded = self
            .download_to(
                id,
                &self.release.asset_url(&asset.asset),
                &archive,
                asset.size,
            )
            .await;

        let result = async {
            let actual = downloaded?;
            self.report(id, InstallProgress::Verifying);
            anyhow::ensure!(
                actual == expected,
                "the downloaded `{id}` driver does not match the checksum the release publishes \
                 (expected {expected}, got {actual}); it has been discarded"
            );
            self.report(id, InstallProgress::Unpacking);
            unpack::install_archive(
                &archive,
                &asset.entry,
                &directory,
                &store::executable_name(id),
            )
            .await
        }
        .await;

        // The archive is never wanted again, and a corrupt one left behind is a
        // download that will be skipped next time on the strength of its name.
        let _ = smol::fs::remove_file(&archive).await;
        let installed = result?;

        store::prune_other_versions_in(&self.root, id, &self.release.version)?;
        Ok(installed)
    }

    /// Streams an asset to `destination`, returning its SHA-256.
    ///
    /// Hashed as it arrives rather than by re-reading the file: the bytes are
    /// already in hand, and re-reading leaves a window in which what was
    /// checked and what gets unpacked are not provably the same file.
    async fn download_to(
        &self,
        id: &str,
        url: &str,
        destination: &PathBuf,
        expected_size: u64,
    ) -> Result<String> {
        ensure_release_host_is_trusted(url)?;
        let mut response = self
            .http
            .get(url, Default::default(), true)
            .await
            .with_context(|| format!("downloading the `{id}` driver from {url}"))?;
        anyhow::ensure!(
            response.status().is_success(),
            "downloading the `{id}` driver failed with status {}",
            response.status()
        );

        let mut file = smol::fs::File::create(destination)
            .await
            .with_context(|| format!("creating {}", destination.display()))?;
        let total = (expected_size > 0).then_some(expected_size);
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];

        loop {
            let read = response
                .body_mut()
                .read(&mut buffer)
                .await
                .with_context(|| format!("reading the `{id}` driver from {url}"))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            file.write_all(&buffer[..read])
                .await
                .with_context(|| format!("writing {}", destination.display()))?;
            received += read as u64;
            self.report(id, InstallProgress::Downloading { received, total });
        }
        file.flush().await?;

        Ok(hex(hasher.finalize().as_slice()))
    }

    /// The release's driver manifest, fetched once per process.
    ///
    /// Cached because it is asked for again on every driver a person installs
    /// in one sitting, and it cannot change underneath a running app: the
    /// release it names is the one this build came from.
    pub async fn manifest(&self) -> Result<Arc<DriverManifest>> {
        if let Some(manifest) = self.manifest.lock().clone() {
            return Ok(manifest);
        }

        let url = self.release.manifest_url();
        ensure_release_host_is_trusted(&url)?;
        let mut response = self
            .http
            .get(&url, Default::default(), true)
            .await
            .with_context(|| format!("fetching the driver manifest from {url}"))?;
        anyhow::ensure!(
            response.status().is_success(),
            "release v{} publishes no driver manifest (fetching it returned {}). \
             In a development build, run `script/build-database-drivers` instead.",
            self.release.version,
            response.status()
        );

        let mut body = Vec::new();
        response.body_mut().read_to_end(&mut body).await?;
        let manifest = Arc::new(DriverManifest::parse(&body)?);
        *self.manifest.lock() = Some(manifest.clone());
        Ok(manifest)
    }

    fn report(&self, id: &str, progress: InstallProgress) {
        let mut watchers = self.watchers.lock();
        if let Some(senders) = watchers.get_mut(id) {
            senders.retain(|sender| sender.unbounded_send(progress).is_ok());
        }
    }
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
    use crate::install::manifest::MANIFEST_ASSET;
    use crate::install::test_support::{driver_archive, manifest_for, sha256_of};
    use futures::StreamExt as _;
    use http_client::{FakeHttpClient, Response};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const VERSION: &str = "0.1.1";

    fn manifest_json(archive: &[u8], sha256: String) -> Vec<u8> {
        manifest_for("postgres", archive, sha256, VERSION)
    }

    /// A release serving `manifest` and one archive, counting what was asked
    /// for so a test can prove a download happened once rather than twice.
    fn release_serving(
        manifest: Vec<u8>,
        archive: Vec<u8>,
        requests: Arc<AtomicUsize>,
    ) -> Arc<dyn HttpClient> {
        FakeHttpClient::create(move |request| {
            let manifest = manifest.clone();
            let archive = archive.clone();
            let requests = requests.clone();
            async move {
                let path = request.uri().path().to_string();
                let body = if path.ends_with(MANIFEST_ASSET) {
                    manifest
                } else if path.ends_with("zode-db-postgres.tar.gz") {
                    requests.fetch_add(1, Ordering::SeqCst);
                    archive
                } else {
                    return Ok(Response::builder().status(404).body(Default::default())?);
                };
                Ok(Response::builder().status(200).body(body.into())?)
            }
        })
    }

    fn installer(http: Arc<dyn HttpClient>, root: &std::path::Path) -> Arc<DriverInstaller> {
        Arc::new(DriverInstaller::with_root(
            http,
            ReleaseCoordinates::new("tgiap04/zode", VERSION),
            root.to_path_buf(),
        ))
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
                store::installed_path_in(root.path(), "postgres", VERSION),
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
            assert_eq!(
                store::installed_path_in(root.path(), "postgres", VERSION),
                None
            );
            assert!(
                !store::version_dir_in(root.path(), "postgres", VERSION)
                    .with_extension("download")
                    .exists(),
                "the rejected archive must not be left to be trusted later"
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
            assert!(message.contains("build-database-drivers"), "{message}");
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
}
