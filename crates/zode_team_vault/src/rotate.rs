//! Moving a team to a new key version, then re-sealing what the old ones
//! sealed.
//!
//! Order matters and is the server's contract (phase 06): the new key is
//! sealed to every member before the version moves, the version moves by
//! compare-and-swap, and old envelopes stay so members keep reading older
//! entries while they are re-sealed one conditional write at a time. Two
//! admins rotating at once: one wins, the other sees a 409 and carries on from
//! the winner's state instead of rotating again.

use zode_env_sync::EntryId;
use zode_sync::sync::SyncContext;
use zode_sync::to_blob;

use crate::client::{
    self, MemberKey, OutgoingEnvelope, RotateWrite, VaultPrecondition, VaultWrite,
};
use crate::pins::{PinCheck, TeamTrust};
use crate::seal::{open_entry, seal_entry};
use crate::sharing_key::SharingKeypair;
use crate::team_key::{EnvelopeIds, TeamKey, TeamKeyring, wrap};
use crate::{TeamCryptoError, TeamVaultError};

/// How many times one entry's re-seal is retried after losing a write race.
const RESEAL_ATTEMPTS: usize = 3;

/// The new key sealed to every member who holds a sharing key.
///
/// Rotation never pins on its own, and seals only to keys the user verified
/// on this machine: a key nobody compared — new, or only remembered on first
/// sight — is not the moment to start trusting it. Such a member, or one
/// whose key changed, stops the rotation with the reason, so the admin
/// decides first.
pub fn rotation_envelopes(
    trust: &TeamTrust,
    team_id: &str,
    my_user_id: &str,
    me: &SharingKeypair,
    members: &[MemberKey],
    key_version: u32,
    team_key: &TeamKey,
) -> Result<Vec<OutgoingEnvelope>, TeamVaultError> {
    let mut envelopes = Vec::with_capacity(members.len());
    let mut includes_me = false;
    for member in members {
        let Some(key) = member.key.as_ref() else {
            continue;
        };
        if member.user_id == my_user_id {
            if key.fingerprint != me.fingerprint() {
                return Err(TeamVaultError::KeyChanged {
                    user_id: member.user_id.clone(),
                });
            }
            includes_me = true;
        } else {
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
        }
        envelopes.push(OutgoingEnvelope {
            user_id: member.user_id.clone(),
            recipient_fingerprint: key.fingerprint,
            wrapped_key: wrap(
                me,
                &key.public_key,
                &EnvelopeIds {
                    team_id,
                    recipient_user_id: &member.user_id,
                    key_version,
                },
                team_key,
            )?,
        });
    }
    if !includes_me {
        return Err(TeamCryptoError::Malformed(
            "the member listing does not include this account".into(),
        )
        .into());
    }
    Ok(envelopes)
}

pub enum RotationStep {
    Rotated {
        key_version: u32,
        team_key: TeamKey,
    },
    /// Another rotation moved the version first. Refresh and carry on from it.
    LostRace,
}

