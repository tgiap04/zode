//! Turning a downloaded archive into a driver on disk.
//!
//! Everything here happens under a temporary directory and is moved into place
//! in one rename at the end. A half-unpacked driver must never be visible where
//! the resolver looks: it would resolve as installed and then fail at `exec`,
//! which is the same class of unhelpful failure the store exists to end.

use anyhow::{Context as _, Result};
use async_compression::futures::bufread::GzipDecoder;
use async_tar::Archive;
use futures::AsyncWriteExt as _;
use futures::io::BufReader;
use std::path::{Path, PathBuf};

use crate::install::store;

/// Refuses an `entry` that is not exactly the driver's own executable name.
///
/// `entry` arrives from the manifest, and `Path::join` is not a containment
/// operation: an absolute value discards the directory it is joined to
/// outright, and a `..` component walks out of it. That reaches
/// `store::make_executable`, which would then chmod a file of the manifest's
/// choosing.
///
/// Equality with the name the driver is installed under settles that and one
/// more thing. The resolver looks for that name directly inside the version
/// directory, so an entry anywhere else -- `nested/zode-db-x` -- would unpack
/// fine and then leave an install the resolver cannot see. It has to be caught
/// here, before the install it would replace is deleted.
fn ensure_entry_is_the_executable(entry: &str, executable_name: &str) -> Result<()> {
    anyhow::ensure!(
        entry == executable_name,
        "the driver manifest names {entry:?} as the executable inside the archive, but a driver \
         is installed as {executable_name:?}; refusing to unpack"
    );
    Ok(())
}

/// Unpacks `archive` into `staging` and moves the driver's executable into place.
///
/// `entry` is the executable's path inside the archive; `destination` is the
/// version directory it belongs in. `staging` is scratch space of the caller's
/// choosing, beside `destination` so the final move stays a rename, and named
/// per install so two installs never share one. Returns the installed
/// executable.
pub async fn install_archive(
    archive: &Path,
    entry: &str,
    staging: &Path,
    destination: &Path,
    executable_name: &str,
) -> Result<PathBuf> {
    ensure_entry_is_the_executable(entry, executable_name)?;

    let installed = unpack_and_publish(archive, entry, staging, destination, executable_name).await;
    if installed.is_err() {
        discard_staging(staging).await;
    }
    installed
}

