use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, EventEmitter, Global, SharedString,
    Subscription, Task, WeakEntity,
};
use zeroize::Zeroizing;
use zode_account::{Account, AccountStatusChanged, PlanLimit, PlanLimitReason, TeamSummary};
use zode_env_sync::EntryId;
use zode_sync::client::{self as sync_client, ClientError, Precondition, WriteOutcome};
use zode_sync::sync::SyncContext;
use zode_sync::{Dek, Resource, from_blob, to_blob};

use crate::client::{
    self, EnvelopeWrite, KeyAnswer, KeyRegistration, MemberKey, MemberKeys, RegisterOutcome,
    VaultListing, VaultPrecondition, VaultWrite,
};
use crate::grant::{open_envelopes, prepare_grant};
use crate::manifest::{ManifestEntry, TeamManifest, manifest_entry_id};
use crate::pins::{self, PinCheck, SaveHandle, TeamTrust};
use crate::rotate::{ResealReport, RotationStep, reseal_older_entries, rotate_key_version};
use crate::seal::{open_entry, seal_entry};
use crate::sharing_key::{Fingerprint, SharingKeypair, open_private_key, seal_private_key};
use crate::team_key::{EnvelopeIds, TeamKey, TeamKeyring, wrap};
use crate::{TeamCryptoError, TeamVaultError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamRole {
    Owner,
    Admin,
    Member,
}

impl TeamRole {
    /// An unknown role is a member: the least a newer server could mean.
    fn parse(role: &str) -> Self {
        match role {
            "owner" => Self::Owner,
            "admin" => Self::Admin,
            _ => Self::Member,
        }
    }

    pub fn can_administer(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeamVaultStatus {
    /// Not in a team, or no longer: every team key has been dropped.
    NoTeam,
    /// This machine has no recovery key, so the sharing key cannot be opened.
    NeedsRecoveryKey,
    /// The stored sharing key does not open under this machine's recovery
    /// key. Enter the recovery key it was sealed under, or replace it
    /// ([`TeamVaultSession::replace_sharing_key`]).
    SharingKeyUnreadable,
    Working,
    /// In the team, but nobody has sealed the current key to this member yet.
    KeyPending,
    /// The current key was sealed by a member whose key this machine has not
    /// verified, on a team it has opened before. Nothing they sealed is
    /// opened until the user compares their fingerprint and trusts it.
    SealerNotVerified {
        user_id: String,
        email: String,
    },
    Ready,
    /// The team's plan lapsed: entries stay readable, writes are refused.
    ReadOnly,
    PlanLimited(PlanLimit),
    Failed(SharedString),
}

impl TeamVaultStatus {
    pub fn sentence(&self) -> SharedString {
        match self {
            Self::NoTeam => "you are not in a team".into(),
            Self::NeedsRecoveryKey => "enter your recovery key first".into(),
            Self::SharingKeyUnreadable => "Your sharing key does not open with the recovery key on this machine. Enter the recovery key you use elsewhere, or replace your sharing key: admins must then grant you the team key again, in every team.".into(),
            Self::Working => "Syncing team vault…".into(),
            Self::KeyPending => "waiting for an admin to grant you the team key".into(),
            Self::SealerNotVerified { email, .. } => format!(
                "{email} sealed the current team key for you, and their key is not verified on this machine. Open Members, compare their fingerprint with them, then trust it."
            )
            .into(),
            Self::Ready => "ready".into(),
            Self::ReadOnly => "Your team's plan has ended. Shared files are read-only.".into(),
            Self::PlanLimited(limit) => limit.sentence().into(),
            Self::Failed(message) => message.clone(),
        }
    }
}

pub struct TeamVaultStatusChanged;

/// One member, as the members window shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberView {
    pub user_id: String,
    pub email: String,
    pub fingerprint: Option<Fingerprint>,
    /// `None` for this account and for members without a key.
    pub pin: Option<PinCheck>,
    pub has_current_envelope: bool,
    pub is_me: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultEntryView {
    pub listing: VaultListing,
    pub manifest: Option<ManifestEntry>,
}

/// An opened entry and the revision it was read at: what a replace offered
/// to the user is conditional on.
pub struct PulledEntry {
    /// Wiped when it drops.
    pub payload: Zeroizing<Vec<u8>>,
    pub revision: String,
}

/// What a write expects to find at the entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PushBase {
    /// Nothing: a new entry. Anything there is a conflict.
    Create,
    /// The revision the user saw (the diff they confirmed). Anything written
    /// since is a conflict, not overwritten.
    Replace(String),
    /// Whatever is there when the write is made. Only for a replace the user
    /// did not review against a revision (re-sharing a connection by name).
    Latest,
}

/// One change to the team's list of entries, applied to the list as it is on
/// the server at the moment of writing, never to a copy loaded earlier.
#[derive(Clone, Debug)]
pub enum ManifestChange {
    Upsert(ManifestEntry),
    Remove(EntryId),
}

/// How many times a list edit is merged again after losing a write race.
const MANIFEST_ATTEMPTS: usize = 3;

/// What a rotation did.
#[derive(Debug, PartialEq, Eq)]
pub struct RotationReport {
    pub key_version: u32,
    /// False when another admin's rotation landed first and this one carried
    /// on from it.
    pub rotated_here: bool,
    /// The server's flag, read after the version moved. True when someone
    /// left after the rotation that landed: rotate again.
    pub still_recommended: bool,
    pub reseal: ResealReport,
}

struct ActiveTeam {
    team_id: String,
    name: SharedString,
    role: TeamRole,
    me: Arc<SharingKeypair>,
    /// Every key version opened. Memory only; dropped with the team.
    keyring: Arc<TeamKeyring>,
    members: Vec<MemberKey>,
    rotation_recommended: bool,
    listing: Vec<VaultListing>,
    manifest: TeamManifest,
    /// Who sealed the current key, when that member is not verified here and
    /// so nothing they sealed was opened: (user id, email).
    unverified_sealer: Option<(String, String)>,
}

struct GlobalTeamVaultSession(Entity<TeamVaultSession>);
impl Global for GlobalTeamVaultSession {}

/// Everything team sharing talks to.
///
/// # The invariants this type exists to hold
///
/// **Nothing here runs on its own.** No request at startup, none on a timer.
/// Each request is the direct result of a call a person made — opening the
/// team tab, pulling, sharing, granting, rotating — the promise `SyncSession`
/// and `EnvSession` make, and the README lists these requests on that basis.
///
/// **Keys live in memory only.** The sharing private key is stored sealed
/// under the recovery key on the server and never written locally; team keys
/// are never written anywhere. Leaving the team drops them all.
///
/// **The server is not trusted with keys.** Members' keys are pinned, the key
/// version only moves forward, and an entry older than one already seen is
/// refused.
pub struct TeamVaultSession {
    account: Entity<Account>,
    status: TeamVaultStatus,
    team: Option<ActiveTeam>,
    /// (user id, record). Loaded on the first refresh for that account.
    trust: Option<(String, TeamTrust)>,
    last_save: Option<SaveHandle>,
    /// Bumped by every refresh and by dropping the team, so a refresh that
    /// finishes after a newer one (or after the team was left) is discarded.
    generation: u64,
    /// A refresh was asked for before the plan was known; it continues when
    /// the plan arrives, and fails when the read ends without one.
    refresh_after_plan: bool,
    _account_subscriptions: [Subscription; 2],
}

impl EventEmitter<TeamVaultStatusChanged> for TeamVaultSession {}

impl TeamVaultSession {
    pub fn new(account: Entity<Account>, cx: &mut Context<Self>) -> Self {
        let subscriptions = [
            cx.subscribe(&account, |this, _, _: &AccountStatusChanged, cx| {
                this.account_changed(cx);
            }),
            cx.observe(&account, |this, _, cx| this.plan_read_settled(cx)),
        ];
        Self {
            account,
            status: TeamVaultStatus::NoTeam,
            team: None,
            trust: None,
            last_save: None,
            generation: 0,
            refresh_after_plan: false,
            _account_subscriptions: subscriptions,
        }
    }

    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalTeamVaultSession>()
            .map(|global| global.0.clone())
    }

    pub fn set_global(session: Entity<Self>, cx: &mut App) {
        cx.set_global(GlobalTeamVaultSession(session));
    }

    pub fn status(&self) -> &TeamVaultStatus {
        &self.status
    }

    pub fn team_name(&self) -> Option<SharedString> {
        self.team.as_ref().map(|team| team.name.clone())
    }

    pub fn role(&self) -> Option<TeamRole> {
        self.team.as_ref().map(|team| team.role)
    }

    /// The server flags this after a member left or was removed since the
    /// last rotation.
    pub fn rotation_recommended(&self) -> bool {
        self.team
            .as_ref()
            .is_some_and(|team| team.rotation_recommended)
    }

    pub fn key_versions(&self) -> Vec<u32> {
        self.team
            .as_ref()
            .map(|team| team.keyring.versions())
            .unwrap_or_default()
    }

    pub fn manifest(&self) -> Option<&TeamManifest> {
        self.team.as_ref().map(|team| &team.manifest)
    }

    /// The entries, without the manifest's own slot.
    pub fn list(&self) -> Vec<VaultEntryView> {
        let Some(team) = &self.team else {
            return Vec::new();
        };
        let reserved = manifest_entry_id().ok();
        team.listing
            .iter()
            .filter(|listing| Some(listing.entry_id) != reserved)
            .map(|listing| VaultEntryView {
                listing: listing.clone(),
                manifest: team.manifest.get(&listing.entry_id).cloned(),
            })
            .collect()
    }

    pub fn members(&self) -> Vec<MemberView> {
        let (Some(team), Some((user_id, trust))) = (&self.team, &self.trust) else {
            return Vec::new();
        };
        team.members
            .iter()
            .map(|member| {
                let is_me = &member.user_id == user_id;
                let fingerprint = member.key.as_ref().map(|key| key.fingerprint);
                MemberView {
                    user_id: member.user_id.clone(),
                    email: member.email.clone(),
                    fingerprint,
                    pin: fingerprint.filter(|_| !is_me).map(|fingerprint| {
                        trust.check(&team.team_id, &member.user_id, &fingerprint)
                    }),
                    has_current_envelope: member.has_current_envelope,
                    is_me,
                }
            })
            .collect()
    }

    /// Members holding a sharing key but not the current team key.
    pub fn pending_members(&self) -> Vec<MemberView> {
        self.members()
            .into_iter()
            .filter(|member| {
                !member.is_me && !member.has_current_envelope && member.fingerprint.is_some()
            })
            .collect()
    }

    /// Loads everything the team tab shows: the sharing key (made the first
    /// time), the team keys, the members and the listing.
    pub fn refresh(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let account = self.account.read(cx);
        let signed_in = account.status().is_signed_in();
        let plan_team = account.plan().map(|plan| plan.team.clone());
        let team = match plan_team {
            Some(Some(team)) => team,
            Some(None) => {
                self.drop_team(cx);
                return Task::ready(());
            }
            None if signed_in => {
                // The plan names the team. Reading it is the account's job; the
                // refresh continues when it lands.
                self.refresh_after_plan = true;
                self.account
                    .update(cx, |account, cx| account.refresh_plan(cx));
                self.set_status(TeamVaultStatus::Working, cx);
                self.plan_read_settled(cx);
                return Task::ready(());
            }
            None => {
                self.drop_team(cx);
                return Task::ready(());
            }
        };

        self.generation += 1;
        let generation = self.generation;
        self.set_status(TeamVaultStatus::Working, cx);
        let account = self.account.clone();
        cx.spawn(async move |this, cx| {
            let outcome = load_team(&this, &account, &team, cx).await;
            this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.apply_load(outcome, cx);
                }
            })
            .ok();
        })
    }

    /// Opens one entry. The payload is wiped when the returned value drops.
    pub fn pull(
        &mut self,
        entry: EntryId,
        cx: &mut Context<Self>,
    ) -> Task<Result<PulledEntry, TeamVaultError>> {
        let Some((team_id, keyring)) = self.team_keys() else {
            return Task::ready(Err(TeamVaultError::NoTeamKey));
        };
        let account = self.account.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<_, TeamVaultError> = async {
                let context = context(&account, cx).await?;
                let document = client::fetch_entry(&context, &team_id, &entry)
                    .await?
                    .ok_or_else(|| ClientError::Rejected("not_found".into()))?;
                let opened = open_document(
                    &this,
                    &team_id,
                    &keyring,
                    &entry,
                    &document.blob,
                    document.key_version,
                    cx,
                )?;
                record_seq(&this, &team_id, &entry, opened.seq, cx)?.await?;
                Ok(PulledEntry {
                    payload: opened.payload,
                    revision: document.revision,
                })
            }
            .await;
            this.update(cx, |this, cx| this.absorb(&result, cx)).ok();
            result
        })
    }

    /// Shares `payload` at `entry`, as `base` expects to find it. A write
    /// that loses a race, or finds something other than `base`, answers
    /// `Conflict`, and nothing is merged or overwritten.
    pub fn push(
        &mut self,
        entry: EntryId,
        payload: Zeroizing<Vec<u8>>,
        base: PushBase,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), TeamVaultError>> {
        let Some((team_id, keyring)) = self.team_keys() else {
            return Task::ready(Err(TeamVaultError::NoTeamKey));
        };
        let account = self.account.clone();
        cx.spawn(async move |this, cx| {
            let result = async {
                let context = context(&account, cx).await?;
                push_entry(
                    &this, &context, &team_id, &keyring, &entry, &payload, &base, cx,
                )
                .await
            }
            .await;
            this.update(cx, |this, cx| this.absorb(&result, cx)).ok();
            result
        })
    }

    /// Applies `change` to the team's list as the server holds it now: read,
    /// open, change, write conditional on the revision just opened, and on a
    /// lost race do it again. Then keeps the result as the local copy.
    pub fn edit_manifest(
        &mut self,
        change: ManifestChange,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), TeamVaultError>> {
        let Some((team_id, keyring)) = self.team_keys() else {
            return Task::ready(Err(TeamVaultError::NoTeamKey));
        };
        let account = self.account.clone();
        cx.spawn(async move |this, cx| {
            let result = async {
                let context = context(&account, cx).await?;
                let manifest =
                    edit_manifest_entry(&this, &context, &team_id, &keyring, &change, cx).await?;
                this.update(cx, |this, cx| {
                    if let Some(team) = this.team.as_mut() {
                        team.manifest = manifest;
                        cx.notify();
                    }
                })
                .map_err(closed)
            }
            .await;
            this.update(cx, |this, cx| this.absorb(&result, cx)).ok();
            result
        })
    }

    /// Removes an entry. Admins only; the server enforces it too.
    pub fn delete(
        &mut self,
        entry: EntryId,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), TeamVaultError>> {
        let Some(team) = &self.team else {
            return Task::ready(Err(TeamVaultError::NoTeamKey));
        };
        if !team.role.can_administer() {
            return Task::ready(Err(ClientError::Rejected("admin_required".into()).into()));
        }
        let team_id = team.team_id.clone();
        let account = self.account.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<(), TeamVaultError> = async {
                let context = context(&account, cx).await?;
                client::delete_entry(&context, &team_id, &entry).await
            }
            .await;
            this.update(cx, |this, cx| {
                this.absorb(&result, cx);
                if result.is_ok()
                    && let Some(team) = this.team.as_mut()
                {
                    team.listing.retain(|listing| listing.entry_id != entry);
                    cx.notify();
                }
            })
            .ok();
            result
        })
    }

    /// Seals the current team key to a member whose key the user verified on
    /// this machine. Nothing is pinned here: a key nobody compared is refused.
    pub fn grant(
        &mut self,
        user_id: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), TeamVaultError>> {
        let Some(team) = &self.team else {
            return Task::ready(Err(TeamVaultError::NoTeamKey));
        };
        let (team_id, me, keyring) = (team.team_id.clone(), team.me.clone(), team.keyring.clone());
        let account = self.account.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<MemberKeys, TeamVaultError> = async {
                let context = context(&account, cx).await?;
                let listed = client::fetch_member_keys(&context, &team_id)
                    .await?
                    .ok_or_else(|| ClientError::Rejected("not_found".into()))?;
                let member = listed
                    .members
                    .iter()
                    .find(|member| member.user_id == user_id)
                    .ok_or_else(|| ClientError::Rejected("not_found".into()))?;
                let trust = trust_snapshot(&this, cx)?;
                let (key_version, envelope) =
                    prepare_grant(&trust, &team_id, &me, &keyring, member)?;
                match client::put_envelopes(&context, &team_id, key_version, &[envelope]).await? {
                    EnvelopeWrite::Stored => Ok(listed),
                    EnvelopeWrite::RecipientKeyChanged => Err(TeamVaultError::KeyChanged {
                        user_id: user_id.clone(),
                    }),
                    EnvelopeWrite::KeyVersionStale => Err(TeamVaultError::KeyVersionStale),
                    EnvelopeWrite::NotMember => {
                        Err(ClientError::Rejected("not_found".into()).into())
                    }
                }
            }
            .await;
            this.update(cx, |this, cx| {
                this.absorb(&result, cx);
                if let (Ok(listed), Some(team)) = (&result, this.team.as_mut()) {
                    team.members = listed.members.clone();
                    team.rotation_recommended = listed.rotation_recommended;
                    for member in &mut team.members {
                        if member.user_id == user_id {
                            member.has_current_envelope = true;
                        }
                    }
                    cx.notify();
                }
            })
            .ok();
            result.map(|_| ())
        })
    }

    /// The user compared this member's fingerprint with them, out of band,
    /// and trusts it: a first look (so a later rotation may seal to it), or a
    /// change they accept. One member at a time, by design: there is no
    /// "trust all".
    pub fn trust_member_key(
        &mut self,
        user_id: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), TeamVaultError>> {
        let Some(team) = &self.team else {
            return Task::ready(Err(TeamVaultError::NoTeamKey));
        };
        let Some((member, key)) = team
            .members
            .iter()
            .find(|member| member.user_id == user_id)
            .and_then(|member| member.key.as_ref().map(|key| (member, key)))
        else {
            return Task::ready(Err(TeamVaultError::NoSharingKey {
                user_id: user_id.to_string(),
            }));
        };
        let (team_id, email, fingerprint) =
            (team.team_id.clone(), member.email.clone(), key.fingerprint);
        let save = match self.trust_mut() {
            Ok(trust) => {
                trust.verify(&team_id, user_id, &email, fingerprint, now());
                self.persist_trust(cx)
            }
            Err(error) => return Task::ready(Err(error)),
        };
        cx.notify();
        // Trusting the member who sealed the current key is what the team was
        // waiting on: read the keys again once the pin is on disk.
        let reopen = matches!(
            &self.status,
            TeamVaultStatus::SealerNotVerified { user_id: sealer, .. } if sealer == user_id
        );
        cx.spawn(async move |this, cx| {
            save.await?;
            if reopen && let Ok(refresh) = this.update(cx, |this, cx| this.refresh(cx)) {
                refresh.await;
            }
            Ok(())
        })
    }

    /// Replaces a sharing key that no recovery key on this machine opens:
    /// a new key pair, sealed under this machine's recovery key, written over
    /// the old slot (conditional on the revision read), then registered.
    ///
    /// The server deletes every envelope sealed to the old key, in every team,
    /// so an admin must grant this member again. Offered only while the status
    /// is [`TeamVaultStatus::SharingKeyUnreadable`], and checked again here:
    /// a slot that opens is never replaced.
    pub fn replace_sharing_key(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), TeamVaultError>> {
        if self.status != TeamVaultStatus::SharingKeyUnreadable {
            return Task::ready(Err(TeamVaultError::Storage(
                "your sharing key opens on this machine; nothing to replace".into(),
            )));
        }
        let account = self.account.clone();
        self.set_status(TeamVaultStatus::Working, cx);
        cx.spawn(async move |this, cx| {
            let result = async {
                let context = context(&account, cx).await?;
                let credentials =
                    account.read_with(cx, |account, _| account.credentials_provider());
                let dek = zode_sync::keystore::read(&credentials, cx)
                    .await
                    .ok_or_else(|| {
                        TeamVaultError::Storage("this machine has no recovery key".into())
                    })?;
                replace_stored_sharing_key(&context, &dek).await
            }
            .await;
            match &result {
                Ok(()) => {
                    if let Ok(refresh) = this.update(cx, |this, cx| this.refresh(cx)) {
                        refresh.await;
                    }
                }
                // The caller shows why; the way out stays offered.
                Err(_) => {
                    this.update(cx, |this, cx| {
                        this.set_status(TeamVaultStatus::SharingKeyUnreadable, cx)
                    })
                    .ok();
                }
            }
            result
        })
    }

    /// Makes the team's first key (version 1) and seals it to this admin.
    /// Members are granted one by one afterwards, each after its pin check.
    pub fn create_team_key(&mut self, cx: &mut Context<Self>) -> Task<Result<(), TeamVaultError>> {
        let Some(team) = &self.team else {
            return Task::ready(Err(TeamVaultError::NoTeamKey));
        };
        if !team.role.can_administer() || !team.keyring.is_empty() {
            return Task::ready(Err(ClientError::Rejected("admin_required".into()).into()));
        }
        let (team_id, me) = (team.team_id.clone(), team.me.clone());
        let account = self.account.clone();
        cx.spawn(async move |this, cx| {
            let result: Result<(), TeamVaultError> = async {
                let context = context(&account, cx).await?;
                let my_user_id = context.credential.user_id.to_string();
                let team_key = TeamKey::generate()?;
                let wrapped_key = wrap(
                    &me,
                    me.public_key(),
                    &EnvelopeIds {
                        team_id: &team_id,
                        recipient_user_id: &my_user_id,
                        key_version: 1,
                    },
                    &team_key,
                )?;
                let envelope = client::OutgoingEnvelope {
                    user_id: my_user_id,
                    recipient_fingerprint: me.fingerprint(),
                    wrapped_key,
                };
                match client::put_envelopes(&context, &team_id, 1, &[envelope]).await? {
                    EnvelopeWrite::Stored => Ok(()),
                    EnvelopeWrite::KeyVersionStale => Err(TeamVaultError::KeyVersionStale),
                    EnvelopeWrite::RecipientKeyChanged => Err(TeamVaultError::KeyChanged {
                        user_id: context.credential.user_id.to_string(),
                    }),
                    EnvelopeWrite::NotMember => {
                        Err(ClientError::Rejected("not_found".into()).into())
                    }
                }
            }
            .await;
            let refresh = this.update(cx, |this, cx| {
                this.absorb(&result, cx);
                result.is_ok().then(|| this.refresh(cx))
            });
            if let Ok(Some(refresh)) = refresh {
                refresh.await;
            }
            result
        })
    }

    /// Moves the team to a new key and re-seals every older entry under it.
    ///
    /// Losing the race to another admin is not a failure: the keys are read
    /// again and, if the version moved, the re-seal carries on under the
    /// winner's key.
    pub fn rotate(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Task<Result<RotationReport, TeamVaultError>> {
        let Some(team) = &self.team else {
            return Task::ready(Err(TeamVaultError::NoTeamKey));
        };
        if !team.role.can_administer() {
            return Task::ready(Err(ClientError::Rejected("admin_required".into()).into()));
        }
        let (team_id, me, keyring) = (team.team_id.clone(), team.me.clone(), team.keyring.clone());
        let account = self.account.clone();
        self.set_status(TeamVaultStatus::Working, cx);
        cx.spawn(async move |this, cx| {
            let result = rotate_team(&this, &account, &team_id, &me, &keyring, cx).await;
            this.update(cx, |this, cx| {
                this.absorb(&result, cx);
                if result.is_ok() && this.status == TeamVaultStatus::Working {
                    this.set_status(TeamVaultStatus::Ready, cx);
                }
            })
            .ok();
            result
        })
    }

    /// Finishes a re-seal a rotation could not complete (a dropped
    /// connection, a closed laptop). Entries already under the current
    /// version are left alone.
    pub fn reseal(&mut self, cx: &mut Context<Self>) -> Task<Result<ResealReport, TeamVaultError>> {
        let Some(team) = &self.team else {
            return Task::ready(Err(TeamVaultError::NoTeamKey));
        };
        if !team.role.can_administer() {
            return Task::ready(Err(ClientError::Rejected("admin_required".into()).into()));
        }
        let (team_id, keyring) = (team.team_id.clone(), team.keyring.clone());
        let account = self.account.clone();
        cx.spawn(async move |this, cx| {
            let result = async {
                let context = context(&account, cx).await?;
                reseal_and_record(&this, &context, &team_id, &keyring, cx).await
            }
            .await;
            this.update(cx, |this, cx| this.absorb(&result, cx)).ok();
            result
        })
    }

    fn team_keys(&self) -> Option<(String, Arc<TeamKeyring>)> {
        self.team
            .as_ref()
            .filter(|team| !team.keyring.is_empty())
            .map(|team| (team.team_id.clone(), team.keyring.clone()))
    }

    fn trust_mut(&mut self) -> Result<&mut TeamTrust, TeamVaultError> {
        self.trust
            .as_mut()
            .map(|(_, trust)| trust)
            .ok_or_else(|| TeamVaultError::Storage("not loaded".into()))
    }

    fn persist_trust(&mut self, cx: &mut Context<Self>) -> SaveHandle {
        use futures::FutureExt as _;
        let Some((user_id, trust)) = &self.trust else {
            return Task::ready(Err(TeamVaultError::Storage("not loaded".into()))).shared();
        };
        let handle = pins::save(user_id, trust, self.last_save.take(), cx);
        self.last_save = Some(handle.clone());
        handle
    }

    fn account_changed(&mut self, cx: &mut Context<Self>) {
        let account = self.account.read(cx);
        let user_id = account.status().user().map(|user| user.id.to_string());
        let plan_team = account
            .plan()
            .map(|plan| plan.team.as_ref().map(|team| team.id.to_string()));

        // Another account, or none: nothing of the last one's stays.
        if self
            .trust
            .as_ref()
            .is_some_and(|(owner, _)| Some(owner) != user_id.as_ref())
        {
            self.trust = None;
            self.last_save = None;
            self.drop_team(cx);
        }
        if user_id.is_none() {
            self.refresh_after_plan = false;
            if self.team.is_some() {
                self.drop_team(cx);
            }
            return;
        }
        match &plan_team {
            Some(None) if self.team.is_some() => self.drop_team(cx),
            Some(Some(team_id))
                if self
                    .team
                    .as_ref()
                    .is_some_and(|team| &team.team_id != team_id) =>
            {
                self.drop_team(cx)
            }
            _ => {}
        }
        if self.refresh_after_plan && plan_team.is_some() {
            self.refresh_after_plan = false;
            self.refresh(cx).detach();
        }
    }

    /// A refresh waiting on the plan, whose read ended (or never started)
    /// without one, stops waiting and says so.
    fn plan_read_settled(&mut self, cx: &mut Context<Self>) {
        if !self.refresh_after_plan {
            return;
        }
        let account = self.account.read(cx);
        if account.plan().is_some() || account.is_reading_plan() {
            return;
        }
        self.refresh_after_plan = false;
        self.set_status(
            TeamVaultStatus::Failed(
                "Could not read your plan from Zode's service. Check your connection, then try again."
                    .into(),
            ),
            cx,
        );
    }

    /// Forgets the team and every key it held.
    fn drop_team(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.team = None;
        self.set_status(TeamVaultStatus::NoTeam, cx);
    }

    fn apply_load(&mut self, outcome: Result<ActiveTeam, Stop>, cx: &mut Context<Self>) {
        match outcome {
            Ok(team) => {
                let status = match (&team.unverified_sealer, team.keyring.is_empty()) {
                    (Some((user_id, email)), _) => TeamVaultStatus::SealerNotVerified {
                        user_id: user_id.clone(),
                        email: email.clone(),
                    },
                    (None, true) => TeamVaultStatus::KeyPending,
                    (None, false) => TeamVaultStatus::Ready,
                };
                self.team = Some(team);
                self.set_status(status, cx);
            }
            Err(Stop::NoTeam) => {
                self.drop_team(cx);
                // The server says the membership is gone; the plan should say so too.
                self.account
                    .update(cx, |account, cx| account.refresh_plan(cx));
            }
            Err(Stop::NeedsRecoveryKey) => {
                self.team = None;
                self.set_status(TeamVaultStatus::NeedsRecoveryKey, cx);
            }
            Err(Stop::SharingKeyUnreadable) => {
                self.team = None;
                self.set_status(TeamVaultStatus::SharingKeyUnreadable, cx);
            }
            Err(Stop::Error(error)) => {
                self.team = None;
                self.absorb::<()>(&Err(error), cx);
            }
        }
    }

    /// Turns an operation's failure into a status where one applies.
    fn absorb<T>(&mut self, result: &Result<T, TeamVaultError>, cx: &mut Context<Self>) {
        let Err(error) = result else {
            return;
        };
        let status = match error {
            TeamVaultError::Client(ClientError::PlanLimit(limit))
                if limit.reason == PlanLimitReason::TeamInactive =>
            {
                TeamVaultStatus::ReadOnly
            }
            TeamVaultError::Client(ClientError::PlanLimit(limit)) => {
                self.account
                    .update(cx, |account, cx| account.refresh_plan(cx));
                TeamVaultStatus::PlanLimited(limit.clone())
            }
            // Per-entry and per-member problems are the caller's to show;
            // they say nothing about the session as a whole.
            TeamVaultError::Conflict
            | TeamVaultError::KeyChanged { .. }
            | TeamVaultError::NotPinned { .. }
            | TeamVaultError::NoSharingKey { .. }
            | TeamVaultError::Crypto(TeamCryptoError::Rollback { .. })
                if self.team.is_some() =>
            {
                if self.status == TeamVaultStatus::Working {
                    TeamVaultStatus::Ready
                } else {
                    return;
                }
            }
            other => TeamVaultStatus::Failed(other.to_string().into()),
        };
        self.set_status(status, cx);
    }

    fn set_status(&mut self, status: TeamVaultStatus, cx: &mut Context<Self>) {
        if self.status != status {
            self.status = status;
            cx.emit(TeamVaultStatusChanged);
        }
        cx.notify();
    }
}

