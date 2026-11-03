//! Giving a team key to a member, and taking one received.
//!
//! Both directions run through the pins. Sealing: only to a key the user
//! verified on this machine; a key never compared, or one that changed since,
//! gets nothing. Opening: an envelope is opened only against the sealer's
//! pinned key, so a server cannot seal a key of its own choosing to anyone.
//! The one exception is a machine opening a team for the very first time,
//! which has nothing pinned yet: it remembers the sealer on first sight,
//! unverified (the residual risk the security page states).

use std::collections::BTreeMap;

use crate::client::{MemberKey, OutgoingEnvelope, SealedEnvelope};
use crate::pins::{PinCheck, TeamTrust};
use crate::sharing_key::SharingKeypair;
use crate::team_key::{EnvelopeIds, TeamKeyring, unwrap, wrap};
use crate::{TeamCryptoError, TeamVaultError};

/// Seals the current team key to `member`, whose key must be verified here.
///
/// Nothing is pinned as a side effect: a key nobody compared, seen or not,
/// is refused with `NotPinned`, and trusting it is the user's explicit step.
pub fn prepare_grant(
    trust: &TeamTrust,
    team_id: &str,
    wrapper: &SharingKeypair,
    keyring: &TeamKeyring,
    member: &MemberKey,
) -> Result<(u32, OutgoingEnvelope), TeamVaultError> {
    let key = member
        .key
        .as_ref()
        .ok_or_else(|| TeamVaultError::NoSharingKey {
            user_id: member.user_id.clone(),
        })?;
    match trust.check(team_id, &member.user_id, &key.fingerprint) {
        PinCheck::Verified => {}
        PinCheck::New | PinCheck::Seen => {
            return Err(TeamVaultError::NotPinned {
                user_id: member.user_id.clone(),
            });
        }
        PinCheck::Changed { .. } => {
            return Err(TeamVaultError::KeyChanged {
                user_id: member.user_id.clone(),
            });
        }
    }
    let (key_version, team_key) = keyring.current().ok_or(TeamVaultError::NoTeamKey)?;
    let wrapped_key = wrap(
        wrapper,
        &key.public_key,
        &EnvelopeIds {
            team_id,
            recipient_user_id: &member.user_id,
            key_version,
        },
        team_key,
    )?;
    Ok((
        key_version,
        OutgoingEnvelope {
            user_id: member.user_id.clone(),
            recipient_fingerprint: key.fingerprint,
            wrapped_key,
        },
    ))
}

