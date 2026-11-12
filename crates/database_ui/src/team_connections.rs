//! The team's shared connections: a "Team" group in the tree, opening one with
//! the password a teammate chose to share, and sharing a personal one.
//!
//! A shared connection keeps the shape of a personal one. It holds no secret
//! in settings and never reaches them; its password, when the sharer included
//! one, goes into the keychain under the connection's URL the first time it
//! connects -- the same place `Set Password…` puts it.

use crate::connection_store::{ConnectionConfig, read_secret, write_secret};
use crate::database_panel::{ConnectionNode, DatabasePanel, NodeState};
use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, SharedString, Subscription,
    Window,
};
use ui::{Checkbox, Modal, ModalFooter, ModalHeader, Section, ToggleState, prelude::*};
use workspace::ModalView;
use zeroize::Zeroizing;
use zode_account::Account;
use zode_env_sync::EntryId;
use zode_team_vault::{
    ConnectionEntry, ManifestChange, ManifestEntry, ManifestEntryKind, PushBase, TeamManifest,
    TeamVaultSession, TeamVaultStatus, VaultEntryView,
};

/// Where a team node came from, and the password shared with it until the
/// keychain has it.
pub(crate) struct TeamOrigin {
    pub(crate) entry: EntryId,
    /// The listing's revision when the node was made: a re-share by an admin
    /// changes it, and the node is rebuilt rather than opened with stale
    /// settings.
    pub(crate) revision: String,
    /// False until the entry has been pulled and opened on this machine.
    pub(crate) fetched: bool,
    pub(crate) shared_password: Option<Zeroizing<String>>,
}

/// What a team connection opens with, and what is written to the keychain
/// once it has worked.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SecretChoice {
    pub(crate) secret: Option<String>,
    pub(crate) remember: Option<Zeroizing<String>>,
}

/// A password already in the keychain wins: it is this person's own, typed
/// here. A shared one is used when there is none and is kept only once a
/// connection with it succeeded. With neither, the connection opens without
/// one and the driver's refusal asks for it, as for any connection.
pub(crate) fn choose_secret(
    stored: Option<String>,
    shared: Option<Zeroizing<String>>,
) -> SecretChoice {
    match (stored, shared) {
        (Some(stored), _) => SecretChoice {
            secret: Some(stored),
            remember: None,
        },
        (None, Some(shared)) => SecretChoice {
            secret: Some(shared.to_string()),
            remember: Some(shared),
        },
        (None, None) => SecretChoice {
            secret: None,
            remember: None,
        },
    }
}

/// The team's connections as (entry, name, revision), by name.
pub(crate) fn shared_connections(entries: &[VaultEntryView]) -> Vec<(EntryId, String, String)> {
    let mut connections: Vec<_> = entries
        .iter()
        .filter_map(|entry| {
            let named = entry.manifest.as_ref()?;
            (named.kind == ManifestEntryKind::DbConnection).then(|| {
                (
                    entry.listing.entry_id,
                    named.label.clone(),
                    entry.listing.revision.clone(),
                )
            })
        })
        .collect();
    connections.sort_by(|left, right| left.1.cmp(&right.1));
    connections
}

/// The entry a share of `name` replaces: a connection of the same name.
pub(crate) fn existing_connection(manifest: &TeamManifest, name: &str) -> Option<EntryId> {
    manifest
        .entries()
        .iter()
        .find(|entry| entry.kind == ManifestEntryKind::DbConnection && entry.label == name)
        .map(|entry| entry.entry_id)
}

/// The line under the Team group header when it has no connections to list.
pub(crate) fn group_note(
    status: &TeamVaultStatus,
    loaded: bool,
    connections: usize,
) -> Option<SharedString> {
    match status {
        TeamVaultStatus::Working => Some("Loading…".into()),
        TeamVaultStatus::KeyPending => {
            Some("Waiting for an admin to grant you the team key".into())
        }
        TeamVaultStatus::Ready | TeamVaultStatus::ReadOnly if connections == 0 => {
            Some("No shared connections".into())
        }
        TeamVaultStatus::Ready | TeamVaultStatus::ReadOnly => None,
        TeamVaultStatus::NoTeam if !loaded => {
            Some("Press reload to load shared connections".into())
        }
        other => Some(other.sentence()),
    }
}

