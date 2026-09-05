//! Turning a downloaded archive into a driver on disk.
//!
//! Everything here happens under a temporary directory and is moved into place
//! in one rename at the end. A half-unpacked driver must never be visible where
//! the resolver looks: it would resolve as installed and then fail at `exec`,
//! which is the same class of unhelpful failure the store exists to end.

use anyhow::{Context as _, Result};
use async_compression::futures::bufread::GzipDecoder;
use async_tar::Archive;
use futures::io::BufReader;
use std::path::{Path, PathBuf};

use crate::install::store;

/// Unpacks `archive` and moves the driver's executable into place.
///
/// `entry` is the executable's path inside the archive; `destination` is the
/// version directory it belongs in. Returns the installed executable.
pub async fn install_archive(
    archive: &Path,
    entry: &str,
    destination: &Path,
    executable_name: &str,
) -> Result<PathBuf> {
    let staging = staging_dir(destination);
    // A staging directory left by a previous attempt that died mid-unpack must
    // not contribute files to this one.
    let _ = smol::fs::remove_dir_all(&staging).await;
    smol::fs::create_dir_all(&staging)
        .await
        .with_context(|| format!("creating {}", staging.display()))?;

    unpack(archive, &staging).await?;

    let unpacked = staging.join(entry);
    anyhow::ensure!(
        unpacked.is_file(),
        "the downloaded archive does not contain {entry}"
    );
    store::make_executable(&unpacked)?;

    // The rename is what publishes the install, so it is the last thing done
    // and it moves a directory that is already complete.
    let _ = smol::fs::remove_dir_all(destination).await;
    if let Some(parent) = destination.parent() {
        smol::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    smol::fs::rename(&staging, destination)
        .await
        .with_context(|| {
            format!(
                "moving the unpacked driver from {} to {}",
                staging.display(),
                destination.display()
            )
        })?;

    let installed = destination.join(executable_name);
    anyhow::ensure!(
        installed.is_file(),
        "{entry} did not land at {}",
        installed.display()
    );
    clear_quarantine(&installed).await;
    Ok(installed)
}

/// Beside the destination rather than in the system temp directory, so the
/// final move is a rename within one filesystem rather than a copy.
fn staging_dir(destination: &Path) -> PathBuf {
    let mut staging = destination.as_os_str().to_owned();
    staging.push(".incoming");
    PathBuf::from(staging)
}

async fn unpack(archive: &Path, into: &Path) -> Result<()> {
    let file = smol::fs::File::open(archive)
        .await
        .with_context(|| format!("opening {}", archive.display()))?;
    let decompressed = GzipDecoder::new(BufReader::new(file));
    Archive::new(decompressed)
        .unpack(into)
        .await
        .context("the downloaded archive could not be unpacked")?;
    Ok(())
}

/// Removes `com.apple.quarantine`, if macOS put it there.
///
/// Defensive rather than expected: the flag is set by the programs that
/// download on a user's behalf, and this download is made by Zode itself. But a
/// quarantined driver is killed on launch with no symptom except every database
/// connection failing to start -- exactly the failure the bundle scripts'
/// codesign step exists to prevent -- so it is cheap insurance against being
/// wrong about who sets it.
///
/// Failure is logged, not propagated: no attribute to remove is the ordinary
/// case, and refusing an install over it would be worse than the risk.
async fn clear_quarantine(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let output = smol::process::Command::new("/usr/bin/xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(path)
            .output()
            .await;
        match output {
            Ok(output) if output.status.success() => {
                log::info!("cleared the quarantine flag on {}", path.display());
            }
            Ok(_) => {}
            Err(error) => log::warn!("could not run xattr on {}: {error}", path.display()),
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_compression::futures::write::GzipEncoder;
    use futures::AsyncWriteExt as _;

    /// Built with the same crates that read it, rather than pulling a second
    /// tar and gzip implementation into the workspace for a test.
    async fn archive_containing(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = async_tar::Builder::new(GzipEncoder::new(Vec::new()));
        for (name, contents) in entries {
            let mut header = async_tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, name, *contents)
                .await
                .unwrap();
        }
        let mut encoder = builder.into_inner().await.unwrap();
        encoder.close().await.unwrap();
        encoder.into_inner()
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn an_archive_lands_as_an_executable_at_its_version() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = write(
                root.path(),
                "driver.tar.gz",
                &archive_containing(&[("zode-db-postgres", b"#!/bin/sh\nexit 0\n")]).await,
            );
            let destination = root.path().join("postgres").join("0.1.1");

            let installed = install_archive(
                &archive,
                "zode-db-postgres",
                &destination,
                "zode-db-postgres",
            )
            .await
            .unwrap();

            assert_eq!(installed, destination.join("zode-db-postgres"));
            assert!(installed.is_file());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let mode = std::fs::metadata(&installed).unwrap().permissions().mode();
                assert!(mode & 0o111 != 0, "the driver must be executable: {mode:o}");
            }
        });
    }

    /// An archive that unpacked but held nothing runnable must not leave a
    /// version directory behind: the resolver would call that installed.
    #[test]
    fn an_archive_without_the_driver_installs_nothing() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = write(
                root.path(),
                "driver.tar.gz",
                &archive_containing(&[("readme.txt", b"not a driver")]).await,
            );
            let destination = root.path().join("postgres").join("0.1.1");

            let error = install_archive(
                &archive,
                "zode-db-postgres",
                &destination,
                "zode-db-postgres",
            )
            .await
            .expect_err("an archive with no driver in it is not an install");

            assert!(error.to_string().contains("does not contain"), "{error}");
            assert!(
                !destination.exists(),
                "nothing may be published from a failed unpack"
            );
        });
    }

    /// Bytes that are not a gzip archive at all: a truncated or corrupt
    /// download that still matched its length.
    #[test]
    fn rubbish_in_place_of_an_archive_installs_nothing() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = write(root.path(), "driver.tar.gz", b"not gzip");
            let destination = root.path().join("mysql").join("0.1.1");

            install_archive(&archive, "zode-db-mysql", &destination, "zode-db-mysql")
                .await
                .expect_err("unpacking rubbish must fail");
            assert!(!destination.exists());
        });
    }
}