async fn unpack_and_publish(
    archive: &Path,
    entry: &str,
    staging: &Path,
    destination: &Path,
    executable_name: &str,
) -> Result<PathBuf> {
    // A staging directory left by a previous attempt that died mid-unpack must
    // not contribute files to this one.
    store::ignore_not_found(smol::fs::remove_dir_all(staging).await)
        .with_context(|| format!("clearing {}", staging.display()))?;
    smol::fs::create_dir_all(staging)
        .await
        .with_context(|| format!("creating {}", staging.display()))?;

    unpack(archive, staging).await?;

    // `symlink_metadata`, not `is_file`: an archive can hold a link by this
    // name, and every call that follows -- the permission change in particular
    // -- would act on whatever it points at.
    let unpacked = staging.join(entry);
    match smol::fs::symlink_metadata(&unpacked).await {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Ok(_) => anyhow::bail!("{entry} in the downloaded archive is not a regular file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("the downloaded archive does not contain {entry}")
        }
        Err(error) => {
            return Err(error).with_context(|| format!("inspecting {}", unpacked.display()));
        }
    }
    store::make_executable(&unpacked)?;
    mark_installed(staging).await?;

    // The rename is what publishes the install, so it is the last thing done
    // and it moves a directory that is already complete.
    store::ignore_not_found(smol::fs::remove_dir_all(destination).await)
        .with_context(|| format!("replacing the install at {}", destination.display()))?;
    if let Some(parent) = destination.parent() {
        smol::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let installed = destination.join(executable_name);
    if let Err(error) = smol::fs::rename(staging, destination).await {
        // Another install of this very version -- verified against the same
        // signed manifest, so the same bytes -- can publish between the removal
        // above and this move, and a rename onto a directory that is not empty
        // fails. What is there is then what was asked for, so it is accepted
        // rather than reported as a failure of an install that has in effect
        // succeeded. Anything else is still an error.
        let published_by_another_install = smol::unblock({
            let installed = installed.clone();
            move || installed.is_file() && store::installed_by_zode(&installed)
        })
        .await;
        if !published_by_another_install {
            return Err(error).with_context(|| {
                format!(
                    "moving the unpacked driver from {} to {}",
                    staging.display(),
                    destination.display()
                )
            });
        }
    }

    clear_quarantine(&installed).await;
    Ok(installed)
}

/// Writes [`store::INSTALL_MARKER`], which says Zode put this driver here.
///
/// `create_new` so an archive that already holds something by that name -- a
/// link to a file elsewhere, say -- fails the install instead of being written
/// through.
async fn mark_installed(directory: &Path) -> Result<()> {
    let path = directory.join(store::INSTALL_MARKER);
    let mut marker = smol::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
        .with_context(|| format!("marking {} as installed by Zode", directory.display()))?;
    marker.write_all(b"installed by Zode\n").await?;
    marker.flush().await?;
    Ok(())
}

/// Best-effort: the install has already failed, and that failure is what the
/// caller must see. A directory that will not delete is logged rather than
/// allowed to replace it, and the next attempt clears it before starting.
async fn discard_staging(staging: &Path) {
    if let Err(error) = store::ignore_not_found(smol::fs::remove_dir_all(staging).await) {
        log::warn!(
            "could not remove the staging directory {}: {error}",
            staging.display()
        );
    }
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

    fn staging_for(destination: &Path) -> PathBuf {
        store::hidden_sibling(destination, "incoming")
    }

    async fn install(
        archive: &Path,
        entry: &str,
        destination: &Path,
        executable_name: &str,
    ) -> Result<PathBuf> {
        install_archive(
            archive,
            entry,
            &staging_for(destination),
            destination,
            executable_name,
        )
        .await
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

            let installed = install(
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

    /// What lets Remove tell a driver Zode installed from one somebody copied
    /// into the store: the install writes the marker, and nothing else does.
    #[test]
    fn an_install_is_marked_as_zodes_own() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = write(
                root.path(),
                "driver.tar.gz",
                &archive_containing(&[("zode-db-postgres", b"#!/bin/sh\nexit 0\n")]).await,
            );
            let destination = root.path().join("postgres").join("0.1.1");

            let installed = install(
                &archive,
                "zode-db-postgres",
                &destination,
                "zode-db-postgres",
            )
            .await
            .unwrap();

            assert!(store::installed_by_zode(&installed));
        });
    }

    /// An archive carrying the marker's name is not trusted to have meant it,
    /// least of all as a link that the write would otherwise follow.
    #[test]
    fn an_archive_that_brings_its_own_marker_is_refused() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = write(
                root.path(),
                "driver.tar.gz",
                &archive_containing(&[
                    ("zode-db-postgres", b"#!/bin/sh\nexit 0\n"),
                    (store::INSTALL_MARKER, b"forged"),
                ])
                .await,
            );
            let destination = root.path().join("postgres").join("0.1.1");

            install(
                &archive,
                "zode-db-postgres",
                &destination,
                "zode-db-postgres",
            )
            .await
            .expect_err("an archive may not write the marker itself");

            assert!(!destination.exists());
            assert!(!staging_for(&destination).exists());
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

            let error = install(
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
            assert!(
                !staging_for(&destination).exists(),
                "the unpacked files must not outlive the refusal"
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

            install(&archive, "zode-db-mysql", &destination, "zode-db-mysql")
                .await
                .expect_err("unpacking rubbish must fail");
            assert!(!destination.exists());
            assert!(!staging_for(&destination).exists());
        });
    }

    /// An attempt that died mid-unpack leaves its directory behind. A later
    /// attempt must start from nothing, or the files the dead one wrote would be
    /// published beside the executable it was verified against.
    #[test]
    fn a_leftover_staging_directory_contributes_nothing_to_the_next_install() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = write(
                root.path(),
                "driver.tar.gz",
                &archive_containing(&[("zode-db-postgres", b"#!/bin/sh\nexit 0\n")]).await,
            );
            let destination = root.path().join("postgres").join("0.1.5-rc.1");
            let staging = staging_for(&destination);
            std::fs::create_dir_all(&staging).unwrap();
            write(&staging, "left-by-a-dead-attempt", b"stale");

            let installed = install(
                &archive,
                "zode-db-postgres",
                &destination,
                "zode-db-postgres",
            )
            .await
            .unwrap();

            assert!(installed.is_file());
            assert!(!destination.join("left-by-a-dead-attempt").exists());
            assert!(!staging.exists());
        });
    }

    /// `Path::join` is not containment: joining an absolute path discards
    /// what it was joined to, and `..` walks out. Either would carry the
    /// manifest's choice of path into `store::make_executable`. Anywhere but
    /// directly in the version directory is also somewhere the resolver does not
    /// look.
    #[test]
    fn an_entry_that_is_not_the_executable_itself_is_refused() {
        for entry in [
            "/etc/passwd",
            "../../../../etc/passwd",
            "..",
            "nested/../../escape",
            "nested/zode-db-mysql",
            "./zode-db-mysql",
            "zode-db-postgres",
            "",
        ] {
            assert!(
                ensure_entry_is_the_executable(entry, "zode-db-mysql").is_err(),
                "{entry:?} must be refused"
            );
        }
        ensure_entry_is_the_executable("zode-db-mysql", "zode-db-mysql").unwrap();
    }

    /// The check has to run before anything touches the filesystem -- a
    /// refusal that happens after the archive is already unpacked has
    /// already done the work it was meant to prevent. In particular it must
    /// precede the removal of an install already in place.
    #[test]
    fn a_misplaced_entry_is_refused_before_anything_is_touched() {
        smol::block_on(async {
            let root = tempfile::tempdir().unwrap();
            let archive = write(
                root.path(),
                "driver.tar.gz",
                &archive_containing(&[("nested/zode-db-mysql", b"#!/bin/sh\nexit 0\n")]).await,
            );
            let destination = root.path().join("mysql").join("0.1.1");
            std::fs::create_dir_all(&destination).unwrap();
            let existing = write(&destination, "zode-db-mysql", b"the install already here");

            let error = install(
                &archive,
                "nested/zode-db-mysql",
                &destination,
                "zode-db-mysql",
            )
            .await
            .expect_err("an entry in a subdirectory must be refused");

            assert!(error.to_string().contains("refusing to unpack"), "{error}");
            assert_eq!(
                std::fs::read(&existing).unwrap(),
                b"the install already here",
                "the install in place must survive a refusal"
            );
            assert!(!staging_for(&destination).exists());
        });
    }

    /// An archive can hold a link by the executable's name. Every call after the
    /// unpack follows links, so one pointing out of the staging directory would
    /// have its target's permissions changed.
    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_the_executable_is_refused_and_its_target_left_alone() {
        smol::block_on(async {
            use std::os::unix::fs::PermissionsExt as _;

            let root = tempfile::tempdir().unwrap();
            let target = write(root.path(), "somebody-elses-file", b"private");
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();

            let mut builder = async_tar::Builder::new(GzipEncoder::new(Vec::new()));
            let mut header = async_tar::Header::new_gnu();
            header.set_entry_type(async_tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_link_name(&target).unwrap();
            header.set_cksum();
            builder
                .append_data(&mut header, "zode-db-postgres", futures::io::empty())
                .await
                .unwrap();
            let mut encoder = builder.into_inner().await.unwrap();
            encoder.close().await.unwrap();
            let archive = write(root.path(), "driver.tar.gz", &encoder.into_inner());
            let destination = root.path().join("postgres").join("0.1.1");

            let error = install(
                &archive,
                "zode-db-postgres",
                &destination,
                "zode-db-postgres",
            )
            .await
            .expect_err("a link is not an executable");

            assert!(error.to_string().contains("not a regular file"), "{error}");
            assert_eq!(
                std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o600,
                "what the link points at must not be made executable"
            );
            assert!(!destination.exists());
        });
    }
}
