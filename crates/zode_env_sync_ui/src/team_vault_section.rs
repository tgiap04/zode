use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{
    App, AppContext as _, Entity, PathPromptOptions, PromptLevel, SharedString, Subscription,
    WeakEntity, Window,
};
use ui::{
    CommonAnimationExt, ListItem, ListSeparator, ToggleButtonGroup, ToggleButtonSimple, Tooltip,
    prelude::*,
};
use workspace::Workspace;
use zeroize::{Zeroize as _, Zeroizing};
use zode_account::Account;
use zode_env_sync::{EntryId, resolve_within};
use zode_sync::TextDiff;
use zode_team_vault::{
    ManifestChange, ManifestEntry, ManifestEntryKind, MemberView, PushBase, TeamManifest, TeamRole,
    TeamVaultError, TeamVaultSession, TeamVaultStatus, VaultEntryView,
};

use crate::masking;
use crate::{ActiveEnvFile, OpenTeamMembers};

/// Said when the team vault is used in a window on another machine.
const TEAM_VAULT_ELSEWHERE: &str = "The team vault works with projects on this computer for now; open this project locally to pull or share its files.";

/// Which buttons the Team tab offers. The server enforces every one of these
/// again; this only keeps a member from being offered what will be refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TeamActions {
    pub pull: bool,
    pub share: bool,
    pub delete: bool,
    pub members: bool,
    pub create_key: bool,
    /// The stored sharing key does not open here; replacing it is the way out.
    pub replace_sharing_key: bool,
    /// Not in a team: the way to one is the Team plan on the website.
    pub team_plans: bool,
}

pub(crate) fn team_actions(
    status: &TeamVaultStatus,
    role: Option<TeamRole>,
    team_has_key: bool,
) -> TeamActions {
    let admin = role.is_some_and(TeamRole::can_administer);
    match status {
        TeamVaultStatus::Ready => TeamActions {
            pull: true,
            share: true,
            delete: admin,
            members: true,
            ..TeamActions::default()
        },
        // The plan lapsed: what was shared stays readable, nothing new goes up.
        TeamVaultStatus::ReadOnly => TeamActions {
            pull: true,
            members: true,
            ..TeamActions::default()
        },
        TeamVaultStatus::KeyPending => TeamActions {
            members: true,
            create_key: admin && !team_has_key,
            ..TeamActions::default()
        },
        // The members window is where the sealer's key is compared and trusted.
        TeamVaultStatus::SealerNotVerified { .. } => TeamActions {
            members: true,
            ..TeamActions::default()
        },
        TeamVaultStatus::SharingKeyUnreadable => TeamActions {
            replace_sharing_key: true,
            ..TeamActions::default()
        },
        TeamVaultStatus::NoTeam => TeamActions {
            team_plans: true,
            ..TeamActions::default()
        },
        TeamVaultStatus::NeedsRecoveryKey
        | TeamVaultStatus::Working
        | TeamVaultStatus::PlanLimited(_)
        | TeamVaultStatus::Failed(_) => TeamActions::default(),
    }
}

