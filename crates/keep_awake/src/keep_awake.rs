//! Keeps the display lit while an agent's CLI is still working.
//!
//! The question is narrower than it looks, and there is one specific way to get
//! it wrong. An agent tab stays open after its CLI exits -- `agent_ui`'s
//! `agent_task` sets `HideStrategy::Never` on purpose, so the transcript
//! survives for reading. So "an agent tab is open" is *not* "an agent is
//! working", and a lock keyed on the former would pin the display on for the
//! rest of the session. Only the task status of the tab's terminal answers it.
//!
//! Almost nothing here polls. Terminals report their own completion through
//! `Terminal::wait_for_completed_task`, and tabs arriving and leaving come
//! through `workspace::Event::{ItemAdded, ItemRemoved}`. The one exception is
//! the power source: pulling the charger raises no event this process can see,
//! so it is re-read on a timer -- and only while some agent is working. That is
//! not the same as "while a lock is held", and the difference matters: on
//! battery there is a working agent and no lock, and something still has to
//! notice the charger going back in. With no agent working this crate holds no
//! lock, runs no timer, and wakes for nothing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use agent_ui::AgentView;
use gpui::{
    Anchor, App, Context, DisplayWakeLock, Entity, EntityId, IntoElement, Render, SharedString,
    Subscription, Task, Window, div,
};
use settings::{RegisterSetting, Settings, SettingsContent, SettingsStore};
use ui::prelude::*;
use ui::{ButtonLike, ContextMenu, IconPosition, PopoverMenu, Tooltip};
use workspace::{StatusItemView, Workspace};

/// Whether the display may be held awake at all.
///
/// Read through `try_get` with a `true` fallback so a context without a settings
/// store -- a test, or startup before settings load -- behaves like the default
/// rather than silently turning the feature off.
#[derive(RegisterSetting)]
pub struct KeepDisplayAwakeSetting(pub bool);

impl Settings for KeepDisplayAwakeSetting {
    fn from_settings(content: &SettingsContent) -> Self {
        Self(content.keep_display_awake.unwrap())
    }
}

impl KeepDisplayAwakeSetting {
    fn is_enabled(cx: &App) -> bool {
        Self::try_get(cx).map(|setting| setting.0).unwrap_or(true)
    }
}

/// Why the display is, or is not, being held awake.
///
/// One value rather than a handful of booleans on the indicator, because the
/// menu has to say *which* reason: "off" and "on battery" and "this platform
/// cannot" all look identical as a dimmed icon, and a dimmed icon with no
/// explanation is the thing people file bugs about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// A lock is held for at least one working agent.
    Holding,
    /// No agent is working, so there is nothing to hold it for.
    Idle,
    /// An agent is working, but the machine is on battery.
    OnBattery,
    /// The user turned it off.
    Disabled,
    /// Everything says yes and the request was still refused.
    ///
    /// Not the same as "this platform has no implementation" -- `crates/zed` asks
    /// `App::can_keep_display_awake` first and does not build this entity at all
    /// where the answer is no, so a platform without an actuator has no
    /// indicator rather than a dimmed one. What is left here is a platform that
    /// claimed it could and then failed, which is worth showing rather than
    /// hiding.
    Unsupported,
}

/// How often the power source is re-read while a lock is held.
///
/// A minute is chosen because the worst case of being late is a display that
/// stays lit for up to a minute after the charger comes out, which is a cost
/// nobody can perceive as a bug. Anything shorter buys nothing and wakes the
/// process more often.
const POWER_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// How long to leave a failed acquisition alone before trying again.
///
/// `sync` is reachable from `observe_global::<SettingsStore>`, which fires on
/// *any* settings change anywhere in the app, and from the 60-second
/// `power_check` timer. Without a cooldown, a machine whose session bus never
/// answers (or whose ScreenSaver service refuses) pays the acquisition
/// attempt's up-to-250ms foreground stall on every single one of those --
/// which on an unrelated settings edit is a UI hitch with no connection to
/// anything the user just did. Thirty seconds is long enough that normal
/// editing does not retrigger the stall, short enough that a bus which comes
/// back (e.g. a session started before `dbus` finished starting) is picked up
/// without a restart.
const FAILED_ACQUISITION_COOLDOWN: Duration = Duration::from_secs(30);

