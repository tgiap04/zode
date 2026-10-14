//! What a version says about the drivers it published.
//!
//! Every version carries one small JSON asset naming every driver binary it
//! shipped, with the checksum of each. Reading it is one request against a
//! fixed URL; discovering the same thing any other way would take a guess at
//! asset naming.

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::install::store;

/// The name the manifest is published under, at every version.
pub const MANIFEST_ASSET: &str = "zode-db-drivers-manifest.json";

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

    /// Fails closed on a `version` or an `asset` that is not the shape the
    /// server publishes.
    ///
    /// Both arrive from the network and become URL segments, and the version a
    /// directory name too, so `../x` or `latest` must stop here rather than be
    /// joined to anything. Every driver is checked, not only the one a caller
    /// will want: a manifest with one entry the server would never have
    /// accepted was not published by it.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let manifest: Self = serde_json::from_slice(bytes)
            .context("the driver manifest is not the shape Zode expects")?;
        anyhow::ensure!(
            store::is_version(&manifest.version),
            "the driver manifest names {:?} as its version, which is not a published driver \
             version (MAJOR.MINOR.PATCH, optionally followed by -prerelease)",
            manifest.version
        );
        for driver in &manifest.drivers {
            anyhow::ensure!(
                is_archive_asset(&driver.asset),
                "the driver manifest names {:?} as the asset for `{}` on {}, which is not a \
                 published driver archive (zode-db-<id>-<target>.tar.gz)",
                driver.asset,
                driver.id,
                driver.target
            );
        }
        Ok(manifest)
    }
}

/// Whether `name` is an archive name the server accepts when a manifest is
/// published: `zode-db-<id>-<target>.tar.gz`, in lowercase letters and digits
/// with `-` or `_` between words, the first separator being the `-` that ends
/// the id.
///
/// Between the fixed prefix and `.tar.gz` nothing but that alphabet is allowed,
/// so the name cannot carry a `/`, a `\`, a `.` or a `%`, which is what makes it
/// safe to put in a URL path as it stands.
fn is_archive_asset(name: &str) -> bool {
    let Some(stem) = name
        .strip_prefix("zode-db-")
        .and_then(|rest| rest.strip_suffix(".tar.gz"))
    else {
        return false;
    };
    let is_word = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    };
    stem.split_once('-')
        .is_some_and(|(id, target)| is_word(id) && target.split(['-', '_']).all(is_word))
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

    /// The version arrives from the network and becomes a URL segment and a
    /// directory name, so a manifest naming anything else is refused before any
    /// of that happens.
    #[test]
    fn a_manifest_naming_a_version_that_is_not_one_is_refused() {
        let json_with_version = |version: &str| {
            let mut manifest = manifest();
            manifest.version = version.into();
            serde_json::to_vec(&manifest).unwrap()
        };
        for version in ["../escape", "latest", "0.1.4+stable", ""] {
            let error = DriverManifest::parse(&json_with_version(version))
                .expect_err("a version that is not a version must not be accepted");
            let message = error.to_string();
            assert!(message.contains(&format!("{version:?}")), "{message}");
        }
        DriverManifest::parse(&json_with_version("0.1.5-rc.1")).unwrap();
    }

    /// The asset becomes a URL segment, so a manifest naming anything the server
    /// would not have accepted as an archive is refused whole -- including when
    /// the bad entry is not the one this platform would have used.
    #[test]
    fn a_manifest_naming_an_asset_that_is_not_an_archive_is_refused() {
        let json_with_asset = |asset: &str| {
            let mut manifest = manifest();
            manifest.drivers[1].asset = asset.into();
            serde_json::to_vec(&manifest).unwrap()
        };
        for asset in [
            "",
            "../zode-db-postgres-aarch64-apple-darwin.tar.gz",
            "nested/zode-db-postgres-aarch64-apple-darwin.tar.gz",
            "..\\zode-db-postgres-aarch64-apple-darwin.tar.gz",
            "zode-db-postgres-aarch64-apple-darwin.tar.gz/",
            "zode-db-postgres-aarch64-apple-darwin.tar.gz?token=1",
            "zode-db-postgres-aarch64-apple-darwin.tar.gz\n",
            "zode-db-postgres-aarch64-apple-darwin.zip",
            "zode-db-postgres.tar.gz",
            "zode-db-drivers-manifest.json",
            "ZODE-db-postgres-aarch64-apple-darwin.tar.gz",
            "zode-db-Postgres-aarch64-apple-darwin.tar.gz",
            "zode-db-postgres_aarch64.tar.gz",
            "zode-db-postgres--aarch64.tar.gz",
            "zode-db--postgres-aarch64.tar.gz",
            "zode-db-postgres-aarch64-.tar.gz",
            "zode-db-postgres-aarch64.x.tar.gz",
            "zode-db-postgres-%2e%2e.tar.gz",
        ] {
            let error = DriverManifest::parse(&json_with_asset(asset))
                .expect_err("an asset that is not an archive name must not be accepted");
            let message = error.to_string();
            assert!(message.contains(&format!("{asset:?}")), "{message}");
        }
        for asset in [
            "zode-db-postgres-aarch64-apple-darwin.tar.gz",
            "zode-db-mysql-x86_64-pc-windows-msvc.tar.gz",
            "zode-db-sqlite-x86_64-unknown-linux-gnu.tar.gz",
            "zode-db-a-b_c.tar.gz",
        ] {
            DriverManifest::parse(&json_with_asset(asset))
                .unwrap_or_else(|error| panic!("{asset:?} must be accepted: {error}"));
        }
    }

    #[test]
    fn this_platform_names_a_target_zode_publishes_for() {
        assert!(
            current_target().is_some(),
            "the test suite runs on a platform Zode ships, so this must resolve"
        );
    }
}
