//! The signature that makes a driver manifest worth believing.
//!
//! A driver is an executable that Zode downloads and runs as the user, so the
//! checksum that gates it cannot come from the channel that serves the binary:
//! whoever can replace the archive can replace the checksum beside it. The
//! manifest is therefore signed offline with an Ed25519 key, and Zode carries
//! the matching public key. Nothing in a manifest is trusted until its
//! signature has verified against [`DRIVER_MANIFEST_PUBLIC_KEY`].
//!
//! The signed file is one asset: a header line, then the manifest exactly as it
//! was signed.
//!
//! ```text
//! zode-driver-manifest ed25519 <base64 signature>\n
//! <manifest json, byte for byte>
//! ```
//!
//! The signature covers the bytes after the first newline and nothing else, so
//! there is no re-serialising on either side to disagree about. Both halves of
//! the format live here -- `cargo xtask driver-manifest sign` writes it and the
//! app reads it with the same code, so the two cannot drift.

use anyhow::{Context as _, Result};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};

/// The public half of the key that signs every published driver manifest.
///
/// The private half is held offline by whoever publishes drivers and reaches CI
/// only as the `ZODE_DRIVER_SIGNING_KEY` secret. Replacing this value is how the
/// key is rotated, and it takes an app release to do so: a build only trusts the
/// key it was built with.
pub const DRIVER_MANIFEST_PUBLIC_KEY: [u8; 32] = [
    0x62, 0x4d, 0xc2, 0x62, 0x4c, 0x5d, 0x82, 0x1e, 0xc3, 0x7a, 0x13, 0x75, 0x95, 0x83, 0x8d, 0x39,
    0x0a, 0x62, 0x21, 0x25, 0xe3, 0x5d, 0x04, 0xef, 0xd6, 0xe3, 0xae, 0x0e, 0xdd, 0x46, 0x54, 0xfa,
];

/// What the signed manifest is published as.
///
/// An archive-shaped name on purpose, whatever the bytes are: the web backend
/// serves an uploaded file only under the manifest's own name or under
/// `zode-db-<id>-<target>.tar.gz`, and the manifest's own name is overwritten
/// with the backend's re-serialisation of what it accepted -- which would drop
/// both the signature and any field the backend does not know. Nothing reads
/// this asset as an archive, and the manifest rows never name it.
pub const SIGNED_MANIFEST_ASSET: &str = "zode-db-manifest-signed.tar.gz";

const HEADER_TAG: &str = "zode-driver-manifest";
const ALGORITHM: &str = "ed25519";

/// The raw bytes of the public key belonging to a PKCS#8 private key.
///
/// Accepts the key without an embedded public half too (what `openssl genpkey
/// -algorithm ed25519` writes), so a key made either way can sign.
pub fn public_key_from_pkcs8(pkcs8: &[u8]) -> Result<[u8; 32]> {
    let pair = key_pair(pkcs8)?;
    pair.public_key()
        .as_ref()
        .try_into()
        .context("an Ed25519 public key is not 32 bytes")
}

/// Signs `manifest` and returns the file that is published.
///
/// Verified against the key's own public half before it is returned, so a
/// signature that could never verify is a failure here and not in a user's
/// download.
pub fn sign_bundle(pkcs8: &[u8], manifest: &[u8]) -> Result<Vec<u8>> {
    let pair = key_pair(pkcs8)?;
    let signature = pair.sign(manifest);

    let mut bundle = format!(
        "{HEADER_TAG} {ALGORITHM} {}\n",
        STANDARD.encode(signature.as_ref())
    )
    .into_bytes();
    bundle.extend_from_slice(manifest);

    verify_bundle(&bundle, pair.public_key().as_ref())
        .context("the signature just made does not verify")?;
    Ok(bundle)
}

/// The manifest inside `bundle`, if and only if `public_key` signed it.
///
/// Fails closed on everything else: a file with no header (an unsigned
/// manifest), a header naming another algorithm, a signature that is not
/// base64, and a signature that does not match the bytes.
pub fn verify_bundle<'bundle>(bundle: &'bundle [u8], public_key: &[u8]) -> Result<&'bundle [u8]> {
    let newline = bundle.iter().position(|byte| *byte == b'\n');
    let (header, manifest) = match newline {
        Some(index) => {
            let (header, rest) = bundle.split_at(index);
            (header, rest.get(1..).unwrap_or_default())
        }
        None => (bundle, <&[u8]>::default()),
    };
    let header = std::str::from_utf8(header).ok();
    let mut parts = header.into_iter().flat_map(|header| header.split(' '));
    let (Some(HEADER_TAG), Some(ALGORITHM), Some(encoded), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        anyhow::bail!(
            "the driver manifest is not signed: it does not start with a \
             `{HEADER_TAG} {ALGORITHM} <signature>` line"
        );
    };

    let signature = STANDARD
        .decode(encoded)
        .context("the signature on the driver manifest is not valid base64")?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(manifest, &signature)
        .map_err(|_| {
            anyhow::anyhow!(
                "the signature on the driver manifest does not match Zode's driver signing key"
            )
        })?;
    Ok(manifest)
}

fn key_pair(pkcs8: &[u8]) -> Result<Ed25519KeyPair> {
    Ed25519KeyPair::from_pkcs8_maybe_unchecked(pkcs8)
        .map_err(|error| anyhow::anyhow!("the signing key is not a PKCS#8 Ed25519 key: {error}"))
}