/// Holds one display-wake lock for as long as any agent tab's CLI is running.
pub struct KeepAwake {
    holds: Holds,
    watched: HashMap<EntityId, Watched>,
    _workspace: Subscription,
    _settings: Subscription,
    /// Re-reads the power source while a lock is held, and does not exist
    /// otherwise -- an idle editor must not wake for this.
    power_check: Option<Task<()>>,
}

/// The bookkeeping half, kept apart from the wiring that feeds it.
///
/// Separate because this is where the mistakes live -- taking a lock twice,
/// releasing it while another agent is still working, never releasing it at all
/// -- and because a `Terminal` cannot be driven from a test without spawning a
/// real process. Split this way, every lock-lifetime rule is exercised by a test
/// (see the module's tests), and what stays uncovered is one straight-line read
/// of `TaskStatus` in `reread` below.
#[derive(Default)]
struct Holds {
    lock: Option<DisplayWakeLock>,
    /// The agent tabs whose CLI is still running. Keyed by tab because the common
    /// operation is removing one known tab when its CLI exits, and the value is
    /// that tab's label, which is what both the lock's reason and the indicator's
    /// tooltip want.
    ///
    /// Bounded by the number of open agent tabs: every insert is matched either
    /// by the completion task or by `ItemRemoved`.
    running: HashMap<EntityId, SharedString>,
    /// When the most recent acquisition attempt was refused, so `sync` can
    /// leave it alone for `FAILED_ACQUISITION_COOLDOWN` instead of retrying an
    /// identical call on every settings change. Cleared as soon as the
    /// situation changes in a way that could plausibly change the outcome --
    /// see the branch in `sync` that returns early with nothing to hold.
    last_failed_at: Option<Instant>,
}

/// What is kept alive per agent tab.
struct Watched {
    /// Fires when the tab's state changes. An agent begins as `State::Starting`
    /// and only later holds a terminal, so the terminal has to be picked up on a
    /// later notification rather than when the tab is added.
    _view: Subscription,
    /// Which terminal `completion` is waiting on. Identity, not a flag, because
    /// `AgentView::restart` puts a brand new terminal in the same tab.
    terminal: Option<EntityId>,
    /// Awaits this tab's CLI exiting. `None` until a running terminal is seen, so
    /// a tab that never starts one costs nothing.
    completion: Option<Task<()>>,
}

/// Whether a tab needs to be looked at again, given what is being awaited.
///
/// A free function so the rule can be tested without a terminal. The rule exists
/// because of `AgentView::restart` (reached from a mode switch, and from the
/// screen shown once a missing CLI is installed): it drops the old terminal and
/// starts a new one in the same tab. Asking merely "is a completion task
/// running" answers yes for the *dead* terminal's task, so the restarted agent
/// would be skipped and never hold the display again for the life of that tab.
fn needs_rereading(awaited: Option<EntityId>, current: Option<EntityId>) -> bool {
    awaited != current
}

impl Holds {
    /// Records that `id` is working. Returns whether the lock changed hands.
    fn set(&mut self, id: EntityId, label: SharedString, cx: &App) -> bool {
        self.running.insert(id, label);
        self.sync(cx)
    }

    /// Records that `id` is no longer working. Returns whether the lock changed.
    fn clear(&mut self, id: EntityId, cx: &App) -> bool {
        if self.running.remove(&id).is_none() {
            return false;
        }
        self.sync(cx)
    }

