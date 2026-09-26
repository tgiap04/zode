//! Keeps the display lit while an agent is producing an answer.
//!
//! The question is narrower than it looks, and there are two ways to get it
//! wrong, one behind the other.
//!
//! The first: an agent tab stays open after its CLI exits -- `agent_ui`'s
//! `agent_task` sets `HideStrategy::Never` on purpose, so the transcript
//! survives for reading. "A tab is open" is not "an agent is alive", and a lock
//! keyed on the former would pin the display on for the rest of the session.
//!
//! The second, and the one that survived the first fix: a CLI sitting at its
//! prompt waiting for you to type is alive and doing nothing. Holding the
//! display for it means walking away from an idle agent and coming back to a
//! screen that never slept. So the lock follows output actually arriving, and
//! not merely a live process.
//!
//! That question is [`AgentView::is_answering`](agent_ui::AgentView::is_answering)'s
//! to answer, and this module asks it rather than reassembling it. The
//! distinction matters: the raw
//! [`is_responding`](agent_ui::AgentView::is_responding) beside it counts pty
//! writes only, and an agent that hands its work to subagents goes quiet while
//! they think -- a CLI waiting on one repaints a spinner once or twice a second,
//! under that threshold, so the display used to sleep in the middle of work the
//! user was waiting for. `is_answering` already folds a running subagent in, and
//! already gates it on the CLI being alive so a subagent record stranded by a
//! killed process cannot pin the display on. Reading the settled answer keeps
//! that rule in one place instead of two crates deriving it separately.
//!
//! That answer is sampled rather than announced, so it is polled, and only
//! while some tab has a live CLI. A tab with none costs nothing: no timer, no
//! lock, no wake. The poll is also why [`RESPONDING_GRACE`] exists -- an agent
//! reading a file or waiting on a model writes nothing for seconds at a time,
//! and a lock taken and dropped across every one of those pauses would be worse
//! than no lock at all.
//!
//! Tabs arriving and leaving still come through
//! `workspace::Event::{ItemAdded, ItemRemoved}`, and the power source is re-read
//! on its own timer: pulling the charger raises no event this process can see.
//! That timer is tied to working agents rather than to holding the lock,
//! because on battery there is a working agent and no lock, and something still
//! has to notice the charger going back in.
//!
//! A plain terminal tab -- centre or dock, with no agent behind it -- obeys the
//! same output rule rather than a liveness one, and deliberately has no
//! substitute for "is the foreground process still running". A shell sitting at
//! its prompt has no `TaskStatus` and writes nothing, so it already fails the
//! rate test on its own; adding a foreground-process-group check would cost real
//! complexity for a case the output rule already gets right. Three cases were
//! checked before settling on this:
//!
//! - **Typing echo.** Roughly eight characters a second is a fast typist's
//!   pace, and crossing the threshold while typing changes nothing: the OS
//!   already resets its own idle timer on keystrokes, and the hold still
//!   expires [`RESPONDING_GRACE`] after the last one.
//! - **Idle repaint.** An `RPROMPT` clock, a `tmux` status line, `top`, `htop`
//!   repaint at one or two hertz -- under the threshold, so they rightly decline
//!   to hold. A TUI animating above that rate does hold, and it is genuinely
//!   producing output.
//! - **A process-group liveness check would be stale exactly when it matters.**
//!   The information such a check would read only refreshes when output is
//!   already flowing -- precisely when the rate rule has already answered, and
//!   stale the rest of the time. It would add a second source of truth that
//!   agrees with the first when correct and lies when it disagrees.
//!
//! A later change must not reach for that heuristic to "fix" a case the rate
//! rule already covers.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use agent_ui::{AgentView, RESPONDING_WINDOW, responding_at};
use gpui::{
    Anchor, App, Context, DisplayWakeLock, Entity, EntityId, IntoElement, Render, SharedString,
    Subscription, Task, WeakEntity, Window, div,
};
use settings::{RegisterSetting, Settings, SettingsContent, SettingsStore};
use terminal_view::TerminalView;
use terminal_view::terminal_panel::{TerminalPanel, TerminalPanelEvent};
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
    /// A lock is held for at least one tab producing output.
    Holding,
    /// Nothing is producing output, so there is nothing to hold it for.
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

/// How often each live tab is asked whether it is answering.
///
/// Matched to the window `is_responding` counts over: sampling slower than the
/// window would step over quiet gaps that never happened, and faster would ask
/// the same question twice about the same second.
const ACTIVITY_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// How long an agent goes on counting as answering after its last output.
///
/// An agent reading a file, running a command or waiting on a model writes
/// nothing for seconds at a time. Releasing the lock into each of those pauses
/// and taking it back afterwards would flap the display's wake state through a
/// single answer -- and each acquisition can cost a foreground stall. A minute
/// covers the pauses that happen inside one answer and still lets the display
/// sleep on time once the answer is finished.
const RESPONDING_GRACE: Duration = Duration::from_secs(60);