/// Generates the next key, seals it to every member and moves the version.
pub async fn rotate_key_version(
    context: &SyncContext,
    trust: &TeamTrust,
    team_id: &str,
    me: &SharingKeypair,
    keyring: &TeamKeyring,
    members: &[MemberKey],
) -> Result<RotationStep, TeamVaultError> {
    let (from_version, _) = keyring.current().ok_or(TeamVaultError::NoTeamKey)?;
    let key_version = from_version + 1;
    let team_key = TeamKey::generate()?;
    let envelopes = rotation_envelopes(
        trust,
        team_id,
        &context.credential.user_id,
        me,
        members,
        key_version,
        &team_key,
    )?;
    match client::rotate_key(context, team_id, from_version, &envelopes).await? {
        RotateWrite::Rotated => Ok(RotationStep::Rotated {
            key_version,
            team_key,
        }),
        RotateWrite::KeyVersionStale => Ok(RotationStep::LostRace),
        RotateWrite::RecipientKeyChanged { user_id } => Err(TeamVaultError::KeyChanged {
            user_id: user_id.unwrap_or_default(),
        }),
        RotateWrite::NotMember => {
            Err(zode_sync::client::ClientError::Rejected("not_found".into()).into())
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ResealReport {
    pub resealed: usize,
    /// Entries whose key version this machine never held. Still readable by
    /// members who do; re-sealed by whoever runs the next rotation with it.
    pub unreadable: Vec<EntryId>,
    /// Entries refused while opening (a rollback, a tampered blob).
    pub refused: Vec<(EntryId, TeamCryptoError)>,
    /// `seq`s written, for the caller to record.
    pub written: Vec<(EntryId, u64)>,
}

/// Re-seals every entry still under an older key version, under the current
/// one. Each write is conditional on the revision just read.
pub async fn reseal_older_entries(
    context: &SyncContext,
    trust: &TeamTrust,
    team_id: &str,
    keyring: &TeamKeyring,
) -> Result<ResealReport, TeamVaultError> {
    let (current_version, current_key) = keyring.current().ok_or(TeamVaultError::NoTeamKey)?;
    let listing = client::list_vault(context, team_id)
        .await?
        .ok_or_else(|| zode_sync::client::ClientError::Rejected("not_found".into()))?;

    let mut report = ResealReport::default();
    for listed in listing
        .iter()
        .filter(|listed| listed.key_version < current_version)
    {
        let entry = listed.entry_id;
        for _ in 0..RESEAL_ATTEMPTS {
            let Some(document) = client::fetch_entry(context, team_id, &entry).await? else {
                break;
            };
            if document.key_version >= current_version {
                break;
            }
            let Some(old_key) = keyring.get(document.key_version) else {
                report.unreadable.push(entry);
                break;
            };
            let envelope = zode_sync::from_blob(&document.blob).map_err(TeamCryptoError::from)?;
            let opened = match open_entry(
                old_key,
                team_id,
                &entry,
                document.key_version,
                trust.seen_seq(team_id, &entry.as_hex()),
                &envelope,
            ) {
                Ok(opened) => opened,
                Err(error) => {
                    report.refused.push((entry, error));
                    break;
                }
            };
            let seq = opened.seq + 1;
            let sealed = seal_entry(
                current_key,
                team_id,
                &entry,
                current_version,
                seq,
                &opened.payload,
            )?;
            let blob = to_blob(&sealed).map_err(TeamCryptoError::from)?;
            match client::store_entry(
                context,
                team_id,
                &entry,
                &blob,
                current_version,
                VaultPrecondition::Replace(&document.revision),
            )
            .await?
            {
                VaultWrite::Stored { .. } => {
                    report.resealed += 1;
                    report.written.push((entry, seq));
                    break;
                }
                // Somebody wrote in between: read again and decide again.
                VaultWrite::Conflict(_) => continue,
                VaultWrite::Gone => break,
                VaultWrite::KeyVersionStale => return Err(TeamVaultError::KeyVersionStale),
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::RegisteredKey;
    use crate::team_key::{TEAM_KEY_LEN, unwrap};
    use remote_relay_protocol::KEY_LEN;
    use zeroize::Zeroizing;

    const TEAM: &str = "68b1f0c2a4d3e5f607182900";
    const ADMIN: &str = "68b1f0c2a4d3e5f60718293a";
    const MEMBER: &str = "68b1f0c2a4d3e5f60718293b";

    fn keypair(byte: u8) -> SharingKeypair {
        SharingKeypair::from_private_key(Zeroizing::new([byte; KEY_LEN])).unwrap()
    }

    fn listed(user_id: &str, keypair: Option<&SharingKeypair>) -> MemberKey {
        MemberKey {
            user_id: user_id.into(),
            email: format!("{user_id}@example.com"),
            key: keypair.map(|keypair| RegisteredKey {
                public_key: *keypair.public_key(),
                fingerprint: keypair.fingerprint(),
            }),
            has_current_envelope: true,
        }
    }

    fn pinned(member: &SharingKeypair) -> TeamTrust {
        let mut trust = TeamTrust::default();
        trust.verify(TEAM, MEMBER, "m@example.com", member.fingerprint(), 1);
        trust
    }

    #[test]
    fn every_member_with_a_key_gets_the_new_key_and_nobody_else() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let key = TeamKey::from_bytes([0x88; TEAM_KEY_LEN]);
        let envelopes = rotation_envelopes(
            &pinned(&member),
            TEAM,
            ADMIN,
            &admin,
            &[
                listed(ADMIN, Some(&admin)),
                listed(MEMBER, Some(&member)),
                listed("keyless", None),
            ],
            4,
            &key,
        )
        .unwrap();
        let recipients: Vec<_> = envelopes.iter().map(|e| e.user_id.as_str()).collect();
        assert_eq!(
            recipients,
            vec![ADMIN, MEMBER],
            "a rotation dropped or invented a member"
        );

        let opened = unwrap(
            &member,
            &envelopes[1].wrapped_key,
            &EnvelopeIds {
                team_id: TEAM,
                recipient_user_id: MEMBER,
                key_version: 4,
            },
            &admin.fingerprint(),
        )
        .unwrap();
        assert_eq!(opened.kid(), key.kid());
    }

    #[test]
    fn a_changed_or_unpinned_member_stops_the_rotation() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let key = TeamKey::from_bytes([0x88; TEAM_KEY_LEN]);
        let members = [
            listed(ADMIN, Some(&admin)),
            listed(MEMBER, Some(&keypair(0x66))),
        ];
        assert_eq!(
            rotation_envelopes(&pinned(&member), TEAM, ADMIN, &admin, &members, 2, &key).err(),
            Some(TeamVaultError::KeyChanged {
                user_id: MEMBER.into()
            })
        );
        assert_eq!(
            rotation_envelopes(
                &TeamTrust::default(),
                TEAM,
                ADMIN,
                &admin,
                &members,
                2,
                &key
            )
            .err(),
            Some(TeamVaultError::NotPinned {
                user_id: MEMBER.into()
            })
        );
    }

    #[test]
    fn a_key_only_seen_on_first_sight_stops_the_rotation() {
        let admin = keypair(0x41);
        let member = keypair(0x42);
        let mut seen = TeamTrust::default();
        seen.remember_first_sight(TEAM, MEMBER, "m@example.com", member.fingerprint(), 1);
        assert_eq!(
            rotation_envelopes(
                &seen,
                TEAM,
                ADMIN,
                &admin,
                &[listed(ADMIN, Some(&admin)), listed(MEMBER, Some(&member))],
                2,
                &TeamKey::from_bytes([0x88; TEAM_KEY_LEN]),
            )
            .err(),
            Some(TeamVaultError::NotPinned {
                user_id: MEMBER.into()
            })
        );
    }

    #[test]
    fn a_listing_that_misstates_or_omits_this_account_stops_the_rotation() {
        let admin = keypair(0x41);
        let key = TeamKey::from_bytes([0x88; TEAM_KEY_LEN]);
        assert_eq!(
            rotation_envelopes(
                &TeamTrust::default(),
                TEAM,
                ADMIN,
                &admin,
                &[listed(ADMIN, Some(&keypair(0x66)))],
                2,
                &key
            )
            .err(),
            Some(TeamVaultError::KeyChanged {
                user_id: ADMIN.into()
            })
        );
        assert!(matches!(
            rotation_envelopes(&TeamTrust::default(), TEAM, ADMIN, &admin, &[], 2, &key),
            Err(TeamVaultError::Crypto(TeamCryptoError::Malformed(_)))
        ));
    }
}
