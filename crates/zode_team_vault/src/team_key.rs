use std::collections::BTreeMap;
use std::sync::Arc;

use remote_relay_protocol::{FIELD_SEPARATOR, KEY_LEN, is_low_order_public_key};
use ring::rand::{SecureRandom as _, SystemRandom};
use sha2::{Digest as _, Sha256};
use snow::Builder;
use zeroize::Zeroizing;
use zode_sync::{DEK_LEN, Dek, KID_LEN};

use crate::TeamCryptoError;
use crate::sharing_key::{Fingerprint, SharingKeypair};

pub const TEAM_KEY_LEN: usize = DEK_LEN;

/// A one-way Noise `X` message carrying a team key: the wrapper's ephemeral
/// key (32), its static key sealed (32 + 16), and the team key sealed
/// (32 + 16). Anything else is not an envelope.
pub const WRAPPED_KEY_LEN: usize = 128;

const NOISE_PATTERN: &str = "Noise_X_25519_AESGCM_SHA256";
const PROLOGUE_LABEL: &str = "zode-team-key-v1";
const NOISE_MAX_MESSAGE: usize = 65535;

/// The 32-byte key a team's vault is sealed under, one per key version.
///
/// Held in memory only. `Debug` is written by hand and the bytes are wiped on
/// drop, for the same reasons as `zode_sync::Dek`.
pub struct TeamKey {
    bytes: Zeroizing<[u8; TEAM_KEY_LEN]>,
}

impl TeamKey {
    pub fn generate() -> Result<Self, TeamCryptoError> {
        let mut bytes = Zeroizing::new([0u8; TEAM_KEY_LEN]);
        SystemRandom::new()
            .fill(bytes.as_mut())
            .map_err(|_| TeamCryptoError::Malformed("no randomness available".into()))?;
        Ok(Self { bytes })
    }

    pub fn from_bytes(bytes: [u8; TEAM_KEY_LEN]) -> Self {
        Self {
            bytes: Zeroizing::new(bytes),
        }
    }

    /// The fingerprint `zode_sync` stamps on every entry sealed under this
    /// key; the same construction as `Dek::kid`.
    pub fn kid(&self) -> [u8; KID_LEN] {
        let digest = Sha256::digest(self.bytes.as_ref());
        let mut kid = [0u8; KID_LEN];
        kid.copy_from_slice(&digest[..KID_LEN]);
        kid
    }

    /// The AEAD key, as `zode_sync` wants it: a short-lived copy that is
    /// zeroed when it drops, rather than a public accessor on `Dek`.
    pub(crate) fn as_dek(&self) -> Dek {
        Dek::copy_from(&self.bytes)
    }

    #[cfg(test)]
    pub(crate) fn expose(&self) -> &[u8; TEAM_KEY_LEN] {
        &self.bytes
    }
}

impl std::fmt::Debug for TeamKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TeamKey(<redacted>)")
    }
}

/// Every team key version this machine has opened. Memory only: nothing here
/// is ever written to disk. Cloning shares the keys rather than copying their
/// bytes, and the last clone dropped wipes them.
#[derive(Clone, Default)]
pub struct TeamKeyring {
    keys: BTreeMap<u32, Arc<TeamKey>>,
}

impl TeamKeyring {
    pub fn insert(&mut self, key_version: u32, key: TeamKey) {
        self.keys.insert(key_version, Arc::new(key));
    }

    pub fn get(&self, key_version: u32) -> Option<&TeamKey> {
        self.keys.get(&key_version).map(|key| key.as_ref())
    }

    /// The highest version held, which is the one new entries are sealed under.
    pub fn current(&self) -> Option<(u32, &TeamKey)> {
        self.keys
            .iter()
            .next_back()
            .map(|(version, key)| (*version, key.as_ref()))
    }

    pub fn versions(&self) -> Vec<u32> {
        self.keys.keys().copied().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

impl std::fmt::Debug for TeamKeyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TeamKeyring(versions: {:?})", self.versions())
    }
}

/// Who an envelope is for. Bound into the Noise prologue, so an envelope
/// replayed into another team, to another member, or as another key version
/// fails to open rather than handing over the wrong key.
#[derive(Debug, Clone, Copy)]
pub struct EnvelopeIds<'a> {
    pub team_id: &'a str,
    pub recipient_user_id: &'a str,
    pub key_version: u32,
}