/// Holds one display-wake lock for as long as an agent is producing an answer.
pub struct KeepAwake {
    holds: Holds,
    watched: HashMap<EntityId, Watched>,
    _workspace: Subscription,
    /// Bound once `workspace::Event::PanelAdded` names the terminal dock --
    /// `None` until then, since the panel does not exist when `new` runs.
    _terminal_panel: Option<Subscription>,
    _settings: Subscription,
    /// Re-reads the power source while a lock is held, and does not exist
    /// otherwise -- an idle editor must not wake for this.
    power_check: Option<Task<()>>,
    /// Asks each live tab whether it is answering. Exists only while some tab
    /// has a live CLI, so an editor with no agent running wakes for nothing.
    activity_check: Option<Task<()>>,
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

/// What is kept alive per watched tab.
struct Watched {
    /// What kind of tab this is, and what following it takes.
    source: Source,
    /// When this tab was last seen producing output. `None` for a tab that has
    /// not answered since it started -- which is every tab that is merely
    /// sitting at its prompt.
    last_answered: Option<Instant>,
}

/// The two kinds of tab this module holds the display awake for, and what each
/// one takes to follow.
///
/// An agent's CLI can exit out from under an open tab, so that source awaits a
/// completion the way `agent_ui` already tracks it. A terminal has no such
/// signal to await -- see the module doc's note on the foreground-pgid
/// heuristic -- so its source is nothing more than the tab to poll.
enum Source {
    /// An agent tab.
    Agent {
        /// The tab itself, so the activity poll can ask it whether it is
        /// answering. Weak: the workspace owns the tab, and a strong handle
        /// here would keep a closed one alive.
        view: WeakEntity<AgentView>,
        /// Fires when the tab's state changes. An agent begins as
        /// `State::Starting` and only later holds a terminal, so the terminal
        /// has to be picked up on a later notification rather than when the
        /// tab is added.
        _view: Subscription,
        /// Which terminal `completion` is waiting on. Identity, not a flag,
        /// because `AgentView::restart` puts a brand new terminal in the same
        /// tab.
        terminal: Option<EntityId>,
        /// Awaits this tab's CLI exiting. `None` until a running terminal is
        /// seen, so a tab that never starts one costs nothing.
        completion: Option<Task<()>>,
    },
    /// A terminal tab, centre or dock.
    Terminal {
        /// Weak for the same reason as `Agent::view`: the workspace or the
        /// panel owns the tab.
        view: WeakEntity<TerminalView>,
        /// Arms the poll the instant output starts, rather than waiting for a
        /// tick of a poll that does not exist yet. Only starts it -- `settled`
        /// is the one place that decides whether it keeps running.
        _wakeup: Subscription,
    },
}

/// Whether a tab needs to be looked at again, given what is being awaited.
///
/// A free function so the rule can be tested without a terminal. The rule exists
/// because of `AgentView::restart` (reached from a mode switch, and from the
/// screen shown once a missing CLI is installed): it drops the old terminal and
/// starts a new one in the same tab. Asking merely "is a completion task
/// running" answers yes for the *dead* terminal's task, so the restarted agent
/// would be skipped and never hold the display again for the life of that tab.
/// Whether a tab still counts as answering, given when it last was.
///
/// A free function so the grace can be asserted without a terminal, a poll or
/// a clock: the rule is the whole difference between a lock that follows one
/// answer and a lock that flaps through it.
fn still_answering(last_answered: Option<Instant>, now: Instant) -> bool {
    last_answered.is_some_and(|at| now.duration_since(at) < RESPONDING_GRACE)
}

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
            (Some(name), 1) => format!("{name} is producing output"),
            (Some(_), count) => format!("{count} tabs are producing output"),
            (None, _) => "a tab is producing output".to_string(),
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
        let subscription = cx.subscribe(handle, |this, workspace, event, cx| match event {
            workspace::Event::ItemAdded { item } => {
                if let Some(view) = item.act_as::<AgentView>(cx) {
                    this.watch(view, cx);
                }
                // A terminal landing in a centre pane comes through here too --
                // `TerminalPanel` keeps its own panes and never reaches this
                // event, but the workspace's own panes do. Re-scanning rather
                // than reading `item` again also picks up any tab this event
                // does not itself describe.
                this.rescan(&workspace, cx);
            }
            workspace::Event::ItemRemoved { item_id } => {
                this.forget(*item_id, cx);
                this.rescan(&workspace, cx);
            }
            workspace::Event::PanelAdded(panel) => {
                // The dock's own terminals have no other way to reach this
                // module -- see the module doc on `TerminalPanelEvent`. The
                // panel does not exist yet when `new` runs, so this is the
                // only place its own subscription can be taken.
                if let Ok(panel) = panel.clone().downcast::<TerminalPanel>() {
                    let workspace = workspace.downgrade();
                    this._terminal_panel = Some(cx.subscribe(&panel, move |this, _, event, cx| {
                        let TerminalPanelEvent::TerminalsChanged = event;
                        if let Some(workspace) = workspace.upgrade() {
                            this.rescan(&workspace, cx);
                        }
                    }));
                }
                this.rescan(&workspace, cx);
            }
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
            _terminal_panel: None,
            _settings: settings,
            power_check: None,
            activity_check: None,
        };
        // Tabs restored from the last session exist before this entity does, so
        // the subscription above would never hear about them. The dock's own
        // panel is not one of them -- it is registered after this runs, and its
        // own `PanelAdded` arm picks it up once it exists.
        let existing: Vec<_> = workspace.items_of_type::<AgentView>(cx).collect();
        for view in existing {
            this.watch(view, cx);
        }
        let terminals: Vec<_> = workspace.items_of_type::<TerminalView>(cx).collect();
        for view in terminals {
            this.watch_terminal(view, cx);
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

    /// Asks every live tab whether it is answering, and moves the lock to match.
    ///
    /// Deliberately not through `settled`: this runs from the activity timer,
    /// and `settled` may drop that very timer from inside itself. Only `sync`
    /// runs here -- the paths that end a tab's CLI cancel the timer on their
    /// own.
    fn sample_activity(&mut self, cx: &mut Context<Self>) {
        // The executor's clock, not the wall clock: `settled`'s eligibility and
        // this grace both have to move together under `advance_clock`, or a
        // test can expire one and not the other.
        let now = cx.background_executor().now();
        let mut answering: Vec<(EntityId, SharedString)> = Vec::new();
        let mut quiet: Vec<EntityId> = Vec::new();

        for (id, watched) in &mut self.watched {
            let label = match &watched.source {
                Source::Agent { view, .. } => {
                    let Some(view) = view.upgrade() else {
                        quiet.push(*id);
                        continue;
                    };
                    let agent = view.read(cx);
                    if agent.is_answering() {
                        watched.last_answered = Some(now);
                    }
                    agent.tab_label()
                }
                Source::Terminal { view, .. } => {
                    let Some(view) = view.upgrade() else {
                        quiet.push(*id);
                        continue;
                    };
                    let terminal = view.read(cx).terminal().read(cx);
                    if responding_at(terminal.pty_writes_within(RESPONDING_WINDOW)) {
                        watched.last_answered = Some(now);
                    }
                    terminal.title(true).into()
                }
            };
            if still_answering(watched.last_answered, now) {
                answering.push((*id, label));
            } else {
                quiet.push(*id);
            }
        }

        // Applied after the walk rather than inside it: `set` and `clear` reach
        // the settings store and the platform, and neither can be called while
        // `watched` is still borrowed.
        let mut changed = false;
        for (id, label) in answering {
            changed |= self.holds.set(id, label, cx);
        }
        for id in quiet {
            changed |= self.holds.clear(id, cx);
        }
        if changed {
            cx.notify();
        }
    }

    /// Whether some watched tab still has something that could start producing
    /// output, which is what keeps the activity poll running.
    ///
    /// A tab that is alive and quiet holds nothing, and it is exactly the one
    /// the poll has to keep asking about, so this is not "is a tab holding" --
    /// that question is `holds.running`. An agent's surrogate is its CLI being
    /// alive; a terminal has no such signal (see the module doc), so its
    /// surrogate is recent output, which falls to zero on its own once
    /// [`RESPONDING_GRACE`] elapses -- the same clock the lock's own grace
    /// uses, so the poll and the lock expire together rather than one outliving
    /// the other.
    fn any_live(&self, cx: &App) -> bool {
        self.watched.values().any(|w| match &w.source {
            Source::Agent { completion, .. } => completion.is_some(),
            Source::Terminal { view, .. } => view.upgrade().is_some_and(|view| {
                view.read(cx)
                    .terminal()
                    .read(cx)
                    .pty_writes_within(RESPONDING_GRACE)
                    > 0
            }),
        })
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
        if !self.any_live(cx) {
            self.activity_check = None;
        } else if self.activity_check.is_none() {
            self.activity_check = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(ACTIVITY_CHECK_INTERVAL)
                        .await;
                    let live = this.update(cx, |this, cx| {
                        this.sample_activity(cx);
                        let live = this.any_live(cx);
                        // Cleared here, on the loop's own way out, rather than
                        // left for a later `settled` to notice: a `Task` still
                        // sitting in this field after its loop already
                        // returned would read as "still polling" to
                        // `is_polling`, and would stop `settled` from ever
                        // spawning a replacement once something worth polling
                        // shows up again.
                        if !live {
                            this.activity_check = None;
                        }
                        live
                    });
                    if !matches!(live, Ok(true)) {
                        break;
                    }
                }
            }));
        }

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
                source: Source::Agent {
                    view: view.downgrade(),
                    _view: subscription,
                    terminal: None,
                    completion: None,
                },
                last_answered: None,
            },
        );
        self.reread(view, cx);
    }

    /// Picks up one terminal tab, centre or dock. Unlike an agent tab, there is
    /// nothing to await here -- see the module doc on why a terminal has no
    /// liveness signal beyond its own output -- so this only arms the poll the
    /// moment output starts.
    fn watch_terminal(&mut self, view: Entity<TerminalView>, cx: &mut Context<Self>) {
        let id = view.entity_id();
        if self.watched.contains_key(&id) {
            return;
        }
        let terminal = view.read(cx).terminal().clone();
        let wakeup = cx.subscribe(&terminal, |this, _terminal, event, cx| {
            if matches!(event, terminal::Event::Wakeup) && this.activity_check.is_none() {
                this.settled(false, cx);
            }
        });
        self.watched.insert(
            id,
            Watched {
                source: Source::Terminal {
                    view: view.downgrade(),
                    _wakeup: wakeup,
                },
                last_answered: None,
            },
        );
    }

    /// Re-enumerates every terminal tab, centre and dock, adding any new one and
    /// dropping any that no longer exists.
    ///
    /// A full sweep rather than reading the triggering event's own payload: the
    /// dock's `TerminalsChanged` carries none on purpose (see its own doc), and
    /// re-enumerating both sides from scratch is simpler than two bespoke paths
    /// that have to agree.
    fn rescan(&mut self, workspace: &Entity<Workspace>, cx: &mut Context<Self>) {
        let mut views: Vec<Entity<TerminalView>> = workspace
            .read(cx)
            .items_of_type::<TerminalView>(cx)
            .collect();
        if let Some(panel) = workspace.read(cx).panel::<TerminalPanel>(cx) {
            for pane in panel.read(cx).panes() {
                views.extend(pane.read(cx).items_of_type::<TerminalView>());
            }
        }

        let live: HashSet<EntityId> = views.iter().map(Entity::entity_id).collect();
        for view in views {
            self.watch_terminal(view, cx);
        }

        let stale: Vec<EntityId> = self
            .watched
            .iter()
            .filter(|(id, watched)| {
                matches!(watched.source, Source::Terminal { .. }) && !live.contains(id)
            })
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.forget(id, cx);
        }
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
        let terminal_id = terminal.as_ref().map(|terminal| terminal.entity_id());

        let awaited = self
            .watched
            .get(&id)
            .and_then(|watched| match &watched.source {
                Source::Agent { terminal, .. } => *terminal,
                Source::Terminal { .. } => None,
            });
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
        if let Some(Watched {
            source:
                Source::Agent {
                    completion,
                    terminal,
                    ..
                },
            ..
        }) = self.watched.get_mut(&id)
        {
            *completion = None;
            *terminal = terminal_id;
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
                if let Some(Watched {
                    source:
                        Source::Agent {
                            completion,
                            terminal,
                            ..
                        },
                    last_answered,
                }) = this.watched.get_mut(&id)
                {
                    *completion = None;
                    *terminal = None;
                    *last_answered = None;
                }
                let changed = this.holds.clear(id, cx);
                this.settled(changed, cx);
            })
            .ok();
        });
        if let Some(Watched {
            source: Source::Agent { completion, .. },
            ..
        }) = self.watched.get_mut(&id)
        {
            *completion = Some(task);
        }
        // A live CLI makes this tab eligible, and nothing more. Whether it is
        // *answering* is what the lock follows, and only the poll can see that:
        // an agent waiting at its prompt is alive and doing nothing.
        let changed = false;
        self.settled(changed, cx);
    }
}

#[cfg(test)]
impl KeepAwake {
    /// Whether the activity poll is currently running. An accessor rather than
    /// a `pub(crate)` field, so a test proves the poll stopped through the same
    /// question `settled` itself asks rather than reaching past it.
    fn is_polling(&self) -> bool {
        self.activity_check.is_some()
    }
}

impl Status {
    /// The one line the menu shows under the toggle.
    fn explanation(self) -> &'static str {
        match self {
            Status::Holding => "The display is being held awake",
            Status::Idle => "Nothing is producing output",
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
                "Keeping the display awake while {} tabs work",
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
                "Keep display awake while a tab is producing output",
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
