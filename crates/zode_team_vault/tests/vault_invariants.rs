//! The team vault held to its invariants against a fake of the phase 06
//! server, with real accounts, real sessions and real crypto.
//!
//! Each test is one of the ways the server (or a race) can try to get a team
//! key or a secret where it should not go, and what the editor does about it.

#[path = "fake_team_store/fake_team_store.rs"]
mod fake_team_store;

use fake_team_store::{FakeTeamServer, MemoryKeychain, StoredEnvelope};
use gpui::{AppContext as _, Entity, TestAppContext};
use zeroize::Zeroizing;
use zode_account::{Account, AccountStatus, AccountUser};
use zode_env_sync::EntryId;
use zode_team_vault::{
    ManifestChange, PinCheck, PushBase, SharingKeypair, TeamCryptoError, TeamVaultError,
    TeamVaultSession, TeamVaultStatus,
};

const TEAM: &str = "68b1f0c2a4d3e5f607182900";
const ADMIN: &str = "68b1f0c2a4d3e5f60718290a";
const MEMBER: &str = "68b1f0c2a4d3e5f60718290b";
const SECOND_ADMIN: &str = "68b1f0c2a4d3e5f60718290c";

struct Machine {
    account: Entity<Account>,
    session: Entity<TeamVaultSession>,
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(db::AppDatabase::test_new()));
}

fn machine(
    cx: &mut TestAppContext,
    server: &FakeTeamServer,
    user_id: &str,
    recovery_byte: u8,
) -> Machine {
    let http_client = server.client_for(user_id);
    let keychain = MemoryKeychain::with_recovery_key(user_id, [recovery_byte; 32]);
    let user_id = user_id.to_string();
    let account = cx.update(|cx| {
        cx.new(|_| {
            let mut account = Account::for_test_with(
                AccountStatus::SignedIn(AccountUser {
                    id: user_id.clone().into(),
                    email: format!("{user_id}@example.com").into(),
                    name: None,
                    avatar_url: None,
                }),
                http_client,
                keychain,
            );
            account.set_tokens_for_test();
            account
        })
    });
    let session = cx.update(|cx| {
        let account = account.clone();
        cx.new(|cx| TeamVaultSession::new(account, cx))
    });
    Machine { account, session }
}

fn team_with_admin_and_member() -> FakeTeamServer {
    let server = FakeTeamServer::new(TEAM, "Acme");
    server.add_member(ADMIN, "ada@example.com", "owner");
    server.add_member(MEMBER, "bo@example.com", "member");
    server
}

/// Reads the plan (as opening the team tab does) and refreshes the session.
fn refresh(machine: &Machine, cx: &mut TestAppContext) -> TeamVaultStatus {
    machine
        .account
        .update(cx, |account, cx| account.refresh_plan(cx));
    cx.run_until_parked();
    let task = machine
        .session
        .update(cx, |session, cx| session.refresh(cx));
    cx.run_until_parked();
    drop(task);
    machine
        .session
        .read_with(cx, |session, _| session.status().clone())
}

fn run<T: 'static>(cx: &mut TestAppContext, task: gpui::Task<T>) -> T {
    let result = cx.foreground_executor().block_test(task);
    cx.run_until_parked();
    result
}

fn entry(byte: u8) -> EntryId {
    EntryId::parse(&format!("{byte:02x}").repeat(16)).unwrap()
}

fn push(
    machine: &Machine,
    cx: &mut TestAppContext,
    entry: EntryId,
    text: &str,
) -> Result<(), TeamVaultError> {
    push_on(machine, cx, entry, text, PushBase::Latest)
}

fn push_on(
    machine: &Machine,
    cx: &mut TestAppContext,
    entry: EntryId,
    text: &str,
    base: PushBase,
) -> Result<(), TeamVaultError> {
    let task = machine.session.update(cx, |session, cx| {
        session.push(entry, Zeroizing::new(text.as_bytes().to_vec()), base, cx)
    });
    run(cx, task)
}

fn pull(
    machine: &Machine,
    cx: &mut TestAppContext,
    entry: EntryId,
) -> Result<String, TeamVaultError> {
    let task = machine
        .session
        .update(cx, |session, cx| session.pull(entry, cx));
    run(cx, task).map(|pulled| String::from_utf8(pulled.payload.to_vec()).unwrap())
}