enum Stop {
    NoTeam,
    NeedsRecoveryKey,
    SharingKeyUnreadable,
    Error(TeamVaultError),
}

impl From<TeamVaultError> for Stop {
    fn from(error: TeamVaultError) -> Self {
        match error {
            TeamVaultError::SharingKeyUnreadable => Stop::SharingKeyUnreadable,
            other => Stop::Error(other),
        }
    }
}

impl From<TeamCryptoError> for Stop {
    fn from(error: TeamCryptoError) -> Self {
        Stop::Error(error.into())
    }
}

impl From<ClientError> for Stop {
    fn from(error: ClientError) -> Self {
        Stop::Error(error.into())
    }
}

/// The session entity went away while an operation was in flight.
fn closed(_: anyhow::Error) -> TeamVaultError {
    TeamVaultError::Storage("the session closed".into())
}

async fn load_team(
    this: &WeakEntity<TeamVaultSession>,
    account: &Entity<Account>,
    team: &TeamSummary,
    cx: &mut AsyncApp,
) -> Result<ActiveTeam, Stop> {
    let context = context(account, cx).await?;
    let user_id = context.credential.user_id.to_string();
    let team_id = team.id.to_string();
    ensure_trust(this, &user_id, cx).await?;

    let credentials = account.read_with(cx, |account, _| account.credentials_provider());
    let Some(dek) = zode_sync::keystore::read(&credentials, cx).await else {
        return Err(Stop::NeedsRecoveryKey);
    };
    let me = Arc::new(load_or_create_sharing_key(&context, &dek).await?);
    drop(dek);
    ensure_registered(&context, &me).await?;

    let answer = client::fetch_team_key(&context, &team_id).await?;
    let listed = client::fetch_member_keys(&context, &team_id)
        .await?
        .ok_or(Stop::NoTeam)?;
    let members = listed.members;
    let base = |keyring: TeamKeyring, listing, manifest, unverified_sealer| ActiveTeam {
        team_id: team_id.clone(),
        name: team.name.clone(),
        role: TeamRole::parse(&team.role),
        me: me.clone(),
        keyring: Arc::new(keyring),
        members: members.clone(),
        rotation_recommended: listed.rotation_recommended,
        listing,
        manifest,
        unverified_sealer,
    };

    let (current_version, envelopes) = match answer {
        KeyAnswer::NotMember => return Err(Stop::NoTeam),
        KeyAnswer::KeyPending => {
            return Ok(base(
                TeamKeyring::default(),
                Vec::new(),
                TeamManifest::new(),
                None,
            ));
        }
        KeyAnswer::Envelopes {
            current_version,
            envelopes,
        } => (current_version, envelopes),
    };

    // The server's word on the current version is checked before anything is
    // opened or sealed under it.
    let (keyring, problems, save) = this
        .update(cx, |this, cx| {
            let trust = this.trust_mut()?;
            let floor = trust.key_version_floor(&team_id);
            if current_version < floor {
                return Err(TeamVaultError::from(TeamCryptoError::KeyVersionRollback {
                    seen: floor,
                    got: current_version,
                }));
            }
            let (keyring, problems) =
                open_envelopes(trust, &team_id, &user_id, &me, &envelopes, &members, now());
            if let Some((opened_version, _)) = keyring.current() {
                trust.observe_key_version(&team_id, opened_version)?;
            }
            Ok((keyring, problems, this.persist_trust(cx)))
        })
        .map_err(closed)??;
    save.await?;

    if keyring.get(current_version).is_none() {
        // An envelope for the current version that did not open is an attack
        // or a fault, not a wait: say which. A sealer not verified here is
        // the user's to settle, so the team stays loaded (members, no keys)
        // for the members window to show and trust them.
        match problems
            .into_iter()
            .find(|(version, _)| *version == current_version)
        {
            Some((_, TeamVaultError::NotPinned { user_id })) => {
                let email = members
                    .iter()
                    .find(|member| member.user_id == user_id)
                    .map(|member| member.email.clone())
                    .unwrap_or_else(|| user_id.clone());
                return Ok(base(
                    TeamKeyring::default(),
                    Vec::new(),
                    TeamManifest::new(),
                    Some((user_id, email)),
                ));
            }
            Some((_, error)) => return Err(Stop::Error(error)),
            None => {
                return Ok(base(
                    TeamKeyring::default(),
                    Vec::new(),
                    TeamManifest::new(),
                    None,
                ));
            }
        }
    }
    for (version, problem) in &problems {
        log::warn!("team key version {version} could not be opened: {problem}");
    }

    let listing = client::list_vault(&context, &team_id)
        .await?
        .ok_or(Stop::NoTeam)?;
    let manifest_id = manifest_entry_id()?;
    let manifest = match listing
        .iter()
        .find(|listing| listing.entry_id == manifest_id)
    {
        None => TeamManifest::new(),
        Some(_) => match client::fetch_entry(&context, &team_id, &manifest_id).await? {
            None => TeamManifest::new(),
            Some(document) => {
                let opened = open_document(
                    this,
                    &team_id,
                    &keyring,
                    &manifest_id,
                    &document.blob,
                    document.key_version,
                    cx,
                )?;
                record_seq(this, &team_id, &manifest_id, opened.seq, cx)?.await?;
                TeamManifest::from_json(&opened.payload)?
            }
        },
    };
    Ok(base(keyring, listing, manifest, None))
}