    /// Brings the lock in line with `running`. The only place a lock is taken or
    /// dropped, so there is exactly one answer to "why is the display awake".
    fn sync(&mut self, cx: &App) -> bool {
        // Cheapest test first, and the one that is true most of the time: with no
        // agent working there is nothing to ask the settings store or the OS.
        // `on_battery` is only consulted when a lock would otherwise be held.
        if self.running.is_empty()
            || !KeepDisplayAwakeSetting::is_enabled(cx)
            || cx.on_battery() == Some(true)
        {
            // Nothing eligible to hold for. Clearing the latch here, rather than
            // only on a timeout, is what makes the setting being toggled or the
            // charger coming back count as "something that could plausibly
            // change the outcome": the next time this becomes eligible again it
            // is treated as a fresh attempt, not a retry still in cooldown.
            self.last_failed_at = None;
            return self.lock.take().is_some();
        }
        // Already held. Deliberately not retaken to refresh the reason: the name
        // is fixed when the OS assertion is created, so a second agent starting
        // leaves `pmset` naming the first one. The indicator reads `holders()`
        // and stays correct; churning the assertion to fix a debug string is not
        // worth a release-and-recreate.
        if self.lock.is_some() {
            return false;
        }
        if let Some(last_failed_at) = self.last_failed_at
            && cx.background_executor().now() - last_failed_at < FAILED_ACQUISITION_COOLDOWN
        {
            // Still cooling down from a refusal, and nothing about the
            // situation has changed since. Skip the attempt rather than pay
            // its foreground stall for a call almost certain to fail again.
            return false;
        }
        let reason = self.reason();
        self.lock = cx.keep_display_awake(&reason);
        if self.lock.is_none() {
            self.last_failed_at = Some(cx.background_executor().now());
            log::warn!("this platform will not keep the display awake for {reason}");
            return false;
        }
        self.last_failed_at = None;
        true
    }

    /// Why the display is or is not held. Read by the indicator and its menu.
    ///
    /// **The lock is asked first, and that ordering is the whole point.** A held
    /// lock is ground truth: the OS assertion exists and the screen will not
    /// sleep, whatever the other conditions say. Everything below it only
    /// explains an *absence*.
    ///
    /// Reading `on_battery()` before the lock is the tempting order and it lies.
    /// `sync` re-reads the power source on events and on a 60-second timer, while
    /// this runs on every frame, so for up to a minute after the charger comes out
    /// the lock is still held and a live battery read is already `true`. In that
    /// window the earlier order reported "paused on battery" over a display that
    /// was demonstrably still being held awake -- an explanation contradicting
    /// what `pmset -g assertions` would show.
    ///
    /// Below the lock, the order is the order the reasons matter in: the setting
    /// is the user's own answer, and `Unsupported` is what is left when every
    /// reason to hold is true and there is still no lock.
    fn status(&self, cx: &App) -> Status {
        if self.lock.is_some() {
            return Status::Holding;
        }
        if !KeepDisplayAwakeSetting::is_enabled(cx) {
            return Status::Disabled;
        }
        if self.running.is_empty() {
            return Status::Idle;
        }
        if cx.on_battery() == Some(true) {
            Status::OnBattery
        } else {
            Status::Unsupported
        }
    }

    fn reason(&self) -> String {
        let count = self.running.len();
        match (self.running.values().next(), count) {
            (Some(name), 1) => format!("{name} is running"),
            (Some(_), count) => format!("{count} agents are running"),
            (None, _) => "an agent is running".to_string(),
        }
    }
}

impl KeepAwake {
    /// `workspace` and `handle` are the same workspace twice over, deliberately.
    /// This runs from `initialize_workspace`, which is inside an `observe_new` on
    /// `Workspace` -- so the entity is mid-update and `handle.read(cx)` would
    /// panic with "cannot read workspace::Workspace while it is already being
    /// updated", taking about thirty of the editor's own tests with it. The
    /// borrow already in hand is the only way to look at the existing tabs; the
    /// handle is for subscribing, which does not read.
    pub fn new(workspace: &Workspace, handle: &Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(handle, |this, _, event, cx| match event {
            workspace::Event::ItemAdded { item } => {
                if let Some(view) = item.act_as::<AgentView>(cx) {
                    this.watch(view, cx);
                }
            }
            workspace::Event::ItemRemoved { item_id } => this.forget(*item_id, cx),
            _ => {}
        });

        // Turning the setting off has to let go of a lock already held, not merely
        // stop the next one from being taken.
        let settings = cx.observe_global::<SettingsStore>(|this, cx| {
            let changed = this.holds.sync(cx);
            this.settled(changed, cx);
        });

        let mut this = Self {
            holds: Holds::default(),
            watched: HashMap::default(),
            _workspace: subscription,
            _settings: settings,
            power_check: None,
        };
        // Tabs restored from the last session exist before this entity does, so
        // the subscription above would never hear about them.
        let existing: Vec<_> = workspace.items_of_type::<AgentView>(cx).collect();
        for view in existing {
            this.watch(view, cx);
        }
        this
    }