/// The user compared `user_id`'s fingerprint with them and pressed Trust Key.
fn trust(machine: &Machine, cx: &mut TestAppContext, user_id: &str) {
    let task = machine
        .session
        .update(cx, |session, cx| session.trust_member_key(user_id, cx));
    run(cx, task).unwrap();
}

fn grant(machine: &Machine, cx: &mut TestAppContext, user_id: &str) -> Result<(), TeamVaultError> {
    let task = machine
        .session
        .update(cx, |session, cx| session.grant(user_id.to_string(), cx));
    run(cx, task)
}

/// Admin creates the team key, the member registers, the admin compares the
/// member's fingerprint, trusts it and grants them.
fn shared_team(cx: &mut TestAppContext, server: &FakeTeamServer) -> (Machine, Machine) {
    let admin = machine(cx, server, ADMIN, 0x11);
    let member = machine(cx, server, MEMBER, 0x22);

    assert_eq!(refresh(&admin, cx), TeamVaultStatus::KeyPending);
    let task = admin
        .session
        .update(cx, |session, cx| session.create_team_key(cx));
    run(cx, task).unwrap();
    assert_eq!(
        admin.session.read_with(cx, |s, _| s.status().clone()),
        TeamVaultStatus::Ready
    );

    assert_eq!(refresh(&member, cx), TeamVaultStatus::KeyPending);
    assert_eq!(refresh(&admin, cx), TeamVaultStatus::Ready);
    assert_eq!(
        grant(&admin, cx, MEMBER).err(),
        Some(TeamVaultError::NotPinned {
            user_id: MEMBER.into()
        }),
        "a key nobody compared was granted"
    );
    trust(&admin, cx, MEMBER);
    grant(&admin, cx, MEMBER).unwrap();
    assert_eq!(refresh(&member, cx), TeamVaultStatus::Ready);
    (admin, member)
}

#[gpui::test]
fn nothing_is_requested_until_somebody_asks(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let admin = machine(cx, &server, ADMIN, 0x11);
    cx.run_until_parked();
    assert_eq!(
        server.request_count(ADMIN),
        0,
        "a session made a request on its own"
    );
    assert_eq!(
        admin.session.read_with(cx, |s, _| s.status().clone()),
        TeamVaultStatus::NoTeam
    );
}

#[gpui::test]
fn two_members_share_an_entry_end_to_end(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);

    push(&admin, cx, entry(0xa1), "STRIPE_KEY=sk_live_x").unwrap();
    assert_eq!(
        pull(&member, cx, entry(0xa1)).unwrap(),
        "STRIPE_KEY=sk_live_x"
    );

    let stored = server.vault_doc(&entry(0xa1).as_hex()).unwrap();
    assert!(
        !stored.blob.contains("sk_live"),
        "the server holds plaintext"
    );
    for envelope in server.envelopes_for(MEMBER) {
        assert_eq!(envelope.wrapped_by, ADMIN);
    }
    assert!(server.registered_key(MEMBER).is_some());
}

/// The phase's success criterion: the sharing key follows the recovery key.
#[gpui::test]
fn a_second_machine_of_the_same_user_opens_team_entries(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, _) = shared_team(cx, &server);
    push(&admin, cx, entry(0xa1), "A=1").unwrap();

    // A fresh machine: its own (empty) local trust record, the same
    // recovery key, and nothing else carried over.
    init(cx);
    let laptop = machine(cx, &server, MEMBER, 0x22);
    assert_eq!(refresh(&laptop, cx), TeamVaultStatus::Ready);
    assert_eq!(pull(&laptop, cx, entry(0xa1)).unwrap(), "A=1");
}

#[gpui::test]
fn without_the_recovery_key_there_is_nothing_to_open_with(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let http_client = server.client_for(MEMBER);
    let account = cx.update(|cx| {
        cx.new(|_| {
            let mut account = Account::for_test_with(
                AccountStatus::SignedIn(AccountUser {
                    id: MEMBER.into(),
                    email: "bo@example.com".into(),
                    name: None,
                    avatar_url: None,
                }),
                http_client,
                MemoryKeychain::empty(),
            );
            account.set_tokens_for_test();
            account
        })
    });
    let session = cx.update(|cx| {
        let account = account.clone();
        cx.new(|cx| TeamVaultSession::new(account, cx))
    });
    let machine = Machine { account, session };
    assert_eq!(refresh(&machine, cx), TeamVaultStatus::NeedsRecoveryKey);
    assert_eq!(
        server.registered_key(MEMBER),
        None,
        "no key was made without the recovery key"
    );
}

