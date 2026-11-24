//! Pushes what an agent wants somebody to know to the paired devices that are
//! not looking at it: an approval it is waiting for, an answer it finished, a
//! session that ended.
//!
//! The decision of who is pushed to is a pure function of plain values, so
//! that every rule can be asserted in a table; the host carries it out.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agent_notify::{AgentAttention, AttentionKind};
use futures::{FutureExt as _, select_biased};
use gpui::{App, Context, Task, WeakEntity};
use remote_relay_client::{
    PushOutcome, PushRequest, PushUrgency, sanitize_display_text, send_push,
    web_push_crypto::{self, PushSubscriptionKeys, WebPushError},
};
use remote_relay_protocol::Control;
use sha2::{Digest as _, Sha256};
use zode_account::PlanSummary;

use crate::{
    hosting::Hosting,
    remote_control_settings::{PushPreferences, RemoteControlSettings},
    remote_host::RemoteHost,
};

/// The longest tab title a push carries.
const MAX_TITLE_CHARS: usize = 120;

/// What `push_status` gives as the reason a device will not be pushed to.
pub(crate) const REASON_NOT_IN_PLAN: &str = "not_in_plan";
pub(crate) const REASON_DISABLED: &str = "disabled";
/// Told to the one device whose keys could not be kept because every place
/// for them is taken.
pub(crate) const REASON_TOO_MANY_DEVICES: &str = "too_many_devices";

/// How long a page's "I am visible" holds. A visible page says it again every
/// minute, so this allows for one message lost or late.
pub(crate) const PAGE_VISIBILITY_FRESHNESS: Duration = Duration::from_secs(150);

/// How long a push may take, credential and request together, before it is
/// given up on.
pub(crate) const PUSH_TIMEOUT: Duration = Duration::from_secs(15);

/// A push on its way, and what it was sent with.
pub(crate) struct PushSend {
    device_id: String,
    /// The keys it was encrypted for: a "gone" answer is about these only.
    keys: PushSubscriptionKeys,
    agent_id: String,
    kind: PushKind,
    /// [`Hosting::plan_generation`] when it was sent.
    plan_generation: u64,
    _task: Task<()>,
}

/// Whether a page that said it was visible at `since` still counts as on
/// screen at `now`.
pub(crate) fn still_visible(since: Option<Instant>, now: Instant) -> bool {
    since.is_some_and(|since| now.saturating_duration_since(since) <= PAGE_VISIBILITY_FRESHNESS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PushKind {
    Approval,
    Answered,
    Ended,
}

impl PushKind {
    pub(crate) fn from_attention(kind: AttentionKind) -> Self {
        match kind {
            AttentionKind::ApprovalNeeded => Self::Approval,
            AttentionKind::Answered { .. } => Self::Answered,
            AttentionKind::Ended => Self::Ended,
        }
    }

    fn wire_name(self) -> &'static str {
        match self {
            Self::Approval => "approval",
            Self::Answered => "answered",
            Self::Ended => "ended",
        }
    }

    pub(crate) fn urgency(self) -> PushUrgency {
        match self {
            Self::Approval => PushUrgency::High,
            Self::Answered | Self::Ended => PushUrgency::Normal,
        }
    }

    fn wanted(self, preferences: &PushPreferences) -> bool {
        match self {
            Self::Approval => preferences.approval,
            Self::Answered => preferences.answered,
            Self::Ended => preferences.ended,
        }
    }
}

/// What a connected device last said it shows.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LiveView<'a> {
    pub device_id: &'a str,
    pub visible: bool,
    pub agent_id: Option<&'a str>,
}

/// Everything besides the event that decides who is pushed to.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PushContext<'a> {
    /// The account service said the plan does not include pushes.
    pub not_in_plan: bool,
    pub preferences: PushPreferences,
    /// The sessions of devices in control right now.
    pub live: &'a [LiveView<'a>],
}

/// The devices, of those with push keys, to push `kind` about `agent_id` to.
pub(crate) fn recipients<'a>(
    kind: PushKind,
    agent_id: &str,
    context: &PushContext<'_>,
    devices_with_keys: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    if context.not_in_plan || !kind.wanted(&context.preferences) {
        return Vec::new();
    }
    devices_with_keys
        .into_iter()
        .filter(|device_id| {
            !context.live.iter().any(|view| {
                view.device_id == *device_id && view.visible && view.agent_id == Some(agent_id)
            })
        })
        .map(str::to_string)
        .collect()
}

