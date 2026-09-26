//! Posts an OS notification when an agent finishes answering, or when its
//! CLI exits.
//!
//! Two triggers, and each answers a different question. "Finished answering"
//! is `AgentView::is_answering()`'s `true → false` edge, held quiet for
//! `agent_notify_settings::AgentFinishedNotificationSetting::quiet_period`
//! before it is believed. "Session ended" is the tab's CLI actually exiting,
//! read the same way `keep_awake` reads it: from the terminal's own task
//! status, not from the tab being open (`agent_task` sets
//! `HideStrategy::Never`, so a tab deliberately outlives the process it
//! hosted). See `agent_notify_watch` for how both are wired to a real tab, and
//! `agent_notify_signals` for what happens once either is known.
//!
//! **The quiet period is a mitigation, not a fix.** There is no protocol-level
//! "reply finished" event -- these agents are ptys, not something this editor
//! speaks a protocol to (stated at `agent_view.rs:736-742`). `is_answering`
//! goes false during long tool calls and model waits exactly as often as it
//! goes false at the end of a real answer, and nothing observable here tells
//! the two apart. `keep_awake` answers the same ambiguity with a 60-second
//! grace; this feature's default is 12 seconds, a different trade that favours
//! fewer late notifications over more early ones. **A notification can and
//! will fire while the agent is still working**, whenever it pauses longer
//! than the configured period -- a slow model response, a long build, a big
//! file read. The setting exists so a user who is bitten by this can raise
//! it; it cannot be raised away entirely, because the signal it reads from
//! does not carry more information than this.
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

use gpui::{App, AppContext as _, Entity, Global};

mod agent_notify_settings;
mod agent_notify_signals;
mod agent_notify_watch;

pub use agent_notify_watch::AgentNotifier;

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

/// Wires the feature up, or does nothing at all.
///
/// Gated on [`App::can_post_notifications`] rather than building the entity
/// and letting it find out later: a platform with no implementation gets no
/// subscriptions, no timers, nothing watching agent tabs for the life of the
/// process -- the same "build nothing where the answer is no" rule
/// `crates/zed` applies to `keep_display_awake`.
pub fn init(cx: &mut App) {
    if !cx.can_post_notifications() {
        return;
    }

    let notifier = cx.new(AgentNotifier::new);
    let click_target = notifier.downgrade();
    let async_cx = cx.to_async();
    // `on_notification_activated`'s callback carries no `App` of its own --
    // unlike every other callback in this crate, which arrives already
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
    cx.set_global(GlobalAgentNotifier(notifier));
}

#[cfg(test)]
mod agent_notify_tests;
#[cfg(test)]
mod agent_notify_tests_more;