/// The line above the list, when the status has something to say.
pub(crate) fn team_notice(
    status: &TeamVaultStatus,
    role: Option<TeamRole>,
    team_has_key: bool,
) -> Option<(SharedString, Color)> {
    let admin = role.is_some_and(TeamRole::can_administer);
    Some(match status {
        TeamVaultStatus::Ready => return None,
        TeamVaultStatus::NoTeam => (
            "You are not in a team. The Team plan shares .env files and database \
             connections with your team, end-to-end encrypted."
                .into(),
            Color::Muted,
        ),
        TeamVaultStatus::NeedsRecoveryKey => (
            "Enter your recovery key first — Account → Enter Recovery Key…".into(),
            Color::Warning,
        ),
        TeamVaultStatus::Working => (status.sentence(), Color::Muted),
        TeamVaultStatus::KeyPending if admin && !team_has_key => (
            "This team has no key yet. Create it, then grant members one by one.".into(),
            Color::Muted,
        ),
        TeamVaultStatus::KeyPending => (
            "Waiting for an admin to grant you the team key.".into(),
            Color::Muted,
        ),
        TeamVaultStatus::ReadOnly
        | TeamVaultStatus::PlanLimited(_)
        | TeamVaultStatus::SealerNotVerified { .. }
        | TeamVaultStatus::SharingKeyUnreadable => (status.sentence(), Color::Warning),
        TeamVaultStatus::Failed(message) => (message.clone(), Color::Error),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TeamEntryRow {
    pub entry: EntryId,
    pub kind: Option<ManifestEntryKind>,
    pub label: SharedString,
    pub project_hint: Option<SharedString>,
    /// "by ada@example.com · 2026-11-03 10:00".
    pub updated: SharedString,
}

pub(crate) fn entry_rows(entries: &[VaultEntryView], members: &[MemberView]) -> Vec<TeamEntryRow> {
    let emails: HashMap<&str, &str> = members
        .iter()
        .map(|member| (member.user_id.as_str(), member.email.as_str()))
        .collect();
    let mut rows: Vec<TeamEntryRow> = entries
        .iter()
        .map(|entry| {
            let by = emails
                .get(entry.listing.updated_by.as_str())
                .copied()
                .unwrap_or("a former member");
            TeamEntryRow {
                entry: entry.listing.entry_id,
                kind: entry.manifest.as_ref().map(|named| named.kind),
                label: entry
                    .manifest
                    .as_ref()
                    .map(|named| SharedString::from(named.label.clone()))
                    .unwrap_or_else(|| "Unnamed entry, missing from the team's list".into()),
                project_hint: entry
                    .manifest
                    .as_ref()
                    .and_then(|named| named.project_hint.clone())
                    .map(Into::into),
                updated: format!("by {by} · {}", short_time(&entry.listing.updated_at)).into(),
            }
        })
        .collect();
    rows.sort_by(|left, right| left.label.cmp(&right.label));
    rows
}

/// `2026-11-03T10:15:42.000Z` as `2026-11-03 10:15`. Anything else is shown
/// as sent rather than guessed at.
fn short_time(iso: &str) -> String {
    match iso.get(..16) {
        Some(minutes) if iso.as_bytes().get(10) == Some(&b'T') => minutes.replacen('T', " ", 1),
        _ => iso.to_string(),
    }
}

/// Whether pulling `label` (a path relative to the project, chosen by
/// whoever shared it) must be confirmed before anything is written.
///
/// A teammate, or a server, names that path. Only `.env` and `.env.*`, and
/// nowhere inside `.git`, go straight in: an editor setting, a task file, a
/// hook or an `.envrc` is a route to running code on this machine, so it is
/// asked about even when the file is new and there is no diff to show.
pub(crate) fn pull_needs_confirmation(label: &str) -> bool {
    let mut components = label.split('/').rev();
    let basename = components.next().unwrap_or_default();
    let plain_env = basename == ".env" || basename.starts_with(".env.");
    !plain_env || components.any(|component| component == ".git")
}

/// The entry a share replaces: the same file name in the same project.
pub(crate) fn existing_env_entry(
    manifest: &TeamManifest,
    label: &str,
    project_hint: Option<&str>,
) -> Option<EntryId> {
    manifest
        .entries()
        .iter()
        .find(|entry| {
            entry.kind == ManifestEntryKind::Env
                && entry.label == label
                && entry.project_hint.as_deref() == project_hint
        })
        .map(|entry| entry.entry_id)
}

/// Whether the vault window offers a Team tab: once the plan names a team, or
/// the session already holds one. An unknown plan offers nothing.
pub(crate) fn team_tab_available(cx: &App) -> bool {
    let in_plan = Account::global(cx)
        .and_then(|account| account.read(cx).plan().map(|plan| plan.team.is_some()))
        .unwrap_or(false);
    in_plan || TeamVaultSession::global(cx).is_some_and(|team| team.read(cx).team_name().is_some())
}

/// "My files" and "Team". The vault window's only change for team sharing.
pub(crate) fn tab_bar(
    showing_team: bool,
    select: impl Fn(bool, &mut Window, &mut App) + Clone + 'static,
) -> impl IntoElement {
    let mine = select.clone();
    h_flex().w_full().p_1().child(
        ToggleButtonGroup::single_row(
            "env-vault-tabs",
            [
                ToggleButtonSimple::new("My Files", move |_, window, cx| mine(false, window, cx)),
                ToggleButtonSimple::new("Team", move |_, window, cx| select(true, window, cx)),
            ],
        )
        .selected_index(usize::from(showing_team)),
    )
}

/// Everything the Team tab draws, read from the session in one place so a
/// draw test can stand one up without a server behind it.
#[derive(Clone, Debug)]
pub(crate) struct TeamState {
    pub status: TeamVaultStatus,
    pub team_name: Option<SharedString>,
    pub role: Option<TeamRole>,
    /// Someone in the team holds a key: this machine, or any member.
    pub team_has_key: bool,
    pub rows: Vec<TeamEntryRow>,
}

impl TeamState {
    fn read(team: &TeamVaultSession) -> Self {
        let members = team.members();
        Self {
            status: team.status().clone(),
            team_name: team.team_name(),
            role: team.role(),
            team_has_key: !team.key_versions().is_empty()
                || members.iter().any(|member| member.has_current_envelope),
            rows: entry_rows(&team.list(), &members),
        }
    }
}

/// A change shown before it is made: the masked difference between what is
/// here and what the other side holds. Wiped when it is dropped.
struct PendingChange {
    /// Taken when the change is confirmed; `Drop` keeps it from being moved
    /// out otherwise.
    target: Option<PendingTarget>,
    diff: TextDiff,
}

enum PendingTarget {
    Pull {
        entry: EntryId,
        destination: PathBuf,
        payload: Zeroizing<Vec<u8>>,
        current: Zeroizing<Vec<u8>>,
    },
    Share {
        named: ManifestEntry,
        payload: Zeroizing<Vec<u8>>,
        /// The revision the diff was made against. The replace is refused if
        /// a teammate wrote after it.
        base: PushBase,
    },
}

impl Drop for PendingChange {
    fn drop(&mut self) {
        self.diff.unified.zeroize();
    }
}

/// The Team tab of the vault window: the team's shared `.env` files and
/// connections, decrypted here and nowhere else.
pub struct TeamVaultSection {
    team: Entity<TeamVaultSession>,
    workspace: WeakEntity<Workspace>,
    pending: Option<PendingChange>,
    message: Option<(SharedString, Color)>,
    #[cfg(test)]
    state_for_test: Option<TeamState>,
    _subscriptions: Vec<Subscription>,
}

impl TeamVaultSection {
    pub fn new(
        team: Entity<TeamVaultSession>,
        workspace: WeakEntity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Opening the tab is the moment the team is read; nothing reads it
        // before someone looks.
        team.update(cx, |team, cx| team.refresh(cx)).detach();
        let subscriptions = vec![cx.observe(&team, |_this, _team, cx| cx.notify())];
        Self {
            team,
            workspace,
            pending: None,
            message: None,
            #[cfg(test)]
            state_for_test: None,
            _subscriptions: subscriptions,
        }
    }

    fn state(&self, cx: &App) -> TeamState {
        #[cfg(test)]
        if let Some(state) = &self.state_for_test {
            return state.clone();
        }
        TeamState::read(self.team.read(cx))
    }

    fn say(&mut self, message: impl Into<SharedString>, color: Color, cx: &mut Context<Self>) {
        self.message = Some((message.into(), color));
        cx.notify();
    }

    fn fail(&mut self, error: &TeamVaultError, cx: &mut Context<Self>) {
        let sentence = match error {
            TeamVaultError::Conflict => {
                "Someone changed it first. Nothing was overwritten; try again to see their version."
                    .to_string()
            }
            other => other.to_string(),
        };
        self.say(sentence, Color::Error, cx);
    }

    fn active_root(&self, cx: &App) -> Option<PathBuf> {
        let workspace = self.workspace.upgrade()?;
        crate::active_checkout(workspace.read(cx), cx)
            .filter(|checkout| checkout.is_local())
            .map(|checkout| checkout.root().to_path_buf())
    }

    /// Whether the open project lives on another machine. The team vault
    /// reads and writes files on this one, so for a project on a WSL, SSH or
    /// dev container host it would put a team's secrets on the wrong disk.
    fn project_is_elsewhere(&self, cx: &App) -> bool {
        self.workspace
            .upgrade()
            .is_some_and(|workspace| !workspace.read(cx).project().read(cx).is_local())
    }

    /// Brings a shared file into the open project, at the path it was shared
    /// under. Asks for a folder only when no project is open.
    fn pull(&mut self, row: TeamEntryRow, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_is_elsewhere(cx) {
            self.say(TEAM_VAULT_ELSEWHERE, Color::Error, cx);
            return;
        }
        if let Some(root) = self.active_root(cx) {
            self.fetch_into(row, root, window, cx);
            return;
        }
        let folders = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            show_hidden: true,
            prompt: Some("Put the file here".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut chosen))) = folders.await else {
                return;
            };
            let Some(folder) = chosen.pop() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.fetch_into(row, folder, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn fetch_into(
        &mut self,
        row: TeamEntryRow,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let destination = match resolve_within(&root, &row.label) {
            Ok(destination) => destination,
            Err(error) => return self.say(error.to_string(), Color::Error, cx),
        };
        if pull_needs_confirmation(&row.label) {
            let answer = window.prompt(
                PromptLevel::Warning,
                &format!("Write {} into this project?", row.label),
                Some(&format!(
                    "Whoever shared it chose this path, and it is not a .env file. A file like this can change what your editor or tools run. Pulling writes\n{}",
                    destination.display()
                )),
                &["Write File", "Cancel"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                if answer.await != Ok(0) {
                    return;
                }
                this.update_in(cx, |this, window, cx| {
                    this.fetch_confirmed(row, destination, window, cx)
                })
                .ok();
            })
            .detach();
            return;
        }
        self.fetch_confirmed(row, destination, window, cx);
    }

    fn fetch_confirmed(
        &mut self,
        row: TeamEntryRow,
        destination: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending = None;
        self.say("Fetching from the team…", Color::Muted, cx);
        let pulled = self.team.update(cx, |team, cx| team.pull(row.entry, cx));
        cx.spawn_in(window, async move |this, cx| {
            let payload = match pulled.await {
                Ok(pulled) => pulled.payload,
                Err(error) => {
                    this.update(cx, |this, cx| this.fail(&error, cx)).ok();
                    return;
                }
            };
            let current = cx
                .background_spawn({
                    let destination = destination.clone();
                    async move { read_existing(&destination) }
                })
                .await;
            this.update(cx, |this, cx| match current {
                Err(error) => this.say(error.to_string(), Color::Error, cx),
                Ok(None) => this.write_pulled(row.entry, destination, payload, None, cx),
                Ok(Some(current)) if current == payload => this.say(
                    format!("{} already matches the team's copy.", row.label),
                    Color::Muted,
                    cx,
                ),
                Ok(Some(current)) => {
                    this.pending = Some(PendingChange {
                        diff: diff_of(&current, &payload),
                        target: Some(PendingTarget::Pull {
                            entry: row.entry,
                            destination,
                            payload,
                            current,
                        }),
                    });
                    this.message = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Writes a pulled file, copying what it replaces aside first, outside the
    /// project, as a personal fetch does.
    fn write_pulled(
        &mut self,
        entry: EntryId,
        destination: PathBuf,
        payload: Zeroizing<Vec<u8>>,
        current: Option<Zeroizing<Vec<u8>>>,
        cx: &mut Context<Self>,
    ) {
        let written = cx.background_spawn({
            let destination = destination.clone();
            async move {
                if let Some(current) = current {
                    zode_env_sync::back_up_secret(&paths::env_backups_dir(), &entry, &current)?;
                }
                zode_env_sync::write_secret_atomic(&destination, &payload)
            }
        });
        cx.spawn(async move |this, cx| {
            let outcome = written.await;
            this.update(cx, |this, cx| match outcome {
                Ok(()) => this.say(
                    format!("Written to {}.", destination.display()),
                    Color::Muted,
                    cx,
                ),
                Err(error) => this.say(error.to_string(), Color::Error, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Shares the `.env` open in the editor, or one picked from disk.
    fn share(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_is_elsewhere(cx) {
            self.say(TEAM_VAULT_ELSEWHERE, Color::Error, cx);
            return;
        }
        let active = self
            .workspace
            .upgrade()
            .and_then(|workspace| crate::active_env_file(workspace.read(cx), cx));
        if let Some(file) = active {
            self.prepare_share(file, window, cx);
            return;
        }
        let root = self.active_root(cx);
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            show_hidden: true,
            prompt: Some("Share with the team".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut chosen))) = paths.await else {
                return;
            };
            let Some(absolute) = chosen.pop() else {
                return;
            };
            let worktree_root = root
                .filter(|root| absolute.starts_with(root))
                .or_else(|| absolute.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| absolute.clone());
            let file = ActiveEnvFile::local(worktree_root, absolute);
            this.update_in(cx, |this, window, cx| this.prepare_share(file, window, cx))
                .ok();
        })
        .detach();
    }

    fn prepare_share(&mut self, file: ActiveEnvFile, window: &mut Window, cx: &mut Context<Self>) {
        let label = file.relative_label().to_string();
        let project_hint = file
            .checkout
            .root()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let existing = self
            .team
            .read(cx)
            .manifest()
            .and_then(|manifest| existing_env_entry(manifest, &label, project_hint.as_deref()));
        let team_name = self
            .state(cx)
            .team_name
            .unwrap_or_else(|| "your team".into());
        self.pending = None;

        let absolute = file.absolute;
        let read = cx.background_spawn(async move { std::fs::read(&absolute).map(Zeroizing::new) });
        cx.spawn_in(window, async move |this, cx| {
            let local = match read.await {
                Ok(local) => local,
                Err(error) => {
                    this.update(cx, |this, cx| this.say(error.to_string(), Color::Error, cx))
                        .ok();
                    return;
                }
            };
            let entry = match existing {
                Some(entry) => entry,
                None => match EntryId::generate() {
                    Ok(entry) => entry,
                    Err(error) => {
                        this.update(cx, |this, cx| this.say(error.to_string(), Color::Error, cx))
                            .ok();
                        return;
                    }
                },
            };
            let named = ManifestEntry {
                entry_id: entry,
                kind: ManifestEntryKind::Env,
                label: label.clone(),
                project_hint,
            };

            if existing.is_none() {
                let Ok(answer) = this.update_in(cx, |_this, window, cx| {
                    window.prompt(
                        PromptLevel::Info,
                        &format!("Share {label} with {team_name}?"),
                        Some(&format!(
                            "Everyone in {team_name} who holds the team key can read every value in it. It is encrypted on this machine; the server sees an opaque id."
                        )),
                        &["Share", "Cancel"],
                        cx,
                    )
                }) else {
                    return;
                };
                if answer.await != Ok(0) {
                    return;
                }
                this.update(cx, |this, cx| {
                    this.send_share(named, local, PushBase::Create, cx)
                })
                .ok();
                return;
            }

            // Replacing: what the team holds now is shown against what would
            // replace it, before anything is sent.
            let Ok(pulled) = this.update(cx, |this, cx| {
                this.team.update(cx, |team, cx| team.pull(entry, cx))
            }) else {
                return;
            };
            let remote = pulled.await;
            this.update(cx, |this, cx| match remote {
                Err(error) => this.fail(&error, cx),
                Ok(remote) if remote.payload == local => this.say(
                    format!("{team_name} already has this exact {label}."),
                    Color::Muted,
                    cx,
                ),
                Ok(remote) => {
                    this.pending = Some(PendingChange {
                        diff: diff_of(&remote.payload, &local),
                        target: Some(PendingTarget::Share {
                            named,
                            payload: local,
                            base: PushBase::Replace(remote.revision),
                        }),
                    });
                    this.message = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn send_share(
        &mut self,
        named: ManifestEntry,
        payload: Zeroizing<Vec<u8>>,
        base: PushBase,
        cx: &mut Context<Self>,
    ) {
        self.say(format!("Sharing {}…", named.label), Color::Muted, cx);
        let pushed = self
            .team
            .update(cx, |team, cx| team.push(named.entry_id, payload, base, cx));
        cx.spawn(async move |this, cx| {
            if let Err(error) = pushed.await {
                this.update(cx, |this, cx| this.fail(&error, cx)).ok();
                return;
            }
            let Ok(saved) = this.update(cx, |this, cx| {
                this.team.update(cx, |team, cx| {
                    team.edit_manifest(ManifestChange::Upsert(named.clone()), cx)
                })
            }) else {
                return;
            };
            let outcome = saved.await;
            this.update(cx, |this, cx| match outcome {
                Ok(()) => this.say(format!("Shared {}.", named.label), Color::Muted, cx),
                Err(error) => this.fail(&error, cx),
            })
            .ok();
        })
        .detach();
    }

    fn apply_pending(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self
            .pending
            .take()
            .and_then(|mut pending| pending.target.take())
        else {
            return;
        };
        match target {
            PendingTarget::Pull {
                entry,
                destination,
                payload,
                current,
            } => self.write_pulled(entry, destination, payload, Some(current), cx),
            PendingTarget::Share {
                named,
                payload,
                base,
            } => self.send_share(named, payload, base, cx),
        }
    }

    fn delete(&mut self, row: TeamEntryRow, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Remove {} from the team?", row.label),
            Some(
                "Every member loses access to it now. Copies already pulled stay on their machines.",
            ),
            &["Remove", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let Ok(deleted) = this.update(cx, |this, cx| {
                this.team.update(cx, |team, cx| team.delete(row.entry, cx))
            }) else {
                return;
            };
            if let Err(error) = deleted.await {
                this.update(cx, |this, cx| this.fail(&error, cx)).ok();
                return;
            }
            let Ok(saved) = this.update(cx, |this, cx| {
                this.team.update(cx, |team, cx| {
                    team.edit_manifest(ManifestChange::Remove(row.entry), cx)
                })
            }) else {
                return;
            };
            let outcome = saved.await;
            this.update(cx, |this, cx| match outcome {
                Ok(()) => this.say(format!("Removed {}.", row.label), Color::Muted, cx),
                Err(error) => this.fail(&error, cx),
            })
            .ok();
        })
        .detach();
    }

    fn create_key(&mut self, cx: &mut Context<Self>) {
        let created = self.team.update(cx, |team, cx| team.create_team_key(cx));
        cx.spawn(async move |this, cx| {
            let outcome = created.await;
            this.update(cx, |this, cx| match outcome {
                Ok(()) => this.say(
                    "The team key exists. Grant it to members from the members window.",
                    Color::Muted,
                    cx,
                ),
                Err(error) => this.fail(&error, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Asked with the platform's own alert, because it cannot be undone: every
    /// team key sealed to the old sharing key is lost.
    fn replace_sharing_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Critical,
            "Replace your sharing key?",
            Some(
                "Try your other recovery key first: Account → Enter Recovery Key…, if you have one that this sharing key was sealed under.\n\nReplacing makes a new sharing key. Every team key sealed to the old one is lost, in every team you are in: an admin of each team must check your new fingerprint and grant you again before you can open shared files.",
            ),
            &["Replace Sharing Key", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let Ok(replaced) = this.update(cx, |this, cx| {
                this.team.update(cx, |team, cx| team.replace_sharing_key(cx))
            }) else {
                return;
            };
            let outcome = replaced.await;
            this.update(cx, |this, cx| match outcome {
                Ok(()) => this.say(
                    "Your sharing key was replaced. Ask an admin to check your new fingerprint and grant you the team key again.",
                    Color::Warning,
                    cx,
                ),
                Err(error) => this.fail(&error, cx),
            })
            .ok();
        })
        .detach();
    }

    fn render_pending(&self, pending: &PendingChange, cx: &mut Context<Self>) -> impl IntoElement {
        let (headline, confirm) = match &pending.target {
            None => (String::new(), "Confirm"),
            Some(PendingTarget::Pull { destination, .. }) => (
                format!(
                    "{} differs from the team's copy. Replacing it copies the current one aside first, outside this project.",
                    destination.display()
                ),
                "Replace Local File",
            ),
            Some(PendingTarget::Share { named, .. }) => (
                format!(
                    "The team already has {}. Replacing it changes it for every member.",
                    named.label
                ),
                "Replace on Team",
            ),
        };
        let added_background = cx.theme().status().created_background;
        let removed_background = cx.theme().status().deleted_background;
        let mut lines = v_flex().w_full();
        for line in masking::mask_block(&pending.diff.unified).lines() {
            let (color, background) = match line.as_bytes().first() {
                Some(b'+') => (Color::Created, Some(added_background)),
                Some(b'-') => (Color::Deleted, Some(removed_background)),
                _ if line.starts_with("@@") => (Color::Accent, None),
                _ => (Color::Muted, None),
            };
            lines = lines.child(
                div()
                    .w_full()
                    .px_2()
                    .when_some(background, |this, background| this.bg(background))
                    .child(
                        Label::new(line.to_string())
                            .size(LabelSize::Small)
                            .buffer_font(cx)
                            .color(color),
                    ),
            );
        }
        v_flex()
            .w_full()
            .p_2()
            .gap_1()
            .child(Label::new(headline).size(LabelSize::Small))
            .child(
                Label::new(format!(
                    "{} added, {} removed. Values are masked.",
                    pending.diff.added, pending.diff.removed
                ))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
            )
            .child(
                div()
                    .id("team-vault-diff")
                    .w_full()
                    .max_h(rems(12.))
                    .overflow_y_scroll()
                    .child(lines),
            )
            .child(
                h_flex()
                    .gap_1()
                    .justify_end()
                    .child(Button::new("team-vault-pending-cancel", "Cancel").on_click(
                        cx.listener(|this, _, _window, cx| {
                            this.pending = None;
                            cx.notify();
                        }),
                    ))
                    .child(
                        Button::new("team-vault-pending-confirm", confirm)
                            .style(ButtonStyle::Filled)
                            .on_click(cx.listener(|this, _, _window, cx| this.apply_pending(cx))),
                    ),
            )
    }

    fn render_row(
        &self,
        row: &TeamEntryRow,
        actions: TeamActions,
        cx: &mut Context<Self>,
    ) -> ListItem {
        let id = row.entry.as_hex();
        let pullable = actions.pull && row.kind == Some(ManifestEntryKind::Env);
        let pull = pullable.then(|| {
            let row = row.clone();
            IconButton::new(
                SharedString::from(format!("team-vault-pull-{id}")),
                IconName::CloudDownload,
            )
            .icon_size(IconSize::Small)
            .tooltip(Tooltip::text(
                "Pull into this project. You see what changes before an existing file is replaced",
            ))
            .on_click(cx.listener(move |this, _, window, cx| this.pull(row.clone(), window, cx)))
        });
        let delete = actions.delete.then(|| {
            let row = row.clone();
            IconButton::new(
                SharedString::from(format!("team-vault-delete-{id}")),
                IconName::Trash,
            )
            .icon_size(IconSize::Small)
            .tooltip(Tooltip::text(
                "Remove it from the team. Copies already pulled stay put",
            ))
            .on_click(cx.listener(move |this, _, window, cx| this.delete(row.clone(), window, cx)))
        });
        let (icon, kind) = match row.kind {
            Some(ManifestEntryKind::Env) => (IconName::FileLock, None),
            Some(ManifestEntryKind::DbConnection) => {
                (IconName::Database, Some("database connection"))
            }
            None => (IconName::Warning, None),
        };
        let detail = [kind.map(SharedString::from), row.project_hint.clone()]
            .into_iter()
            .flatten()
            .chain([row.updated.clone()])
            .collect::<Vec<_>>()
            .join(" · ");
        ListItem::new(SharedString::from(format!("team-vault-entry-{id}")))
            .rounded()
            .start_slot(Icon::new(icon).size(IconSize::Small).color(Color::Muted))
            .child(
                v_flex()
                    .min_w_0()
                    .child(Label::new(row.label.clone()).size(LabelSize::Small))
                    .child(
                        Label::new(detail)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .end_slot(h_flex().gap_0p5().children(pull).children(delete))
    }
}

/// The file at `path`, or `None` when there is none yet.
fn read_existing(path: &Path) -> std::io::Result<Option<Zeroizing<Vec<u8>>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn diff_of(before: &[u8], after: &[u8]) -> TextDiff {
    let before = Zeroizing::new(String::from_utf8_lossy(before).into_owned());
    let after = Zeroizing::new(String::from_utf8_lossy(after).into_owned());
    zode_sync::diff::between(&before, &after)
}

impl Render for TeamVaultSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state(cx);
        let actions = team_actions(&state.status, state.role, state.team_has_key);
        let notice = team_notice(&state.status, state.role, state.team_has_key);
        let working = matches!(state.status, TeamVaultStatus::Working);
        let plan_limited = matches!(state.status, TeamVaultStatus::PlanLimited(_));
        let failed = matches!(state.status, TeamVaultStatus::Failed(_));
        let role = state.role.map(|role| match role {
            TeamRole::Owner => "owner",
            TeamRole::Admin => "admin",
            TeamRole::Member => "member",
        });

        let header = h_flex()
            .w_full()
            .px_2()
            .py_1()
            .gap_2()
            .child(Label::new(state.team_name.clone().unwrap_or_else(|| "Team".into())))
            .when_some(role, |this, role| {
                this.child(Label::new(role).size(LabelSize::XSmall).color(Color::Muted))
            })
            .child(div().flex_1())
            .when(actions.members, |this| {
                this.child(
                    Button::new("team-vault-members", "Members\u{2026}")
                        .start_icon(Icon::new(IconName::Person).size(IconSize::Small))
                        .tooltip(Tooltip::text(
                            "Who is in the team, their key fingerprints, granting and rotating the key",
                        ))
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenTeamMembers), cx)
                        }),
                )
            })
            .child(
                IconButton::new("team-vault-refresh", IconName::RotateCw)
                    .icon_size(IconSize::Small)
                    .disabled(working)
                    .tooltip(Tooltip::text("Read the team's entries again"))
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.team.update(cx, |team, cx| team.refresh(cx)).detach();
                    })),
            );

        let notice_line = notice.map(|(sentence, color)| {
            h_flex()
                .w_full()
                .p_2()
                .gap_1p5()
                .when(working, |this| {
                    this.child(
                        Icon::new(IconName::LoadCircle)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                            .with_rotate_animation(2),
                    )
                })
                .child(Label::new(sentence).size(LabelSize::Small).color(color))
                .child(div().flex_1())
                .when(plan_limited, |this| {
                    this.child(
                        Button::new("team-vault-upgrade", "Upgrade\u{2026}")
                            .style(ButtonStyle::Filled)
                            .tooltip(Tooltip::text("Opens the plans page in your browser"))
                            .on_click(|_, _window, cx| cx.open_url(&zode_account::pricing_url())),
                    )
                    .child(
                        Button::new("team-vault-manage-plan", "Manage Plan\u{2026}")
                            .tooltip(Tooltip::text("Opens your billing page in your browser"))
                            .on_click(|_, _window, cx| cx.open_url(&zode_account::billing_url())),
                    )
                })
                .when(actions.team_plans, |this| {
                    this.child(
                        Button::new("team-vault-team-plans", "See Team Plans\u{2026}")
                            .tooltip(Tooltip::text("Opens the plans page in your browser"))
                            .on_click(|_, _window, cx| cx.open_url(&zode_account::pricing_url())),
                    )
                })
                .when(actions.create_key, |this| {
                    this.child(
                        Button::new("team-vault-create-key", "Create Team Key")
                            .style(ButtonStyle::Filled)
                            .start_icon(Icon::new(IconName::LockOutlined).size(IconSize::Small))
                            .on_click(cx.listener(|this, _, _window, cx| this.create_key(cx))),
                    )
                })
                .when(actions.replace_sharing_key, |this| {
                    this.child(
                        Button::new(
                            "team-vault-replace-sharing-key",
                            "Replace My Sharing Key\u{2026}",
                        )
                        .tooltip(Tooltip::text(
                            "Makes a new sharing key; admins must grant you the team key again",
                        ))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.replace_sharing_key(window, cx)),
                        ),
                    )
                })
                .when(failed, |this| {
                    this.child(
                        Button::new("team-vault-retry", "Try Again").on_click(cx.listener(
                            |this, _, _window, cx| {
                                this.team.update(cx, |team, cx| team.refresh(cx)).detach();
                            },
                        )),
                    )
                })
        });

        let mut rows = v_flex().w_full().gap_0p5();
        let showing_entries = actions.pull || actions.share;
        if showing_entries {
            for row in &state.rows {
                rows = rows.child(self.render_row(row, actions, cx));
            }
            if state.rows.is_empty() {
                rows = rows.child(
                    div().p_2().child(
                        Label::new("Nothing is shared yet.")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
                );
            }
        }

        let pending = self
            .pending
            .as_ref()
            .map(|pending| self.render_pending(pending, cx));

        v_flex()
            .w_full()
            .debug_selector(|| "team-vault-section".into())
            .child(header)
            .children(notice_line)
            .when_some(pending, |this, pending| {
                this.child(ListSeparator).child(pending).child(ListSeparator)
            })
            .child(
                div()
                    .id("team-vault-body")
                    .w_full()
                    .max_h(rems(18.))
                    .p_1()
                    .overflow_y_scroll()
                    .child(rows),
            )
            .when(actions.share, |this| {
                this.child(
                    h_flex().w_full().p_2().child(
                        Button::new("team-vault-share", "Share a .env File\u{2026}")
                            .start_icon(Icon::new(IconName::Plus).size(IconSize::Small))
                            .tooltip(Tooltip::text(
                                "Shares the .env open in the editor, or one you pick. Replacing one shows the difference first",
                            ))
                            .on_click(cx.listener(|this, _, window, cx| this.share(window, cx))),
                    ),
                )
            })
            .when_some(self.message.clone(), |this, (message, color)| {
                this.child(ListSeparator).child(
                    h_flex()
                        .w_full()
                        .p_2()
                        .child(Label::new(message).size(LabelSize::Small).color(color)),
                )
            })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use gpui::TestAppContext;
    use zode_account::{AccountStatus, AccountUser, PlanLimit};
    use zode_team_vault::client::VaultListing;

    const ADMIN: Option<TeamRole> = Some(TeamRole::Admin);
    const MEMBER: Option<TeamRole> = Some(TeamRole::Member);

    /// Someone on Free or Pro opening the Team tab was told only that they
    /// are not in a team, with no way from there to the plan that puts them in
    /// one. Only that state offers it: inside a team it would be an advert in
    /// the way of the work.
    #[test]
    fn only_someone_without_a_team_is_shown_the_team_plans() {
        assert!(team_actions(&TeamVaultStatus::NoTeam, None, false).team_plans);
        for status in [
            TeamVaultStatus::Ready,
            TeamVaultStatus::ReadOnly,
            TeamVaultStatus::KeyPending,
            TeamVaultStatus::NeedsRecoveryKey,
            TeamVaultStatus::Working,
        ] {
            assert!(
                !team_actions(&status, MEMBER, true).team_plans,
                "{status:?} is someone already in a team or on the way in"
            );
        }
        let (sentence, _) = team_notice(&TeamVaultStatus::NoTeam, None, false).expect("said");
        assert!(
            sentence.contains("Team plan"),
            "the sentence says what the button leads to: {sentence}"
        );
    }

    #[test]
    fn a_member_never_sees_delete_and_an_admin_does() {
        let member = team_actions(&TeamVaultStatus::Ready, MEMBER, true);
        assert!(member.pull && member.share && member.members);
        assert!(!member.delete, "{member:?}");

        for role in [Some(TeamRole::Admin), Some(TeamRole::Owner)] {
            let admin = team_actions(&TeamVaultStatus::Ready, role, true);
            assert!(admin.pull && admin.share && admin.delete && admin.members);
        }
    }

    #[test]
    fn a_lapsed_team_is_read_only_and_keeps_pull() {
        for role in [MEMBER, ADMIN] {
            let actions = team_actions(&TeamVaultStatus::ReadOnly, role, true);
            assert!(actions.pull, "pull survives a lapse");
            assert!(!actions.share && !actions.delete && !actions.create_key);
        }
        let (sentence, color) =
            team_notice(&TeamVaultStatus::ReadOnly, MEMBER, true).expect("a lapse is said");
        assert_eq!(color, Color::Warning);
        assert!(sentence.contains("read-only"), "{sentence}");
    }

    #[test]
    fn waiting_for_the_key_offers_nothing_but_says_so() {
        let actions = team_actions(&TeamVaultStatus::KeyPending, MEMBER, true);
        assert!(!actions.pull && !actions.share && !actions.delete && !actions.create_key);
        assert!(actions.members, "who holds the key is still worth seeing");
        let (sentence, _) =
            team_notice(&TeamVaultStatus::KeyPending, MEMBER, true).expect("a wait is said");
        assert_eq!(sentence, "Waiting for an admin to grant you the team key.");
    }

    /// A teammate names the file a pull writes. Only `.env` and `.env.*`
    /// go straight in; anything else (an editor setting, a task file, a git
    /// hook, `.envrc`) is a route to running code and is confirmed first,
    /// even when the file is new and there is no diff to show.
    #[test]
    fn only_an_env_file_is_pulled_without_asking() {
        for plain in [
            ".env",
            ".env.local",
            "apps/api/.env.production",
            "a/b/.env.test",
        ] {
            assert!(!pull_needs_confirmation(plain), "{plain}");
        }
        for risky in [
            ".zed/settings.json",
            ".vscode/tasks.json",
            ".envrc",
            "package.json",
            ".git/hooks/.env.pre-commit",
            "certs/server.pem",
            "",
            ".env/",
        ] {
            assert!(pull_needs_confirmation(risky), "{risky}");
        }
    }

    /// The only way out of an unopenable sharing key is offered, and the
    /// notice says what it costs before anyone presses it.
    #[test]
    fn an_unreadable_sharing_key_offers_its_replacement_and_says_the_cost() {
        let actions = team_actions(&TeamVaultStatus::SharingKeyUnreadable, MEMBER, true);
        assert!(actions.replace_sharing_key);
        assert!(!actions.pull && !actions.share && !actions.delete && !actions.create_key);
        let (sentence, color) =
            team_notice(&TeamVaultStatus::SharingKeyUnreadable, MEMBER, true).expect("said");
        assert_eq!(color, Color::Warning);
        assert!(
            sentence.contains("recovery key") && sentence.contains("grant you"),
            "{sentence}"
        );
        assert!(!team_actions(&TeamVaultStatus::Ready, ADMIN, true).replace_sharing_key);
    }

    #[test]
    fn an_unverified_sealer_offers_the_members_window_and_says_why() {
        let status = TeamVaultStatus::SealerNotVerified {
            user_id: "u".into(),
            email: "cy@example.com".into(),
        };
        let actions = team_actions(&status, MEMBER, true);
        assert!(
            actions.members,
            "the sealer is trusted from the members window"
        );
        assert!(!actions.pull && !actions.share && !actions.delete && !actions.create_key);
        let (sentence, color) = team_notice(&status, MEMBER, true).expect("said");
        assert_eq!(color, Color::Warning);
        assert!(
            sentence.contains("cy@example.com") && sentence.contains("not verified"),
            "{sentence}"
        );
    }

    #[test]
    fn only_an_admin_of_a_team_without_any_key_creates_one() {
        assert!(team_actions(&TeamVaultStatus::KeyPending, ADMIN, false).create_key);
        assert!(
            !team_actions(&TeamVaultStatus::KeyPending, ADMIN, true).create_key,
            "a team that has a key needs a grant, not a second key"
        );
        assert!(!team_actions(&TeamVaultStatus::KeyPending, MEMBER, false).create_key);
        let (sentence, _) = team_notice(&TeamVaultStatus::KeyPending, ADMIN, false).expect("said");
        assert!(sentence.contains("no key yet"), "{sentence}");
    }

    #[test]
    fn every_other_status_offers_nothing_and_says_something() {
        let limited = TeamVaultStatus::PlanLimited(PlanLimit::from_body(
            r#"{"error":"team_vault_not_in_plan"}"#,
            &zode_account::web_url(),
        ));
        for status in [
            TeamVaultStatus::NoTeam,
            TeamVaultStatus::NeedsRecoveryKey,
            TeamVaultStatus::Working,
            limited,
            TeamVaultStatus::Failed("the service is unreachable".into()),
        ] {
            let actions = team_actions(&status, ADMIN, true);
            assert!(
                !actions.pull && !actions.share && !actions.delete && !actions.create_key,
                "{status:?} offers {actions:?}"
            );
            let notice = team_notice(&status, ADMIN, true);
            assert!(
                notice
                    .as_ref()
                    .is_some_and(|(sentence, _)| !sentence.is_empty()),
                "{status:?} says nothing"
            );
        }
        assert_eq!(team_notice(&TeamVaultStatus::Ready, ADMIN, true), None);
    }

    fn id(byte: u8) -> EntryId {
        EntryId::parse(&format!("{byte:02x}").repeat(16)).expect("valid id")
    }

    fn view(byte: u8, manifest: Option<ManifestEntry>, by: &str) -> VaultEntryView {
        VaultEntryView {
            listing: VaultListing {
                entry_id: id(byte),
                revision: "1".into(),
                key_version: 1,
                byte_length: 4096,
                updated_at: "2026-11-03T10:15:42.000Z".into(),
                updated_by: by.into(),
            },
            manifest,
        }
    }

    fn named(byte: u8, kind: ManifestEntryKind, label: &str, hint: Option<&str>) -> ManifestEntry {
        ManifestEntry {
            entry_id: id(byte),
            kind,
            label: label.into(),
            project_hint: hint.map(Into::into),
        }
    }

    fn ada() -> MemberView {
        MemberView {
            user_id: "u-ada".into(),
            email: "ada@example.com".into(),
            fingerprint: None,
            pin: None,
            has_current_envelope: true,
            is_me: true,
        }
    }

    #[test]
    fn rows_name_the_entry_its_project_and_who_wrote_it_when() {
        let entries = [
            view(
                0xb2,
                Some(named(
                    0xb2,
                    ManifestEntryKind::Env,
                    ".env.production",
                    Some("acme-api"),
                )),
                "u-ada",
            ),
            view(
                0xa1,
                Some(named(
                    0xa1,
                    ManifestEntryKind::DbConnection,
                    "staging",
                    None,
                )),
                "u-gone",
            ),
            view(0xc3, None, "u-ada"),
        ];
        let rows = entry_rows(&entries, &[ada()]);
        assert_eq!(rows.len(), 3);

        let env = rows
            .iter()
            .find(|row| row.entry == id(0xb2))
            .expect("env row");
        assert_eq!(env.label, ".env.production");
        assert_eq!(env.project_hint, Some(SharedString::from("acme-api")));
        assert_eq!(env.kind, Some(ManifestEntryKind::Env));
        assert_eq!(env.updated, "by ada@example.com · 2026-11-03 10:15");

        let connection = rows
            .iter()
            .find(|row| row.entry == id(0xa1))
            .expect("db row");
        assert!(
            connection.updated.starts_with("by a former member"),
            "{}",
            connection.updated
        );

        let unnamed = rows
            .iter()
            .find(|row| row.entry == id(0xc3))
            .expect("unnamed row");
        assert_eq!(
            unnamed.kind, None,
            "an entry missing from the catalogue is not guessed at"
        );
        assert!(!unnamed.label.is_empty());

        let labels: Vec<_> = rows.iter().map(|row| row.label.to_string()).collect();
        let mut sorted = labels.clone();
        sorted.sort();
        assert_eq!(labels, sorted, "rows are in a stable order");
    }

    #[test]
    fn a_share_replaces_the_same_file_in_the_same_project_only() {
        let mut manifest = TeamManifest::new();
        manifest
            .upsert(named(
                0xa1,
                ManifestEntryKind::Env,
                ".env",
                Some("acme-api"),
            ))
            .expect("upsert");
        manifest
            .upsert(named(
                0xb2,
                ManifestEntryKind::DbConnection,
                ".env",
                Some("acme-api"),
            ))
            .expect("upsert");

        assert_eq!(
            existing_env_entry(&manifest, ".env", Some("acme-api")),
            Some(id(0xa1))
        );
        assert_eq!(
            existing_env_entry(&manifest, ".env", Some("acme-web")),
            None
        );
        assert_eq!(
            existing_env_entry(&manifest, ".env.local", Some("acme-api")),
            None
        );
        assert_eq!(existing_env_entry(&manifest, ".env", None), None);
    }

    pub(crate) fn init_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
        });
    }

    /// A signed-in account with no client behind it, and its team session.
    /// Nothing here can reach a server: the plan is unknown, so the session's
    /// first refresh parks waiting for it.
    pub(crate) fn team_session(cx: &mut TestAppContext) -> Entity<TeamVaultSession> {
        cx.update(|cx| {
            let account = cx.new(|_| {
                Account::for_test(AccountStatus::SignedIn(AccountUser {
                    id: "u-ada".into(),
                    email: "ada@example.com".into(),
                    name: None,
                    avatar_url: None,
                }))
            });
            cx.new(|cx| TeamVaultSession::new(account, cx))
        })
    }

    fn state(status: TeamVaultStatus, role: Option<TeamRole>) -> TeamState {
        let entries = [
            view(
                0xb2,
                Some(named(
                    0xb2,
                    ManifestEntryKind::Env,
                    ".env.production",
                    Some("acme-api"),
                )),
                "u-ada",
            ),
            view(
                0xa1,
                Some(named(
                    0xa1,
                    ManifestEntryKind::DbConnection,
                    "staging",
                    None,
                )),
                "u-gone",
            ),
            view(0xc3, None, "u-ada"),
        ];
        TeamState {
            status,
            team_name: Some("Acme".into()),
            role,
            team_has_key: true,
            rows: entry_rows(&entries, &[ada()]),
        }
    }

    #[gpui::test]
    fn the_team_tab_draws_in_every_state(cx: &mut TestAppContext) {
        init_theme(cx);
        let team = team_session(cx);
        let (section, cx) = cx.add_window_view(|_window, cx| {
            TeamVaultSection::new(team, WeakEntity::new_invalid(), cx)
        });
        cx.run_until_parked();

        let limited = TeamVaultStatus::PlanLimited(PlanLimit::from_body(
            r#"{"error":"team_vault_not_in_plan"}"#,
            &zode_account::web_url(),
        ));
        for (status, role) in [
            (TeamVaultStatus::Ready, ADMIN),
            (TeamVaultStatus::Ready, MEMBER),
            (TeamVaultStatus::ReadOnly, MEMBER),
            (TeamVaultStatus::KeyPending, MEMBER),
            (TeamVaultStatus::KeyPending, ADMIN),
            (TeamVaultStatus::Working, MEMBER),
            (TeamVaultStatus::NeedsRecoveryKey, MEMBER),
            (TeamVaultStatus::SharingKeyUnreadable, MEMBER),
            (
                TeamVaultStatus::SealerNotVerified {
                    user_id: "u".into(),
                    email: "cy@example.com".into(),
                },
                MEMBER,
            ),
            (limited, ADMIN),
            (
                TeamVaultStatus::Failed("the service is unreachable".into()),
                ADMIN,
            ),
        ] {
            section.update(cx, |section, cx| {
                section.state_for_test = Some(state(status.clone(), role));
                cx.notify();
            });
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            assert!(
                cx.debug_bounds("team-vault-section").is_some(),
                "{status:?} must draw"
            );
        }
    }

    #[gpui::test]
    fn a_difference_waiting_for_confirmation_draws_and_cancel_drops_it(cx: &mut TestAppContext) {
        init_theme(cx);
        let team = team_session(cx);
        let (section, cx) = cx.add_window_view(|_window, cx| {
            TeamVaultSection::new(team, WeakEntity::new_invalid(), cx)
        });
        cx.run_until_parked();

        section.update(cx, |section, cx| {
            section.state_for_test = Some(state(TeamVaultStatus::Ready, MEMBER));
            section.pending = Some(PendingChange {
                diff: diff_of(b"API_KEY=old-secret\n", b"API_KEY=new-secret\n"),
                target: Some(PendingTarget::Share {
                    named: named(0xb2, ManifestEntryKind::Env, ".env", Some("acme-api")),
                    payload: Zeroizing::new(b"API_KEY=new-secret\n".to_vec()),
                    base: PushBase::Replace("rev-1".into()),
                }),
            });
            cx.notify();
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert!(cx.debug_bounds("team-vault-section").is_some());

        section.update(cx, |section, cx| {
            let pending = section.pending.as_ref().expect("still waiting");
            assert_eq!((pending.diff.added, pending.diff.removed), (1, 1));
            section.pending = None;
            cx.notify();
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// The team vault writes and reads files on this machine, so in a window
    /// whose project lives elsewhere -- a WSL distro, an SSH host -- a pull
    /// would put a team's secret on the wrong disk. Both actions refuse and
    /// say why instead.
    #[gpui::test]
    async fn a_project_on_another_machine_neither_pulls_nor_shares(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });
        let fs = fs::FakeFs::new(cx.executor());
        fs::Fs::create_dir(fs.as_ref(), std::path::Path::new("/root"))
            .await
            .expect("the project folder");
        let project = project::Project::test(fs.clone(), ["/root".as_ref()], cx).await;
        project.update(cx, |project, _| project.mark_as_collab_for_testing());
        let window = cx.add_window(|window, cx| {
            workspace::MultiWorkspace::test_new(project.clone(), window, cx)
        });
        let workspace = window
            .read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone())
            .expect("the window is open");
        let team = team_session(cx);
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        let section = cx.new(|cx| TeamVaultSection::new(team, workspace.downgrade(), cx));

        section.update_in(cx, |section, window, cx| section.share(window, cx));
        section.read_with(cx, |section, _| {
            assert_eq!(
                section
                    .message
                    .as_ref()
                    .map(|(message, _)| message.as_ref()),
                Some(TEAM_VAULT_ELSEWHERE),
                "a share must refuse rather than read a file on this machine"
            );
        });

        section.update(cx, |section, _| section.message = None);
        let row = TeamEntryRow {
            entry: EntryId::parse(&"c3".repeat(16)).expect("a valid entry id"),
            kind: Some(ManifestEntryKind::Env),
            label: ".env".into(),
            project_hint: None,
            updated: "".into(),
        };
        section.update_in(cx, |section, window, cx| section.pull(row, window, cx));
        cx.run_until_parked();
        section.read_with(cx, |section, _| {
            assert_eq!(
                section
                    .message
                    .as_ref()
                    .map(|(message, _)| message.as_ref()),
                Some(TEAM_VAULT_ELSEWHERE),
                "a pull must refuse rather than write into this machine"
            );
        });
        assert!(
            fs::Fs::load(fs.as_ref(), std::path::Path::new("/root/.env"))
                .await
                .is_err(),
            "nothing may be written into the project"
        );
    }
}