async fn ensure_trust(
    this: &WeakEntity<TeamVaultSession>,
    user_id: &str,
    cx: &mut AsyncApp,
) -> Result<(), TeamVaultError> {
    let loaded = this
        .read_with(cx, |this, _| {
            this.trust
                .as_ref()
                .is_some_and(|(owner, _)| owner == user_id)
        })
        .map_err(closed)?;
    if loaded {
        return Ok(());
    }
    let trust = cx.update(|cx| pins::load(user_id, cx)).await?;
    this.update(cx, |this, _| {
        this.trust = Some((user_id.to_string(), trust))
    })
    .map_err(closed)
}

/// Opens the sharing private key from the user's own store, or makes one.
///
/// Two machines making one at once: the store's create precondition lets one
/// win, and the loser opens the winner's instead of keeping its own.
async fn load_or_create_sharing_key(
    context: &SyncContext,
    dek: &Dek,
) -> Result<SharingKeypair, TeamVaultError> {
    let user_id = &context.credential.user_id;
    let resource = Resource::sharing_key();
    let stored = sync_client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
    )
    .await?;
    if let Some(document) = stored {
        return open_stored_key(dek, user_id, &document.blob);
    }

    let keypair = SharingKeypair::generate()?;
    let blob =
        to_blob(&seal_private_key(dek, user_id, &keypair)?).map_err(TeamCryptoError::from)?;
    match sync_client::store(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
        &blob,
        Precondition::Create,
    )
    .await?
    {
        WriteOutcome::Stored { .. } => Ok(keypair),
        WriteOutcome::Conflict(document) => open_stored_key(dek, user_id, &document.blob),
        WriteOutcome::Gone => Err(ClientError::Rejected("not_found".into()).into()),
    }
}

