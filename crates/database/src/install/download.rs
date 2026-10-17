//! Fetching the most recently published driver.
//!
//! The order matters more than any single step: the bytes are hashed while
//! they stream, compared against the manifest, and only then unpacked. Nothing
//! unverified is ever handed to an archive reader, and nothing half-installed
//! is ever visible where the resolver looks.

use anyhow::{Context as _, Result};
use collections::HashMap;
use futures::future::{BoxFuture, FutureExt as _, Shared};
use futures::{AsyncReadExt as _, AsyncWriteExt as _, channel::mpsc};
use http_client::{HttpClient, StatusCode};
use parking_lot::Mutex;
use sha2::{Digest as _, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

use crate::install::endpoint::DriverEndpoint;
use crate::install::manifest::{DriverManifest, current_target, parse_sha256};
use crate::install::{store, unpack};

/// The most a manifest may weigh. A real one is a few kilobytes -- a line per
/// driver per platform -- so this is generous, and exists only so that a server
/// that answers with something endless cannot fill memory.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

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
    endpoint: DriverEndpoint,
    root: PathBuf,
    in_flight: Mutex<HashMap<String, SharedInstall>>,
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
        let manifest = self.fetch_manifest().await?;
        let version = manifest.version.as_str();
        let asset = manifest.find(id, target).with_context(|| {
            let published = manifest.targets_for(id);
            if published.is_empty() {
                format!(
                    "version {version} publishes no `{id}` driver. In a development build, run \
                     `script/build-database-drivers` (or `make drivers`) instead, or install \
                     the driver by hand into {}",
                    self.root.display()
                )
            } else {
                format!(
                    "version {version} has no `{id}` driver for {target} (it publishes: {}). In a \
                     development build, run `script/build-database-drivers` (or `make drivers`) \
                     instead, or install the driver by hand into {}",
                    published.join(", "),
                    self.root.display()
                )
            }
        })?;
        let expected = parse_sha256(&asset.asset, &asset.sha256)?;

        let directory = store::version_dir_in(&self.root, id, version);
        smol::fs::create_dir_all(directory.parent().unwrap_or(&self.root))
            .await
            .with_context(|| format!("creating {}", self.root.display()))?;

        // Downloaded beside where it will be unpacked, so nothing crosses a
        // filesystem boundary and the final move stays a rename.
        let archive = store::hidden_sibling(&directory, "download");
        let downloaded = self
            .download_to(
                id,
                &self.endpoint.asset_url(version, &asset.asset),
                &archive,
                asset.size,
            )
            .await;

        let result = async {
            let actual = downloaded?;
            self.report(id, InstallProgress::Verifying);
            anyhow::ensure!(
                actual == expected,
                "the downloaded `{id}` driver does not match the checksum the manifest publishes \
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
        if let Err(error) = store::ignore_not_found(smol::fs::remove_file(&archive).await) {
            log::warn!(
                "could not remove the downloaded archive {}: {error}",
                archive.display()
            );
        }
        let installed = result?;

        store::prune_other_versions_in(&self.root, id, version)?;
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
        self.endpoint.ensure_trusted(url)?;
        let mut response = self
            .http
            .get(url, Default::default(), false)
            .await
            .with_context(|| format!("downloading the `{id}` driver from {url}"))?;
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

    /// The manifest of the most recently published drivers.
    ///
    /// Fetched afresh for every install and never kept: a publish deletes the
    /// version a held manifest names, so one remembered from an earlier install
    /// would send the next one to an archive that is no longer there.
    async fn fetch_manifest(&self) -> Result<DriverManifest> {
        let url = self.endpoint.latest_manifest_url();
        self.endpoint.ensure_trusted(&url)?;
        let mut response = self
            .http
            .get(&url, Default::default(), false)
            .await
            .with_context(|| format!("fetching the driver manifest from {url}"))?;
        anyhow::ensure!(
            response.status().is_success(),
            "no driver manifest is published at {url} (the server answered {}). \
             In a development build, run `script/build-database-drivers` (or `make drivers`) \
             instead.",
            response.status()
        );

        // One byte past the limit, so a body of exactly the limit is accepted
        // and anything longer is told apart from it.
        let mut body = Vec::new();
        response
            .body_mut()
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut body)
            .await
            .with_context(|| format!("reading the driver manifest from {url}"))?;
        anyhow::ensure!(
            body.len() as u64 <= MAX_MANIFEST_BYTES,
            "the driver manifest at {url} is larger than {MAX_MANIFEST_BYTES} bytes, which no \
             manifest Zode publishes comes close to; refusing to read it"
        );
        DriverManifest::parse(&body)
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
    use crate::install::test_support::{
        archive_asset_for, directory_entries, driver_archive, manifest_for, sha256_of,
    };
    use futures::StreamExt as _;
    use http_client::{AsyncBody, FakeHttpClient, Response};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const VERSION: &str = "0.1.1";

    fn manifest_json(archive: &[u8], sha256: String) -> Vec<u8> {
        manifest_for("postgres", archive, sha256, VERSION)
    }

    fn archive_path(version: &str) -> String {
        format!("/api/drivers/{version}/{}", archive_asset_for("postgres"))
    }

    /// A server publishing `manifest` under `latest` and one archive under
    /// [`VERSION`], counting archive fetches so a test can prove a download
    /// happened once rather than twice.
    fn release_serving(
        manifest: Vec<u8>,
        archive: Vec<u8>,
        requests: Arc<AtomicUsize>,
    ) -> Arc<dyn HttpClient> {
        release_publishing(
            vec![manifest],
            archive,
            &[VERSION],
            requests,
            Arc::default(),
        )
    }

    /// The server's real URL contract and nothing looser: the manifest answers
    /// only at `/drivers/latest/`, an archive only under one of `versions`, and
    /// every other path is a 404 -- so a client that builds a URL wrongly fails
    /// a test instead of passing against a fake that matched on a suffix.
    ///
    /// Each manifest request is answered with the next of `manifests`, the last
    /// repeating, and `paths` records every path asked for, in order.
    fn release_publishing(
        manifests: Vec<Vec<u8>>,
        archive: Vec<u8>,
        versions: &'static [&'static str],
        archive_requests: Arc<AtomicUsize>,
        paths: Arc<Mutex<Vec<String>>>,
    ) -> Arc<dyn HttpClient> {
        let manifest_path = format!("/api/drivers/latest/{MANIFEST_ASSET}");
        let archive_paths: Vec<String> = versions
            .iter()
            .map(|version| archive_path(version))
            .collect();
        let manifests_served = Arc::new(AtomicUsize::new(0));
        FakeHttpClient::create(move |request| {
            let manifests = manifests.clone();
            let archive = archive.clone();
            let archive_requests = archive_requests.clone();
            let paths = paths.clone();
            let manifest_path = manifest_path.clone();
            let archive_paths = archive_paths.clone();
            let manifests_served = manifests_served.clone();
            async move {
                let path = request.uri().path().to_string();
                paths.lock().push(path.clone());
                let body = if path == manifest_path {
                    let turn = manifests_served.fetch_add(1, Ordering::SeqCst);
                    manifests[turn.min(manifests.len() - 1)].clone()
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

    fn installer(http: Arc<dyn HttpClient>, root: &std::path::Path) -> Arc<DriverInstaller> {
        Arc::new(DriverInstaller::with_root(
            http,
            DriverEndpoint::new("https://api.test.invalid/api"),
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
                    archive_path("0.1.0"),
                    format!("/api/drivers/latest/{MANIFEST_ASSET}"),
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
            let http = FakeHttpClient::create(move |request| {
                let manifest = manifest.clone();
                async move {
                    if request.uri().path() == manifest_path() {
                        Ok(Response::builder().status(200).body(manifest.into())?)
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
    /// stop the install before any archive is asked for.
    #[test]
    fn a_manifest_naming_an_asset_that_is_not_an_archive_fetches_no_archive() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = driver_archive("postgres").await;
            let mut manifest = DriverManifest::parse(&manifest_json(&archive, sha256_of(&archive)))
                .expect("the fixture manifest is well formed");
            manifest.drivers[0].asset = "../../etc/passwd".to_string();
            let archive_requests = Arc::new(AtomicUsize::new(0));
            let paths = Arc::new(Mutex::new(Vec::new()));
            let installer = installer(
                release_publishing(
                    vec![serde_json::to_vec(&manifest).unwrap()],
                    archive,
                    &[VERSION],
                    archive_requests.clone(),
                    paths.clone(),
                ),
                root.path(),
            );

            let error = installer.install("postgres").await.unwrap_err();

            assert!(error.to_string().contains("../../etc/passwd"), "{error}");
            assert_eq!(paths.lock().as_slice(), [manifest_path()]);
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
            let seen = Arc::new(Mutex::new(None));
            let http = {
                let root = root.path().to_path_buf();
                let seen = seen.clone();
                FakeHttpClient::create(move |request| {
                    let manifest = manifest.clone();
                    let archive = archive.clone();
                    let root = root.clone();
                    let seen = seen.clone();
                    async move {
                        if request.uri().path() == manifest_path() {
                            return Ok(Response::builder().status(200).body(manifest.into())?);
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
            assert_eq!(entries, [".0.1.5-rc.1.download"]);
            assert!(entries.iter().all(|name| !store::is_version(name)));
            assert_eq!(resolved, None);
            assert_eq!(directory_entries(root.path(), "postgres"), ["0.1.5-rc.1"]);
        });
    }
}
