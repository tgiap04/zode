use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, PromptLevel, SharedString,
    Subscription, Window,
};
use ui::{
    CommonAnimationExt, ListItem, ListSeparator, Modal, ModalFooter, ModalHeader, Section, Tooltip,
    prelude::*,
};
use workspace::ModalView;
use zode_team_vault::{MemberView, PinCheck, TeamVaultError, TeamVaultSession, VaultEntryView};

/// What this machine knows about one member's sharing key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum KeyState {
    You,
    /// The member has not opened team sharing in Zode yet.
    NoKey,
    /// Not verified on this machine: never seen, or only remembered on first
    /// sight when this machine first opened the team. Pins are per machine, so
    /// a member already checked elsewhere is unverified here too. Nothing is
    /// sealed to it.
    Unverified,
    Verified,
    /// Differs from the key pinned here; `previous` is the pinned fingerprint.
    Changed {
        previous: SharedString,
    },
}

/// The explicit step that pins a key, offered one member at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrustAction {
    FirstLook,
    AcceptChange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemberRow {
    pub user_id: String,
    pub email: SharedString,
    /// Eight groups of four hex digits.
    pub fingerprint: Option<SharedString>,
    pub key: KeyState,
    pub holds_team_key: bool,
    pub trust: Option<TrustAction>,
    pub grant: bool,
}

pub(crate) fn member_rows(
    members: &[MemberView],
    can_administer: bool,
    holds_team_key_here: bool,
) -> Vec<MemberRow> {
    members
        .iter()
        .map(|member| {
            let key = match (&member.fingerprint, &member.pin) {
                _ if member.is_me => KeyState::You,
                (None, _) => KeyState::NoKey,
                (Some(_), None | Some(PinCheck::New) | Some(PinCheck::Seen)) => {
                    KeyState::Unverified
                }
                (Some(_), Some(PinCheck::Verified)) => KeyState::Verified,
                (Some(_), Some(PinCheck::Changed { pinned })) => KeyState::Changed {
                    previous: pinned.fingerprint.to_string().into(),
                },
            };
            let trust = match key {
                KeyState::Unverified => Some(TrustAction::FirstLook),
                KeyState::Changed { .. } => Some(TrustAction::AcceptChange),
                KeyState::You | KeyState::NoKey | KeyState::Verified => None,
            };
            // Verified only: a key becomes trusted by the explicit trust step,
            // never as a side effect of granting or of first sight.
            let grant = can_administer
                && holds_team_key_here
                && key == KeyState::Verified
                && !member.has_current_envelope;
            MemberRow {
                user_id: member.user_id.clone(),
                email: member.email.clone().into(),
                fingerprint: member
                    .fingerprint
                    .map(|fingerprint| fingerprint.to_string().into()),
                key,
                holds_team_key: member.has_current_envelope,
                trust,
                grant,
            }
        })
        .collect()
}

