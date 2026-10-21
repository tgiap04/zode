//! Signing the driver manifest, which is what makes a downloaded driver
//! trustworthy: the app refuses a manifest that does not verify against the
//! public key built into it (`database::signature`).
//!
//! The format and the checking live in the `database` crate and are used here
//! as they are, so the file this writes is the file the app reads. Nothing here
//! prints a private key, and none of it reaches a log.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use base64::Engine as _;
use clap::{Args, Subcommand};
use database::signature::{DRIVER_MANIFEST_PUBLIC_KEY, public_key_from_pkcs8, sign_bundle};
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;

/// Where CI hands the signing key over: the secret of the same name, holding
/// the base64 of the PKCS#8 file.
const KEY_ENVIRONMENT_VARIABLE: &str = "ZODE_DRIVER_SIGNING_KEY";

#[derive(Args)]
pub struct DriverManifestArgs {
    #[command(subcommand)]
    command: DriverManifestCommand,
}

#[derive(Subcommand)]
enum DriverManifestCommand {
    /// Writes a new Ed25519 signing key, and prints the public half to embed in
    /// `crates/database/src/signature.rs`.
    GenerateKey(GenerateKeyArgs),
    /// Signs a driver manifest, writing the file that is published beside the
    /// driver archives.
    Sign(SignArgs),
    /// Fails unless the signing key is there and is the one the app trusts.
    /// Signs nothing, so a release can find out at its start rather than at its
    /// end.
    CheckKey(CheckKeyArgs),
    /// Says whether the first version is older than, the same as, or newer than
    /// the second -- by semver, so `0.1.5-rc.1` is older than `0.1.5`.
    CompareVersions(CompareVersionsArgs),
}

#[derive(Args)]
struct GenerateKeyArgs {
    /// Where to write the PKCS#8 private key. Refuses to overwrite a file. Keep
    /// it out of the repository.
    #[arg(long)]
    out: PathBuf,
}

#[derive(Args)]
struct SignArgs {
    /// The manifest to sign, exactly as it will be published.
    #[arg(long)]
    manifest: PathBuf,
    /// Where to write the signed manifest.
    #[arg(long)]
    out: PathBuf,
    /// A PKCS#8 private key file. Without it the key is read, base64 encoded,
    /// from `ZODE_DRIVER_SIGNING_KEY`.
    #[arg(long)]
    key_file: Option<PathBuf>,
}

#[derive(Args)]
struct CheckKeyArgs {
    /// A PKCS#8 private key file. Without it the key is read, base64 encoded,
    /// from `ZODE_DRIVER_SIGNING_KEY`.
    #[arg(long)]
    key_file: Option<PathBuf>,
}

#[derive(Args)]
struct CompareVersionsArgs {
    version: String,
    other: String,
}

pub fn run_driver_manifest(args: DriverManifestArgs) -> Result<()> {
    match args.command {
        DriverManifestCommand::GenerateKey(args) => generate_key(&args.out),
        DriverManifestCommand::Sign(args) => {
            let key = load_key(
                args.key_file.as_deref(),
                std::env::var(KEY_ENVIRONMENT_VARIABLE).ok().as_deref(),
            )?;
            let manifest = std::fs::read(&args.manifest)
                .with_context(|| format!("reading {}", args.manifest.display()))?;
            let signed = sign_manifest(&key, &manifest, &DRIVER_MANIFEST_PUBLIC_KEY)?;
            std::fs::write(&args.out, signed)
                .with_context(|| format!("writing {}", args.out.display()))?;
            println!(
                "signed {} with the key whose public half is {}",
                args.manifest.display(),
                hex(&DRIVER_MANIFEST_PUBLIC_KEY)
            );
            Ok(())
        }
        DriverManifestCommand::CheckKey(args) => {
            let key = load_key(
                args.key_file.as_deref(),
                std::env::var(KEY_ENVIRONMENT_VARIABLE).ok().as_deref(),
            )?;
            let public_key = ensure_key_is_trusted(&key, &DRIVER_MANIFEST_PUBLIC_KEY)?;
            println!(
                "the signing key is the one this build of Zode trusts ({})",
                hex(&public_key)
            );
            Ok(())
        }
        DriverManifestCommand::CompareVersions(args) => {
            println!("{}", compare_versions(&args.version, &args.other)?);
            Ok(())
        }
    }
}