fn open_stored_key(dek: &Dek, user_id: &str, blob: &str) -> Result<SharingKeypair, TeamVaultError> {
    let envelope = from_blob(blob).map_err(TeamCryptoError::from)?;
    open_private_key(dek, user_id, &envelope).map_err(|error| match error {
        // The recovery key on this machine is not the one the sharing key was
        // sealed under. Nothing is regenerated on our own: a new sharing key
        // would cost this member every envelope in every team.
        TeamCryptoError::KeyRotated | TeamCryptoError::WrongKey => {
            TeamVaultError::SharingKeyUnreadable
        }
        other => other.into(),
    })
}

async fn replace_stored_sharing_key(
    context: &SyncContext,
    dek: &Dek,
) -> Result<(), TeamVaultError> {
    let user_id = &context.credential.user_id;
    let resource = Resource::sharing_key();
    let stored = sync_client::fetch(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
    )
    .await?;
    if let Some(document) = &stored
        && open_stored_key(dek, user_id, &document.blob).is_ok()
    {
        return Err(TeamVaultError::Storage(
            "your sharing key opens on this machine; nothing was replaced".into(),
        ));
    }

    let keypair = SharingKeypair::generate()?;
    let blob =
        to_blob(&seal_private_key(dek, user_id, &keypair)?).map_err(TeamCryptoError::from)?;
    let precondition = match &stored {
        Some(document) => Precondition::Replace(&document.revision),
        None => Precondition::Create,
    };
    match sync_client::store(
        &context.http_client,
        &context.api_url,
        &context.credential.access_token,
        &resource,
        &blob,
        precondition,
    )
    .await?
    {
        WriteOutcome::Stored { .. } => ensure_registered(context, &keypair).await,
        // Another machine replaced it at the same moment: theirs stands.
        WriteOutcome::Conflict(_) | WriteOutcome::Gone => Err(TeamVaultError::Conflict),
    }
}