    /// Whether the display is being held awake. The indicator reads this.
    pub fn is_holding(&self) -> bool {
        self.holds.lock.is_some()
    }

    /// The tabs currently keeping the display awake, for the tooltip.
    pub fn holders(&self) -> impl Iterator<Item = &SharedString> {
        self.holds.running.values()
    }

    /// Why the display is or is not held, for the indicator and its menu.
    pub fn status(&self, cx: &App) -> Status {
        self.holds.status(cx)
    }

    /// Applies a change in `holds`, keeping the power-check timer's existence tied
    /// to whether any agent is working. Every caller that touches `holds` goes
    /// through here, so there is one place where the timer can leak or go missing.
    ///
    /// Tied to *working agents* and not to *holding the lock*, which is the
    /// tempting version and is wrong: on battery there is an agent working and no
    /// lock, and something still has to notice the charger going back in. Keyed
    /// on the lock, the watcher would die with the release it performed and the
    /// hold would never come back.
    fn settled(&mut self, changed: bool, cx: &mut Context<Self>) {
        let watching_wanted = !self.holds.running.is_empty();
        if !watching_wanted {
            // Dropping the task cancels it. Safe from here because every path
            // that empties `running` is a different task from the timer's own.
            self.power_check = None;
        } else if self.power_check.is_none() {
            self.power_check = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(POWER_CHECK_INTERVAL).await;
                    // Deliberately not `settled`: that could drop this very task
                    // from inside itself. Only `sync` runs here, and the paths
                    // that empty `running` cancel this task on their own.
                    let working = this.update(cx, |this, cx| {
                        if this.holds.sync(cx) {
                            cx.notify();
                        }
                        !this.holds.running.is_empty()
                    });
                    if !matches!(working, Ok(true)) {
                        break;
                    }
                }
            }));
        }
        if changed {
            cx.notify();
        }
    }

    fn watch(&mut self, view: Entity<AgentView>, cx: &mut Context<Self>) {
        let id = view.entity_id();
        if self.watched.contains_key(&id) {
            return;
        }
        let subscription = cx.observe(&view, |this, view, cx| this.reread(view, cx));
        self.watched.insert(
            id,
            Watched {
                _view: subscription,
                terminal: None,
                completion: None,
            },
        );
        self.reread(view, cx);
    }

    fn forget(&mut self, id: EntityId, cx: &mut Context<Self>) {
        // Dropping `Watched` cancels the completion task, which is what stops a
        // closed tab from reporting back into a map it has left.
        self.watched.remove(&id);
        let changed = self.holds.clear(id, cx);
        self.settled(changed, cx);
    }

    /// Reads one tab's terminal and starts awaiting its exit if it is working.
    fn reread(&mut self, view: Entity<AgentView>, cx: &mut Context<Self>) {
        let id = view.entity_id();
        if !self.watched.contains_key(&id) {
            return;
        }

        let agent = view.read(cx);
        let terminal = agent
            .terminal()
            .map(|terminal_view| terminal_view.read(cx).terminal().clone());
        let label = agent.tab_label();
        let terminal_id = terminal.as_ref().map(|terminal| terminal.entity_id());

        let awaited = self.watched.get(&id).and_then(|watched| watched.terminal);
        if !needs_rereading(awaited, terminal_id) {
            return;
        }

        // `Unknown` means the terminal went away without reporting an exit code.
        // Counted as not working: releasing a moment early costs a display that
        // sleeps on time, while holding on a status nobody will ever update pins
        // the display on until the window closes. The rule lives on `AgentView`
        // so the sidebar's "which agents are running here" asks the same
        // question this does, and gets the same answer.
        let working = agent.is_working(cx);

        // Dropping any previous completion cancels the waiter on the terminal that
        // has just been replaced.
        if let Some(watched) = self.watched.get_mut(&id) {
            watched.completion = None;
            watched.terminal = terminal_id;
        }

        let Some(terminal) = terminal.filter(|_| working) else {
            let changed = self.holds.clear(id, cx);
            self.settled(changed, cx);
            return;
        };

        let completion = terminal.read(cx).wait_for_completed_task(cx);
        let task = cx.spawn(async move |this, cx| {
            completion.await;
            this.update(cx, |this, cx| {
                // Cleared so a later restart in this same tab is picked up rather
                // than mistaken for the terminal already being awaited.
                if let Some(watched) = this.watched.get_mut(&id) {
                    watched.completion = None;
                    watched.terminal = None;
                }
                let changed = this.holds.clear(id, cx);
                this.settled(changed, cx);
            })
            .ok();
        });
        if let Some(watched) = self.watched.get_mut(&id) {
            watched.completion = Some(task);
        }
        let changed = self.holds.set(id, label, cx);
        self.settled(changed, cx);
    }
}