/// The `push_status` a push-capable device is told.
pub(crate) fn push_status(not_in_plan: bool, preferences: &PushPreferences) -> Control {
    let reason = if not_in_plan {
        Some(REASON_NOT_IN_PLAN)
    } else if !preferences.any() {
        Some(REASON_DISABLED)
    } else {
        None
    };
    Control::PushStatus {
        enabled: reason.is_none(),
        reason: reason.map(str::to_string),
    }
}

/// The message a device's service worker reads once it has decrypted it.
pub(crate) fn payload(
    kind: PushKind,
    agent_id: &str,
    title: &str,
    host: &str,
    at_unix_ms: u64,
) -> Vec<u8> {
    serde_json::json!({
        "v": 1,
        "kind": kind.wire_name(),
        "agentId": agent_id,
        "title": sanitize_display_text(title, MAX_TITLE_CHARS),
        "host": host,
        "at": at_unix_ms,
    })
    .to_string()
    .into_bytes()
}

/// Groups pushes about the same agent, so the push service keeps only the
/// newest one waiting for a device that is offline: 32 hex characters, which
/// are base64url characters too.
pub(crate) fn topic(host_device_id: &str, agent_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(host_device_id.as_bytes());
    hasher.update([0]);
    hasher.update(agent_id.as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

impl Hosting {
    fn current_push_status(&self, cx: &App) -> Control {
        push_status(
            self.push_not_in_plan,
            &RemoteControlSettings::push_preferences(cx),
        )
    }

    /// Tells one device whether it will be pushed to, if it understands being
    /// told.
    pub(crate) fn send_push_status_to(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) {
        if !self
            .sessions
            .get(&session_id)
            .is_some_and(|session| session.push_capable())
        {
            return;
        }
        let status = self.current_push_status(cx);
        self.announced_push_status = Some(status.clone());
        self.push_control(session_id, status, cx);
    }

    /// Tells every device that understands it whether it will be pushed to, if
    /// that has changed since they were last told.
    pub(crate) fn announce_push_status(&mut self, cx: &mut Context<RemoteHost>) {
        let status = self.current_push_status(cx);
        if self.announced_push_status.as_ref() == Some(&status) {
            return;
        }
        self.announced_push_status = Some(status.clone());
        let capable: Vec<u32> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.push_capable())
            .map(|(id, _)| *id)
            .collect();
        for session_id in capable {
            self.push_control(session_id, status.clone(), cx);
        }
    }

    /// Only a device that has proven it holds the keys of a pin may leave its
    /// push keys here: anything else could have this host push its agents'
    /// titles to whoever it likes.
    pub(crate) fn subscribe_push(
        &mut self,
        session_id: u32,
        keys: PushSubscriptionKeys,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(session) = self.sessions.get(&session_id) else {
            return;
        };
        let device_id = session.peer_device_id.clone();
        if !session.confirmed || self.trust.get(&device_id).is_none() {
            log::warn!("ignoring push keys from a session that is not a paired device's");
            return;
        }
        if !self.push_keys.set(&device_id, keys, cx) {
            log::warn!("not keeping push keys: too many devices already have them");
            if self
                .sessions
                .get(&session_id)
                .is_some_and(|session| session.push_capable())
            {
                self.push_control(
                    session_id,
                    Control::PushStatus {
                        enabled: false,
                        reason: Some(REASON_TOO_MANY_DEVICES.to_string()),
                    },
                    cx,
                );
            }
            return;
        }
        self.send_push_status_to(session_id, cx);
    }

    pub(crate) fn unsubscribe_push(&mut self, session_id: u32, cx: &mut Context<RemoteHost>) {
        if let Some(device_id) = self
            .sessions
            .get(&session_id)
            .map(|session| session.peer_device_id.clone())
        {
            self.push_keys.forget(&device_id, cx);
        }
    }

    pub(crate) fn note_page_visibility(
        &mut self,
        session_id: u32,
        visible: bool,
        agent_id: Option<String>,
        now: Instant,
    ) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.page_visible_since = visible.then_some(now);
            session.page_agent = agent_id;
        }
    }

    /// Forgets a device's keys, and what was on its way to it: a device that
    /// was just forgotten gets nothing more.
    pub(crate) fn forget_push_keys(&mut self, device_id: &str, cx: &App) {
        self.push_keys.forget(device_id, cx);
        self.push_sends
            .retain(|_, send| send.device_id != device_id);
    }

    /// The account changed somehow. Only a different plan can change whether
    /// pushes are in it: anything else leaves a refusal standing, and so does
    /// a different plan that still says pushes are not in it. A plan that
    /// became unknown (signed out) says nothing, so it lifts nothing: only a
    /// plan that is read and has pushes in it does.
    pub(crate) fn plan_may_have_changed(
        &mut self,
        plan: Option<PlanSummary>,
        cx: &mut Context<RemoteHost>,
    ) {
        if plan == self.seen_plan {
            return;
        }
        let pushes_in_plan = plan
            .as_ref()
            .is_some_and(|plan| plan.limits.push_notifications);
        self.seen_plan = plan;
        self.plan_generation = self.plan_generation.wrapping_add(1);
        if !self.push_not_in_plan || !pushes_in_plan {
            return;
        }
        self.push_not_in_plan = false;
        self.announce_push_status(cx);
        self.mark_changed(cx);
    }

    /// Pushes what `attention` says about the agent listed as `agent_id` to
    /// every device that should hear of it.
    pub(crate) fn push_attention(
        &mut self,
        agent_id: &str,
        attention: &AgentAttention,
        cx: &mut Context<RemoteHost>,
    ) {
        let kind = PushKind::from_attention(attention.kind);
        let now = Self::now(cx);
        let live: Vec<LiveView<'_>> = self
            .sessions
            .values()
            .filter(|session| session.confirmed)
            .map(|session| LiveView {
                device_id: &session.peer_device_id,
                visible: still_visible(session.page_visible_since, now),
                agent_id: session.page_agent.as_deref(),
            })
            .collect();
        let context = PushContext {
            not_in_plan: self.push_not_in_plan,
            preferences: RemoteControlSettings::push_preferences(cx),
            live: &live,
        };
        let targets = recipients(kind, agent_id, &context, self.push_keys.device_ids());
        if targets.is_empty() {
            return;
        }
        let plaintext = payload(
            kind,
            agent_id,
            &attention.title,
            &self.host_name,
            unix_now_ms(),
        );
        let topic = topic(&self.local_device_id(cx).unwrap_or_default(), agent_id);
        for device_id in targets {
            // One push about the same thing at a time per device: a request
            // that hangs must not be joined by another each time it happens.
            let already_on_its_way = self.push_sends.values().any(|send| {
                send.device_id == device_id && send.agent_id == agent_id && send.kind == kind
            });
            if already_on_its_way {
                continue;
            }
            let Some(keys) = self.push_keys.get(&device_id).cloned() else {
                continue;
            };
            let body = match web_push_crypto::encrypt(&plaintext, &keys) {
                Ok(body) => body,
                Err(WebPushError::BadKey) => {
                    log::warn!("forgetting push keys that are not a usable P-256 key");
                    self.forget_push_keys(&device_id, cx);
                    continue;
                }
                Err(error) => {
                    log::error!("could not encrypt a push: {error}");
                    continue;
                }
            };
            let ticket = self.next_push_ticket;
            self.next_push_ticket = self.next_push_ticket.wrapping_add(1);
            let credentials = self.credentials.clone();
            let http_client = self.http_client.clone();
            let api_url = self.api_url.clone();
            let topic = topic.clone();
            let urgency = kind.urgency();
            let send_device_id = device_id.clone();
            let task = cx.spawn(async move |this: WeakEntity<RemoteHost>, cx| {
                let timeout = cx.background_executor().timer(PUSH_TIMEOUT).fuse();
                let credential = credentials.credential(cx);
                let delivery = async {
                    match credential.await {
                        Ok(credential) => {
                            send_push(
                                &http_client,
                                &api_url,
                                &credential.bearer,
                                PushRequest {
                                    device_id: &send_device_id,
                                    payload: &body,
                                    urgency,
                                    topic: &topic,
                                },
                            )
                            .await
                        }
                        Err(error) => PushOutcome::Dropped(error.to_string()),
                    }
                }
                .fuse();
                futures::pin_mut!(timeout, delivery);
                let outcome = select_biased! {
                    outcome = delivery => outcome,
                    _ = timeout => PushOutcome::Dropped(format!(
                        "no answer within {} seconds",
                        PUSH_TIMEOUT.as_secs()
                    )),
                };
                this.update(cx, |host, cx| host.finish_push(ticket, outcome, cx))
                    .ok();
            });
            self.push_sends.insert(
                ticket,
                PushSend {
                    device_id,
                    keys,
                    agent_id: agent_id.to_string(),
                    kind,
                    plan_generation: self.plan_generation,
                    _task: task,
                },
            );
        }
    }

    /// Acts on what the account service said about a push. Nothing is retried:
    /// the next event pushes again.
    pub(crate) fn finish_push(
        &mut self,
        ticket: u64,
        outcome: PushOutcome,
        cx: &mut Context<RemoteHost>,
    ) {
        let Some(send) = self.push_sends.remove(&ticket) else {
            return;
        };
        match outcome {
            PushOutcome::Accepted => {}
            // Sent under a plan that has since changed: what it says about
            // that plan is not news about this one.
            PushOutcome::NotInPlan if send.plan_generation != self.plan_generation => {}
            PushOutcome::NotInPlan => {
                if !self.push_not_in_plan {
                    log::info!("push notifications are not in this account's plan");
                    self.push_not_in_plan = true;
                    self.announce_push_status(cx);
                    self.mark_changed(cx);
                }
            }
            PushOutcome::SubscriptionGone => {
                self.push_keys
                    .forget_if_still(&send.device_id, &send.keys, cx);
            }
            PushOutcome::Dropped(reason) => log::warn!("a push was not delivered: {reason}"),
        }
    }

    pub(crate) fn push_needs_upgrade(&self) -> bool {
        self.push_not_in_plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT: &str = "terminal-7";

    fn live(
        device_id: &'static str,
        visible: bool,
        agent_id: Option<&'static str>,
    ) -> LiveView<'static> {
        LiveView {
            device_id,
            visible,
            agent_id,
        }
    }

    fn on<'a>(live: &'a [LiveView<'a>]) -> PushContext<'a> {
        PushContext {
            not_in_plan: false,
            preferences: PushPreferences::ALL,
            live,
        }
    }

    #[test]
    fn who_is_pushed_to() {
        let only_approval = PushPreferences {
            approval: true,
            answered: false,
            ended: false,
        };
        let watching = [live("phone", true, Some(AGENT))];
        let watching_another = [live("phone", true, Some("terminal-9"))];
        let watching_nothing = [live("phone", true, None)];
        let hidden_on_it = [live("phone", false, Some(AGENT))];
        let tablet_watching = [live("tablet", true, Some(AGENT))];
        let two_sessions_one_watching = [
            live("phone", false, Some(AGENT)),
            live("phone", true, Some(AGENT)),
        ];
        let not_in_plan = PushContext {
            not_in_plan: true,
            ..on(&[])
        };
        let nothing_wanted = PushContext {
            preferences: PushPreferences {
                approval: false,
                answered: false,
                ended: false,
            },
            ..on(&[])
        };
        let approval_only = PushContext {
            preferences: only_approval,
            ..on(&[])
        };
        let cases: Vec<(&str, PushKind, PushContext<'_>, Vec<&str>)> = vec![
            (
                "away devices are pushed to",
                PushKind::Answered,
                on(&[]),
                vec!["phone", "tablet"],
            ),
            (
                "plan without pushes",
                PushKind::Approval,
                not_in_plan,
                vec![],
            ),
            (
                "every event switched off",
                PushKind::Ended,
                nothing_wanted,
                vec![],
            ),
            (
                "this event switched on",
                PushKind::Approval,
                approval_only,
                vec!["phone", "tablet"],
            ),
            (
                "this event switched off",
                PushKind::Answered,
                approval_only,
                vec![],
            ),
            (
                "watching this agent",
                PushKind::Approval,
                on(&watching),
                vec!["tablet"],
            ),
            (
                "watching another agent",
                PushKind::Approval,
                on(&watching_another),
                vec!["phone", "tablet"],
            ),
            (
                "on screen, no agent open",
                PushKind::Ended,
                on(&watching_nothing),
                vec!["phone", "tablet"],
            ),
            (
                "page hidden on this agent",
                PushKind::Answered,
                on(&hidden_on_it),
                vec!["phone", "tablet"],
            ),
            (
                "another device watching",
                PushKind::Answered,
                on(&tablet_watching),
                vec!["phone"],
            ),
            (
                "any session of the device watching",
                PushKind::Answered,
                on(&two_sessions_one_watching),
                vec!["tablet"],
            ),
        ];
        for (name, kind, context, expected) in cases {
            assert_eq!(
                recipients(kind, AGENT, &context, ["phone", "tablet"]),
                expected,
                "{name}"
            );
        }
        assert!(
            recipients(PushKind::Approval, AGENT, &on(&[]), []).is_empty(),
            "nobody has keys"
        );
    }

    #[test]
    fn each_kind_has_its_wire_name_urgency_and_switch() {
        assert_eq!(
            PushKind::from_attention(AttentionKind::ApprovalNeeded),
            PushKind::Approval
        );
        assert_eq!(
            PushKind::from_attention(AttentionKind::Answered {
                from_transcript: false
            }),
            PushKind::Answered
        );
        assert_eq!(
            PushKind::from_attention(AttentionKind::Ended),
            PushKind::Ended
        );
        assert_eq!(PushKind::Approval.urgency(), PushUrgency::High);
        assert_eq!(PushKind::Answered.urgency(), PushUrgency::Normal);
        assert_eq!(PushKind::Ended.urgency(), PushUrgency::Normal);
    }

    #[test]
    fn the_status_says_why_not() {
        let none = PushPreferences {
            approval: false,
            answered: false,
            ended: false,
        };
        assert_eq!(
            push_status(false, &PushPreferences::ALL),
            Control::PushStatus {
                enabled: true,
                reason: None
            }
        );
        assert_eq!(
            push_status(true, &PushPreferences::ALL),
            Control::PushStatus {
                enabled: false,
                reason: Some("not_in_plan".into())
            }
        );
        assert_eq!(
            push_status(false, &none),
            Control::PushStatus {
                enabled: false,
                reason: Some("disabled".into())
            }
        );
        assert_eq!(
            push_status(true, &none),
            Control::PushStatus {
                enabled: false,
                reason: Some("not_in_plan".into())
            },
            "the plan is what an upgrade answers, so it is said first"
        );
    }

    #[test]
    fn the_payload_is_the_agreed_json_with_a_short_clean_title() {
        let bytes = payload(
            PushKind::Approval,
            AGENT,
            "Fix the build",
            "Ada's Mac",
            1_700_000_000_123,
        );
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(
            json,
            serde_json::json!({
                "v": 1,
                "kind": "approval",
                "agentId": AGENT,
                "title": "Fix the build",
                "host": "Ada's Mac",
                "at": 1_700_000_000_123u64,
            })
        );
        for kind in [PushKind::Answered, PushKind::Ended] {
            let json: serde_json::Value =
                serde_json::from_slice(&payload(kind, AGENT, "t", "h", 0)).expect("json");
            assert_eq!(json["kind"], kind.wire_name());
        }

        let long = format!("{}\u{202E}\n", "é".repeat(500));
        let json: serde_json::Value =
            serde_json::from_slice(&payload(PushKind::Ended, AGENT, &long, "h", 0)).expect("json");
        let title = json["title"].as_str().expect("a title");
        assert_eq!(title.chars().count(), MAX_TITLE_CHARS);
        assert!(!title.contains(['\u{202E}', '\n']));
    }

    #[test]
    fn the_topic_is_short_stable_base64url_and_per_agent() {
        let topic = |agent_id: &str| super::topic("host-device", agent_id);
        let first = topic(AGENT);
        assert_eq!(first.len(), 32);
        assert!(
            first
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
        );
        assert_eq!(topic(AGENT), first);
        assert_ne!(topic("terminal-8"), first);
        assert!(
            !first.contains(AGENT),
            "the agent id is not readable from it"
        );
        assert_ne!(
            super::topic("another-host", AGENT),
            first,
            "two hosts' tabs of the same id do not replace each other's pushes"
        );
    }
}
