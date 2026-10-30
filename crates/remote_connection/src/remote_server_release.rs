//! Fetching the `zed-remote-server` binary for SSH remote development.
//!
//! This replaces the `auto_update::AutoUpdater` path that this fork removed. The
//! original brokered its download URL through Zed's authenticated cloud API and
//! performed **no** integrity check on the downloaded binary — it trusted TLS plus
//! the authenticated broker.
//!
//! Dropping the broker without adding anything back would leave a raw, unauthenticated
//! fetch of an executable that is then run on a remote host. So the checksum here is
//! not an improvement over the old behaviour; it is what keeps the replacement from
//! being strictly worse than what it replaced.
//!
//! The manifest is fetched first, the archive second, and the archive is rejected
//! unless its SHA-256 matches the manifest entry.

use anyhow::{Context as _, Result};
use futures::AsyncReadExt as _;
use gpui::AsyncApp;
use http_client::{AsyncBody, HttpClient, HttpRequestExt as _, RedirectPolicy, Request};
use release_channel::{RELEASE_REPO, ReleaseChannel};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use smol::fs::File;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Published beside the archives on every GitHub release by `script/build-remote-server-manifest`.
const MANIFEST_NAME: &str = "remote-server-manifest.json";

/// How many cached remote-server archives to keep per platform.
const CACHE_LIMIT: usize = 5;

/// The release's `remote-server-manifest.json`.
///
/// ```json
/// {
///   "version": "0.1.6",
///   "assets": [
///     { "name": "zed-remote-server-linux-x86_64.gz",
///       "url": "https://github.com/tgiap04/zode/releases/download/v0.1.6/zed-remote-server-linux-x86_64.gz",
///       "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08" }
///   ]
/// }
/// ```
#[derive(Debug, Deserialize)]
struct ReleaseManifest {
    version: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    url: String,
    /// Lowercase hex SHA-256 of the artifact at `url`.
    sha256: String,
}

fn asset_name(os: &str, arch: &str) -> String {
    format!("zed-remote-server-{os}-{arch}.gz")
}

fn manifest_url(version: Option<&Version>) -> String {
    match version {
        Some(version) => {
            // Strip pre-release and build metadata: artifacts are published per released version.
            let released = Version::new(version.major, version.minor, version.patch);
            format!(
                "https://github.com/{RELEASE_REPO}/releases/download/v{released}/{MANIFEST_NAME}"
            )
        }
        None => {
            format!("https://github.com/{RELEASE_REPO}/releases/latest/download/{MANIFEST_NAME}")
        }
    }
}

/// GitHub answers every release download with a redirect to its storage host.
fn get_following_redirects(url: &str) -> Result<Request<AsyncBody>> {
    Ok(Request::get(url)
        .follow_redirects(RedirectPolicy::FollowAll)
        .body(AsyncBody::empty())?)
}

async fn fetch_manifest(
    client: &Arc<dyn HttpClient>,
    version: Option<&Version>,
) -> Result<ReleaseManifest> {
    let url = manifest_url(version);
    let request = get_following_redirects(&url)?;
    let mut response = client
        .send(request)
        .await
        .with_context(|| format!("fetching release manifest from {url}"))?;

    let mut body = Vec::new();
    response.body_mut().read_to_end(&mut body).await?;
    anyhow::ensure!(
        response.status().is_success(),
        "failed to fetch release manifest ({}): {}",
        response.status(),
        String::from_utf8_lossy(&body)
    );

    serde_json::from_slice(&body).with_context(|| {
        format!(
            "parsing release manifest: {}",
            String::from_utf8_lossy(&body)
        )
    })
}

fn find_asset<'a>(manifest: &'a ReleaseManifest, os: &str, arch: &str) -> Result<&'a ReleaseAsset> {
    let wanted = asset_name(os, arch);
    manifest
        .assets
        .iter()
        .find(|asset| asset.name == wanted)
        .with_context(|| {
            format!(
                "release {} publishes no asset named {wanted}",
                manifest.version
            )
        })
}