/// `"zode-team-key-v1" ‖ 0x1F ‖ teamId ‖ 0x1F ‖ recipientUserId ‖ 0x1F ‖ keyVersion`,
/// the version in decimal ASCII.
pub fn prologue(ids: &EnvelopeIds<'_>) -> Result<Vec<u8>, TeamCryptoError> {
    if ids.key_version == 0 {
        return Err(TeamCryptoError::Malformed(
            "team key versions start at 1".into(),
        ));
    }
    let mut bytes = PROLOGUE_LABEL.as_bytes().to_vec();
    for (field, value) in [
        ("team id", ids.team_id),
        ("recipient user id", ids.recipient_user_id),
    ] {
        if value.is_empty() || value.bytes().any(|byte| byte == FIELD_SEPARATOR) {
            return Err(TeamCryptoError::Malformed(format!(
                "{field} must be non-empty and free of separators"
            )));
        }
        bytes.push(FIELD_SEPARATOR);
        bytes.extend_from_slice(value.as_bytes());
    }
    bytes.push(FIELD_SEPARATOR);
    bytes.extend_from_slice(ids.key_version.to_string().as_bytes());
    Ok(bytes)
}

/// Seals a team key to one member.
pub fn wrap(
    wrapper: &SharingKeypair,
    recipient_public_key: &[u8; KEY_LEN],
    ids: &EnvelopeIds<'_>,
    team_key: &TeamKey,
) -> Result<Vec<u8>, TeamCryptoError> {
    wrap_inner(wrapper, recipient_public_key, ids, team_key, None)
}

/// [`wrap`] with a pinned ephemeral key, so the published vectors are
/// reproducible. Never for real envelopes: a repeated ephemeral key makes
/// two envelopes to one member linkable.
#[cfg(any(test, feature = "test-support"))]
pub fn wrap_with_fixed_ephemeral_for_test(
    wrapper: &SharingKeypair,
    recipient_public_key: &[u8; KEY_LEN],
    ids: &EnvelopeIds<'_>,
    team_key: &TeamKey,
    ephemeral_private_key: &[u8; KEY_LEN],
) -> Result<Vec<u8>, TeamCryptoError> {
    wrap_inner(
        wrapper,
        recipient_public_key,
        ids,
        team_key,
        Some(ephemeral_private_key),
    )
}

fn wrap_inner(
    wrapper: &SharingKeypair,
    recipient_public_key: &[u8; KEY_LEN],
    ids: &EnvelopeIds<'_>,
    team_key: &TeamKey,
    ephemeral_private_key: Option<&[u8; KEY_LEN]>,
) -> Result<Vec<u8>, TeamCryptoError> {
    // snow does not look at the result of its Diffie–Hellman. Against a
    // low-order recipient key both DH results are predictable, and so is the
    // team key in the envelope. The server supplies this key; refuse it here.
    if is_low_order_public_key(recipient_public_key) {
        return Err(TeamCryptoError::LowOrderKey);
    }
    let prologue = prologue(ids)?;
    let mut builder = Builder::new(noise_params()?)
        .prologue(&prologue)
        .map_err(noise_error)?
        .local_private_key(wrapper.private_key())
        .map_err(noise_error)?
        .remote_public_key(recipient_public_key)
        .map_err(noise_error)?;
    if let Some(ephemeral_private_key) = ephemeral_private_key {
        builder = builder.fixed_ephemeral_key_for_testing_only(ephemeral_private_key);
    }
    let mut state = builder.build_initiator().map_err(noise_error)?;
    let mut message = vec![0u8; NOISE_MAX_MESSAGE];
    let length = state
        .write_message(team_key.bytes.as_ref(), &mut message)
        .map_err(noise_error)?;
    message.truncate(length);
    if message.len() != WRAPPED_KEY_LEN {
        return Err(TeamCryptoError::Malformed(format!(
            "an envelope is {WRAPPED_KEY_LEN} bytes, this one came out {}",
            message.len()
        )));
    }
    Ok(message)
}