impl Status {
    /// The one line the menu shows under the toggle.
    fn explanation(self) -> &'static str {
        match self {
            Status::Holding => "The display is being held awake",
            Status::Idle => "No agent is working",
            Status::OnBattery => "Paused - running on battery",
            Status::Disabled => "Turned off in settings",
            Status::Unsupported => "The system refused the request",
        }
    }

    fn icon_color(self) -> Color {
        match self {
            Status::Holding => Color::Accent,
            // Every other state means "not holding". Worth telling apart in the
            // menu, not on the bar: one dimmed glyph, one meaning.
            _ => Color::Muted,
        }
    }
}

impl Render for KeepAwake {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.holds.status(cx);
        let holders: Vec<SharedString> = self.holders().cloned().collect();
        let tooltip: SharedString = match (status, holders.as_slice()) {
            (Status::Holding, [one]) => {
                format!("Keeping the display awake while {one} works").into()
            }
            (Status::Holding, several) => format!(
                "Keeping the display awake while {} agents work",
                several.len()
            )
            .into(),
            (status, _) => status.explanation().into(),
        };

        div().child(
            PopoverMenu::new("keep-awake")
                .menu(move |window, cx| Some(Self::build_menu(status, window, cx)))
                // The item sits on a bar at the bottom of the window, so anywhere
                // below it is off-screen. `attach` is left unset on purpose:
                // `PopoverMenu` defaults it to the opposite corner of `anchor`,
                // which is already what this wants. (`right_click_menu` does not,
                // which is why the one in `agent_usage` states it.)
                .anchor(Anchor::BottomLeft)
                .trigger_with_tooltip(
                    ButtonLike::new("keep-awake-trigger")
                        .style(ButtonStyle::Subtle)
                        .child(
                            Icon::new(IconName::BoltOutlined)
                                .size(IconSize::Small)
                                .color(status.icon_color()),
                        ),
                    Tooltip::text(tooltip),
                ),
        )
    }
}

impl KeepAwake {
    /// The click menu: one switch, and one line saying what is actually going on.
    ///
    /// Built fresh on every open so the tick reflects the setting at that moment.
    /// A menu kept between opens would show the previous answer.
    fn build_menu(status: Status, window: &mut Window, cx: &mut App) -> Entity<ContextMenu> {
        ContextMenu::build(window, cx, move |menu, _window, cx| {
            let enabled = KeepDisplayAwakeSetting::is_enabled(cx);
            // `None` for the icon is what makes the tick appear: `ContextMenu`
            // draws `Icon::new(icon.unwrap_or(IconName::Check))`, so passing one
            // would *replace* the checkmark rather than join it.
            menu.toggleable_entry(
                "Keep display awake while an agent works",
                enabled,
                IconPosition::Start,
                None,
                move |_window, cx| {
                    // Re-read rather than invert the value captured when the menu
                    // was built: that reading is as old as the menu, and a handler
                    // flipping it would write the state from two changes ago.
                    let now_enabled = KeepDisplayAwakeSetting::is_enabled(cx);
                    settings::update_settings_file(
                        <dyn fs::Fs>::global(cx),
                        cx,
                        move |content, _| {
                            content.keep_display_awake = Some(!now_enabled);
                        },
                    );
                },
            )
            .separator()
            // A label, not an entry: there is nothing to click. It exists because
            // four of the five states are the same dimmed glyph on the bar, and
            // have to be told apart somewhere.
            .label(status.explanation())
        })
    }
}

impl StatusItemView for KeepAwake {
    /// Nothing to do. What holds the display awake is which agents are working,
    /// which is the same answer whichever tab is in front.
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn workspace::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

#[cfg(test)]
mod keep_awake_tests;