/// Makes sure the server holds this sharing key's public half.
async fn ensure_registered(
    context: &SyncContext,
    me: &SharingKeypair,
) -> Result<(), TeamVaultError> {
    let registration = match client::fetch_registered_key(context).await? {
        Some(registered) if registered.fingerprint == me.fingerprint() => return Ok(()),
        // The registered key is not one this account can open with, so the
        // envelopes made for it are already lost; the synced key wins.
        Some(registered) => {
            client::register_sharing_key(
                context,
                me.public_key(),
                KeyRegistration::Rotate(&registered.fingerprint),
            )
            .await?
        }
        None => {
            client::register_sharing_key(context, me.public_key(), KeyRegistration::Create).await?
        }
    };
    match registration {
        RegisterOutcome::Registered => Ok(()),
        RegisterOutcome::Conflict => match client::fetch_registered_key(context).await? {
            Some(registered) if registered.fingerprint == me.fingerprint() => Ok(()),
            _ => Err(ClientError::Rejected("sharing_key_conflict".into()).into()),
        },
    }
}

/// What is stored at an entry right now, opened.
struct Current {
    revision: String,
    seq: u64,
    payload: Zeroizing<Vec<u8>>,
}

async fn read_current(
    this: &WeakEntity<TeamVaultSession>,
    context: &SyncContext,
    team_id: &str,
    keyring: &TeamKeyring,
    entry: &EntryId,
    cx: &mut AsyncApp,
) -> Result<Option<Current>, TeamVaultError> {
    let Some(document) = client::fetch_entry(context, team_id, entry).await? else {
        return Ok(None);
    };
    let opened = open_document(
        this,
        team_id,
        keyring,
        entry,
        &document.blob,
        document.key_version,
        cx,
    )?;
    Ok(Some(Current {
        revision: document.revision,
        seq: opened.seq,
        payload: opened.payload,
    }))
}

