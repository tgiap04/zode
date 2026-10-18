//! A release, in miniature, for tests that need one.
//!
//! Lives here rather than in each caller so that the archive a test serves is
//! built by the same crates that read it, and so that `database_ui` can test
//! the dialog's download step without taking a tar and gzip implementation of
//! its own.
//!
//! What it serves is signed with [`signature::testing`]'s key, and an installer
//! only believes that key once it has been given it with
//! `DriverInstaller::trusting`. A test installer therefore proves the signature
//! path is the one being exercised, and no shipped build trusts what is made
//! here.

use crate::install::manifest::{DriverAsset, DriverManifest, current_target};
use crate::install::store;
use crate::protocol::PROTOCOL_VERSION;
use crate::signature;
use async_compression::futures::write::GzipEncoder;
use futures::AsyncWriteExt as _;
use sha2::{Digest as _, Sha256};
use std::path::Path;

/// A gzipped tar holding one executable named after `id`.
///
/// Named through `store::executable_name`, so the entry carries `.exe` on
/// Windows exactly as `script/build-driver-manifest` writes it for a windows
/// target. Hard-coding the unix spelling made every install test pass on macOS
/// and fail on Windows, where the unpacked file is not the one looked for.
pub async fn driver_archive(id: &str) -> Vec<u8> {
    let contents = b"#!/bin/sh\nexit 0\n";
    let mut builder = async_tar::Builder::new(GzipEncoder::new(Vec::new()));
    let mut header = async_tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, store::executable_name(id), &contents[..])
        .await
        .expect("writing a tar into memory cannot fail");
    let mut encoder = builder
        .into_inner()
        .await
        .expect("finishing a tar into memory cannot fail");
    encoder.close().await.expect("flushing gzip cannot fail");
    encoder.into_inner()
}

fn this_platform() -> &'static str {
    current_target().expect("the test suite runs on a platform Zode publishes for")
}

/// What the release calls the archive: `zode-db-<id>-<target>.tar.gz`, the only
/// shape the manifest is allowed to name. A name without the target would be
/// refused, and would also never be what `script/package-database-drivers`
/// produced.
pub fn archive_asset_for(id: &str) -> String {
    format!("zode-db-{id}-{}.tar.gz", this_platform())
}

/// A manifest naming that archive for this platform, as a value.
///
/// `sha256` is taken rather than computed so a test can publish a checksum that
/// deliberately does not match -- the case the verification exists for. The row
/// carries the protocol this build speaks, as a signed manifest's rows do.
pub fn driver_manifest_for(
    id: &str,
    archive: &[u8],
    sha256: String,
    version: &str,
) -> DriverManifest {
    DriverManifest {
        version: version.to_string(),
        drivers: vec![DriverAsset {
            id: id.to_string(),
            target: this_platform().to_string(),
            asset: archive_asset_for(id),
            entry: store::executable_name(id),
            sha256,
            size: archive.len() as u64,
            protocol: Some(PROTOCOL_VERSION),
        }],
    }
}

/// What `latest` serves: the manifest as the backend re-serialises it, without
/// the `protocol` the backend does not know about, and unsigned. An installer
/// reads only its `version`.
pub fn manifest_for(id: &str, archive: &[u8], sha256: String, version: &str) -> Vec<u8> {
    let mut manifest = driver_manifest_for(id, archive, sha256, version);
    for driver in &mut manifest.drivers {
        driver.protocol = None;
    }
    serde_json::to_vec(&manifest).expect("a manifest of owned strings always serialises")
}

/// The file published beside the archives: `manifest` signed with the test key.
pub fn signed(manifest: &DriverManifest) -> Vec<u8> {
    let json = serde_json::to_vec(manifest).expect("a manifest of owned strings always serialises");
    signature::sign_bundle(&signature::testing::pkcs8(), &json)
        .expect("the test key is a valid signing key")
}

/// [`driver_manifest_for`], signed.
pub fn signed_manifest_for(id: &str, archive: &[u8], sha256: String, version: &str) -> Vec<u8> {
    signed(&driver_manifest_for(id, archive, sha256, version))
}

/// A driver that is simply there, as if installed under `version`.
///
/// `by_zode` says whether it carries the marker an install leaves, which is what
/// separates a downloaded driver from one somebody copied in by hand. Returns
/// the executable.
pub fn plant_driver(root: &Path, id: &str, version: &str, by_zode: bool) -> std::path::PathBuf {
    let directory = store::version_dir_in(root, id, version);
    std::fs::create_dir_all(&directory).expect("a test store is writable");
    let executable = directory.join(store::executable_name(id));
    std::fs::write(&executable, b"not really a driver").expect("a test store is writable");
    if by_zode {
        std::fs::write(
            directory.join(store::INSTALL_MARKER),
            b"installed by Zode\n",
        )
        .expect("a test store is writable");
    }
    executable
}

/// Every entry under one driver's directory, sorted by name: versions, but also
/// any staging or archive leftover an install left beside them. Empty when the
/// driver has no directory at all.
pub fn directory_entries(root: &Path, id: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join(id))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

pub fn sha256_of(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}