/// Members whose key a rotation would stop on: they hold a key, and this
/// machine has not verified it (or knows a different one).
pub(crate) fn rotation_blockers(rows: &[MemberRow]) -> Vec<SharedString> {
    rows.iter()
        .filter(|row| matches!(row.key, KeyState::Unverified | KeyState::Changed { .. }))
        .map(|row| row.email.clone())
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RotateButton {
    pub visible: bool,
    pub highlighted: bool,
    pub enabled: bool,
    pub reason: Option<SharedString>,
}

pub(crate) fn rotate_button(
    can_administer: bool,
    holds_team_key_here: bool,
    rotation_recommended: bool,
    blockers: &[SharedString],
) -> RotateButton {
    if !can_administer || !holds_team_key_here {
        return RotateButton::default();
    }
    let reason = (!blockers.is_empty()).then(|| {
        format!(
            "Check the key of {} first: a rotation seals only to keys this machine has verified",
            blockers.join(", ")
        )
        .into()
    });
    RotateButton {
        visible: true,
        highlighted: rotation_recommended,
        enabled: reason.is_none(),
        reason,
    }
}

/// Whether some entry is still sealed under an older key version than the
/// newest this machine holds: what an interrupted rotation leaves behind.
pub(crate) fn needs_reseal(entries: &[VaultEntryView], key_versions: &[u32]) -> bool {
    let Some(current) = key_versions.iter().max() else {
        return false;
    };
    entries
        .iter()
        .any(|entry| entry.listing.key_version < *current)
}

/// Everything the members window draws, read from the session in one place so
/// a draw test can stand one up without a server behind it.
#[derive(Clone, Debug, Default)]
pub(crate) struct MembersState {
    pub team_name: Option<SharedString>,
    pub can_administer: bool,
    pub holds_team_key: bool,
    pub rotation_recommended: bool,
    pub needs_reseal: bool,
    pub working: bool,
    pub rows: Vec<MemberRow>,
}

impl MembersState {
    fn read(team: &TeamVaultSession) -> Self {
        let can_administer = team.role().is_some_and(|role| role.can_administer());
        let key_versions = team.key_versions();
        let holds_team_key = !key_versions.is_empty();
        Self {
            team_name: team.team_name(),
            can_administer,
            holds_team_key,
            rotation_recommended: team.rotation_recommended(),
            needs_reseal: needs_reseal(&team.list(), &key_versions),
            working: matches!(team.status(), zode_team_vault::TeamVaultStatus::Working),
            rows: member_rows(&team.members(), can_administer, holds_team_key),
        }
    }
}

/// Who is in the team, which keys this machine has checked, and the admin's
/// two levers: granting the team key, and rotating it after someone leaves.
///
/// Trusting a key is its own visible step, one member at a time. Rotation
/// seals only to keys verified here, so it is never the moment trust begins.
pub struct TeamMembersModal {
    team: Entity<TeamVaultSession>,
    focus_handle: FocusHandle,
    rotating: bool,
    message: Option<(SharedString, Color)>,
    #[cfg(test)]
    state_for_test: Option<MembersState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DismissEvent> for TeamMembersModal {}
impl ModalView for TeamMembersModal {}

impl Focusable for TeamMembersModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TeamMembersModal {
    pub fn new(team: Entity<TeamVaultSession>, cx: &mut Context<Self>) -> Self {
        // Opening this window is one of the moments the team is read: who
        // joined, whose key changed, whether the server asks for a rotation.
        team.update(cx, |team, cx| team.refresh(cx)).detach();
        let subscriptions = vec![cx.observe(&team, |_this, _team, cx| cx.notify())];
        Self {
            team,
            focus_handle: cx.focus_handle(),
            rotating: false,
            message: None,
            #[cfg(test)]
            state_for_test: None,
            _subscriptions: subscriptions,
        }
    }

    fn state(&self, cx: &App) -> MembersState {
        #[cfg(test)]
        if let Some(state) = &self.state_for_test {
            return state.clone();
        }
        MembersState::read(self.team.read(cx))
    }

    fn say(&mut self, message: impl Into<SharedString>, color: Color, cx: &mut Context<Self>) {
        self.message = Some((message.into(), color));
        cx.notify();
    }

    /// Pins a key after the person compared it out of band. Asked with the
    /// platform's own alert, with the fingerprint in it, so the comparison is
    /// the thing being confirmed rather than a button press.
    fn trust(&mut self, row: MemberRow, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fingerprint) = row.fingerprint.clone() else {
            return;
        };
        let (level, message, detail, accept) = match &row.key {
            KeyState::Changed { previous } => (
                PromptLevel::Critical,
                format!("The sharing key of {} changed", row.email),
                format!(
                    "This machine pinned\n{previous}\n\nThe server now names\n{fingerprint}\n\nA new machine or a reinstall changes it legitimately; a server substituting a key changes it too. Accept it only after {} reads you the new fingerprint over a channel Zode's server does not carry.",
                    row.email
                ),
                "Accept New Key",
            ),
            _ => (
                PromptLevel::Warning,
                format!("Trust the sharing key of {}?", row.email),
                format!(
                    "{fingerprint}\n\nCompare it with {} in person or over a channel Zode's server does not carry. Trust is remembered on this machine only.",
                    row.email
                ),
                "Trust Key",
            ),
        };
        let answer = window.prompt(level, &message, Some(&detail), &[accept, "Cancel"], cx);
        let user_id = row.user_id.clone();
        let email = row.email;
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let Ok(trusted) = this.update(cx, |this, cx| {
                this.team
                    .update(cx, |team, cx| team.trust_member_key(&user_id, cx))
            }) else {
                return;
            };
            let outcome = trusted.await;
            this.update(cx, |this, cx| match outcome {
                Ok(()) => this.say(
                    format!("{email}'s key is verified on this machine."),
                    Color::Muted,
                    cx,
                ),
                Err(error) => this.say(error.to_string(), Color::Error, cx),
            })
            .ok();
        })
        .detach();
    }

    fn grant(&mut self, row: MemberRow, cx: &mut Context<Self>) {
        let granted = self
            .team
            .update(cx, |team, cx| team.grant(row.user_id.clone(), cx));
        cx.spawn(async move |this, cx| {
            let outcome = granted.await;
            this.update(cx, |this, cx| match outcome {
                Ok(()) => this.say(
                    format!("{} can now open the team's entries.", row.email),
                    Color::Muted,
                    cx,
                ),
                Err(error) => this.say(describe(&error), Color::Error, cx),
            })
            .ok();
        })
        .detach();
    }

    fn rotate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            "Rotate the team key?",
            Some(
                "A new key is sealed to every member whose key is verified on this machine, and every shared entry is re-sealed under it. Someone who left keeps what they already opened, and nothing written from now on.",
            ),
            &["Rotate", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let Ok(rotation) = this.update(cx, |this, cx| {
                this.rotating = true;
                this.message = None;
                cx.notify();
                this.team.update(cx, |team, cx| team.rotate(cx))
            }) else {
                return;
            };
            let outcome = rotation.await;
            this.update(cx, |this, cx| {
                this.rotating = false;
                match outcome {
                    Ok(report) => {
                        let mut sentence = format!(
                            "The team key is now version {}; {} {} re-sealed.",
                            report.key_version,
                            report.reseal.resealed,
                            if report.reseal.resealed == 1 {
                                "entry"
                            } else {
                                "entries"
                            },
                        );
                        if !report.rotated_here {
                            sentence.push_str(
                                " Another admin rotated first; this carried on from theirs.",
                            );
                        }
                        if report.still_recommended {
                            sentence.push_str(
                                " Someone left after that rotation, so the server still asks for one: rotate again.",
                            );
                        }
                        let skipped = report.reseal.unreadable.len() + report.reseal.refused.len();
                        let color = if skipped > 0 || report.still_recommended {
                            sentence.push_str(&format!(
                                " {skipped} could not be opened here and stay under an older key."
                            ));
                            Color::Warning
                        } else {
                            Color::Muted
                        };
                        this.say(sentence, color, cx);
                    }
                    Err(error) => this.say(describe(&error), Color::Error, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn reseal(&mut self, cx: &mut Context<Self>) {
        self.rotating = true;
        cx.notify();
        let resealed = self.team.update(cx, |team, cx| team.reseal(cx));
        cx.spawn(async move |this, cx| {
            let outcome = resealed.await;
            this.update(cx, |this, cx| {
                this.rotating = false;
                match outcome {
                    Ok(report) => this.say(
                        format!("{} re-sealed under the current key.", report.resealed),
                        Color::Muted,
                        cx,
                    ),
                    Err(error) => this.say(describe(&error), Color::Error, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn render_row(&self, row: &MemberRow, cx: &mut Context<Self>) -> ListItem {
        let (state, color, icon) = match &row.key {
            KeyState::You => ("you", Color::Muted, None),
            KeyState::NoKey => (
                "has not opened team sharing in Zode yet",
                Color::Muted,
                None,
            ),
            KeyState::Unverified => (
                "not verified on this machine — compare, then trust",
                Color::Muted,
                Some((IconName::Info, Color::Muted)),
            ),
            KeyState::Verified => (
                "verified on this machine",
                Color::Muted,
                Some((IconName::Check, Color::Success)),
            ),
            KeyState::Changed { .. } => (
                "key changed since it was verified — nothing is sealed to it",
                Color::Warning,
                Some((IconName::Warning, Color::Warning)),
            ),
        };
        let custody = match (&row.key, row.holds_team_key) {
            (KeyState::NoKey, _) => None,
            (_, true) => Some("holds the team key"),
            (_, false) => Some("waiting for the team key"),
        };
        let user = row.user_id.clone();
        let mut details = v_flex()
            .min_w_0()
            .child(
                h_flex()
                    .gap_1()
                    .child(Label::new(row.email.clone()))
                    .when_some(custody, |this, custody| {
                        this.child(
                            Label::new(custody)
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                    }),
            )
            .when_some(row.fingerprint.clone(), |this, fingerprint| {
                this.child(
                    Label::new(fingerprint)
                        .size(LabelSize::Small)
                        .buffer_font(cx)
                        .color(Color::Muted),
                )
            });
        if let KeyState::Changed { previous } = &row.key {
            details = details.child(
                Label::new(format!("was {previous}"))
                    .size(LabelSize::XSmall)
                    .buffer_font(cx)
                    .color(Color::Warning),
            );
        }
        details = details.child(Label::new(state).size(LabelSize::XSmall).color(color));

        let trust = row.trust.map(|action| {
            let row = row.clone();
            Button::new(
                SharedString::from(format!("team-member-trust-{user}")),
                match action {
                    TrustAction::FirstLook => "Trust Key\u{2026}",
                    TrustAction::AcceptChange => "Accept New Key\u{2026}",
                },
            )
            .tooltip(Tooltip::text(
                "Compare the fingerprint with them first, over a channel Zode's server does not carry",
            ))
            .on_click(cx.listener(move |this, _, window, cx| this.trust(row.clone(), window, cx)))
        });
        let grant = row.grant.then(|| {
            let row = row.clone();
            Button::new(
                SharedString::from(format!("team-member-grant-{user}")),
                "Grant Access",
            )
            .style(ButtonStyle::Filled)
            .tooltip(Tooltip::text(
                "Seals the team key to this verified key, so they can open shared entries",
            ))
            .on_click(cx.listener(move |this, _, _window, cx| this.grant(row.clone(), cx)))
        });

        ListItem::new(SharedString::from(format!("team-member-{user}")))
            .rounded()
            .when_some(icon, |this, (icon, color)| {
                this.start_slot(Icon::new(icon).size(IconSize::Small).color(color))
            })
            .child(details)
            .end_slot(h_flex().gap_1().children(trust).children(grant))
    }
}

/// The errors a person can act on, in the words of the action.
fn describe(error: &TeamVaultError) -> String {
    match error {
        TeamVaultError::NotPinned { .. } => format!(
            "Stopped before anything was sealed: {error}. Trust their key above, then try again."
        ),
        TeamVaultError::KeyChanged { .. } => format!(
            "Stopped before anything was sealed: {error}. Accept the new key above once you have compared it."
        ),
        other => other.to_string(),
    }
}

impl Render for TeamMembersModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state(cx);
        let blockers = rotation_blockers(&state.rows);
        let rotate = rotate_button(
            state.can_administer,
            state.holds_team_key,
            state.rotation_recommended,
            &blockers,
        );
        let busy = self.rotating || state.working;

        let mut rows = v_flex().w_full().gap_0p5();
        for row in &state.rows {
            rows = rows.child(self.render_row(row, cx));
        }
        if state.rows.is_empty() {
            rows = rows.child(
                Label::new(if state.working {
                    "Reading the team…"
                } else {
                    "No members to show."
                })
                .size(LabelSize::Small)
                .color(Color::Muted),
            );
        }

        let headline = match &state.team_name {
            Some(name) => format!("Members of {name}"),
            None => "Team members".to_string(),
        };
        let progress = self
            .rotating
            .then_some("Rotating the team key and re-sealing entries…");
        let recommended = (rotate.visible && state.rotation_recommended).then_some(
            "Someone left since the key was last rotated. Rotate it so new entries are sealed under a key they never held.",
        );

        div()
            .key_context("TeamMembersModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(40.))
            .max_h(rems(40.))
            .debug_selector(|| "team-members-modal".into())
            .on_action(cx.listener(|_this, _: &menu::Cancel, _window, cx| cx.emit(DismissEvent)))
            .child(
                Modal::new("team-members", None)
                    .header(
                        ModalHeader::new()
                            .icon(
                                Icon::new(IconName::Person)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .headline(headline)
                            .description(
                                "Compare fingerprints with each person over a channel Zode's server does not carry. What you verify is remembered on this machine only.",
                            ),
                    )
                    .section(
                        Section::new().padded(false).child(
                            v_flex()
                                .w_full()
                                .child(ListSeparator)
                                .when_some(recommended, |this, sentence| {
                                    this.child(
                                        h_flex()
                                            .p_2()
                                            .gap_1p5()
                                            .child(
                                                Icon::new(IconName::Warning)
                                                    .size(IconSize::XSmall)
                                                    .color(Color::Warning),
                                            )
                                            .child(
                                                Label::new(sentence)
                                                    .size(LabelSize::Small)
                                                    .color(Color::Warning),
                                            ),
                                    )
                                })
                                .child(
                                    div()
                                        .id("team-members-body")
                                        .w_full()
                                        .max_h(rems(24.))
                                        .p_1()
                                        .overflow_y_scroll()
                                        .child(rows),
                                )
                                .when(progress.is_some() || self.message.is_some(), |this| {
                                    let (sentence, color) = match (progress, &self.message) {
                                        (Some(progress), _) => (SharedString::from(progress), Color::Muted),
                                        (None, Some((message, color))) => (message.clone(), *color),
                                        (None, None) => (SharedString::default(), Color::Muted),
                                    };
                                    this.child(ListSeparator).child(
                                        h_flex()
                                            .w_full()
                                            .p_2()
                                            .gap_1p5()
                                            .when(progress.is_some(), |this| {
                                                this.child(
                                                    Icon::new(IconName::LoadCircle)
                                                        .size(IconSize::Small)
                                                        .color(Color::Muted)
                                                        .with_rotate_animation(2),
                                                )
                                            })
                                            .child(
                                                Label::new(sentence)
                                                    .size(LabelSize::Small)
                                                    .color(color),
                                            ),
                                    )
                                }),
                        ),
                    )
                    .footer(
                        ModalFooter::new()
                            .start_slot(
                                h_flex()
                                    .gap_1()
                                    .when(rotate.visible, |this| {
                                        this.child(
                                            Button::new("team-members-rotate", "Rotate Team Key\u{2026}")
                                                .style(if rotate.highlighted {
                                                    ButtonStyle::Filled
                                                } else {
                                                    ButtonStyle::Subtle
                                                })
                                                .disabled(!rotate.enabled || busy)
                                                .tooltip(Tooltip::text(rotate.reason.clone().unwrap_or_else(|| {
                                                    "A new key for everyone who stays, and every entry re-sealed under it".into()
                                                })))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.rotate(window, cx)
                                                })),
                                        )
                                    })
                                    .when(rotate.visible && state.needs_reseal, |this| {
                                        this.child(
                                            Button::new("team-members-reseal", "Finish Re-sealing")
                                                .disabled(busy)
                                                .tooltip(Tooltip::text(
                                                    "Some entries are still under an older key, probably from an interrupted rotation",
                                                ))
                                                .on_click(cx.listener(|this, _, _window, cx| this.reseal(cx))),
                                        )
                                    }),
                            )
                            .end_slot(
                                Button::new("team-members-close", "Close")
                                    .on_click(cx.listener(|_this, _, _window, cx| cx.emit(DismissEvent))),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_env_sync::EntryId;
    use zode_team_vault::client::VaultListing;
    use zode_team_vault::{Fingerprint, PinnedKey};

    fn fingerprint(byte: u8) -> Fingerprint {
        Fingerprint::of(&[byte; 32])
    }

    fn member(user: &str, key: Option<u8>, pin: Option<PinCheck>, envelope: bool) -> MemberView {
        MemberView {
            user_id: user.into(),
            email: format!("{user}@example.com"),
            fingerprint: key.map(fingerprint),
            pin,
            has_current_envelope: envelope,
            is_me: false,
        }
    }

    fn me() -> MemberView {
        MemberView {
            is_me: true,
            pin: None,
            ..member("me", Some(1), None, true)
        }
    }

    fn changed() -> PinCheck {
        PinCheck::Changed {
            pinned: PinnedKey {
                fingerprint: fingerprint(9),
                email: "bob@example.com".into(),
                pinned_at: 0,
                verified: true,
            },
        }
    }

    fn row<'a>(rows: &'a [MemberRow], user: &str) -> &'a MemberRow {
        rows.iter()
            .find(|row| row.user_id == user)
            .unwrap_or_else(|| panic!("no row for {user}"))
    }

    #[test]
    fn a_member_never_sees_grant_but_still_checks_keys() {
        let members = [
            me(),
            member("new", Some(2), Some(PinCheck::New), false),
            member("same", Some(3), Some(PinCheck::Verified), false),
        ];
        let rows = member_rows(&members, false, true);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|row| !row.grant), "{rows:?}");
        // Pins are this machine's, not an admin power: a member must be able
        // to accept an admin's key, or the envelopes that admin seals never open.
        assert_eq!(row(&rows, "new").trust, Some(TrustAction::FirstLook));
    }

    #[test]
    fn an_admin_grants_only_a_verified_key_without_the_team_key() {
        let members = [
            me(),
            member("verified", Some(2), Some(PinCheck::Verified), false),
            member("holder", Some(3), Some(PinCheck::Verified), true),
            member("new", Some(4), Some(PinCheck::New), false),
            member("changed", Some(5), Some(changed()), false),
            member("keyless", None, None, false),
        ];
        let rows = member_rows(&members, true, true);
        assert!(row(&rows, "verified").grant);
        assert!(!row(&rows, "holder").grant, "already holds the key");
        assert!(
            !row(&rows, "new").grant,
            "a first sighting is trusted explicitly before anything is sealed to it"
        );
        assert!(
            !row(&rows, "changed").grant,
            "a changed key blocks Grant until it is accepted"
        );
        assert!(!row(&rows, "keyless").grant);
        assert!(!row(&rows, "me").grant);
    }

    /// A key remembered on first sight was never compared: it is shown as
    /// not verified, offered Trust Key, never Grant, and blocks a rotation.
    #[test]
    fn a_key_only_seen_on_first_sight_is_not_verified() {
        let members = [me(), member("seen", Some(2), Some(PinCheck::Seen), false)];
        let rows = member_rows(&members, true, true);
        assert_eq!(row(&rows, "seen").key, KeyState::Unverified);
        assert_eq!(row(&rows, "seen").trust, Some(TrustAction::FirstLook));
        assert!(!row(&rows, "seen").grant);
        assert_eq!(
            rotation_blockers(&rows),
            vec![SharedString::from("seen@example.com")]
        );
    }

    #[test]
    fn an_admin_without_the_team_key_cannot_grant_it() {
        let members = [
            me(),
            member("verified", Some(2), Some(PinCheck::Verified), false),
        ];
        let rows = member_rows(&members, true, false);
        assert!(!row(&rows, "verified").grant);
    }

    #[test]
    fn key_states_and_trust_actions_follow_the_pin() {
        let members = [
            me(),
            member("new", Some(2), Some(PinCheck::New), true),
            member("same", Some(3), Some(PinCheck::Verified), true),
            member("changed", Some(5), Some(changed()), true),
            member("keyless", None, None, false),
        ];
        let rows = member_rows(&members, true, true);
        assert_eq!(row(&rows, "me").key, KeyState::You);
        assert_eq!(row(&rows, "me").trust, None);
        assert_eq!(row(&rows, "new").key, KeyState::Unverified);
        assert_eq!(row(&rows, "same").key, KeyState::Verified);
        assert_eq!(row(&rows, "same").trust, None);
        assert_eq!(
            row(&rows, "changed").key,
            KeyState::Changed {
                previous: fingerprint(9).to_string().into()
            }
        );
        assert_eq!(row(&rows, "changed").trust, Some(TrustAction::AcceptChange));
        assert_eq!(row(&rows, "keyless").key, KeyState::NoKey);
        assert_eq!(row(&rows, "keyless").fingerprint, None);
        assert_eq!(
            row(&rows, "same").fingerprint,
            Some(fingerprint(3).to_string().into()),
            "the fingerprint is shown in its eight groups"
        );
        assert!(row(&rows, "same").holds_team_key);
    }

    #[test]
    fn unchecked_and_changed_keys_block_a_rotation() {
        let members = [
            me(),
            member("new", Some(2), Some(PinCheck::New), true),
            member("same", Some(3), Some(PinCheck::Verified), true),
            member("changed", Some(5), Some(changed()), true),
            member("keyless", None, None, false),
        ];
        let rows = member_rows(&members, true, true);
        let blockers = rotation_blockers(&rows);
        assert_eq!(
            blockers,
            vec![
                SharedString::from("new@example.com"),
                SharedString::from("changed@example.com")
            ]
        );
    }

    #[test]
    fn the_rotate_button_is_an_admins_and_says_why_it_waits() {
        assert!(
            !rotate_button(false, true, true, &[]).visible,
            "members never see it"
        );
        assert!(
            !rotate_button(true, false, true, &[]).visible,
            "an admin without the key has nothing to rotate from"
        );

        let calm = rotate_button(true, true, false, &[]);
        assert!(calm.visible && calm.enabled && !calm.highlighted);

        let flagged = rotate_button(true, true, true, &[]);
        assert!(flagged.highlighted && flagged.enabled);

        let blocked = rotate_button(true, true, true, &["bob@example.com".into()]);
        assert!(blocked.visible && !blocked.enabled);
        let reason = blocked.reason.expect("a disabled button says why");
        assert!(reason.contains("bob@example.com"), "{reason}");
    }

    fn listed(byte: u8, key_version: u32) -> VaultEntryView {
        VaultEntryView {
            listing: VaultListing {
                entry_id: EntryId::parse(&format!("{byte:02x}").repeat(16)).expect("valid id"),
                revision: "1".into(),
                key_version,
                byte_length: 4096,
                updated_at: "2026-11-03T10:00:00.000Z".into(),
                updated_by: "u".into(),
            },
            manifest: None,
        }
    }

    #[test]
    fn an_entry_under_an_older_key_needs_a_reseal() {
        assert!(!needs_reseal(&[listed(1, 2), listed(2, 2)], &[1, 2]));
        assert!(needs_reseal(&[listed(1, 1), listed(2, 2)], &[1, 2]));
        assert!(
            !needs_reseal(&[listed(1, 1)], &[]),
            "no key, nothing to re-seal with"
        );
        assert!(!needs_reseal(&[], &[1, 2]));
    }

    #[gpui::test]
    fn the_members_window_draws_every_key_state_and_a_flagged_rotation(
        cx: &mut gpui::TestAppContext,
    ) {
        crate::team_vault_section::tests::init_theme(cx);
        let team = crate::team_vault_section::tests::team_session(cx);
        let (modal, cx) = cx.add_window_view(|_window, cx| TeamMembersModal::new(team, cx));
        cx.run_until_parked();

        let members = [
            me(),
            member("new", Some(2), Some(PinCheck::New), false),
            member("same", Some(3), Some(PinCheck::Verified), false),
            member("changed", Some(5), Some(changed()), true),
            member("keyless", None, None, false),
        ];
        for (can_administer, rotation_recommended, rotating) in [
            (true, true, false),
            (true, false, true),
            (false, false, false),
        ] {
            modal.update(cx, |modal, cx| {
                modal.rotating = rotating;
                modal.state_for_test = Some(MembersState {
                    team_name: Some("Acme".into()),
                    can_administer,
                    holds_team_key: true,
                    rotation_recommended,
                    needs_reseal: true,
                    working: false,
                    rows: member_rows(&members, can_administer, true),
                });
                cx.notify();
            });
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
            assert!(cx.debug_bounds("team-members-modal").is_some());
        }

        let medium = cx.update(|window, _| rems(40.).to_pixels(window.rem_size()));
        let bounds = cx
            .debug_bounds("team-members-modal")
            .expect("the modal must be drawn");
        assert_eq!(
            bounds.size.width, medium,
            "a window of lists is the medium width"
        );
    }
}