/// Opens an envelope sealed to this member.
///
/// `expected_wrapper` is the pinned fingerprint of the member the server says
/// sealed it. An envelope that authenticates but was sealed by any other key
/// is refused and its key is not handed out: otherwise a server could seal a
/// key of its own choosing to every member, and read everything they shared.
pub fn unwrap(
    me: &SharingKeypair,
    wrapped: &[u8],
    ids: &EnvelopeIds<'_>,
    expected_wrapper: &Fingerprint,
) -> Result<TeamKey, TeamCryptoError> {
    if wrapped.len() != WRAPPED_KEY_LEN {
        return Err(TeamCryptoError::Malformed(format!(
            "an envelope is {WRAPPED_KEY_LEN} bytes, this one is {}",
            wrapped.len()
        )));
    }
    if let Some((ephemeral, _)) = wrapped.split_first_chunk::<KEY_LEN>()
        && is_low_order_public_key(ephemeral)
    {
        return Err(TeamCryptoError::LowOrderKey);
    }

    let prologue = prologue(ids)?;
    let mut state = Builder::new(noise_params()?)
        .prologue(&prologue)
        .map_err(noise_error)?
        .local_private_key(me.private_key())
        .map_err(noise_error)?
        .build_responder()
        .map_err(noise_error)?;

    let mut payload = Zeroizing::new(vec![0u8; NOISE_MAX_MESSAGE]);
    let length = state
        .read_message(wrapped, &mut payload)
        .map_err(|_| TeamCryptoError::WrongKey)?;
    if !state.is_handshake_finished() {
        return Err(TeamCryptoError::Malformed(
            "the envelope is incomplete".into(),
        ));
    }

    let wrapper_key: [u8; KEY_LEN] = state
        .get_remote_static()
        .and_then(|key| key.try_into().ok())
        .ok_or_else(|| TeamCryptoError::Malformed("the envelope names no sealer".into()))?;
    if is_low_order_public_key(&wrapper_key) {
        return Err(TeamCryptoError::LowOrderKey);
    }
    let found = Fingerprint::of(&wrapper_key);
    if &found != expected_wrapper {
        return Err(TeamCryptoError::WrapperMismatch {
            expected: *expected_wrapper,
            found,
        });
    }

    // Copied straight from the (zeroized) payload into the key's own wiped
    // storage: no by-value array on the way.
    let opened = payload
        .get(..length)
        .filter(|slice| slice.len() == TEAM_KEY_LEN)
        .ok_or_else(|| {
            TeamCryptoError::Malformed(format!(
                "the sealed team key is {length} bytes, not {TEAM_KEY_LEN}"
            ))
        })?;
    let mut bytes = Zeroizing::new([0u8; TEAM_KEY_LEN]);
    bytes.copy_from_slice(opened);
    Ok(TeamKey { bytes })
}

fn noise_params() -> Result<snow::params::NoiseParams, TeamCryptoError> {
    NOISE_PATTERN.parse().map_err(noise_error)
}

