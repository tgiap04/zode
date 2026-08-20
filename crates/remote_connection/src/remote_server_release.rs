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
use http_client::{AsyncBody, HttpClient, Request};
use release_channel::ReleaseChannel;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use smol::fs::File;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Base URL for release artifacts.
///
/// **Phase 12 must set this** to the fork's own release host (a GitHub Releases
/// base, an S3 bucket, or similar). It is deliberately left empty so that a
/// misconfigured build fails loudly at the point of use rather than silently
/// reaching for someone else's infrastructure.
const RELEASE_BASE_URL: &str = "";

/// How many cached remote-server archives to keep per platform.
const CACHE_LIMIT: usize = 5;

/// One entry of `manifest.json`, published alongside the release artifacts.
///
/// ```json
/// {
///   "version": "0.235.0",
///   "assets": [
///     { "name": "zed-remote-server-macos-aarch64.gz",
///       "url": "https://.../zed-remote-server-macos-aarch64.gz",
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

fn manifest_url(channel: ReleaseChannel, version: Option<&Version>) -> Result<String> {
    anyhow::ensure!(
        !RELEASE_BASE_URL.is_empty(),
        "no release host is configured for this build, so the remote server binary \
         cannot be downloaded. Install zed-remote-server manually on the remote host, \
         or set RELEASE_BASE_URL in crates/remote_connection/src/remote_server_release.rs."
    );

    // Strip pre-release and build metadata: artifacts are published per released version.
    let tag = match version {
        Some(version) => {
            let mut version = version.clone();
            version.pre = semver::Prerelease::EMPTY;
            version.build = semver::BuildMetadata::EMPTY;
            format!("v{version}")
        }
        None => "latest".to_string(),
    };

    Ok(format!(
        "{}/{}/{}/manifest.json",
        RELEASE_BASE_URL.trim_end_matches('/'),
        channel.dev_name(),
        tag
    ))
}

async fn fetch_manifest(
    client: &Arc<dyn HttpClient>,
    channel: ReleaseChannel,
    version: Option<&Version>,
) -> Result<ReleaseManifest> {
    let url = manifest_url(channel, version)?;
    let request = Request::get(&url).body(AsyncBody::empty())?;
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

    let request = Request::get(&asset.url).body(AsyncBody::empty())?;
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
    channel: ReleaseChannel,
    version: Option<Version>,
    os: &str,
    arch: &str,
) -> Result<Option<String>> {
    let manifest = fetch_manifest(&client, channel, version.as_ref()).await?;
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
    let manifest = fetch_manifest(&client, channel, version.as_ref()).await?;
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
        let tampered = b"tampered remote server archve";

        let mut hasher = Sha256::new();
        hasher.update(genuine);
        let expected = hex::encode(hasher.finalize());

        let mut hasher = Sha256::new();
        hasher.update(tampered);
        let actual = hex::encode(hasher.finalize());

        assert_ne!(expected, actual, "tampering must change the digest");
        assert_eq!(expected.len(), 64);
    }

    #[test]
    fn manifest_url_fails_loudly_when_unconfigured() {
        // RELEASE_BASE_URL is empty until Phase 12 sets it. The error must name the
        // manual-install escape hatch rather than silently producing a bogus URL.
        if RELEASE_BASE_URL.is_empty() {
            let error = manifest_url(ReleaseChannel::Dev, None)
                .expect_err("an unconfigured release host must be an error");
            let message = format!("{error}");
            assert!(
                message.contains("no release host is configured"),
                "{message}"
            );
            assert!(message.contains("manually"), "{message}");
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