/// Opens every envelope the server holds for this member.
///
/// Each is checked against the pinned key of the member the server says
/// sealed it — or this machine's own key, for envelopes it sealed itself.
///
/// A sealer with no pin is taken on first sight only when this machine has
/// never opened the team ([`TeamTrust::has_opened`]), and then only as
/// *seen*, once its envelope verified against the listed key. On a team
/// opened before, an unknown sealer is `NotPinned`: the user compares and
/// trusts it first. Two envelopes for one key version are `Malformed` and
/// neither is used — the server stores one per member and version, and
/// keeping either would let the later one silently replace the key.
pub fn open_envelopes(
    trust: &mut TeamTrust,
    team_id: &str,
    my_user_id: &str,
    me: &SharingKeypair,
    envelopes: &[SealedEnvelope],
    members: &[MemberKey],
    now: u64,
) -> (TeamKeyring, Vec<(u32, TeamVaultError)>) {
    let mut keyring = TeamKeyring::default();
    let mut problems = Vec::new();
    let first_open = !trust.has_opened(team_id);

    let mut per_version: BTreeMap<u32, usize> = BTreeMap::new();
    for envelope in envelopes {
        *per_version.entry(envelope.key_version).or_default() += 1;
    }
    for (version, count) in &per_version {
        if *count > 1 {
            problems.push((
                *version,
                TeamCryptoError::Malformed(format!(
                    "the server sent {count} envelopes for team key version {version}"
                ))
                .into(),
            ));
        }
    }

    for envelope in envelopes {
        if per_version.get(&envelope.key_version).copied() != Some(1) {
            continue;
        }
        let sealer = envelope.wrapped_by_user_id.as_str();
        let (expected, first_sighting) = if sealer == my_user_id {
            (me.fingerprint(), None)
        } else if let Some(pinned) = trust.pinned(team_id, sealer) {
            (pinned.fingerprint, None)
        } else {
            let listed = members
                .iter()
                .find(|member| member.user_id == sealer)
                .and_then(|member| member.key.as_ref().map(|key| (member, key)));
            match listed {
                Some((member, key)) if first_open => (key.fingerprint, Some(member)),
                _ => {
                    problems.push((
                        envelope.key_version,
                        TeamVaultError::NotPinned {
                            user_id: sealer.to_string(),
                        },
                    ));
                    continue;
                }
            }
        };

        let opened = unwrap(
            me,
            &envelope.wrapped_key,
            &EnvelopeIds {
                team_id,
                recipient_user_id: my_user_id,
                key_version: envelope.key_version,
            },
            &expected,
        );
        match opened {
            Ok(team_key) => {
                if let Some(member) = first_sighting {
                    trust.remember_first_sight(team_id, sealer, &member.email, expected, now);
                }
                keyring.insert(envelope.key_version, team_key);
            }
            Err(error) => problems.push((envelope.key_version, error.into())),
        }
    }
    (keyring, problems)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TeamCryptoError;
    use crate::client::RegisteredKey;
    use crate::team_key::{TEAM_KEY_LEN, TeamKey};
    use remote_relay_protocol::KEY_LEN;
    use zeroize::Zeroizing;

    const TEAM: &str = "68b1f0c2a4d3e5f607182900";
    const ADMIN: &str = "68b1f0c2a4d3e5f60718293a";
    const MEMBER: &str = "68b1f0c2a4d3e5f60718293b";

    fn keypair(byte: u8) -> SharingKeypair {
        SharingKeypair::from_private_key(Zeroizing::new([byte; KEY_LEN])).unwrap()
    }

    fn listed(user_id: &str, keypair: &SharingKeypair) -> MemberKey {
        MemberKey {
            user_id: user_id.into(),
            email: format!("{user_id}@example.com"),
            key: Some(RegisteredKey {
                public_key: *keypair.public_key(),
                fingerprint: keypair.fingerprint(),
            }),
            has_current_envelope: false,
        }
    }

    fn keyring(version: u32, byte: u8) -> TeamKeyring {
        let mut keyring = TeamKeyring::default();
        keyring.insert(version, TeamKey::from_bytes([byte; TEAM_KEY_LEN]));
        keyring
    }

    fn sealed_by(admin: &SharingKeypair, to: &SharingKeypair, version: u32) -> SealedEnvelope {
        SealedEnvelope {
            key_version: version,
            wrapped_key: wrap(
                admin,
                to.public_key(),
                &EnvelopeIds {
                    team_id: TEAM,
                    recipient_user_id: MEMBER,
                    key_version: version,
                },
                &TeamKey::from_bytes([0x70 + version as u8; TEAM_KEY_LEN]),
            )
            .unwrap(),
            wrapped_by_user_id: ADMIN.into(),
        }
    }

    fn verified(member: &SharingKeypair) -> TeamTrust {
        let mut trust = TeamTrust::default();
        trust.verify(TEAM, MEMBER, "m@example.com", member.fingerprint(), 1);
        trust
    }

    #[test]
    fn a_grant_seals_the_current_key_to_a_verified_member() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let (version, envelope) = prepare_grant(
            &verified(&member),
            TEAM,
            &admin,
            &keyring(3, 0x77),
            &listed(MEMBER, &member),
        )
        .unwrap();
        assert_eq!(version, 3);
        assert_eq!(envelope.recipient_fingerprint, member.fingerprint());

        let opened = unwrap(
            &member,
            &envelope.wrapped_key,
            &EnvelopeIds {
                team_id: TEAM,
                recipient_user_id: MEMBER,
                key_version: 3,
            },
            &admin.fingerprint(),
        )
        .unwrap();
        assert_eq!(
            opened.kid(),
            TeamKey::from_bytes([0x77; TEAM_KEY_LEN]).kid()
        );
    }

    /// A grant is never the moment trust begins: a key nobody compared, or
    /// one only remembered on first sight, is refused and not pinned.
    #[test]
    fn a_grant_to_a_new_or_only_seen_key_is_refused_and_pins_nothing() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let trust = TeamTrust::default();
        assert_eq!(
            prepare_grant(
                &trust,
                TEAM,
                &admin,
                &keyring(1, 0x77),
                &listed(MEMBER, &member)
            )
            .err(),
            Some(TeamVaultError::NotPinned {
                user_id: MEMBER.into()
            })
        );
        assert_eq!(trust.pinned(TEAM, MEMBER), None);

        let mut seen = TeamTrust::default();
        seen.remember_first_sight(TEAM, MEMBER, "m@example.com", member.fingerprint(), 1);
        assert_eq!(
            prepare_grant(
                &seen,
                TEAM,
                &admin,
                &keyring(1, 0x77),
                &listed(MEMBER, &member)
            )
            .err(),
            Some(TeamVaultError::NotPinned {
                user_id: MEMBER.into()
            })
        );
    }

    #[test]
    fn a_changed_key_gets_nothing_until_accepted() {
        let admin = keypair(0x41);
        let trust = verified(&keypair(0x42));
        let substituted = keypair(0x66);
        assert_eq!(
            prepare_grant(
                &trust,
                TEAM,
                &admin,
                &keyring(1, 0x77),
                &listed(MEMBER, &substituted)
            )
            .err(),
            Some(TeamVaultError::KeyChanged {
                user_id: MEMBER.into()
            })
        );
    }

    #[test]
    fn a_member_without_a_key_or_a_grant_without_a_team_key_is_refused() {
        let admin = keypair(0x41);
        let mut keyless = listed(MEMBER, &keypair(0x42));
        keyless.key = None;
        assert_eq!(
            prepare_grant(
                &TeamTrust::default(),
                TEAM,
                &admin,
                &keyring(1, 1),
                &keyless
            )
            .err(),
            Some(TeamVaultError::NoSharingKey {
                user_id: MEMBER.into()
            })
        );
        assert_eq!(
            prepare_grant(
                &verified(&keypair(0x42)),
                TEAM,
                &admin,
                &TeamKeyring::default(),
                &listed(MEMBER, &keypair(0x42))
            )
            .err(),
            Some(TeamVaultError::NoTeamKey)
        );
    }

    #[test]
    fn a_first_open_remembers_the_sealer_as_seen_not_verified() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let mut trust = TeamTrust::default();
        let (keyring, problems) = open_envelopes(
            &mut trust,
            TEAM,
            MEMBER,
            &member,
            &[sealed_by(&admin, &member, 1), sealed_by(&admin, &member, 2)],
            &[listed(ADMIN, &admin)],
            5,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(keyring.versions(), vec![1, 2]);
        assert_eq!(
            trust.check(TEAM, ADMIN, &admin.fingerprint()),
            PinCheck::Seen
        );
    }

    /// Once a team was opened here, a sealer this machine never pinned is
    /// not taken on the server's word, even if its envelope authenticates.
    #[test]
    fn after_the_first_open_an_unpinned_sealer_is_refused() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let mut trust = TeamTrust::default();
        trust.observe_key_version(TEAM, 1).unwrap();

        let (keyring, problems) = open_envelopes(
            &mut trust,
            TEAM,
            MEMBER,
            &member,
            &[sealed_by(&admin, &member, 2)],
            &[listed(ADMIN, &admin)],
            5,
        );
        assert!(keyring.is_empty());
        assert_eq!(
            problems,
            vec![(
                2,
                TeamVaultError::NotPinned {
                    user_id: ADMIN.into()
                }
            )]
        );
        assert_eq!(trust.pinned(TEAM, ADMIN), None);

        // Verified, the same envelope opens.
        trust.verify(TEAM, ADMIN, "a@example.com", admin.fingerprint(), 6);
        let (keyring, problems) = open_envelopes(
            &mut trust,
            TEAM,
            MEMBER,
            &member,
            &[sealed_by(&admin, &member, 2)],
            &[listed(ADMIN, &admin)],
            7,
        );
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(keyring.versions(), vec![2]);
    }

    /// Two envelopes for one version: neither is used, even when both open.
    #[test]
    fn two_envelopes_for_one_version_are_refused() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let mut trust = TeamTrust::default();
        trust.verify(TEAM, ADMIN, "a@example.com", admin.fingerprint(), 1);

        let (keyring, problems) = open_envelopes(
            &mut trust,
            TEAM,
            MEMBER,
            &member,
            &[
                sealed_by(&admin, &member, 1),
                sealed_by(&admin, &member, 2),
                sealed_by(&admin, &member, 2),
            ],
            &[listed(ADMIN, &admin)],
            5,
        );
        assert_eq!(keyring.versions(), vec![1]);
        assert!(
            matches!(
                problems.as_slice(),
                [(2, TeamVaultError::Crypto(TeamCryptoError::Malformed(_)))]
            ),
            "{problems:?}"
        );
    }

    /// The server lists its own key as the admin's and seals with it.
    #[test]
    fn an_envelope_from_a_substituted_sealer_is_not_opened() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let server = keypair(0x66);
        let mut trust = TeamTrust::default();
        trust.verify(TEAM, ADMIN, "a@example.com", admin.fingerprint(), 1);

        let (keyring, problems) = open_envelopes(
            &mut trust,
            TEAM,
            MEMBER,
            &member,
            &[sealed_by(&server, &member, 1)],
            &[listed(ADMIN, &server)],
            5,
        );
        assert!(keyring.is_empty());
        assert_eq!(
            problems,
            vec![(
                1,
                TeamVaultError::Crypto(TeamCryptoError::WrapperMismatch {
                    expected: admin.fingerprint(),
                    found: server.fingerprint(),
                })
            )]
        );
    }

    #[test]
    fn a_sealer_nobody_lists_is_not_trusted_and_a_mismatch_is_not_pinned() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let mut trust = TeamTrust::default();
        let (keyring, problems) = open_envelopes(
            &mut trust,
            TEAM,
            MEMBER,
            &member,
            &[sealed_by(&admin, &member, 1)],
            &[],
            5,
        );
        assert!(keyring.is_empty());
        assert!(matches!(
            problems.as_slice(),
            [(1, TeamVaultError::NotPinned { .. })]
        ));

        // Listed under a key that is not the one that sealed: refused, and the
        // listed key is not pinned on the strength of it.
        let (keyring, _) = open_envelopes(
            &mut trust,
            TEAM,
            MEMBER,
            &member,
            &[sealed_by(&admin, &member, 1)],
            &[listed(ADMIN, &keypair(0x66))],
            5,
        );
        assert!(keyring.is_empty());
        assert_eq!(trust.pinned(TEAM, ADMIN), None);
    }
}
