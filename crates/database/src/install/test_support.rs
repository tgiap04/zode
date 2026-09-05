//! A release, in miniature, for tests that need one.
//!
//! Lives here rather than in each caller so that the archive a test serves is
//! built by the same crates that read it, and so that `database_ui` can test
//! the dialog's download step without taking a tar and gzip implementation of
//! its own.

use crate::install::manifest::{DriverAsset, DriverManifest, current_target};
use crate::install::store;
use async_compression::futures::write::GzipEncoder;
use futures::AsyncWriteExt as _;
use sha2::{Digest as _, Sha256};

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

/// A manifest naming that archive for this platform.
///
/// `sha256` is taken rather than computed so a test can publish a checksum that
/// deliberately does not match -- the case the verification exists for.
pub fn manifest_for(id: &str, archive: &[u8], sha256: String, version: &str) -> Vec<u8> {
    serde_json::to_vec(&DriverManifest {
        version: version.to_string(),
        drivers: vec![DriverAsset {
            id: id.to_string(),
            target: current_target()
                .expect("the test suite runs on a platform Zode publishes for")
                .to_string(),
            asset: format!("zode-db-{id}.tar.gz"),
            entry: store::executable_name(id),
            sha256,
            size: archive.len() as u64,
        }],
    })
    .expect("a manifest of owned strings always serialises")
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