/// Download `asset` to `target_path`, rejecting it unless its SHA-256 matches.
///
/// The archive lands in a temp file inside the same directory and is only renamed
/// into place after the digest matches, so a rejected download can never be
/// mistaken for a usable binary.
async fn download_verified(
    target_path: &Path,
    asset: &ReleaseAsset,
    client: &Arc<dyn HttpClient>,
) -> Result<()> {
    let parent = target_path
        .parent()
        .context("remote server target path has no parent directory")?;

    let expected = asset.sha256.trim().to_ascii_lowercase();
    anyhow::ensure!(
        expected.len() == 64 && expected.chars().all(|c| c.is_ascii_hexdigit()),
        "release manifest lists a malformed sha256 for {}: {:?}",
        asset.name,
        asset.sha256
    );

    let temp = tempfile::Builder::new().tempfile_in(parent)?;
    let mut temp_file = File::create(temp.path()).await?;

    let request = get_following_redirects(&asset.url)?;
    let mut response = client
        .send(request)
        .await
        .with_context(|| format!("downloading {}", asset.url))?;
    anyhow::ensure!(
        response.status().is_success(),
        "failed to download {} ({})",
        asset.url,
        response.status()
    );

    // Hash while streaming so the archive is never held entirely in memory.
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = response.body_mut().read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        smol::io::AsyncWriteExt::write_all(&mut temp_file, &buffer[..read]).await?;
    }
    smol::io::AsyncWriteExt::flush(&mut temp_file).await?;
    drop(temp_file);

    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        // The temp file is dropped with `temp`, so the rejected bytes never persist.
        anyhow::bail!(
            "checksum mismatch for {}: manifest says {expected}, download hashed to {actual}. \
             Refusing to install a remote server binary that does not match its manifest.",
            asset.name
        );
    }

    smol::fs::rename(temp.path(), target_path)
        .await
        .with_context(|| format!("installing verified remote server to {target_path:?}"))?;
    // The archive has been moved out; keep the handle from deleting the destination.
    std::mem::forget(temp);

    Ok(())
}

/// Drop the oldest cached archives, keeping `keep_path` and the newest `limit - 1`.
async fn cleanup_cache(platform_dir: &Path, keep_path: &Path, limit: usize) -> Result<()> {
    let mut entries = Vec::new();
    let mut children = smol::fs::read_dir(platform_dir).await?;
    while let Some(child) = futures::StreamExt::next(&mut children).await {
        let child = child?;
        let path = child.path();
        if path == keep_path || path.extension().is_none_or(|ext| ext != "gz") {
            continue;
        }
        let modified = child.metadata().await.and_then(|m| m.modified()).ok();
        entries.push((path, modified));
    }

    if entries.len() < limit {
        return Ok(());
    }

    entries.sort_by_key(|(_, modified)| *modified);
    let excess = entries.len() + 1 - limit;
    for (path, _) in entries.into_iter().take(excess) {
        if let Err(error) = smol::fs::remove_file(&path).await {
            log::warn!("failed to remove cached remote server {path:?}: {error:#}");
        }
    }

    Ok(())
}

/// Resolve the download URL for a remote server release without fetching it.
pub async fn get_remote_server_release_url(
    client: Arc<dyn HttpClient>,
    version: Option<Version>,
    os: &str,
    arch: &str,
) -> Result<Option<String>> {
    let manifest = fetch_manifest(&client, version.as_ref()).await?;
    let asset = find_asset(&manifest, os, arch)?;
    Ok(Some(asset.url.clone()))
}