async fn push_entry(
    this: &WeakEntity<TeamVaultSession>,
    context: &SyncContext,
    team_id: &str,
    keyring: &TeamKeyring,
    entry: &EntryId,
    payload: &[u8],
    base: &PushBase,
    cx: &mut AsyncApp,
) -> Result<(), TeamVaultError> {
    let current = read_current(this, context, team_id, keyring, entry, cx).await?;
    let expected = match (base, &current) {
        (PushBase::Create, None) | (PushBase::Latest, _) => true,
        (PushBase::Replace(seen), Some(current)) => *seen == current.revision,
        (PushBase::Create, Some(_)) | (PushBase::Replace(_), None) => false,
    };
    if !expected {
        return Err(TeamVaultError::Conflict);
    }
    if write_after(
        this,
        context,
        team_id,
        keyring,
        entry,
        payload,
        current.as_ref(),
        cx,
    )
    .await?
    {
        Ok(())
    } else {
        Err(TeamVaultError::Conflict)
    }
}

async fn edit_manifest_entry(
    this: &WeakEntity<TeamVaultSession>,
    context: &SyncContext,
    team_id: &str,
    keyring: &TeamKeyring,
    change: &ManifestChange,
    cx: &mut AsyncApp,
) -> Result<TeamManifest, TeamVaultError> {
    let entry = manifest_entry_id()?;
    for _ in 0..MANIFEST_ATTEMPTS {
        let current = read_current(this, context, team_id, keyring, &entry, cx).await?;
        let mut manifest = match &current {
            Some(current) => TeamManifest::from_json(&current.payload)?,
            None => TeamManifest::new(),
        };
        match change {
            ManifestChange::Upsert(named) => manifest.upsert(named.clone())?,
            ManifestChange::Remove(entry_id) => {
                if !manifest.remove(entry_id) {
                    return Ok(manifest);
                }
            }
        }
        let payload = Zeroizing::new(manifest.to_json()?);
        if write_after(
            this,
            context,
            team_id,
            keyring,
            &entry,
            &payload,
            current.as_ref(),
            cx,
        )
        .await?
        {
            return Ok(manifest);
        }
        // Someone changed the list between the read and the write: read it
        // again and apply the change to theirs.
    }
    Err(TeamVaultError::Conflict)
}

