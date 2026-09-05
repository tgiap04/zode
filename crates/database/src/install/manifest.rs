//! What the release says about the drivers it published.
//!
//! A release carries one small JSON asset naming every driver binary it
//! shipped, with the checksum of each. Reading it is one request against a
//! fixed URL; discovering the same thing through the GitHub API would take a
//! token, a page of release metadata, and a guess at asset naming.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// The name the manifest is published under, in every release.
pub const MANIFEST_ASSET: &str = "zode-db-drivers-manifest.json";

/// Where a build looks for its drivers.
///
/// The version is the app's own. A driver speaks a pinned protocol
/// (`crate::PROTOCOL_VERSION`), and the release that shipped this app is the
/// one whose drivers were built against it -- so an app never fetches from a
/// release other than its own, and never runs a driver left by another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseCoordinates {
    /// `owner/repo`, from `release_channel::RELEASE_REPO`.
    pub repo: String,
    /// The app version, which is also the release tag without its `v`.
    pub version: String,
}

impl ReleaseCoordinates {
    pub fn new(repo: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            version: version.into(),
        }
    }

    pub fn asset_url(&self, asset: &str) -> String {
        let Self { repo, version } = self;
        format!("https://github.com/{repo}/releases/download/v{version}/{asset}")
    }

    pub fn manifest_url(&self) -> String {
        self.asset_url(MANIFEST_ASSET)
    }

    pub fn release_url(&self) -> String {
        let Self { repo, version } = self;
        format!("https://github.com/{repo}/releases/tag/v{version}")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DriverManifest {
    pub version: String,
    pub drivers: Vec<DriverAsset>,
}

/// One driver, for one platform.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DriverAsset {
    /// The driver id, as the registry and stored connections name it.
    pub id: String,
    /// The Rust target triple the binary was built for.
    pub target: String,
    /// The release asset holding it.
    pub asset: String,
    /// The executable's path *inside* the archive.
    ///
    /// Present so several drivers may share one asset. Notarising four archives
    /// per platform costs release time that notarising one does not, and if
    /// that trade turns out badly, the four rows simply name one `asset` and
    /// four different `entry` values. Nothing else has to change.
    pub entry: String,
    /// Lowercase hex SHA-256 of the asset, as published.
    pub sha256: String,
    pub size: u64,
}

impl DriverManifest {
    pub fn find(&self, id: &str, target: &str) -> Option<&DriverAsset> {
        self.drivers
            .iter()
            .find(|driver| driver.id == id && driver.target == target)
    }

    /// Which platforms a driver was published for, for an error that can say
    /// what *is* available rather than only what is not.
    pub fn targets_for(&self, id: &str) -> Vec<&str> {
        self.drivers
            .iter()
            .filter(|driver| driver.id == id)
            .map(|driver| driver.target.as_str())
            .collect()
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("the driver manifest is not the shape Zode expects")
    }
}

/// Lowercase hex, 64 characters. Anything else is refused rather than compared.
///
/// Fails closed, for the same reason the updater does: a checksum that cannot
/// be parsed cannot be checked against, and an unverifiable binary is one that
/// gets executed.
pub fn parse_sha256(asset: &str, digest: &str) -> Result<String> {
    let hex = digest.trim().to_ascii_lowercase();
    anyhow::ensure!(
        hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
        "the driver manifest lists a malformed sha256 for {asset}: {digest:?}"
    );
    Ok(hex)
}

/// The target triple this build runs on.
///
/// Assembled rather than taken from `env!("TARGET")`, which cargo does not set
/// for a normal build. The spellings match what the bundle scripts pass to
/// `cargo build --target`, because that is what names the published asset.
pub fn current_target() -> Option<&'static str> {
    Some(match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "windows") => "aarch64-pc-windows-msvc",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        // A platform Zode is not published for. `None` rather than a guess: a
        // fabricated triple asks the release for an asset that cannot exist,
        // and the 404 that comes back says nothing about why.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> DriverManifest {
        DriverManifest {
            version: "0.1.1".into(),
            drivers: vec![
                DriverAsset {
                    id: "postgres".into(),
                    target: "aarch64-apple-darwin".into(),
                    asset: "zode-db-postgres-aarch64-apple-darwin.tar.gz".into(),
                    entry: "zode-db-postgres".into(),
                    sha256: "a".repeat(64),
                    size: 4_812_345,
                },
                DriverAsset {
                    id: "postgres".into(),
                    target: "x86_64-unknown-linux-gnu".into(),
                    asset: "zode-db-postgres-x86_64-unknown-linux-gnu.tar.gz".into(),
                    entry: "zode-db-postgres".into(),
                    sha256: "b".repeat(64),
                    size: 4_912_345,
                },
            ],
        }
    }

    #[test]
    fn a_manifest_survives_a_round_trip() {
        let json = serde_json::to_vec(&manifest()).unwrap();
        assert_eq!(DriverManifest::parse(&json).unwrap(), manifest());
    }

    /// The same driver on the wrong platform is a miss, not a fallback. A
    /// Linux binary fetched onto macOS fails at `exec` with nothing to go on.
    #[test]
    fn a_driver_is_found_only_for_the_platform_asking() {
        let manifest = manifest();
        assert_eq!(
            manifest
                .find("postgres", "aarch64-apple-darwin")
                .map(|driver| driver.asset.as_str()),
            Some("zode-db-postgres-aarch64-apple-darwin.tar.gz")
        );
        assert!(
            manifest
                .find("postgres", "aarch64-pc-windows-msvc")
                .is_none()
        );
        assert!(manifest.find("mongodb", "aarch64-apple-darwin").is_none());
    }

    #[test]
    fn a_missing_driver_can_still_say_what_was_published() {
        let manifest = manifest();
        let mut targets = manifest.targets_for("postgres");
        targets.sort_unstable();
        assert_eq!(
            targets,
            ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"]
        );
        assert!(manifest.targets_for("mongodb").is_empty());
    }

    #[test]
    fn urls_are_built_from_the_tag_that_matches_the_app() {
        let release = ReleaseCoordinates::new("tgiap04/zode", "0.1.1");
        assert_eq!(
            release.manifest_url(),
            "https://github.com/tgiap04/zode/releases/download/v0.1.1/zode-db-drivers-manifest.json"
        );
        assert_eq!(
            release.asset_url("zode-db-postgres-aarch64-apple-darwin.tar.gz"),
            "https://github.com/tgiap04/zode/releases/download/v0.1.1/zode-db-postgres-aarch64-apple-darwin.tar.gz"
        );
    }

    /// Fails closed. A digest that cannot be parsed cannot be compared, and
    /// treating that as "no check needed" is how an unverified binary runs.
    #[test]
    fn a_malformed_checksum_is_refused_rather_than_ignored() {
        assert!(parse_sha256("x", &"a".repeat(64)).is_ok());
        assert_eq!(parse_sha256("x", &"A".repeat(64)).unwrap(), "a".repeat(64));
        assert!(parse_sha256("x", "").is_err());
        assert!(parse_sha256("x", &"a".repeat(63)).is_err());
        assert!(parse_sha256("x", &"z".repeat(64)).is_err());
        assert!(parse_sha256("x", "sha256:{}").is_err());
    }

    #[test]
    fn this_platform_names_a_target_zode_publishes_for() {
        assert!(
            current_target().is_some(),
            "the test suite runs on a platform Zode ships, so this must resolve"
        );
    }
}