/// The Team group's header and note. Visible once the plan names a team or
/// the team session holds one.
#[derive(Default)]
pub(crate) struct TeamGroup {
    pub(crate) name: Option<SharedString>,
    pub(crate) note: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

/// Whether "Share with Team…" has anywhere to go right now.
pub(crate) fn can_share(cx: &App) -> bool {
    TeamVaultSession::global(cx)
        .is_some_and(|team| *team.read(cx).status() == TeamVaultStatus::Ready)
}

impl DatabasePanel {
    /// Follows the team session and the plan. Watching sends nothing: the
    /// group lists what the session already decrypted, and loading it is a
    /// button press.
    pub(crate) fn watch_team(&mut self, cx: &mut Context<Self>) {
        let mut subscriptions = Vec::new();
        if let Some(team) = TeamVaultSession::global(cx) {
            subscriptions.push(cx.observe(&team, |this, _, cx| this.sync_team_connections(cx)));
        }
        if let Some(account) = Account::global(cx) {
            subscriptions.push(cx.observe(&account, |this, _, cx| this.sync_team_connections(cx)));
        }
        self.team_group._subscriptions = subscriptions;
        self.sync_team_connections(cx);
    }

    /// Rebuilds the team nodes from the session's listing, keeping a node
    /// whose entry and revision are unchanged (and whatever it has open).
    pub(crate) fn sync_team_connections(&mut self, cx: &mut Context<Self>) {
        let in_plan = Account::global(cx).and_then(|account| {
            account
                .read(cx)
                .plan()
                .and_then(|plan| plan.team.as_ref().map(|team| team.name.clone()))
        });
        let (name, status, loaded, wanted) = match TeamVaultSession::global(cx) {
            Some(team) => {
                let team = team.read(cx);
                let loaded = team.team_name().is_some();
                (
                    team.team_name().or(in_plan),
                    team.status().clone(),
                    loaded,
                    shared_connections(&team.list()),
                )
            }
            None => (None, TeamVaultStatus::NoTeam, false, Vec::new()),
        };
        let wanted = if name.is_some() { wanted } else { Vec::new() };
        self.team_group.note = name
            .as_ref()
            .and_then(|_| group_note(&status, loaded, wanted.len()));
        self.team_group.name = name;

        let first_team = self
            .connections
            .iter()
            .position(|node| node.team.is_some())
            .unwrap_or(self.connections.len());
        let unchanged = self.connections.len() - first_team == wanted.len()
            && self.connections[first_team..].iter().zip(&wanted).all(
                |(node, (entry, _, revision))| {
                    node.team
                        .as_ref()
                        .is_some_and(|team| team.entry == *entry && &team.revision == revision)
                },
            );
        if unchanged {
            cx.notify();
            return;
        }

        for index in first_team..self.connections.len() {
            self.forget_connection_state(index);
        }
        let mut previous: Vec<ConnectionNode> = self.connections.split_off(first_team);
        for (entry, name, revision) in wanted {
            let kept = previous.iter().position(|node| {
                node.team
                    .as_ref()
                    .is_some_and(|team| team.entry == entry && team.revision == revision)
            });
            self.connections.push(match kept {
                Some(position) => previous.swap_remove(position),
                None => team_node(entry, name, revision),
            });
        }
        cx.notify();
    }