#[gpui::test]
fn a_changed_member_key_blocks_grant_until_accepted(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, _member) = shared_team(cx, &server);

    // The server swaps the member's key for its own and drops the member's
    // envelope, so the member shows as pending again.
    let impostor = SharingKeypair::generate().unwrap();
    server.substitute_key_as_server(
        MEMBER,
        &zode_team_vault::sharing_key::encode_public_key(impostor.public_key()),
    );
    assert_eq!(refresh(&admin, cx), TeamVaultStatus::Ready);

    let pin = admin.session.read_with(cx, |session, _| {
        session
            .members()
            .into_iter()
            .find(|m| m.user_id == MEMBER)
            .and_then(|m| m.pin)
    });
    assert!(matches!(pin, Some(PinCheck::Changed { .. })), "{pin:?}");

    let before = server.envelope_count();
    assert_eq!(
        grant(&admin, cx, MEMBER).err(),
        Some(TeamVaultError::KeyChanged {
            user_id: MEMBER.into()
        })
    );
    assert_eq!(
        server.envelope_count(),
        before,
        "a key was sealed to a changed key"
    );

    trust(&admin, cx, MEMBER);
    grant(&admin, cx, MEMBER).unwrap();
    assert_eq!(server.envelope_count(), before + 1);
}

/// The server seals a key of its choosing to the member, claiming the admin did.
#[gpui::test]
fn the_server_cannot_hand_a_member_a_key_it_chose(cx: &mut TestAppContext) {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use zode_team_vault::{EnvelopeIds, TeamKey, wrap};

    init(cx);
    let server = team_with_admin_and_member();
    let (_admin, member) = shared_team(cx, &server);

    let member_key =
        zode_team_vault::sharing_key::decode_public_key(&server.registered_key(MEMBER).unwrap())
            .unwrap();
    let server_key = SharingKeypair::generate().unwrap();
    let forged = wrap(
        &server_key,
        &member_key,
        &EnvelopeIds {
            team_id: TEAM,
            recipient_user_id: MEMBER,
            key_version: 1,
        },
        &TeamKey::from_bytes([0x99; 32]),
    )
    .unwrap();
    server.inject_envelope_as_server(StoredEnvelope {
        user_id: MEMBER.into(),
        key_version: 1,
        wrapped_key: STANDARD.encode(forged),
        wrapped_by: ADMIN.into(),
    });

    let status = refresh(&member, cx);
    assert!(
        matches!(&status, TeamVaultStatus::Failed(message) if message.contains("pinned")),
        "{status:?}"
    );
    assert!(
        member
            .session
            .read_with(cx, |s, _| s.key_versions().is_empty()),
        "a forged key was kept"
    );
}

#[gpui::test]
fn a_replayed_older_entry_is_refused(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);

    push(&admin, cx, entry(0xa1), "API_KEY=revoked").unwrap();
    let old = server.vault_doc(&entry(0xa1).as_hex()).unwrap();
    assert_eq!(pull(&member, cx, entry(0xa1)).unwrap(), "API_KEY=revoked");
    push(&admin, cx, entry(0xa1), "API_KEY=fresh").unwrap();
    assert_eq!(pull(&member, cx, entry(0xa1)).unwrap(), "API_KEY=fresh");

    server.replay_entry_as_server(&entry(0xa1).as_hex(), old);
    assert_eq!(
        pull(&member, cx, entry(0xa1)).err(),
        Some(TeamVaultError::Crypto(TeamCryptoError::Rollback {
            seen: 2,
            got: 1
        }))
    );
}