/// Seals `payload` as the next `seq` after `current` and stores it,
/// conditional on `current`'s revision (or on nothing being there). Answers
/// whether it landed; `false` is a lost race.
async fn write_after(
    this: &WeakEntity<TeamVaultSession>,
    context: &SyncContext,
    team_id: &str,
    keyring: &TeamKeyring,
    entry: &EntryId,
    payload: &[u8],
    current: Option<&Current>,
    cx: &mut AsyncApp,
) -> Result<bool, TeamVaultError> {
    let (key_version, team_key) = keyring.current().ok_or(TeamVaultError::NoTeamKey)?;
    let floor = this
        .read_with(cx, |this, _| {
            this.trust
                .as_ref()
                .map(|(_, trust)| trust.key_version_floor(team_id))
        })
        .map_err(closed)?
        .ok_or_else(|| TeamVaultError::Storage("not loaded".into()))?;
    if key_version < floor {
        return Err(TeamCryptoError::KeyVersionRollback {
            seen: floor,
            got: key_version,
        }
        .into());
    }

    let seen = seen_seq(this, team_id, entry, cx)?.unwrap_or(0);
    let seq = current.map_or(seen, |current| current.seq.max(seen)) + 1;
    let sealed = seal_entry(team_key, team_id, entry, key_version, seq, payload)?;
    let blob = to_blob(&sealed).map_err(TeamCryptoError::from)?;
    let precondition = match current {
        Some(current) => VaultPrecondition::Replace(&current.revision),
        None => VaultPrecondition::Create,
    };
    match client::store_entry(context, team_id, entry, &blob, key_version, precondition).await? {
        VaultWrite::Stored { revision } => {
            record_seq(this, team_id, entry, seq, cx)?.await?;
            this.update(cx, |this, cx| {
                if let Some(team) = this.team.as_mut() {
                    let listing = VaultListing {
                        entry_id: *entry,
                        revision,
                        key_version,
                        byte_length: blob.len() as u64,
                        updated_at: String::new(),
                        updated_by: context.credential.user_id.to_string(),
                    };
                    match team
                        .listing
                        .iter_mut()
                        .find(|existing| existing.entry_id == *entry)
                    {
                        Some(existing) => *existing = listing,
                        None => team.listing.push(listing),
                    }
                    cx.notify();
                }
            })
            .ok();
            Ok(true)
        }
        VaultWrite::Conflict(_) | VaultWrite::Gone => Ok(false),
        VaultWrite::KeyVersionStale => Err(TeamVaultError::KeyVersionStale),
    }
}