    /// Pulls a team connection's settings, then opens it as any other.
    pub(crate) fn fetch_team_connection(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self
            .connections
            .get(index)
            .and_then(|node| node.team.as_ref())
            .map(|team| team.entry)
        else {
            return;
        };
        let Some(team) = TeamVaultSession::global(cx) else {
            return;
        };
        if let Some(node) = self.connections.get_mut(index) {
            node.state = NodeState::Connecting;
        }
        cx.notify();

        let pulled = team.update(cx, |team, cx| team.pull(entry, cx));
        self.tasks.push(cx.spawn_in(window, async move |this, cx| {
            let shared = pulled
                .await
                .map_err(|error| error.to_string())
                .and_then(|pulled| {
                    ConnectionEntry::from_json(&pulled.payload).map_err(|error| error.to_string())
                });
            this.update_in(cx, |this, window, cx| {
                let Some(node) = this.connections.get_mut(index) else {
                    return;
                };
                // A sync while this was out may have put another node here.
                let Some(origin) = node.team.as_mut().filter(|team| team.entry == entry) else {
                    return;
                };
                match shared {
                    Ok(shared) => {
                        node.config.driver = shared.driver.clone();
                        node.config.url = shared.url.clone();
                        origin.shared_password = shared.password.clone().map(Zeroizing::new);
                        origin.fetched = true;
                        node.state = NodeState::Idle;
                        this.toggle_connection(index, window, cx);
                    }
                    Err(error) => node.state = NodeState::Failed(error.into()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Puts a shared password into the keychain after it opened a
    /// connection, and forgets the in-memory copy once it is there. From then
    /// on the connection opens like a personal one.
    pub(crate) fn remember_shared_password(
        &mut self,
        index: usize,
        password: Zeroizing<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.connections.get(index) else {
            return;
        };
        let Some(entry) = node.team.as_ref().map(|team| team.entry) else {
            return;
        };
        let written = write_secret(
            zed_credentials_provider::global(cx),
            node.config.credential_key().to_string(),
            node.config.name.clone(),
            password.to_string(),
            cx,
        );
        self.tasks.push(cx.spawn(async move |this, cx| {
            if let Err(error) = written.await {
                log::warn!("could not keep a shared database password in the keychain: {error:#}");
                return;
            }
            this.update(cx, |this, _cx| {
                if let Some(origin) = this
                    .connections
                    .get_mut(index)
                    .and_then(|node| node.team.as_mut())
                    .filter(|team| team.entry == entry)
                {
                    origin.shared_password = None;
                }
            })
            .ok();
        }));
    }

    /// Opens the share window for a personal connection.
    pub(crate) fn share_with_team(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self
            .connections
            .get(index)
            .filter(|node| node.team.is_none())
        else {
            return;
        };
        let Some(team) = TeamVaultSession::global(cx) else {
            return;
        };
        let config = node.config.clone();
        self.workspace
            .update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |_window, cx| {
                    ShareConnectionModal::new(team, config, cx)
                });
            })
            .ok();
    }

    /// Loads the team's connections. A button, never a side effect of drawing.
    pub(crate) fn load_team_connections(&mut self, cx: &mut Context<Self>) {
        if let Some(team) = TeamVaultSession::global(cx) {
            team.update(cx, |team, cx| team.refresh(cx)).detach();
        }
    }
}

fn team_node(entry: EntryId, name: String, revision: String) -> ConnectionNode {
    ConnectionNode {
        config: ConnectionConfig {
            name,
            driver: String::new(),
            url: String::new(),
        },
        state: NodeState::Idle,
        expanded: false,
        schemas: Vec::new(),
        collapse_schema_level: false,
        team: Some(TeamOrigin {
            entry,
            revision,
            fetched: false,
            shared_password: None,
        }),
        from_project: false,
        unattended: false,
    }
}

/// Shares one personal connection with the team. The password stays home
/// unless the box is ticked.
pub(crate) struct ShareConnectionModal {
    team: Entity<TeamVaultSession>,
    config: ConnectionConfig,
    pub(crate) include_password: bool,
    sending: bool,
    error: Option<SharedString>,
    focus_handle: FocusHandle,
}

impl ModalView for ShareConnectionModal {}
impl EventEmitter<DismissEvent> for ShareConnectionModal {}

impl Focusable for ShareConnectionModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ShareConnectionModal {
    pub(crate) fn new(
        team: Entity<TeamVaultSession>,
        config: ConnectionConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            team,
            config,
            include_password: false,
            sending: false,
            error: None,
            focus_handle: cx.focus_handle(),
        }
    }

    fn team_name(&self, cx: &App) -> SharedString {
        self.team
            .read(cx)
            .team_name()
            .unwrap_or_else(|| "your team".into())
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.sending {
            return;
        }
        self.sending = true;
        self.error = None;
        cx.notify();

        let password = self.include_password.then(|| {
            read_secret(
                zed_credentials_provider::global(cx),
                self.config.credential_key().to_string(),
                cx,
            )
        });
        let config = self.config.clone();
        cx.spawn(async move |this, cx| {
            let outcome: anyhow::Result<()> = async {
                let password = match password {
                    Some(read) => Some(read.await?.ok_or_else(|| {
                        anyhow::anyhow!(
                            "no password is stored for this connection; share it without one, or set one first"
                        )
                    })?),
                    None => None,
                };
                let shared = ConnectionEntry {
                    name: config.name.clone(),
                    driver: config.driver.clone(),
                    url: config.url.clone(),
                    password,
                };
                let payload = shared.to_json()?;
                let (entry, pushed) = this.update(cx, |this, cx| {
                    let existing = this
                        .team
                        .read(cx)
                        .manifest()
                        .and_then(|manifest| existing_connection(manifest, &config.name));
                    // Re-sharing a name replaces that entry, whatever it holds
                    // now; a new name must not land on an existing entry.
                    let (entry, base) = match existing {
                        Some(entry) => (entry, PushBase::Latest),
                        None => (EntryId::generate()?, PushBase::Create),
                    };
                    let pushed = this
                        .team
                        .update(cx, |team, cx| team.push(entry, payload, base, cx));
                    anyhow::Ok((entry, pushed))
                })??;
                pushed.await?;
                let saved = this.update(cx, |this, cx| {
                    this.team.update(cx, |team, cx| {
                        team.edit_manifest(
                            ManifestChange::Upsert(ManifestEntry {
                                entry_id: entry,
                                kind: ManifestEntryKind::DbConnection,
                                label: config.name.clone(),
                                project_hint: None,
                            }),
                            cx,
                        )
                    })
                })?;
                saved.await?;
                Ok(())
            }
            .await;
            this.update(cx, |this, cx| {
                this.sending = false;
                match outcome {
                    Ok(()) => cx.emit(DismissEvent),
                    Err(error) => {
                        this.error = Some(format!("{error:#}").into());
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Render for ShareConnectionModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let team_name = self.team_name(cx);
        let who = if self.include_password {
            format!(
                "Everyone in {team_name} who holds the team key gets the driver, the URL and the password."
            )
        } else {
            format!(
                "Everyone in {team_name} who holds the team key gets the driver and the URL; each person types their own password."
            )
        };
        div()
            .key_context("DatabaseShareConnection")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(28.))
            .debug_selector(|| "database-share-connection".into())
            .on_action(cx.listener(|_this, _: &menu::Cancel, _window, cx| cx.emit(DismissEvent)))
            .child(
                Modal::new("database-share-connection", None)
                    .header(
                        ModalHeader::new()
                            .child(
                                Headline::new(format!(
                                    "Share {} with {team_name}",
                                    self.config.name
                                ))
                                .size(HeadlineSize::Small),
                            )
                            .description(
                                "Sealed on this machine under the team key. The server sees an opaque id.",
                            ),
                    )
                    .section(
                        Section::new()
                            .child(
                                Checkbox::new(
                                    "database-share-include-password",
                                    ToggleState::from(self.include_password),
                                )
                                .label("Include password")
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(|this, state: &ToggleState, _window, cx| {
                                    this.include_password = *state == ToggleState::Selected;
                                    cx.notify();
                                })),
                            )
                            .child(Label::new(who).size(LabelSize::XSmall).color(Color::Muted))
                            .child(
                                Label::new(
                                    "A URL can carry credentials of its own. Check this one does not before sharing it.",
                                )
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                            )
                            .when_some(self.error.clone(), |section, error| {
                                section.child(
                                    Label::new(error).size(LabelSize::Small).color(Color::Error),
                                )
                            }),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(Button::new("database-share-cancel", "Cancel").on_click(
                                    cx.listener(|_this, _, _window, cx| cx.emit(DismissEvent)),
                                ))
                                .child(
                                    Button::new("database-share-confirm", "Share")
                                        .style(ButtonStyle::Filled)
                                        .disabled(self.sending)
                                        .on_click(
                                            cx.listener(|this, _, _window, cx| this.confirm(cx)),
                                        ),
                                ),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_team_vault::ManifestEntry;
    use zode_team_vault::client::VaultListing;

    fn secret(value: &str) -> Zeroizing<String> {
        Zeroizing::new(value.to_string())
    }

    #[test]
    fn a_shared_password_is_used_and_kept_only_when_the_keychain_has_none() {
        assert_eq!(
            choose_secret(None, Some(secret("shared"))),
            SecretChoice {
                secret: Some("shared".into()),
                remember: Some(secret("shared")),
            }
        );
        assert_eq!(
            choose_secret(Some("mine".into()), Some(secret("shared"))),
            SecretChoice {
                secret: Some("mine".into()),
                remember: None,
            },
            "a password typed on this machine is not overwritten by a shared one"
        );
        assert_eq!(
            choose_secret(Some("mine".into()), None),
            SecretChoice {
                secret: Some("mine".into()),
                remember: None,
            }
        );
    }

    #[test]
    fn without_a_password_it_opens_without_one_as_today() {
        assert_eq!(
            choose_secret(None, None),
            SecretChoice {
                secret: None,
                remember: None,
            }
        );
    }

    fn id(byte: u8) -> EntryId {
        EntryId::parse(&format!("{byte:02x}").repeat(16)).expect("valid id")
    }

    fn view(byte: u8, kind: Option<ManifestEntryKind>, label: &str) -> VaultEntryView {
        VaultEntryView {
            listing: VaultListing {
                entry_id: id(byte),
                revision: format!("r{byte:02x}"),
                key_version: 1,
                byte_length: 4096,
                updated_at: "2026-11-03T10:00:00.000Z".into(),
                updated_by: "u".into(),
            },
            manifest: kind.map(|kind| ManifestEntry {
                entry_id: id(byte),
                kind,
                label: label.into(),
                project_hint: None,
            }),
        }
    }

    #[test]
    fn only_connections_named_in_the_catalogue_join_the_group() {
        let entries = [
            view(0xb2, Some(ManifestEntryKind::DbConnection), "staging"),
            view(0xa1, Some(ManifestEntryKind::Env), ".env"),
            view(0xc3, None, ""),
            view(0xd4, Some(ManifestEntryKind::DbConnection), "analytics"),
        ];
        assert_eq!(
            shared_connections(&entries),
            vec![
                (id(0xd4), "analytics".to_string(), "rd4".to_string()),
                (id(0xb2), "staging".to_string(), "rb2".to_string()),
            ]
        );
    }

    #[test]
    fn a_share_replaces_the_connection_of_the_same_name_only() {
        let mut manifest = TeamManifest::new();
        for (byte, kind, label) in [
            (0xa1, ManifestEntryKind::DbConnection, "staging"),
            (0xb2, ManifestEntryKind::Env, "analytics"),
        ] {
            manifest
                .upsert(ManifestEntry {
                    entry_id: id(byte),
                    kind,
                    label: label.into(),
                    project_hint: None,
                })
                .expect("upsert");
        }
        assert_eq!(existing_connection(&manifest, "staging"), Some(id(0xa1)));
        assert_eq!(
            existing_connection(&manifest, "analytics"),
            None,
            "a .env of the same name is not a connection"
        );
        assert_eq!(existing_connection(&manifest, "prod"), None);
    }

    #[test]
    fn the_group_says_how_to_load_and_why_it_is_empty() {
        let unloaded = group_note(&TeamVaultStatus::NoTeam, false, 0).expect("said");
        assert!(unloaded.contains("load"), "{unloaded}");
        assert_eq!(
            group_note(&TeamVaultStatus::Ready, true, 0)
                .map(|note| note.to_string())
                .as_deref(),
            Some("No shared connections")
        );
        assert_eq!(group_note(&TeamVaultStatus::Ready, true, 2), None);
        let pending = group_note(&TeamVaultStatus::KeyPending, true, 0).expect("said");
        assert!(pending.contains("grant"), "{pending}");
        assert_eq!(
            group_note(&TeamVaultStatus::Working, false, 0)
                .map(|note| note.to_string())
                .as_deref(),
            Some("Loading…")
        );
    }
}