#[gpui::test]
fn rotation_reseals_old_entries_and_members_follow(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);
    push(&admin, cx, entry(0xa1), "A=1").unwrap();
    push(&admin, cx, entry(0xb2), "B=2").unwrap();

    let task = admin.session.update(cx, |session, cx| session.rotate(cx));
    let report = run(cx, task).unwrap();
    assert_eq!(
        (
            report.key_version,
            report.rotated_here,
            report.reseal.resealed
        ),
        (2, true, 2)
    );
    assert_eq!(server.vault_versions(), vec![2, 2]);
    assert_eq!(server.key_version(), 2);

    // The member still holds only version 1 until it refreshes.
    assert_eq!(
        pull(&member, cx, entry(0xa1)).err(),
        Some(TeamVaultError::NoTeamKey)
    );
    assert_eq!(refresh(&member, cx), TeamVaultStatus::Ready);
    assert_eq!(
        member.session.read_with(cx, |s, _| s.key_versions()),
        vec![1, 2]
    );
    assert_eq!(pull(&member, cx, entry(0xa1)).unwrap(), "A=1");
}

/// Between the version moving and an entry being re-sealed, members read it
/// with the old key, which their kept envelope still opens; the re-seal is
/// then finished without another rotation.
#[gpui::test]
fn members_read_old_entries_while_a_rotation_is_under_way(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);
    push(&admin, cx, entry(0xa1), "A=1").unwrap();

    // The version moves, then every re-seal write fails.
    server.fail_next_vault_writes(usize::MAX);
    let task = admin.session.update(cx, |session, cx| session.rotate(cx));
    assert!(run(cx, task).is_err());
    assert_eq!(
        (server.key_version(), server.vault_versions()),
        (2, vec![1])
    );

    assert_eq!(refresh(&member, cx), TeamVaultStatus::Ready);
    assert_eq!(
        member.session.read_with(cx, |s, _| s.key_versions()),
        vec![1, 2]
    );
    assert_eq!(pull(&member, cx, entry(0xa1)).unwrap(), "A=1");

    server.fail_next_vault_writes(0);
    assert_eq!(refresh(&admin, cx), TeamVaultStatus::Ready);
    let task = admin.session.update(cx, |session, cx| session.reseal(cx));
    assert_eq!(run(cx, task).unwrap().resealed, 1);
    assert_eq!(server.vault_versions(), vec![2]);
    assert_eq!(pull(&member, cx, entry(0xa1)).unwrap(), "A=1");
}

#[gpui::test]
fn a_concurrent_rotation_loses_cleanly_and_carries_on(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);
    server.add_member(SECOND_ADMIN, "cy@example.com", "admin");
    let second = machine(cx, &server, SECOND_ADMIN, 0x33);
    assert_eq!(refresh(&second, cx), TeamVaultStatus::KeyPending);
    assert_eq!(refresh(&admin, cx), TeamVaultStatus::Ready);
    trust(&admin, cx, SECOND_ADMIN);
    grant(&admin, cx, SECOND_ADMIN).unwrap();
    assert_eq!(refresh(&second, cx), TeamVaultStatus::Ready);
    // The second admin compares everyone's fingerprint before rotating.
    for user_id in [ADMIN, MEMBER] {
        trust(&second, cx, user_id);
    }
    push(&admin, cx, entry(0xa1), "A=1").unwrap();

    // The second admin rotates first; the first still holds version 1.
    let task = second.session.update(cx, |session, cx| session.rotate(cx));
    let winner = run(cx, task).unwrap();
    assert_eq!((winner.key_version, winner.rotated_here), (2, true));

    let task = admin.session.update(cx, |session, cx| session.rotate(cx));
    let loser = run(cx, task).unwrap();
    assert_eq!(
        (loser.key_version, loser.rotated_here),
        (2, false),
        "the loser rotated again"
    );
    assert_eq!(server.key_version(), 2);

    // The member has opened this team before and never checked the second
    // admin, who sealed the new key: it waits for that check.
    assert_eq!(
        refresh(&member, cx),
        TeamVaultStatus::SealerNotVerified {
            user_id: SECOND_ADMIN.into(),
            email: "cy@example.com".into(),
        }
    );
    trust(&member, cx, SECOND_ADMIN);
    assert_eq!(
        member.session.read_with(cx, |s, _| s.status().clone()),
        TeamVaultStatus::Ready,
        "trusting the sealer reads the keys again"
    );
    assert_eq!(pull(&member, cx, entry(0xa1)).unwrap(), "A=1");
}