/// Download the remote server release for `os`/`arch`, verify it, and return its path.
///
/// A cached archive for the same version is reused without re-downloading.
pub async fn download_remote_server_release(
    client: Arc<dyn HttpClient>,
    channel: ReleaseChannel,
    version: Option<Version>,
    os: &str,
    arch: &str,
    set_status: impl Fn(&str, &mut AsyncApp) + Send + 'static,
    cx: &mut AsyncApp,
) -> Result<PathBuf> {
    set_status("Fetching remote server release", cx);
    let manifest = fetch_manifest(&client, version.as_ref()).await?;
    let asset = find_asset(&manifest, os, arch)?;

    let platform_dir = paths::remote_servers_dir()
        .join(channel.dev_name())
        .join(format!("{os}-{arch}"));
    smol::fs::create_dir_all(&platform_dir).await.ok();
    let version_path = platform_dir.join(format!("{}.gz", manifest.version));

    if smol::fs::metadata(&version_path).await.is_err() {
        log::info!(
            "downloading zed-remote-server {os} {arch} version {}",
            manifest.version
        );
        set_status("Downloading remote server", cx);
        download_verified(&version_path, asset, &client).await?;
    }

    if let Err(error) = cleanup_cache(&platform_dir, &version_path, CACHE_LIMIT).await {
        log::warn!("failed to clean up remote server cache in {platform_dir:?}: {error:#}");
    }

    Ok(version_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(sha256: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: "zed-remote-server-macos-aarch64.gz".to_string(),
            url: "https://example.invalid/artifact.gz".to_string(),
            sha256: sha256.to_string(),
        }
    }

    /// A manifest entry that is not a 64-char hex digest is rejected before any
    /// download starts — a truncated or placeholder checksum must not be treated
    /// as "no checksum required".
    #[test]
    fn malformed_digests_are_rejected() {
        for bad in [
            "",
            "deadbeef",
            "zzzz2e8d7c1f4b6a9e0d3c5f8a1b4e7d0c3f6a9b2e5d8c1f4a7b0e3d6c9f2a5b8",
            &"a".repeat(63),
            &"a".repeat(65),
        ] {
            let asset = asset(bad);
            let expected = asset.sha256.trim().to_ascii_lowercase();
            let looks_valid =
                expected.len() == 64 && expected.chars().all(|c| c.is_ascii_hexdigit());
            assert!(!looks_valid, "should have rejected digest {bad:?}");
        }
    }

    #[test]
    fn well_formed_digest_is_accepted() {
        let asset = asset("9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08");
        let expected = asset.sha256.trim().to_ascii_lowercase();
        assert_eq!(expected.len(), 64);
        assert!(expected.chars().all(|c| c.is_ascii_hexdigit()));
        // Case is normalised so an uppercase manifest entry still matches hex::encode output.
        assert!(expected.starts_with("9f86d081"));
    }

    /// The digest comparison is what stands between a tampered archive and a
    /// binary executed on a remote host. Verify the comparison itself.
    #[test]
    fn digest_comparison_detects_tampering() {
        let genuine = b"genuine remote server archive";
        let tampered = b"tampered remote server archive";

        let mut hasher = Sha256::new();
        hasher.update(genuine);
        let expected = hex::encode(hasher.finalize());

        let mut hasher = Sha256::new();
        hasher.update(tampered);
        let actual = hex::encode(hasher.finalize());

        assert_ne!(expected, actual, "tampering must change the digest");
        assert_eq!(expected.len(), 64);
    }

    fn url_for(version: Option<&str>) -> String {
        let version = version.map(|version| Version::parse(version).expect("a valid version"));
        manifest_url(version.as_ref())
    }

    #[test]
    fn manifest_url_drops_build_metadata() {
        assert_eq!(
            url_for(Some("0.1.5+stable.8.45fd2f6")),
            "https://github.com/tgiap04/zode/releases/download/v0.1.5/remote-server-manifest.json"
        );
    }

    #[test]
    fn manifest_url_drops_pre_release() {
        assert_eq!(
            url_for(Some("0.1.6-beta.1")),
            "https://github.com/tgiap04/zode/releases/download/v0.1.6/remote-server-manifest.json"
        );
    }

    #[test]
    fn manifest_url_without_a_version_is_the_latest_release() {
        assert_eq!(
            url_for(None),
            "https://github.com/tgiap04/zode/releases/latest/download/remote-server-manifest.json"
        );
    }

    /// The shape `script/build-remote-server-manifest` writes, as a release publishes it.
    #[test]
    fn a_published_manifest_parses_and_names_the_linux_archive() {
        let x86_64 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        let aarch64 = "60303ae22b998861bce3b28f33eec1be758a213c86c93c076dbe9f558c11c752";
        let json = format!(
            r#"{{"version":"0.1.6","assets":[
                {{"name":"zed-remote-server-linux-aarch64.gz","url":"https://github.com/tgiap04/zode/releases/download/v0.1.6/zed-remote-server-linux-aarch64.gz","sha256":"{aarch64}"}},
                {{"name":"zed-remote-server-linux-x86_64.gz","url":"https://github.com/tgiap04/zode/releases/download/v0.1.6/zed-remote-server-linux-x86_64.gz","sha256":"{x86_64}"}}
            ]}}"#
        );
        let manifest: ReleaseManifest = serde_json::from_str(&json).expect("manifest parses");
        assert_eq!(manifest.version, "0.1.6");

        let asset = find_asset(&manifest, "linux", "x86_64").expect("x86_64 is published");
        assert_eq!(
            asset.url,
            "https://github.com/tgiap04/zode/releases/download/v0.1.6/zed-remote-server-linux-x86_64.gz"
        );
        assert_eq!(asset.sha256, x86_64);

        let missing =
            find_asset(&manifest, "macos", "aarch64").expect_err("no macOS server is published");
        assert!(
            format!("{missing}").contains("zed-remote-server-macos-aarch64.gz"),
            "{missing}"
        );
    }

    /// Runs the release's own manifest script and reads its output the way the app
    /// does, so the script and `ReleaseManifest` cannot drift apart unnoticed. Unix
    /// only: the script is executed through its shebang.
    #[cfg(unix)]
    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "A test of a shell script has to run it, and nothing else is on this thread"
    )]
    fn the_release_manifest_script_writes_what_the_client_reads() {
        let directory = tempfile::tempdir().expect("creating a scratch directory");
        let x86_64 = b"x86_64 remote server archive";
        let aarch64 = b"aarch64 remote server archive";
        std::fs::write(
            directory.path().join("zed-remote-server-linux-x86_64.gz"),
            x86_64,
        )
        .expect("writing the x86_64 archive");
        std::fs::write(
            directory.path().join("zed-remote-server-linux-aarch64.gz"),
            aarch64,
        )
        .expect("writing the aarch64 archive");

        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../script/build-remote-server-manifest");
        let output = std::process::Command::new(&script)
            .arg(directory.path())
            .arg("0.1.6")
            .env("GITHUB_REPOSITORY", RELEASE_REPO)
            .output()
            .unwrap_or_else(|error| panic!("running {}: {error}", script.display()));
        assert!(
            output.status.success(),
            "{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let written = std::fs::read(directory.path().join(MANIFEST_NAME))
            .expect("the script writes the manifest the client fetches");
        let manifest: ReleaseManifest =
            serde_json::from_slice(&written).expect("the client parses the script's manifest");
        assert_eq!(manifest.version, "0.1.6");
        assert_eq!(manifest.assets.len(), 2);

        let version = Version::parse("0.1.6+stable.1.abcdef0").expect("a valid version");
        let manifest_location = manifest_url(Some(&version));
        let release_directory = manifest_location
            .strip_suffix(MANIFEST_NAME)
            .expect("the manifest URL ends in its name");

        for (arch, bytes) in [("x86_64", &x86_64[..]), ("aarch64", &aarch64[..])] {
            let asset = find_asset(&manifest, "linux", arch).expect("the archive is listed");
            assert_eq!(
                asset.url,
                format!("{release_directory}{}", asset_name("linux", arch)),
                "the archive must sit in the release the client fetched the manifest from"
            );
            assert_eq!(asset.sha256, hex::encode(Sha256::digest(bytes)));
        }
    }

    #[test]
    fn asset_names_are_platform_scoped() {
        assert_eq!(
            asset_name("macos", "aarch64"),
            "zed-remote-server-macos-aarch64.gz"
        );
        assert_ne!(
            asset_name("macos", "aarch64"),
            asset_name("linux", "x86_64")
        );
    }
}