fn generate_key(out: &Path) -> Result<()> {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .map_err(|error| anyhow::anyhow!("generating a key: {error}"))?;
    write_new_private_file(out, pkcs8.as_ref())?;

    let public_key = public_key_from_pkcs8(pkcs8.as_ref())?;
    println!("wrote the private key to {}", out.display());
    println!("public key (hex): {}", hex(&public_key));
    println!(
        "for DRIVER_MANIFEST_PUBLIC_KEY: [{}]",
        public_key
            .iter()
            .map(|byte| format!("{byte:#04x}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "for the {KEY_ENVIRONMENT_VARIABLE} secret: the base64 of that file, on one line \
         (`base64 < file | tr -d '\\n'`)"
    );
    Ok(())
}

/// `create_new` and mode 0600 from the first byte: a key written with the
/// default mode and tightened afterwards is readable in between.
fn write_new_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write as _;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {} (it must not already exist)", path.display()))?;
    file.write_all(contents)
        .with_context(|| format!("writing {}", path.display()))
}

/// The key to sign with: a file if one was named, otherwise the base64 in the
/// environment. Neither is an error that says where to put one.
fn load_key(key_file: Option<&Path>, from_environment: Option<&str>) -> Result<Vec<u8>> {
    if let Some(path) = key_file {
        return std::fs::read(path)
            .with_context(|| format!("reading the signing key at {}", path.display()));
    }
    let encoded = from_environment
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .with_context(|| {
            format!(
                "no signing key: set {KEY_ENVIRONMENT_VARIABLE} to the base64 of the PKCS#8 key, \
                 or pass --key-file"
            )
        })?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .with_context(|| format!("{KEY_ENVIRONMENT_VARIABLE} is not valid base64"))
}

/// The public half of `pkcs8`, provided it is the one `trusted` names.
///
/// A manifest signed with the wrong key verifies against nothing, and the first
/// anyone would hear of it is every download failing, so this is what both
/// signing and the check at the start of a release stop on.
fn ensure_key_is_trusted(pkcs8: &[u8], trusted: &[u8; 32]) -> Result<[u8; 32]> {
    let public_key = public_key_from_pkcs8(pkcs8)?;
    anyhow::ensure!(
        &public_key == trusted,
        "the signing key's public half is {}, but this build of Zode trusts {}; a manifest \
         signed with it would be refused by every app. Is {KEY_ENVIRONMENT_VARIABLE} the right \
         secret, or has the key been rotated without updating DRIVER_MANIFEST_PUBLIC_KEY?",
        hex(&public_key),
        hex(trusted)
    );
    Ok(public_key)
}

/// Signs `manifest`, refusing when the key is not the one `trusted` names.
fn sign_manifest(pkcs8: &[u8], manifest: &[u8], trusted: &[u8; 32]) -> Result<Vec<u8>> {
    let parsed: serde_json::Value = serde_json::from_slice(manifest)
        .context("the manifest to sign is not JSON; is this the right file?")?;
    anyhow::ensure!(
        parsed
            .get("version")
            .is_some_and(serde_json::Value::is_string)
            && parsed
                .get("drivers")
                .is_some_and(serde_json::Value::is_array),
        "the manifest to sign has no `version` and `drivers`; is this the right file?"
    );

    ensure_key_is_trusted(pkcs8, trusted)?;
    sign_bundle(pkcs8, manifest)
}

fn compare_versions(version: &str, other: &str) -> Result<&'static str> {
    let parse = |text: &str| {
        semver::Version::parse(text).with_context(|| format!("{text:?} is not a version"))
    };
    Ok(match parse(version)?.cmp(&parse(other)?) {
        std::cmp::Ordering::Less => "older",
        std::cmp::Ordering::Equal => "same",
        std::cmp::Ordering::Greater => "newer",
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::signature::verify_bundle;

    const MANIFEST: &[u8] = br#"{"version":"0.1.5","drivers":[]}"#;

    fn key() -> (Vec<u8>, [u8; 32]) {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .expect("a key can be generated")
            .as_ref()
            .to_vec();
        let public = public_key_from_pkcs8(&pkcs8).expect("a generated key has a public half");
        (pkcs8, public)
    }

    /// What CI does: the secret is the base64 of the key file, and what comes
    /// out is what the app verifies.
    #[test]
    fn a_manifest_signed_with_the_trusted_key_verifies_in_the_app() {
        let (pkcs8, public) = key();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&pkcs8);

        let loaded = load_key(None, Some(&encoded)).unwrap();
        let signed = sign_manifest(&loaded, MANIFEST, &public).unwrap();

        assert_eq!(verify_bundle(&signed, &public).unwrap(), MANIFEST);
    }

    /// The failure this guard exists for: the wrong secret in CI.
    #[test]
    fn a_key_the_app_does_not_trust_is_refused_with_both_public_keys() {
        let (pkcs8, public) = key();
        let (_, trusted) = key();

        let error = sign_manifest(&pkcs8, MANIFEST, &trusted).unwrap_err();

        let message = error.to_string();
        assert!(message.contains(&hex(&public)), "{message}");
        assert!(message.contains(&hex(&trusted)), "{message}");
    }

    /// What the first job of a release does, long before anything is built.
    #[test]
    fn the_check_passes_for_the_trusted_key_and_fails_for_any_other() {
        let (pkcs8, public) = key();
        assert_eq!(ensure_key_is_trusted(&pkcs8, &public).unwrap(), public);

        let (_, trusted) = key();
        let message = ensure_key_is_trusted(&pkcs8, &trusted)
            .unwrap_err()
            .to_string();
        assert!(message.contains("ZODE_DRIVER_SIGNING_KEY"), "{message}");
        assert!(message.contains(&hex(&public)), "{message}");
        assert!(ensure_key_is_trusted(b"not a key", &trusted).is_err());
    }

    #[test]
    fn something_that_is_not_a_manifest_is_not_signed() {
        let (pkcs8, public) = key();
        for not_a_manifest in [
            &b"not json"[..],
            br#"{"version":"0.1.5"}"#,
            br#"{"drivers":[]}"#,
            br#"{"version":1,"drivers":[]}"#,
            b"[]",
        ] {
            assert!(
                sign_manifest(&pkcs8, not_a_manifest, &public).is_err(),
                "{not_a_manifest:?}"
            );
        }
    }

    #[test]
    fn no_key_at_all_says_where_to_put_one() {
        for nothing in [None, Some(""), Some("  \n")] {
            let message = load_key(None, nothing).unwrap_err().to_string();
            assert!(message.contains("ZODE_DRIVER_SIGNING_KEY"), "{message}");
            assert!(message.contains("--key-file"), "{message}");
        }
    }

    #[test]
    fn a_secret_that_is_not_base64_is_refused_without_echoing_it() {
        let message = load_key(None, Some("!!! not base64 !!!"))
            .unwrap_err()
            .to_string();
        assert!(message.contains("not valid base64"), "{message}");
        assert!(!message.contains("!!! not base64 !!!"), "{message}");
    }

    #[test]
    fn a_secret_pasted_with_a_trailing_newline_still_loads() {
        let (pkcs8, _) = key();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&pkcs8);
        assert_eq!(
            load_key(None, Some(&format!("{encoded}\n"))).unwrap(),
            pkcs8
        );
    }

    #[test]
    fn a_generated_key_is_written_once_and_privately() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("signing.pk8");

        generate_key(&path).unwrap();

        let pkcs8 = std::fs::read(&path).unwrap();
        public_key_from_pkcs8(&pkcs8).expect("what was written is a usable key");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "a private key must not be readable by others");
        }
        generate_key(&path).expect_err("a key must never be overwritten");
        assert_eq!(std::fs::read(&path).unwrap(), pkcs8);
    }

    #[test]
    fn versions_compare_as_semver() {
        for (version, other, expected) in [
            ("0.1.5", "0.1.5", "same"),
            ("0.1.5", "0.1.4", "newer"),
            ("0.1.9", "0.1.10", "older"),
            ("0.1.5-rc.1", "0.1.5", "older"),
            ("0.1.5", "0.1.5-rc.1", "newer"),
            ("0.1.5-rc.2", "0.1.5-rc.10", "older"),
        ] {
            assert_eq!(
                compare_versions(version, other).unwrap(),
                expected,
                "{version} against {other}"
            );
        }
        assert!(compare_versions("latest", "0.1.5").is_err());
        assert!(compare_versions("0.1.5", "").is_err());
    }

    #[test]
    fn a_version_with_a_leading_zero_cannot_be_ordered_and_is_refused() {
        for version in ["0.1.05", "0.01.5", "1.2.3-rc.01"] {
            assert!(
                compare_versions(version, version).is_err(),
                "{version} must be refused"
            );
        }
    }
}