#[gpui::test]
fn a_lower_key_version_from_the_server_is_refused(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);
    let task = admin.session.update(cx, |session, cx| session.rotate(cx));
    run(cx, task).unwrap();
    assert_eq!(refresh(&member, cx), TeamVaultStatus::Ready);

    // Back to the version a removed member would still hold.
    server.roll_back_key_version_as_server(1);
    let status = refresh(&member, cx);
    assert_eq!(
        status,
        TeamVaultStatus::Failed(
            TeamVaultError::Crypto(TeamCryptoError::KeyVersionRollback { seen: 2, got: 1 })
                .to_string()
                .into()
        )
    );
    assert!(
        member
            .session
            .read_with(cx, |s, _| s.key_versions().is_empty())
    );
    assert_eq!(
        push(&member, cx, entry(0xa1), "A=2").err(),
        Some(TeamVaultError::NoTeamKey)
    );
}

#[gpui::test]
fn a_removed_member_loses_the_team_and_its_keys(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);
    push(&admin, cx, entry(0xa1), "A=1").unwrap();

    server.remove_member(MEMBER);
    let status = refresh(&member, cx);
    assert_eq!(status, TeamVaultStatus::NoTeam);
    assert!(
        member
            .session
            .read_with(cx, |s, _| s.key_versions().is_empty())
    );
    assert_eq!(
        pull(&member, cx, entry(0xa1)).err(),
        Some(TeamVaultError::NoTeamKey)
    );
}

#[gpui::test]
fn a_write_that_loses_a_race_is_a_conflict_not_an_overwrite(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);
    push(&admin, cx, entry(0xa1), "A=1").unwrap();
    let theirs = server.vault_doc(&entry(0xa1).as_hex()).unwrap();

    // Someone writes between the member reading the entry and storing it.
    server.lose_next_vault_write(&theirs.blob);
    assert_eq!(
        push(&member, cx, entry(0xa1), "A=3").err(),
        Some(TeamVaultError::Conflict)
    );
    let after = server.vault_doc(&entry(0xa1).as_hex()).unwrap();
    assert_eq!(
        after.blob, theirs.blob,
        "the losing write replaced the winner's"
    );
    assert_eq!(pull(&admin, cx, entry(0xa1)).unwrap(), "A=1");
}

#[gpui::test]
fn the_manifest_is_shared_like_any_entry(cx: &mut TestAppContext) {
    use zode_team_vault::{ManifestEntry, ManifestEntryKind};

    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);
    push(&admin, cx, entry(0xa1), "A=1").unwrap();

    let task = admin.session.update(cx, |session, cx| {
        session.edit_manifest(
            ManifestChange::Upsert(ManifestEntry {
                entry_id: entry(0xa1),
                kind: ManifestEntryKind::Env,
                label: ".env.production".into(),
                project_hint: Some("acme-api".into()),
            }),
            cx,
        )
    });
    run(cx, task).unwrap();

    assert_eq!(refresh(&member, cx), TeamVaultStatus::Ready);
    let listed = member.session.read_with(cx, |session, _| session.list());
    assert_eq!(listed.len(), 1, "the manifest's own slot is not an entry");
    assert_eq!(
        listed[0].manifest.as_ref().map(|m| m.label.as_str()),
        Some(".env.production")
    );
}

#[gpui::test]
fn signing_out_drops_the_team(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (_, member) = shared_team(cx, &server);
    let task = member
        .account
        .update(cx, |account, cx| account.sign_out(cx));
    run(cx, task);
    assert_eq!(
        member.session.read_with(cx, |s, _| s.status().clone()),
        TeamVaultStatus::NoTeam
    );
    assert!(
        member
            .session
            .read_with(cx, |s, _| s.key_versions().is_empty())
    );
}

const FAKE: &str = "68b1f0c2a4d3e5f60718290f";