/// A key pair that exists only so tests can sign something the app will accept.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    /// The fixed seed. Public on purpose: nothing it signs is trusted by a
    /// build that does not swap its trust anchor for [`public_key`], which no
    /// shipped code can do.
    const SEED: [u8; 32] = [0x5a; 32];

    /// PKCS#8 v1 for an Ed25519 key: this prefix, then the 32-byte seed.
    const PKCS8_PREFIX: [u8; 16] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];

    pub fn pkcs8() -> Vec<u8> {
        let mut key = PKCS8_PREFIX.to_vec();
        key.extend_from_slice(&SEED);
        key
    }

    pub fn public_key() -> [u8; 32] {
        super::public_key_from_pkcs8(&pkcs8()).expect("the test key is a valid Ed25519 key")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &[u8] = b"{\"version\":\"0.1.5\",\n \"drivers\":[]}\n";

    fn signed() -> Vec<u8> {
        sign_bundle(&testing::pkcs8(), MANIFEST).unwrap()
    }

    #[test]
    fn a_signed_manifest_verifies_and_comes_back_byte_for_byte() {
        let bundle = signed();
        let manifest = verify_bundle(&bundle, &testing::public_key()).unwrap();
        assert_eq!(manifest, MANIFEST);
    }

    /// The whole point: a manifest someone else wrote, with a checksum that
    /// matches their own archive, must not be believed.
    #[test]
    fn a_manifest_signed_by_another_key_is_refused() {
        let other =
            ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                .unwrap();
        let bundle = sign_bundle(other.as_ref(), MANIFEST).unwrap();

        let error = verify_bundle(&bundle, &testing::public_key()).unwrap_err();

        assert!(
            error.to_string().contains("does not match"),
            "the refusal must name the signature: {error}"
        );
    }

    #[test]
    fn a_manifest_edited_after_signing_is_refused() {
        let mut bundle = signed();
        let last = bundle.len() - 2;
        bundle[last] ^= 1;
        assert!(verify_bundle(&bundle, &testing::public_key()).is_err());
    }

    #[test]
    fn a_signature_edited_after_signing_is_refused() {
        let bundle = signed();
        let newline = bundle.iter().position(|byte| *byte == b'\n').unwrap();
        let header = std::str::from_utf8(&bundle[..newline]).unwrap();
        let mut signature = STANDARD.decode(header.rsplit(' ').next().unwrap()).unwrap();
        signature[0] ^= 1;
        let mut tampered =
            format!("{HEADER_TAG} {ALGORITHM} {}\n", STANDARD.encode(signature)).into_bytes();
        tampered.extend_from_slice(&bundle[newline + 1..]);

        assert!(verify_bundle(&tampered, &testing::public_key()).is_err());
    }

    /// What the currently published manifests look like: plain JSON, no header.
    #[test]
    fn an_unsigned_manifest_is_refused_and_says_it_is_unsigned() {
        for unsigned in [
            MANIFEST.to_vec(),
            Vec::new(),
            b"\n".to_vec(),
            b"zode-driver-manifest\n{}".to_vec(),
            b"zode-driver-manifest rsa AAAA\n{}".to_vec(),
            b"zode-driver-manifest ed25519\n{}".to_vec(),
            b"zode-driver-manifest ed25519 AAAA extra\n{}".to_vec(),
            vec![0xff, 0xfe, b'\n'],
        ] {
            let error = verify_bundle(&unsigned, &testing::public_key()).unwrap_err();
            assert!(
                error.to_string().contains("not signed"),
                "{unsigned:?} must be refused as unsigned: {error}"
            );
        }
    }

    #[test]
    fn a_signature_that_is_not_base64_is_refused() {
        let bundle = b"zode-driver-manifest ed25519 !!!not-base64!!!\n{}";
        let error = verify_bundle(bundle, &testing::public_key()).unwrap_err();
        assert!(error.to_string().contains("base64"), "{error}");
    }

    #[test]
    fn a_signature_of_the_wrong_length_is_refused() {
        let bundle = b"zode-driver-manifest ed25519 AAAA\n{}";
        assert!(verify_bundle(bundle, &testing::public_key()).is_err());
    }

    #[test]
    fn a_key_that_is_not_a_signing_key_cannot_sign() {
        let error = sign_bundle(b"definitely not pkcs8", MANIFEST).unwrap_err();
        assert!(error.to_string().contains("PKCS#8"), "{error}");
    }

    /// A key made by `openssl genpkey` carries no public half, and is what
    /// someone rotating the key by hand is likely to have.
    #[test]
    fn a_key_without_an_embedded_public_half_can_sign() {
        assert_eq!(testing::pkcs8().len(), 48, "the test key is the short form");
        sign_bundle(&testing::pkcs8(), MANIFEST).unwrap();
    }

    /// Shipping the test key would let anyone sign a manifest the app trusts.
    #[test]
    fn the_key_the_app_trusts_is_not_the_one_the_tests_sign_with() {
        assert_ne!(DRIVER_MANIFEST_PUBLIC_KEY, testing::public_key());
        assert_ne!(DRIVER_MANIFEST_PUBLIC_KEY, [0u8; 32], "no key has been set");
    }

    /// The name has to be one the backend will accept as an upload and serve.
    #[test]
    fn the_signed_asset_has_the_shape_of_an_archive_the_backend_serves() {
        let stem = SIGNED_MANIFEST_ASSET
            .strip_prefix("zode-db-")
            .and_then(|rest| rest.strip_suffix(".tar.gz"))
            .expect("an archive name");
        let (id, target) = stem.split_once('-').expect("an id and a target");
        for word in [id, target] {
            assert!(
                !word.is_empty()
                    && word
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()),
                "{word:?}"
            );
        }
    }
}
