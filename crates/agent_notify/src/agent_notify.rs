//! Posts an OS notification when an agent finishes answering, or when its
//! CLI exits.
//!
//! Two triggers, and each answers a different question. "Finished answering"
//! has two sources. For a tracked Claude tab (`AgentView::reports_turns`) it is
//! the transcript's own turn end (`TurnEvent::Ended`), held for a few seconds
//! so an immediate `Started` or `Interrupted` can retract it; that same
//! transcript also reports when the agent is waiting on an approval, which
//! posts its own notification. For every other tab it is
//! `AgentView::is_answering()`'s `true -> false` edge, held quiet for
//! `agent_notify_settings::AgentFinishedNotificationSetting::quiet_period`
//! before it is believed. "Session ended" is the tab's CLI actually exiting,
//! read the same way `keep_awake` reads it: from the terminal's own task
//! status, not from the tab being open (`agent_task` sets
//! `HideStrategy::Never`, so a tab deliberately outlives the process it
//! hosted). See `agent_notify_watch` for how both are wired to a real tab, and
//! `agent_notify_signals` for what happens once either is known.
//!
//! **On the heuristic path the quiet period is a mitigation, not a fix.**
//! There is no protocol-level "reply finished" event for these ptys (stated at
//! `agent_view.rs:736-742`). `is_answering` goes false during long tool calls
//! and model waits exactly as often as it goes false at the end of a real
//! answer, and nothing observable there tells the two apart. `keep_awake`
//! answers the same ambiguity with a 60-second grace; this feature's default
//! is 12 seconds, a different trade that favours fewer late notifications over
//! more early ones. **On that path a notification can and will fire while the
//! agent is still working**, whenever it pauses longer than the configured
//! period. The setting exists so a user who is bitten by this can raise it. A
//! transcript-backed tab does not read `is_answering` at all, so it has no
//! such false positives. If its transcript never produces a turn mark while
//! the CLI writes output for a minute -- it is somewhere this editor does not
//! look, or its format changed -- the tab stops reporting turns
//! (`AgentView::reports_turns` turns false) and takes the heuristic's
//! notification instead, the one every tab had before transcripts were read;
//! the first mark that does arrive puts it back.
//!
//! No focus check anywhere, by explicit user decision: a notification fires
//! even while a zode window is frontmost. `agent_notify_tests` carries a
//! guard test that scans this crate's production source for the two
//! window-focus signals a future contributor might reach for and fails if
//! either appears.
//!
//! Watched app-globally through `App::observe_new::<AgentView>`, not through
//! workspace events -- see `agent_notify_watch`'s doc for why that reaches
//! more of the app than `keep_awake`'s approach does. Notification bodies are
//! fixed phrases carrying no transcript content; titles are the tab's own
//! label. Notification history is visible on a lock screen on all three
//! platforms, which is why nothing from the conversation ever reaches the OS.

use gpui::{App, AppContext as _, Entity, EntityId, Global, SharedString};

mod agent_notify_settings;
mod agent_notify_signals;
mod agent_notify_watch;

pub use agent_notify_watch::AgentNotifier;

/// What an agent tab wants somebody to know, raised by [`AgentNotifier`]
/// whatever the desktop notification setting says, so that another device can
/// be told even while this screen is not to be interrupted. Raised once per
/// episode: a second approval request, or a second "finished", waits until the
/// tab has done something in between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAttention {
    /// The `AgentView` tab's own entity id.
    pub tab: EntityId,
    /// The tab's label.
    pub title: SharedString,
    pub kind: AttentionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionKind {
    /// The agent is waiting for an approval.
    ApprovalNeeded,
    /// The agent finished answering. `from_transcript` is true when the
    /// agent's own transcript said so; otherwise the terminal has been quiet
    /// for [`HEURISTIC_ANSWER_QUIET`], which a long tool call can also be.
    Answered { from_transcript: bool },
    /// The tab's CLI exited.
    Ended,
}

/// How long a tab whose turns are not reported by a transcript must be quiet
/// before it is announced as finished to another device -- the same grace
/// `keep_awake` gives the same ambiguous signal, and longer than the desktop
/// notification's, because a push that turns out wrong reaches somebody away
/// from the screen.
pub const HEURISTIC_ANSWER_QUIET: std::time::Duration = std::time::Duration::from_secs(60);

/// The least time between two answers the heuristic path raises for one tab.
/// Typing into a tab, or resizing it, looks like the agent working again.
pub const HEURISTIC_ANSWER_MIN_GAP: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Holds the one [`AgentNotifier`] this process builds. A newtype rather than
/// storing the entity directly as a global, so a stray `cx.global::<Entity<_>>()`
/// elsewhere in the app can never collide with this one by accident.
///
/// The field exists to be held, not read: it is the strong reference that
/// keeps the entity (and everything it watches) alive for the life of the
/// process. `agent_notify_tests` reads it back to reach the entity directly,
/// which is the only reason `dead_code` would otherwise fire on it outside
/// that build.
#[allow(dead_code)]
struct GlobalAgentNotifier(Entity<AgentNotifier>);

impl Global for GlobalAgentNotifier {}

impl AgentNotifier {
    /// The notifier [`init`] built.
    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalAgentNotifier>()
            .map(|global| global.0.clone())
    }

    /// Says whether anybody wants [`AgentAttention`] events. While nobody
    /// does, none is raised and no timer is armed for one.
    pub fn set_attention_wanted(&mut self, wanted: bool) {
        self.attention_wanted = wanted;
        if !wanted {
            for watched in self.watched.values_mut() {
                watched.attention_quiet = None;
            }
        }
    }
}

/// Wires the feature up.
///
/// The notifier is built on every platform, because [`AgentAttention`] is
/// what remote control pushes to another device, and that must not depend on
/// whether this machine can show a notification -- macOS cannot outside an
/// app bundle, which includes every `cargo run` build. Only the desktop side
/// is gated on [`App::can_post_notifications`]: no click handler is
/// registered here, and nothing is posted (see `agent_notify_signals`).
pub fn init(cx: &mut App) {
    let notifier = cx.new(AgentNotifier::new);
    if cx.can_post_notifications() {
        let click_target = notifier.downgrade();
        let async_cx = cx.to_async();
        // `on_notification_activated`'s callback carries no `App` of its own
        // -- unlike every other callback in this crate, which arrives already
        // inside one -- so an `AsyncApp` captured at registration time is what
        // gets one back. `AsyncApp::update` panics only if the app itself has
        // already been torn down, which this callback could not be invoked
        // after anyway.
        cx.on_notification_activated(move |id| {
            let Some(notifier) = click_target.upgrade() else {
                return;
            };
            async_cx.update(|cx| {
                notifier.update(cx, |notifier, cx| notifier.handle_click(&id, cx));
            });
        });
    }
    cx.set_global(GlobalAgentNotifier(notifier));
}

#[cfg(test)]
mod agent_notify_tests;
#[cfg(test)]
mod agent_notify_tests_attention;
#[cfg(test)]
mod agent_notify_tests_more;
#[cfg(test)]
mod agent_notify_tests_turns;