/// A key the server holds, sealed to `recipient` as if `FAKE` had sealed it.
fn forged_envelope(
    server: &FakeTeamServer,
    server_key: &SharingKeypair,
    recipient: &str,
    key_version: u32,
) -> StoredEnvelope {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use zode_team_vault::{EnvelopeIds, TeamKey, wrap};

    let recipient_key =
        zode_team_vault::sharing_key::decode_public_key(&server.registered_key(recipient).unwrap())
            .unwrap();
    let forged = wrap(
        server_key,
        &recipient_key,
        &EnvelopeIds {
            team_id: TEAM,
            recipient_user_id: recipient,
            key_version,
        },
        &TeamKey::from_bytes([0x99; 32]),
    )
    .unwrap();
    StoredEnvelope {
        user_id: recipient.into(),
        key_version,
        wrapped_key: STANDARD.encode(forged),
        wrapped_by: FAKE.into(),
    }
}

/// The server lists a member of its own making and moves the team to a key
/// version only that "member" sealed. A machine that has already opened the
/// team never takes a new sealer on trust.
#[gpui::test]
fn an_injected_fake_member_sealer_is_refused_and_not_shown_as_verified(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (_admin, member) = shared_team(cx, &server);

    let server_key = SharingKeypair::generate().unwrap();
    server.add_member(FAKE, "zed@example.com", "admin");
    server.register_key_as_server(
        FAKE,
        &zode_team_vault::sharing_key::encode_public_key(server_key.public_key()),
    );
    server.set_key_version_as_server(2);
    server.inject_envelope_as_server(forged_envelope(&server, &server_key, MEMBER, 2));

    let status = refresh(&member, cx);
    assert_eq!(
        status,
        TeamVaultStatus::SealerNotVerified {
            user_id: FAKE.into(),
            email: "zed@example.com".into(),
        },
        "a fake sealer was trusted, or the refusal was not surfaced"
    );
    assert!(
        !member
            .session
            .read_with(cx, |s, _| s.key_versions().contains(&2)),
        "the server's key was kept"
    );
    let pin = member.session.read_with(cx, |session, _| {
        session
            .members()
            .into_iter()
            .find(|m| m.user_id == FAKE)
            .and_then(|m| m.pin)
    });
    assert_eq!(pin, Some(PinCheck::New), "a fake sealer was pinned");
}

/// The server hands a machine opening the team for the first time two
/// envelopes for one version: the admin's and one of its own. Whichever
/// came last would win, so neither is used.
#[gpui::test]
fn a_duplicate_envelope_for_the_current_version_is_refused(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (_admin, _member) = shared_team(cx, &server);

    let server_key = SharingKeypair::generate().unwrap();
    server.add_member(FAKE, "zed@example.com", "admin");
    server.register_key_as_server(
        FAKE,
        &zode_team_vault::sharing_key::encode_public_key(server_key.public_key()),
    );
    server.add_envelope_as_server(forged_envelope(&server, &server_key, MEMBER, 1));

    // The same member on a machine that has never opened the team.
    init(cx);
    let laptop = machine(cx, &server, MEMBER, 0x22);
    let status = refresh(&laptop, cx);
    assert!(
        matches!(&status, TeamVaultStatus::Failed(message) if message.contains("2 envelopes")),
        "two envelopes for one version were accepted: {status:?}"
    );
    assert!(
        laptop
            .session
            .read_with(cx, |s, _| s.key_versions().is_empty()),
        "a key was kept from a duplicated version"
    );
}

/// A sealer pinned on first sight is remembered, not verified: a rotation
/// does not seal the new key to it.
#[gpui::test]
fn a_rotation_refuses_a_tofu_only_pin(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, _member) = shared_team(cx, &server);
    server.add_member(SECOND_ADMIN, "cy@example.com", "admin");
    let second = machine(cx, &server, SECOND_ADMIN, 0x33);
    assert_eq!(refresh(&second, cx), TeamVaultStatus::KeyPending);
    assert_eq!(refresh(&admin, cx), TeamVaultStatus::Ready);
    trust(&admin, cx, SECOND_ADMIN);
    grant(&admin, cx, SECOND_ADMIN).unwrap();
    // Opening the admin's envelope pins the admin on first sight only.
    assert_eq!(refresh(&second, cx), TeamVaultStatus::Ready);
    let admin_pin = second.session.read_with(cx, |session, _| {
        session
            .members()
            .into_iter()
            .find(|m| m.user_id == ADMIN)
            .and_then(|m| m.pin)
    });
    assert_eq!(
        admin_pin,
        Some(PinCheck::Seen),
        "first sight is not verified"
    );
    trust(&second, cx, MEMBER);

    let task = second.session.update(cx, |session, cx| session.rotate(cx));
    assert_eq!(
        run(cx, task).err(),
        Some(TeamVaultError::NotPinned {
            user_id: ADMIN.into()
        })
    );
    assert_eq!(server.key_version(), 1, "a rotation sealed to a TOFU pin");
}