async fn rotate_team(
    this: &WeakEntity<TeamVaultSession>,
    account: &Entity<Account>,
    team_id: &str,
    me: &SharingKeypair,
    keyring: &TeamKeyring,
    cx: &mut AsyncApp,
) -> Result<RotationReport, TeamVaultError> {
    let context = context(account, cx).await?;
    let members = client::fetch_member_keys(&context, team_id)
        .await?
        .ok_or_else(|| ClientError::Rejected("not_found".into()))?
        .members;
    let trust = trust_snapshot(this, cx)?;
    let (from_version, _) = keyring.current().ok_or(TeamVaultError::NoTeamKey)?;

    let (keyring, rotated_here) =
        match rotate_key_version(&context, &trust, team_id, me, keyring, &members).await? {
            RotationStep::Rotated {
                key_version,
                team_key,
            } => {
                let mut next = keyring.clone();
                next.insert(key_version, team_key);
                (Arc::new(next), true)
            }
            RotationStep::LostRace => {
                // Read the keys again. If the version moved, another admin's
                // rotation did what this one set out to do; carry on from it.
                let refreshed = this
                    .update(cx, |this, cx| this.refresh(cx))
                    .map_err(closed)?;
                refreshed.await;
                let keyring = this
                    .read_with(cx, |this, _| {
                        this.team.as_ref().map(|team| team.keyring.clone())
                    })
                    .map_err(closed)?
                    .ok_or(TeamVaultError::NoTeamKey)?;
                match keyring.current() {
                    Some((version, _)) if version > from_version => (keyring, false),
                    _ => return Err(TeamVaultError::KeyVersionStale),
                }
            }
        };

    let (key_version, _) = keyring.current().ok_or(TeamVaultError::NoTeamKey)?;
    // The flag is the server's: it clears it with the compare-and-swap, and
    // sets it again if someone leaves afterwards. Read it, never assume it.
    let listed = client::fetch_member_keys(&context, team_id)
        .await?
        .ok_or_else(|| ClientError::Rejected("not_found".into()))?;
    let still_recommended = listed.rotation_recommended;
    let save = this
        .update(cx, |this, cx| {
            this.trust_mut()?
                .observe_key_version(team_id, key_version)?;
            if let Some(team) = this.team.as_mut() {
                team.keyring = keyring.clone();
                team.members = listed.members;
                team.rotation_recommended = still_recommended;
            }
            Ok::<_, TeamVaultError>(this.persist_trust(cx))
        })
        .map_err(closed)??;
    save.await?;

    let reseal = reseal_and_record(this, &context, team_id, &keyring, cx).await?;
    Ok(RotationReport {
        key_version,
        rotated_here,
        still_recommended,
        reseal,
    })
}

/// Re-seals entries still under older versions and records the `seq`s
/// written. Idempotent, so an interrupted pass is finished by running it again.
async fn reseal_and_record(
    this: &WeakEntity<TeamVaultSession>,
    context: &SyncContext,
    team_id: &str,
    keyring: &TeamKeyring,
    cx: &mut AsyncApp,
) -> Result<ResealReport, TeamVaultError> {
    let trust = trust_snapshot(this, cx)?;
    let reseal = reseal_older_entries(context, &trust, team_id, keyring).await?;
    let save = this
        .update(cx, |this, cx| {
            let trust = this.trust_mut()?;
            for (entry, seq) in &reseal.written {
                trust.record_seq(team_id, &entry.as_hex(), *seq);
            }
            Ok::<_, TeamVaultError>(this.persist_trust(cx))
        })
        .map_err(closed)??;
    save.await?;
    Ok(reseal)
}

fn open_document(
    this: &WeakEntity<TeamVaultSession>,
    team_id: &str,
    keyring: &TeamKeyring,
    entry: &EntryId,
    blob: &str,
    key_version: u32,
    cx: &mut AsyncApp,
) -> Result<crate::OpenedEntry, TeamVaultError> {
    let team_key = keyring.get(key_version).ok_or(TeamVaultError::NoTeamKey)?;
    let envelope = from_blob(blob).map_err(TeamCryptoError::from)?;
    let seen = seen_seq(this, team_id, entry, cx)?;
    Ok(open_entry(
        team_key,
        team_id,
        entry,
        key_version,
        seen,
        &envelope,
    )?)
}

fn seen_seq(
    this: &WeakEntity<TeamVaultSession>,
    team_id: &str,
    entry: &EntryId,
    cx: &mut AsyncApp,
) -> Result<Option<u64>, TeamVaultError> {
    this.read_with(cx, |this, _| {
        this.trust
            .as_ref()
            .map(|(_, trust)| trust.seen_seq(team_id, &entry.as_hex()))
            .ok_or_else(|| TeamVaultError::Storage("not loaded".into()))
    })
    .map_err(closed)?
}

fn record_seq(
    this: &WeakEntity<TeamVaultSession>,
    team_id: &str,
    entry: &EntryId,
    seq: u64,
    cx: &mut AsyncApp,
) -> Result<SaveHandle, TeamVaultError> {
    this.update(cx, |this, cx| {
        this.trust_mut()?.record_seq(team_id, &entry.as_hex(), seq);
        Ok(this.persist_trust(cx))
    })
    .map_err(closed)?
}

fn trust_snapshot(
    this: &WeakEntity<TeamVaultSession>,
    cx: &mut AsyncApp,
) -> Result<TeamTrust, TeamVaultError> {
    this.read_with(cx, |this, _| {
        this.trust.as_ref().map(|(_, trust)| trust.clone())
    })
    .map_err(closed)?
    .ok_or_else(|| TeamVaultError::Storage("not loaded".into()))
}

async fn context(
    account: &Entity<Account>,
    cx: &mut AsyncApp,
) -> Result<SyncContext, TeamVaultError> {
    // `api_credential` answers `None` for every reason not to proceed:
    // signed out, credential rejected, service unreachable.
    let credential = account
        .update(cx, |account, cx| account.api_credential(cx))
        .await
        .ok_or_else(no_credential)?;
    let (http_client, api_url) = account.read_with(cx, |account, _| {
        (account.http_client(), account.api_url().to_string())
    });
    Ok(SyncContext {
        http_client,
        api_url,
        credential,
    })
}

/// No credential to send. Usually offline or signed out, rarely a refused
/// session, so it does not claim the server refused anything.
pub(crate) fn no_credential() -> TeamVaultError {
    TeamVaultError::Client(ClientError::Unreachable(
        "check your connection, and that you are signed in".into(),
    ))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Installs the global [`TeamVaultSession`]. Reads nothing and sends nothing.
pub fn init(account: Entity<Account>, cx: &mut App) {
    let session = cx.new(|cx| TeamVaultSession::new(account, cx));
    TeamVaultSession::set_global(session, cx);
}