fn noise_error(error: snow::Error) -> TeamCryptoError {
    TeamCryptoError::Malformed(format!("noise: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEAM: &str = "68b1f0c2a4d3e5f607182900";
    const OTHER_TEAM: &str = "68b1f0c2a4d3e5f607182911";
    const MEMBER: &str = "68b1f0c2a4d3e5f60718293b";
    const OTHER_MEMBER: &str = "68b1f0c2a4d3e5f60718293c";

    fn keypair(byte: u8) -> SharingKeypair {
        SharingKeypair::from_private_key(Zeroizing::new([byte; KEY_LEN])).unwrap()
    }

    fn ids(
        team_id: &'static str,
        recipient: &'static str,
        key_version: u32,
    ) -> EnvelopeIds<'static> {
        EnvelopeIds {
            team_id,
            recipient_user_id: recipient,
            key_version,
        }
    }

    fn sealed_to_member() -> (SharingKeypair, SharingKeypair, Vec<u8>) {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let wrapped = wrap(
            &admin,
            member.public_key(),
            &ids(TEAM, MEMBER, 2),
            &TeamKey::from_bytes([0x77; TEAM_KEY_LEN]),
        )
        .unwrap();
        (admin, member, wrapped)
    }

    #[test]
    fn a_wrapped_key_opens_for_its_member_and_names_its_sealer() {
        let (admin, member, wrapped) = sealed_to_member();
        assert_eq!(wrapped.len(), WRAPPED_KEY_LEN);
        let opened = unwrap(
            &member,
            &wrapped,
            &ids(TEAM, MEMBER, 2),
            &admin.fingerprint(),
        )
        .unwrap();
        assert_eq!(opened.expose(), &[0x77; TEAM_KEY_LEN]);
    }

    #[test]
    fn the_key_is_not_in_the_envelope() {
        let (_, _, wrapped) = sealed_to_member();
        assert!(
            !wrapped
                .windows(TEAM_KEY_LEN)
                .any(|window| window == [0x77; TEAM_KEY_LEN])
        );
    }

    #[test]
    fn another_member_cannot_open_it() {
        let (admin, _, wrapped) = sealed_to_member();
        let eavesdropper = keypair(0x43);
        assert_eq!(
            unwrap(
                &eavesdropper,
                &wrapped,
                &ids(TEAM, MEMBER, 2),
                &admin.fingerprint()
            )
            .err(),
            Some(TeamCryptoError::WrongKey)
        );
    }

    #[test]
    fn an_envelope_replayed_into_another_team_member_or_version_fails() {
        let (admin, member, wrapped) = sealed_to_member();
        for (label, replayed) in [
            ("another team", ids(OTHER_TEAM, MEMBER, 2)),
            ("another member", ids(TEAM, OTHER_MEMBER, 2)),
            ("an older version", ids(TEAM, MEMBER, 1)),
            ("a newer version", ids(TEAM, MEMBER, 3)),
        ] {
            assert_eq!(
                unwrap(&member, &wrapped, &replayed, &admin.fingerprint()).err(),
                Some(TeamCryptoError::WrongKey),
                "an envelope replayed as {label} opened",
            );
        }
    }

    #[test]
    fn a_tampered_byte_anywhere_fails() {
        let (admin, member, wrapped) = sealed_to_member();
        for index in 0..WRAPPED_KEY_LEN {
            let mut tampered = wrapped.clone();
            tampered[index] ^= 0x01;
            assert!(
                unwrap(
                    &member,
                    &tampered,
                    &ids(TEAM, MEMBER, 2),
                    &admin.fingerprint()
                )
                .is_err(),
                "flipping byte {index} went unnoticed",
            );
        }
    }

    #[test]
    fn a_truncated_or_extended_envelope_is_refused() {
        let (admin, member, wrapped) = sealed_to_member();
        let mut extended = wrapped.clone();
        extended.push(0);
        for bad in [&wrapped[..WRAPPED_KEY_LEN - 1], &extended[..], &[][..]] {
            assert!(matches!(
                unwrap(&member, bad, &ids(TEAM, MEMBER, 2), &admin.fingerprint()),
                Err(TeamCryptoError::Malformed(_))
            ));
        }
    }

    /// The server substitutes its own key as the sealer: it can make a valid
    /// envelope to the member, so only the pin stands in its way.
    #[test]
    fn an_envelope_sealed_by_anyone_but_the_pinned_wrapper_is_refused() {
        let (admin, member, _) = sealed_to_member();
        let impostor = keypair(0x66);
        let forged = wrap(
            &impostor,
            member.public_key(),
            &ids(TEAM, MEMBER, 2),
            &TeamKey::from_bytes([0x99; TEAM_KEY_LEN]),
        )
        .unwrap();
        assert_eq!(
            unwrap(
                &member,
                &forged,
                &ids(TEAM, MEMBER, 2),
                &admin.fingerprint()
            )
            .err(),
            Some(TeamCryptoError::WrapperMismatch {
                expected: admin.fingerprint(),
                found: impostor.fingerprint(),
            })
        );
    }

    #[test]
    fn a_low_order_recipient_key_is_never_sealed_to() {
        let admin = keypair(0x41);
        assert_eq!(
            wrap(
                &admin,
                &[0u8; KEY_LEN],
                &ids(TEAM, MEMBER, 1),
                &TeamKey::from_bytes([0x77; TEAM_KEY_LEN])
            )
            .err(),
            Some(TeamCryptoError::LowOrderKey)
        );
    }

    #[test]
    fn a_low_order_ephemeral_key_is_refused_before_any_dh() {
        let (admin, member, mut wrapped) = sealed_to_member();
        wrapped[..KEY_LEN].copy_from_slice(&[0u8; KEY_LEN]);
        assert_eq!(
            unwrap(
                &member,
                &wrapped,
                &ids(TEAM, MEMBER, 2),
                &admin.fingerprint()
            )
            .err(),
            Some(TeamCryptoError::LowOrderKey)
        );
    }

    #[test]
    fn version_zero_and_separators_are_not_identities() {
        assert!(prologue(&ids(TEAM, MEMBER, 0)).is_err());
        assert!(
            prologue(&EnvelopeIds {
                team_id: "",
                recipient_user_id: MEMBER,
                key_version: 1
            })
            .is_err()
        );
        assert!(
            prologue(&EnvelopeIds {
                team_id: "a\u{1f}b",
                recipient_user_id: MEMBER,
                key_version: 1
            })
            .is_err()
        );
    }

    #[test]
    fn two_envelopes_of_one_key_differ() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let key = TeamKey::from_bytes([0x77; TEAM_KEY_LEN]);
        let first = wrap(&admin, member.public_key(), &ids(TEAM, MEMBER, 1), &key).unwrap();
        let second = wrap(&admin, member.public_key(), &ids(TEAM, MEMBER, 1), &key).unwrap();
        assert_ne!(first, second, "a fresh ephemeral key per envelope");
    }

    #[test]
    fn debug_never_reveals_the_team_key() {
        assert_eq!(
            format!("{:?}", TeamKey::from_bytes([0xAB; TEAM_KEY_LEN])),
            "TeamKey(<redacted>)"
        );
    }
}