/// Two members share at once, each from the list it loaded earlier. The
/// second write is merged into the list the first one left, never sealed
/// over it from a stale copy.
#[gpui::test]
fn a_stale_team_list_is_merged_not_written_over(cx: &mut TestAppContext) {
    use zode_team_vault::{ManifestEntry, ManifestEntryKind};

    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);
    push(&admin, cx, entry(0xa1), "A=1").unwrap();
    push(&member, cx, entry(0xb2), "B=2").unwrap();
    // Both hold the same, empty, list now.
    assert_eq!(refresh(&admin, cx), TeamVaultStatus::Ready);
    assert_eq!(refresh(&member, cx), TeamVaultStatus::Ready);

    let named = |byte: u8, label: &str| ManifestEntry {
        entry_id: entry(byte),
        kind: ManifestEntryKind::Env,
        label: label.into(),
        project_hint: None,
    };
    for (machine, byte, label) in [(&admin, 0xa1, ".env.a"), (&member, 0xb2, ".env.b")] {
        let task = machine.session.update(cx, |session, cx| {
            session.edit_manifest(ManifestChange::Upsert(named(byte, label)), cx)
        });
        run(cx, task).unwrap();
    }
    assert_eq!(
        member.session.read_with(cx, |session, _| session
            .manifest()
            .map(|manifest| manifest.entries().len())),
        Some(2),
        "the local copy is the merged list"
    );

    // A removal from a stale copy removes only that entry.
    let task = admin.session.update(cx, |session, cx| {
        session.edit_manifest(ManifestChange::Remove(entry(0xa1)), cx)
    });
    run(cx, task).unwrap();
    assert_eq!(refresh(&member, cx), TeamVaultStatus::Ready);
    let task = member.session.update(cx, |session, cx| {
        session.edit_manifest(ManifestChange::Upsert(named(0xa1, ".env.a")), cx)
    });
    run(cx, task).unwrap();

    assert_eq!(refresh(&admin, cx), TeamVaultStatus::Ready);
    let mut labels: Vec<String> = admin.session.read_with(cx, |session, _| {
        session
            .list()
            .into_iter()
            .filter_map(|view| view.manifest.map(|named| named.label))
            .collect()
    });
    labels.sort();
    assert_eq!(
        labels,
        vec![".env.a".to_string(), ".env.b".to_string()],
        "a stale copy of the list was written over a newer one"
    );
}

/// A replace is conditional on the revision the user reviewed, not on one
/// read again when they confirm: a teammate's write made during the review is
/// a conflict, never overwritten.
#[gpui::test]
fn a_replace_after_review_does_not_overwrite_a_newer_write(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, member) = shared_team(cx, &server);
    push_on(&admin, cx, entry(0xa1), "A=1", PushBase::Create).unwrap();

    // The admin reviews the diff against what they pulled...
    let task = admin
        .session
        .update(cx, |session, cx| session.pull(entry(0xa1), cx));
    let seen = run(cx, task).unwrap().revision;
    // ...while the member writes.
    push(&member, cx, entry(0xa1), "A=2").unwrap();

    assert_eq!(
        push_on(&admin, cx, entry(0xa1), "A=3", PushBase::Replace(seen)).err(),
        Some(TeamVaultError::Conflict)
    );
    assert_eq!(pull(&admin, cx, entry(0xa1)).unwrap(), "A=2");
    assert_eq!(
        push_on(&admin, cx, entry(0xa1), "A=4", PushBase::Create).err(),
        Some(TeamVaultError::Conflict),
        "a create over an existing entry is a conflict"
    );
}

/// Another admin's rotation wins, then someone leaves before this one
/// carries on from it. The server flags the team again; this machine must not
/// report it rotated, nor clear the flag on its own.
#[gpui::test]
fn a_lost_rotation_race_keeps_the_servers_rotation_flag(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, _member) = shared_team(cx, &server);
    server.add_member(SECOND_ADMIN, "cy@example.com", "admin");
    let second = machine(cx, &server, SECOND_ADMIN, 0x33);
    assert_eq!(refresh(&second, cx), TeamVaultStatus::KeyPending);
    assert_eq!(refresh(&admin, cx), TeamVaultStatus::Ready);
    trust(&admin, cx, SECOND_ADMIN);
    grant(&admin, cx, SECOND_ADMIN).unwrap();
    assert_eq!(refresh(&second, cx), TeamVaultStatus::Ready);
    for user_id in [ADMIN, MEMBER] {
        trust(&second, cx, user_id);
    }

    let task = second.session.update(cx, |session, cx| session.rotate(cx));
    run(cx, task).unwrap();
    server.remove_member(MEMBER);

    let task = admin.session.update(cx, |session, cx| session.rotate(cx));
    let report = run(cx, task).unwrap();
    assert!(!report.rotated_here);
    assert!(
        report.still_recommended,
        "the report must say to rotate again"
    );
    assert!(
        admin
            .session
            .read_with(cx, |session, _| session.rotation_recommended()),
        "the flag was cleared locally while the server still asks for a rotation"
    );
}

/// The sharing key in the store was sealed under a recovery key nobody has
/// any more. That is not a dead end: the member replaces it, and an admin
/// grants them again.
#[gpui::test]
fn an_unopenable_sharing_key_can_be_replaced_and_granted_again(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    let (admin, _member) = shared_team(cx, &server);
    let before = server.registered_key(MEMBER);

    // The same member, with a recovery key the stored sharing key does not
    // open under.
    init(cx);
    let lost = machine(cx, &server, MEMBER, 0x55);
    let status = refresh(&lost, cx);
    assert_eq!(
        status,
        TeamVaultStatus::SharingKeyUnreadable,
        "an unopenable sharing key is a dead end"
    );
    assert_eq!(
        server.registered_key(MEMBER),
        before,
        "nothing replaced on its own"
    );

    let task = lost
        .session
        .update(cx, |session, cx| session.replace_sharing_key(cx));
    run(cx, task).unwrap();
    assert_ne!(
        server.registered_key(MEMBER),
        before,
        "the new key is registered"
    );
    assert!(
        server.envelopes_for(MEMBER).is_empty(),
        "envelopes sealed to the old key are gone"
    );
    assert_eq!(
        lost.session.read_with(cx, |s, _| s.status().clone()),
        TeamVaultStatus::KeyPending
    );

    // A slot that opens is never replaced.
    let task = lost
        .session
        .update(cx, |session, cx| session.replace_sharing_key(cx));
    assert!(run(cx, task).is_err());

    // The admin sees a changed key, compares it, accepts it and grants.
    assert_eq!(refresh(&admin, cx), TeamVaultStatus::Ready);
    assert_eq!(
        grant(&admin, cx, MEMBER).err(),
        Some(TeamVaultError::KeyChanged {
            user_id: MEMBER.into()
        })
    );
    trust(&admin, cx, MEMBER);
    grant(&admin, cx, MEMBER).unwrap();
    assert_eq!(refresh(&lost, cx), TeamVaultStatus::Ready);
}

/// Opening the Team tab before the plan is known reads the plan first. When
/// that read fails, the tab says so instead of spinning for ever.
#[gpui::test]
fn a_failed_plan_read_does_not_leave_the_team_tab_working(cx: &mut TestAppContext) {
    init(cx);
    let server = team_with_admin_and_member();
    server.fail_plan_reads(true);
    let member = machine(cx, &server, MEMBER, 0x22);

    let task = member.session.update(cx, |session, cx| session.refresh(cx));
    cx.run_until_parked();
    drop(task);
    let status = member
        .session
        .read_with(cx, |session, _| session.status().clone());
    assert!(
        matches!(&status, TeamVaultStatus::Failed(message) if message.contains("plan")),
        "{status:?}"
    );

    // Once the plan can be read, the next refresh carries on as usual.
    server.fail_plan_reads(false);
    let task = member.session.update(cx, |session, cx| session.refresh(cx));
    cx.run_until_parked();
    drop(task);
    assert_eq!(
        member.session.read_with(cx, |s, _| s.status().clone()),
        TeamVaultStatus::KeyPending
    );
}
