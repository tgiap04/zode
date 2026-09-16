//! Agent subscription quota on the status bar.
//!
//! Shows how much of each agent's plan has been used and when the window resets,
//! in the shape the agents report it themselves:
//!
//! ```text
//! ✳ 53% used 1h 17m · 10% used 6d 12h · 0% used Fable
//! ```
//!
//! The two agents answer this question in completely different ways -- Claude
//! over HTTP against an OAuth endpoint, Codex over JSON-RPC to a subprocess that
//! holds its own credentials -- so they meet at [`UsageWindow`], a shape this
//! crate owns rather than either vendor's.

pub mod claude;
pub mod codex;
mod status_bar_items;
mod usage_store;
pub mod usage_panel;

use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui::{Anchor, App, Context, Entity, IntoElement, Render, Subscription, Window, div};
use project::AgentId;

use crate::usage_store::AgentUsageStore;
use settings::AgentUsageDisplay;
use ui::prelude::*;
use ui::{
    ButtonLike, CommonAnimationExt as _, ContextMenu, IconPosition, PopoverMenu, PopoverMenuHandle,
    right_click_menu,
};
use workspace::{ItemHandle, StatusBarSettings, StatusItemView, item::Settings as _};

use crate::usage_panel::UsagePanel;

gpui::actions!(
    agent_usage,
    [
        /// Toggles the agent usage panel on the status bar.
        ToggleUsagePanel
    ]
);

/// What kind of window a window is.
///
/// The two sources answer this in incompatible ways, which is the whole reason
/// this is an enum rather than a string. Claude names the kind outright
/// (`limits[].kind`); Codex names nothing at all but states the window's length,
/// so `Span` carries the length and the name is derived from it.
///
/// `Unknown` exists so an unrecognised kind costs a *label* and never a row: the
/// percentage is still true, and dropping the window would under-report the quota
/// silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    /// Claude's `kind: "session"` — the rolling window that fills up fastest.
    Session,
    /// Claude's `weekly_all` and `weekly_scoped` — the weekly allowance.
    Weekly,
    /// A window that reports its length instead of a name. Codex does this.
    Span(Duration),
    /// The source named a kind this build does not recognise, or named none.
    Unknown,
}

impl WindowKind {
    /// The two-or-three character tag that fits on the status bar: `5h`, `wk`,
    /// `30d`.
    ///
    /// Empty for `Unknown`, so an unrecognised window renders as a bare
    /// percentage rather than as a guess.
    pub fn short_tag(&self) -> String {
        match self {
            // Five hours is an assumption, and this is the line that would be
            // wrong first if it ever changed. It has two pieces of evidence
            // behind it: the same response carries a sibling field named
            // literally `five_hour` for this window, and the reference display
            // labels it `5h`.
            Self::Session => "5h".into(),
            Self::Weekly => "wk".into(),
            Self::Span(length) => Self::span_tag(*length),
            Self::Unknown => String::new(),
        }
    }

    /// The name with room to breathe, for the per-window detail.
    pub fn long_name(&self) -> String {
        match self {
            Self::Session => "Session (5h)".into(),
            Self::Weekly => "Weekly".into(),
            Self::Span(length) => format!("{} window", Self::span_tag(*length)),
            Self::Unknown => "Window".into(),
        }
    }

    /// A length as a tag, rounded to the unit that describes it.
    ///
    /// Derived rather than looked up in a table of known lengths: the one payload
    /// recorded from a real Codex account reports 43 200 minutes — thirty days —
    /// which is neither of the two lengths a table would have held.
    fn span_tag(length: Duration) -> String {
        let minutes = length.as_secs() / 60;
        if minutes >= 1_440 {
            format!("{}d", minutes / 1_440)
        } else if minutes >= 60 {
            format!("{}h", minutes / 60)
        } else {
            format!("{minutes}m")
        }
    }
}

/// One quota window, normalised away from whatever reported it.
///
/// A window is "50% used, resets in 1h 17m" or "0% used, for the Fable model" --
/// the second kind has no reset instant at all, which is why `resets_at` is an
/// `Option` rather than a sentinel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageWindow {
    /// Percent consumed, 0..=100.
    ///
    /// An integer because both sources already report one, and because the float
    /// alternative is ambiguous: Anthropic's `utilization` field can read as
    /// either a fraction or a percentage, and code that consumes it has to guess
    /// which. `limits[].percent` needs no guessing, so nothing here does either.
    pub percent: u8,
    /// When this window resets, for the windows that have a reset instant.
    ///
    /// `None` is a real answer, not missing data: a model-scoped weekly window
    /// reports no reset time, and the reference display shows the model's name
    /// where the countdown would otherwise go.
    pub resets_at: Option<DateTime<Utc>>,
    /// What this window is *about*, when a countdown cannot say it — the model
    /// name for a model-scoped window.
    pub label: Option<SharedString>,
    /// Which of the account's windows this is.
    ///
    /// Carried rather than inferred from position in the list: the order the
    /// sources happen to return their windows in is not a contract, and a display
    /// that labels rows by index is right until the day the order changes.
    pub kind: WindowKind,
}

/// What a source's answer means for what is on screen.
///
/// The two sources fail in different vocabularies — an HTTP status on one side, a
/// missing binary on the other — but the display only cares about three
/// outcomes, so each source maps its own errors onto these and the indicator
/// never learns either vocabulary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Fresh windows.
    Windows(Vec<UsageWindow>),
    /// Something went wrong, but the numbers already on screen are still the best
    /// available answer — a network blink, a timeout. Emptying the bar for these
    /// would make it flicker for no gain.
    Keep(SharedString),
    /// The numbers must go: the user is not entitled to them, or they describe a
    /// state that no longer exists. A stale figure here would be a wrong one.
    Clear(SharedString),
}

impl From<Result<Vec<UsageWindow>, claude::Unavailable>> for Outcome {
    fn from(result: Result<Vec<UsageWindow>, claude::Unavailable>) -> Self {
        match result {
            Ok(windows) => Outcome::Windows(windows),
            // A failed request keeps what is there; anything else means the
            // display is no longer warranted at all.
            Err(claude::Unavailable::Request(reason)) => Outcome::Keep(reason),
            // Keep, not Clear: being asked too often says nothing about whether
            // the numbers already on screen are still true.
            //
            // It no longer says "retrying", because it no longer does: this
            // endpoint refuses bursts, so asking again a second later was the
            // burst. The next poll comes round on its own.
            //
            // Short on purpose. This lands in a 340px panel row beside the agent's
            // name, and beside a countdown when there are stale numbers to report:
            // "Resets in 27d 2h · <this>". A sentence there is a sentence nobody
            // finishes reading — see `fixed_reasons_fit_the_panel_row`.
            Err(claude::Unavailable::RateLimited) => Outcome::Keep("asked too often".into()),
            // Also short. This one was 84 characters and would have overflowed
            // the panel row on its own, which nobody had noticed because it only
            // shows for a user who has set an ANTHROPIC_* variable.
            Err(claude::Unavailable::RuntimeOverride) => {
                Outcome::Clear("an ANTHROPIC_* override is set".into())
            }
            Err(claude::Unavailable::NoCredentials) => {
                Outcome::Clear("no Claude Code sign-in was found on this machine".into())
            }
            Err(claude::Unavailable::UnsupportedPlan) => {
                Outcome::Clear("this plan reports no quota windows".into())
            }
        }
    }
}

impl From<Result<Vec<UsageWindow>, codex::Unavailable>> for Outcome {
    fn from(result: Result<Vec<UsageWindow>, codex::Unavailable>) -> Self {
        match result {
            Ok(windows) => Outcome::Windows(windows),
            // Starting or reaching the app-server can fail transiently, so this
            // keeps; a missing CLI or a payload this build cannot read will not
            // fix itself between ticks, so those clear.
            Err(codex::Unavailable::Failed(reason)) => Outcome::Keep(reason),
            Err(codex::Unavailable::NotInstalled) => {
                Outcome::Clear("the codex CLI is not installed".into())
            }
            Err(codex::Unavailable::Unreadable(reason)) => Outcome::Clear(reason),
        }
    }
}

/// One agent's corner of the indicator.
///
/// Readable across the crate rather than private to the indicator, because the
/// panel renders the same state and must render *the same* state -- a second copy
/// kept in the panel would disagree with the bar the moment a refresh landed
/// between the two.
#[derive(Clone)]
pub(crate) struct SourceState {
    pub(crate) agent: AgentId,
    pub(crate) windows: Vec<UsageWindow>,
    pub(crate) fetched_at: Option<DateTime<Utc>>,
    /// Why there is nothing, or why what is there is not fresh.
    pub(crate) reason: Option<SharedString>,
}

impl SourceState {
    pub(crate) fn new(agent: AgentId) -> Self {
        Self {
            agent,
            windows: Vec::new(),
            fetched_at: None,
            reason: None,
        }
    }

    pub(crate) fn apply(&mut self, outcome: Outcome, now: DateTime<Utc>) {
        match outcome {
            Outcome::Windows(windows) => {
                self.windows = windows;
                self.reason = None;
                self.fetched_at = Some(now);
            }
            Outcome::Keep(reason) => {
                self.reason = Some(reason);
            }
            Outcome::Clear(reason) => {
                self.windows.clear();
                self.fetched_at = None;
                self.reason = Some(reason);
            }
        }
    }
}

/// The status-bar indicator.
///
/// A view over [`AgentUsageStore`] and nothing more. It holds no numbers of its
/// own: the status bar builds one of these per `Workspace`, and this fork keeps a
/// workspace per project inside one window, so state kept here would be state
/// kept N times — and, when it was, N poll loops against one endpoint on one
/// token.
///
/// Rendering nothing at all while there is nothing to say is the point rather
/// than a placeholder — a status bar that reserves space for absent data is worse
/// than one that does not mention it.
pub struct AgentUsageIndicator {
    store: Entity<AgentUsageStore>,
    /// Redraws this indicator when the shared numbers change.
    _observe: Subscription,
    _activation: Option<Subscription>,
    /// The panel's open/closed state, held here so an action can toggle it from
    /// outside the render.
    panel_handle: PopoverMenuHandle<UsagePanel>,
}

impl AgentUsageIndicator {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = AgentUsageStore::global(cx);

        let mut this = Self {
            _observe: cx.observe(&store, |_, _, cx| cx.notify()),
            store: store.clone(),
            _activation: None,
            panel_handle: PopoverMenuHandle::default(),
        };

        // Polling follows the application's attention. Nothing about quota is
        // urgent enough to justify a request every minute at a machine nobody is
        // sitting at -- and coming back to a stale number would be worse than the
        // request, which is why regaining focus asks at once rather than waiting
        // for the next tick.
        //
        // Every indicator in a window reports that one window's activation, so
        // the store is called once per indicator for one event and throttles
        // them back down to a single request.
        this._activation = Some(cx.observe_window_activation(window, |this, window, cx| {
            let active = window.is_window_active();
            this.store.update(cx, |store, cx| {
                if active {
                    store.window_activated(cx);
                } else {
                    store.window_deactivated(cx);
                }
            });
        }));

        // This window's own flag, deliberately not the store's
        // `should_keep_polling` -- the two answer different questions. "Is my
        // window active" is what decides whether this indicator opens the first
        // read; "is any window active" is what decides whether the shared loop
        // keeps running. Asking the second one here would let a window built
        // while a *different* window has focus claim that focus as its own.
        if window.is_window_active() {
            store.update(cx, |store, cx| store.window_activated(cx));
        }
        this
    }

    /// Whether an agent is one the user has asked to see.
    ///
    /// Named per agent rather than by index because that is how the settings name
    /// them, and because an agent this build gains later should be visible by
    /// default rather than silently off.
    fn agent_is_enabled(agent: &AgentId, settings: &StatusBarSettings) -> bool {
        match agent.as_ref() {
            project::CLAUDE_CODE_AGENT_ID => settings.claude_usage_button,
            project::CODEX_AGENT_ID => settings.codex_usage_button,
            _ => true,
        }
    }

    /// The sources with both something to say and permission to say it.
    ///
    /// A free function over a slice rather than a method: the numbers live in the
    /// store now, and the indicator reads a snapshot of them per draw.
    fn visible_sources<'a>(
        sources: &'a [SourceState],
        settings: &'a StatusBarSettings,
    ) -> impl Iterator<Item = &'a SourceState> {
        sources.iter().filter(move |source| {
            !source.windows.is_empty() && Self::agent_is_enabled(&source.agent, settings)
        })
    }

    /// Whether there is anything to draw.
    ///
    /// A source that answered but reported no windows counts as nothing: there is
    /// no number to show, and an icon on its own would read as "0%". A source the
    /// user has switched off counts as nothing for the same reason.
    fn has_anything_to_show(sources: &[SourceState], settings: &StatusBarSettings) -> bool {
        Self::visible_sources(sources, settings).next().is_some()
    }

    /// The panel's handle, for an action registered outside this crate.
    pub fn panel_handle(&self) -> PopoverMenuHandle<UsagePanel> {
        self.panel_handle.clone()
    }

    /// Whether a read is in flight, for the panel's refresh glyph.
    pub(crate) fn is_fetching(&self, cx: &App) -> bool {
        self.store.read(cx).is_fetching()
    }

    /// The state as it stands, for the panel to render.
    ///
    /// A clone taken in one read rather than a borrow held across a render: the
    /// panel is a separate entity, and reaching back into this one while it is
    /// being drawn is the mistake this crate's neighbours have paid for more than
    /// once.
    pub(crate) fn source_snapshot(&self, cx: &App) -> Vec<SourceState> {
        self.store.read(cx).source_states()
    }

    /// Re-reads both agents now, at the panel's request.
    ///
    /// Restarts the loop rather than firing a bare fetch, so the next automatic
    /// read is a full interval after this one instead of arriving moments later.
    pub(crate) fn refresh_now(&mut self, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| store.refresh_now(cx));
    }
}


impl Render for AgentUsageIndicator {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // An empty `div()` and not `h_flex()`: the latter is still a flex box
        // that the status bar's own gap applies to, so an "empty" item would
        // still push its neighbours over by one gap.
        let settings = StatusBarSettings::get_global(cx);
        // One read of the shared state per draw, rather than a borrow held while
        // the element tree is built.
        let sources = self.store.read(cx).source_states();
        if !Self::has_anything_to_show(&sources, settings) {
            return div();
        }

        let now = Utc::now();
        let fetching = self.store.read(cx).is_fetching();
        let compact = settings.agent_usage_display == AgentUsageDisplay::Compact;
        let groups = Self::visible_sources(&sources, settings)
            .map(|source| {
                // Compact keeps one window per agent rather than one window
                // overall: which agent a number belongs to is the thing the icon
                // is there to say, and dropping an agent entirely would make its
                // icon disappear rather than its detail.
                let line = if compact {
                    most_constrained(&source.windows, now)
                        .map(|window| render_window(window, now))
                        .unwrap_or_default()
                } else {
                    render_windows(&source.windows, now)
                };
                h_flex()
                    .gap_1()
                    .child(
                        Icon::new(agent_icon(&source.agent))
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(Label::new(line).size(LabelSize::Small).color(Color::Muted))
            })
            .collect::<Vec<_>>();

        let indicator = cx.weak_entity();
        let panel_handle = self.panel_handle.clone();
        // The toggle menu wraps the popover rather than the other way round: the
        // popover owns the left click, and `right_click_menu` only claims the
        // secondary button, so the two do not compete for the same gesture.
        div().child(
            right_click_menu::<ContextMenu>("agent-usage-toggles")
                .menu(|window, cx| Self::build_toggle_menu(window, cx))
                // Stated rather than left to the overflow clamp. Without this the
                // menu's own corner defaults to `TopLeft` and it opens downward off
                // the bottom of the window, then gets pushed back up by
                // `snap_to_window_with_margin` -- which lands in roughly the right
                // place today and stops doing so the moment the menu grows taller.
                .anchor(Anchor::BottomLeft)
                .attach(Anchor::TopLeft)
                .trigger(move |_open, _window, _cx| {
                    Self::render_trigger(indicator, groups, fetching, panel_handle)
                }),
        )
    }
}

/// Which glyph the refresh affordance draws while a read is or is not running.
///
/// Pulled out of the render so the choice has somewhere to be tested. It has to
/// agree with what `IconButton::loading` picks for the panel's own refresh
/// button -- the status bar and the panel are two views of one fact, and a
/// glyph that drifts on one of them is how that stops being true.
fn refresh_glyph(fetching: bool) -> IconName {
    if fetching {
        IconName::LoadCircle
    } else {
        IconName::ArrowCircle
    }
}

impl AgentUsageIndicator {
    /// The numbers, wrapped in the popover that opens the panel.
    fn render_trigger(
        indicator: gpui::WeakEntity<Self>,
        groups: Vec<impl IntoElement + 'static>,
        fetching: bool,
        panel_handle: PopoverMenuHandle<UsagePanel>,
    ) -> impl IntoElement {
        // Read before the handle is moved into the menu closure below.
        let indicator_id = indicator.entity_id();
        PopoverMenu::new("agent-usage")
            .menu(move |window, cx| {
                let indicator = indicator.clone();
                Some(cx.new(|cx| UsagePanel::new(indicator, window, cx)))
            })
            // Above and left-aligned: the item lives on a bar at the bottom of
            // the window, so anywhere below it is off-screen.
            .anchor(Anchor::BottomLeft)
            .with_handle(panel_handle)
            // A `ButtonLike` and not the bare row, because a popover trigger
            // has to be able to show itself pressed while the panel is open --
            // and it is the only button in this repo that takes arbitrary
            // children, which is what a row of numbers is.
            // `trigger`, not `trigger_with_tooltip`: the panel behind this click
            // already says every one of the things a tooltip could -- which agent,
            // when it was read, and why one is silent -- so a tooltip would be a
            // second copy of the panel that appears whether you asked for it or not.
            .trigger(
                ButtonLike::new("agent-usage-trigger")
                    .style(ButtonStyle::Subtle)
                    .child(
                        h_flex()
                            .gap_2()
                            .children(groups)
                            // The refresh glyph is part of the same target
                            // rather than a button beside it -- the whole group
                            // reads as one control, which is what the reference
                            // shows.
                            .child({
                                // The same two states the panel's own refresh
                                // button draws, in the same vocabulary: the
                                // glyph becomes `LoadCircle` and turns while a
                                // read is in flight. Colour alone said
                                // "working" only to someone who already knew
                                // what the two colours meant, and saying it
                                // one way here and another way there is how a
                                // mark stops carrying information.
                                let icon = Icon::new(refresh_glyph(fetching))
                                    .size(IconSize::XSmall)
                                    .color(if fetching {
                                        Color::Accent
                                    } else {
                                        Color::Muted
                                    });
                                if fetching {
                                    // Keyed by the entity rather than by this
                                    // call site's location. A window keeps its
                                    // own element state, so two windows would
                                    // not collide -- but this fork retains a
                                    // workspace per project inside one window,
                                    // and a caller-located id is one animation
                                    // state for every indicator that ever draws
                                    // under that roof.
                                    icon.with_keyed_rotate_animation(
                                        ("agent-usage-status-spinner", indicator_id),
                                        2,
                                    )
                                    .into_any_element()
                                } else {
                                    icon.into_any_element()
                                }
                            }),
                    ),
            )
    }

    /// The right-click menu: which agents' usage is shown.
    ///
    /// Built fresh on every open rather than kept around, so the ticks are read
    /// from the settings in force at that moment. A menu cached across opens would
    /// show yesterday's answer.
    ///
    /// `toggleable_entry` and no icon, deliberately: `ContextMenu` draws a toggled
    /// row as `Icon::new(icon.unwrap_or(IconName::Check))`, so an icon *replaces*
    /// the checkmark rather than joining it. Passing both shipped a menu where
    /// every row showed its own glyph twice and no row showed whether it was on.
    fn build_toggle_menu(window: &mut Window, cx: &mut App) -> Entity<ContextMenu> {
        ContextMenu::build(window, cx, |mut menu, _window, cx| {
            let settings = StatusBarSettings::get_global(cx);
            for item in status_bar_items::TOGGLEABLE_ITEMS {
                let read = item.read;
                let write = item.write;
                menu = menu.toggleable_entry(
                    item.label,
                    (item.read)(settings),
                    IconPosition::Start,
                    None,
                    move |_window, cx| {
                        // Re-read rather than invert a value captured when the menu
                        // was built: that reading is as old as the menu, and a
                        // handler that flipped it would write the setting the user
                        // had two states ago.
                        let now_showing = read(StatusBarSettings::get_global(cx));
                        settings::update_settings_file(
                            <dyn fs::Fs>::global(cx),
                            cx,
                            move |content, _| {
                                write(content.status_bar.get_or_insert_default(), !now_showing);
                            },
                        );
                    },
                );
            }
            menu
        })
    }
}

/// The name an agent goes by in a tooltip.
///
/// A second copy of what `project::BUILTIN_AGENTS` carries, reached through it
/// rather than duplicated: an id this build does not know still needs something
/// readable, and the id itself is the honest fallback.
fn agent_display_name(agent: &AgentId) -> String {
    project::builtin_agent(agent.as_ref())
        .map(|builtin| builtin.display_name.to_string())
        .unwrap_or_else(|| agent.as_ref().to_string())
}

/// The glyph for an agent.
///
/// Two match arms rather than a lookup, and deliberately a second copy of the one
/// in `agent_ui`: an agent's glyph is its vendor's mark, so a third agent is a
/// choice someone makes in both places rather than something to be derived. A
/// dependency on the whole conversation stack to share four lines would cost more
/// than it saves.
fn agent_icon(agent: &AgentId) -> IconName {
    match agent.as_ref() {
        project::CLAUDE_CODE_AGENT_ID => IconName::AiClaude,
        project::CODEX_AGENT_ID => IconName::AiOpenAi,
        _ => IconName::Sparkle,
    }
}

/// One window, in the shape the agents report it.
///
/// `now` is a parameter and not `Utc::now()` because a countdown read from the
/// wall clock cannot be asserted — every test of this would be a test of what
/// time it happened to be.
pub fn render_window(window: &UsageWindow, now: DateTime<Utc>) -> String {
    let mut rendered = format!("{}% used", window.percent);

    // A reset instant beats a label when both are present: the countdown is the
    // more useful of the two, and no real row carries both anyway.
    if let Some(resets_at) = window.resets_at {
        // `to_std` fails on a negative span, which is exactly the stale-data case
        // — the countdown then disappears rather than claiming `0m`.
        if let Some(countdown) = (resets_at - now).to_std().ok().and_then(format_countdown) {
            rendered.push(' ');
            rendered.push_str(&countdown);
        }
    } else if let Some(label) = &window.label {
        rendered.push(' ');
        rendered.push_str(label);
    }

    rendered
}

/// Every window of one agent, joined the way the reference display joins them.
pub fn render_windows(windows: &[UsageWindow], now: DateTime<Utc>) -> String {
    windows
        .iter()
        .map(|window| render_window(window, now))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The window closest to actually stopping you.
///
/// Highest percentage first, because that is the one that runs out. Ties break on
/// the nearer reset — of two windows equally full, the one that frees up sooner is
/// the less pressing of the two, so the *later* reset is the more constrained.
/// A window with no reset instant at all sorts last on that tiebreak: it is not
/// urgent in the way a countdown is.
///
/// Takes `now` so the comparison is against a fixed instant rather than the clock
/// moving under it mid-sort.
///
/// Two windows tied on *both* keys resolve to the later of the two, because
/// `max_by_key` returns the last maximum. Their percentages and countdowns are
/// identical by definition, so only the name shown differs — worth knowing, not
/// worth a tiebreak.
pub fn most_constrained(windows: &[UsageWindow], now: DateTime<Utc>) -> Option<&UsageWindow> {
    windows.iter().max_by_key(|window| {
        let until_reset = window
            .resets_at
            .map(|at| (at - now).num_seconds())
            // No reset instant means no deadline pressing, so it loses the
            // tiebreak rather than winning it by default.
            .unwrap_or(i64::MIN);
        (window.percent, until_reset)
    })
}

impl StatusItemView for AgentUsageIndicator {
    /// Nothing to do — and that is worth saying, because every other status item
    /// in this repo does something here.
    ///
    /// Quota belongs to the account, not to the buffer in front of you. The
    /// numbers are the same whichever tab is active, so following the active item
    /// would be work that could only produce the same answer.
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

/// Two units, never three: `6d 12h`, `1h 17m`, `17m`.
///
/// `None` once the instant has passed. A window whose reset time is in the past
/// means the data is stale, and rendering `0m` would state something the data
/// does not support — the countdown disappears and the percentage stands alone.
pub fn format_countdown(remaining: Duration) -> Option<String> {
    let total = remaining.as_secs();
    if total == 0 {
        return None;
    }

    let days = total / 86_400;
    let hours = (total % 86_400) / 3_600;
    let minutes = (total % 3_600) / 60;

    Some(if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    })
}

#[cfg(test)]
mod tests {
    use crate::usage_store::{AgentUsageStore, PollReason};
    use super::*;

    /// The countdown thresholds, including the one that produces nothing.
    ///
    /// `Duration` rather than a wall clock so this is deterministic — the same
    /// reason `render_window` will take `now` as an argument rather than reading
    /// it.
    /// The glyph, not just the tint, says whether a read is running.
    ///
    /// Colour alone carried this once, and it told a user who already knew what
    /// the two colours meant and nobody else. Reverting to a single glyph would
    /// still compile and still pass every other test here, which is the whole
    /// reason this one exists.
    #[test]
    fn the_refresh_glyph_changes_while_a_read_is_running() {
        assert_eq!(refresh_glyph(false), IconName::ArrowCircle);
        // Must stay the glyph `IconButton::loading` draws for the panel's own
        // refresh button, or the two surfaces say the same thing differently.
        assert_eq!(refresh_glyph(true), IconName::LoadCircle);
    }

    #[test]
    fn a_countdown_shows_the_two_largest_units() {
        assert_eq!(
            format_countdown(Duration::from_secs(6 * 86_400 + 12 * 3_600)).as_deref(),
            Some("6d 12h"),
            "the weekly window in the reference display"
        );
        assert_eq!(
            format_countdown(Duration::from_secs(3_600 + 17 * 60)).as_deref(),
            Some("1h 17m"),
            "the session window in the reference display"
        );
        assert_eq!(
            format_countdown(Duration::from_secs(17 * 60)).as_deref(),
            Some("17m"),
            "under an hour, minutes carry it alone"
        );
        assert_eq!(
            format_countdown(Duration::from_secs(0)),
            None,
            "a window already past its reset says nothing rather than `0m`"
        );
    }

    /// Days are shown with hours, never with minutes — `6d 12h`, not `6d 12h 30m`.
    #[test]
    fn a_countdown_never_shows_three_units() {
        let rendered = format_countdown(Duration::from_secs(6 * 86_400 + 12 * 3_600 + 30 * 60))
            .expect("well inside the window");
        assert_eq!(rendered, "6d 12h");
        assert!(
            !rendered.contains('m'),
            "minutes alongside days is noise at that scale"
        );
    }

    /// The three real rows, rendered exactly as the reference display shows them.
    ///
    /// `now` is pinned to the instant the payload was recorded, so this asserts
    /// the formatting rather than the clock.
    #[test]
    fn the_reference_display_is_reproduced() {
        let now: DateTime<Utc> = "2026-08-21T11:12:59.983789+00:00".parse().unwrap();

        let session = UsageWindow {
            percent: 53,
            resets_at: Some("2026-08-21T12:29:59.983789+00:00".parse().unwrap()),
            label: None,
            kind: WindowKind::Session,
        };
        let weekly = UsageWindow {
            percent: 10,
            resets_at: Some("2026-08-27T23:59:59.983816+00:00".parse().unwrap()),
            label: None,
            kind: WindowKind::Weekly,
        };
        let scoped = UsageWindow {
            percent: 0,
            resets_at: None,
            label: Some("Fable".into()),
            kind: WindowKind::Weekly,
        };

        assert_eq!(render_window(&session, now), "53% used 1h 17m");
        assert_eq!(render_window(&weekly, now), "10% used 6d 12h");
        assert_eq!(render_window(&scoped, now), "0% used Fable");

        assert_eq!(
            render_windows(&[session, weekly, scoped], now),
            "53% used 1h 17m · 10% used 6d 12h · 0% used Fable",
            "the whole line, joined the way the reference joins it"
        );
    }

    /// A reset time already past means the data is stale. The percentage stands
    /// alone rather than claiming the window resets in `0m`.
    #[test]
    fn a_window_past_its_reset_drops_the_countdown() {
        let now: DateTime<Utc> = "2026-08-21T13:00:00+00:00".parse().unwrap();
        let stale = UsageWindow {
            percent: 53,
            resets_at: Some("2026-08-21T12:29:59+00:00".parse().unwrap()),
            label: None,
            kind: WindowKind::Session,
        };

        assert_eq!(
            render_window(&stale, now),
            "53% used",
            "a negative countdown is not rendered at all"
        );
    }

    /// A window with neither a reset time nor a name is just its percentage.
    #[test]
    fn a_window_with_nothing_to_qualify_it_is_just_a_percentage() {
        let now = Utc::now();
        let bare = UsageWindow {
            percent: 42,
            resets_at: None,
            label: None,
            kind: WindowKind::Unknown,
        };
        assert_eq!(render_window(&bare, now), "42% used");
    }

    /// Each agent keeps its own glyph, and an unknown one still gets something.
    #[test]
    fn every_agent_has_its_own_glyph() {
        let claude = agent_icon(&AgentId::new(project::CLAUDE_CODE_AGENT_ID.to_string()));
        let codex = agent_icon(&AgentId::new(project::CODEX_AGENT_ID.to_string()));

        assert_ne!(
            claude, codex,
            "with two agents on one bar the glyph is what says which numbers are whose"
        );
        assert_eq!(
            agent_icon(&AgentId::new("something-else".to_string())),
            IconName::Sparkle,
            "an agent this build does not know still gets a mark"
        );
    }

    fn a_window() -> UsageWindow {
        UsageWindow {
            percent: 53,
            resets_at: None,
            label: None,
            kind: WindowKind::Session,
        }
    }

    fn nothing() -> Outcome {
        Outcome::Clear("nothing here".into())
    }

    /// Compact keeps the fullest window, not the first one.
    ///
    /// The reference payload happens to list the fullest window first, so a
    /// `.first()` would pass on it and be wrong the moment the weekly window
    /// overtakes the session one — which is the ordinary case late in a week.
    #[test]
    fn compact_keeps_the_window_closest_to_running_out() {
        let now: DateTime<Utc> = "2026-08-21T11:12:59+00:00".parse().unwrap();
        let windows = vec![
            UsageWindow {
                percent: 12,
                resets_at: Some("2026-08-21T12:29:59+00:00".parse().unwrap()),
                label: None,
                kind: WindowKind::Session,
            },
            UsageWindow {
                percent: 88,
                resets_at: Some("2026-08-27T23:59:59+00:00".parse().unwrap()),
                label: None,
                kind: WindowKind::Weekly,
            },
        ];

        let chosen = most_constrained(&windows, now).expect("two windows to choose from");
        assert_eq!(
            chosen.percent, 88,
            "the weekly window is the one that will stop you, even listed second"
        );
    }

    /// Equal percentages break on the reset that is further away.
    ///
    /// Of two windows equally full, the one that frees up sooner is the *less*
    /// pressing — so the later reset wins, not the nearer one.
    #[test]
    fn a_tie_on_percent_breaks_on_the_later_reset() {
        let now: DateTime<Utc> = "2026-08-21T11:12:59+00:00".parse().unwrap();
        let soon = UsageWindow {
            percent: 40,
            resets_at: Some("2026-08-21T12:00:00+00:00".parse().unwrap()),
            label: None,
            kind: WindowKind::Session,
        };
        let later = UsageWindow {
            percent: 40,
            resets_at: Some("2026-08-27T12:00:00+00:00".parse().unwrap()),
            label: None,
            kind: WindowKind::Weekly,
        };

        assert_eq!(
            most_constrained(&[soon.clone(), later.clone()], now)
                .unwrap()
                .kind,
            WindowKind::Weekly
        );
        assert_eq!(
            most_constrained(&[later, soon], now).unwrap().kind,
            WindowKind::Weekly,
            "and the answer does not depend on the order they arrived in"
        );
    }

    /// A window with no reset instant loses the tiebreak rather than winning it.
    ///
    /// `None` here is a model-scoped window, which has no deadline at all — it
    /// must not outrank a genuine countdown by accident.
    #[test]
    fn a_window_with_no_deadline_does_not_win_the_tiebreak() {
        let now = Utc::now();
        let scoped = UsageWindow {
            percent: 40,
            resets_at: None,
            label: Some("Fable".into()),
            kind: WindowKind::Weekly,
        };
        let counting = UsageWindow {
            percent: 40,
            resets_at: Some(now + chrono::Duration::hours(1)),
            label: None,
            kind: WindowKind::Session,
        };

        assert_eq!(
            most_constrained(&[scoped, counting], now).unwrap().kind,
            WindowKind::Session
        );
    }

    /// Nothing to choose from is `None`, not a panic and not a placeholder.
    #[test]
    fn no_windows_means_no_choice() {
        assert!(most_constrained(&[], Utc::now()).is_none());
    }

    /// Switching an agent off hides that agent and only that agent.
    #[test]
    fn switching_one_agent_off_leaves_the_other_alone() {
        let mut store = AgentUsageStore::test_new();
        store.apply(
            Outcome::Windows(vec![a_window()]),
            Outcome::Windows(vec![a_window()]),
            Utc::now(),
        );

        let mut settings = all_agents_shown();
        settings.codex_usage_button = false;

        let sources = store.source_states();
        let visible: Vec<_> = AgentUsageIndicator::visible_sources(&sources, &settings)
            .map(|source| source.agent.as_ref().to_string())
            .collect();
        assert_eq!(
            visible,
            vec![project::CLAUDE_CODE_AGENT_ID.to_string()],
            "Codex is switched off; Claude is untouched"
        );
        assert!(AgentUsageIndicator::has_anything_to_show(&store.source_states(), &settings));
    }

    /// Switching both off leaves nothing to draw — and nothing to right-click.
    ///
    /// The second half is the known cost of putting the toggle menu on this item:
    /// with both agents hidden there is no target left, so the way back is the
    /// settings file. Asserted here so the hole is recorded rather than discovered.
    #[test]
    fn switching_both_agents_off_leaves_nothing_at_all() {
        let mut store = AgentUsageStore::test_new();
        store.apply(
            Outcome::Windows(vec![a_window()]),
            Outcome::Windows(vec![a_window()]),
            Utc::now(),
        );

        let mut settings = all_agents_shown();
        settings.claude_usage_button = false;
        settings.codex_usage_button = false;

        assert!(
            !AgentUsageIndicator::has_anything_to_show(&store.source_states(), &settings),
            "no numbers, no icons, and no right-click target"
        );
    }

    /// The settings a fresh install has: both agents on, every window shown.
    ///
    /// Built rather than read from the global store, because `Settings::get_global`
    /// needs a settings store initialised in the test app and these are tests of
    /// the display rules, not of settings loading.
    fn all_agents_shown() -> StatusBarSettings {
        StatusBarSettings {
            show: true,
            show_active_file: false,
            active_language_button: true,
            cursor_position_button: true,
            line_endings_button: false,
            active_encoding_button: settings::EncodingDisplayOptions::NonUtf8,
            claude_usage_button: true,
            codex_usage_button: true,
            agent_usage_display: AgentUsageDisplay::Detailed,
            activity_indicator: true,
            active_toolchain_button: true,
            vim_mode_button: true,
            image_info_button: true,
        }
    }

    /// The seam `refresh` leans on to decide which agents to ask.
    ///
    /// `may_read_usage()` disables all real I/O under `test-support`, so no test
    /// here can observe an HTTP request or a subprocess spawn actually being
    /// skipped -- this is what can be proven instead: the decision is read
    /// straight off the settings, independently per agent.
    #[test]
    fn agents_to_fetch_reflects_each_agent_independently() {
        assert_eq!(
            AgentUsageStore::agents_to_fetch(&all_agents_shown()),
            (true, true),
            "defaults: both agents are asked"
        );

        let mut claude_off = all_agents_shown();
        claude_off.claude_usage_button = false;
        assert_eq!(
            AgentUsageStore::agents_to_fetch(&claude_off),
            (false, true),
            "Claude switched off, Codex untouched"
        );

        let mut codex_off = all_agents_shown();
        codex_off.codex_usage_button = false;
        assert_eq!(
            AgentUsageStore::agents_to_fetch(&codex_off),
            (true, false),
            "Codex switched off, Claude untouched"
        );

        let mut both_off = all_agents_shown();
        both_off.claude_usage_button = false;
        both_off.codex_usage_button = false;
        assert_eq!(
            AgentUsageStore::agents_to_fetch(&both_off),
            (false, false),
            "both off: nothing to fetch"
        );
    }

    /// Clearing a disabled agent's source must not disturb the agent still on
    /// screen.
    ///
    /// This is the guarantee the substitution in `refresh` leans on: `apply`
    /// writes each source independently, so an `Outcome::Clear` standing in for
    /// a disabled Claude cannot bleed into Codex's windows.
    #[test]
    fn clearing_one_source_leaves_the_other_untouched() {
        let now = Utc::now();
        let mut store = AgentUsageStore::test_new();
        store.apply(
            Outcome::Windows(vec![a_window()]),
            Outcome::Windows(vec![a_window()]),
            now,
        );

        // Codex gets `Keep`, not another `Windows` or a `Clear`: this isolates
        // the assertion to `apply`'s independence between sources rather than
        // to what a real second fetch would have returned.
        store.apply(
            AgentUsageStore::disabled_outcome(),
            Outcome::Keep("still up to date".into()),
            now,
        );

        assert!(
            store.source_at(0).windows.is_empty(),
            "the disabled agent's windows are cleared"
        );
        assert_eq!(
            store.source_at(0).fetched_at, None,
            "and its read time goes with them"
        );
        assert_eq!(
            store.source_at(1).windows.len(),
            1,
            "the agent still on screen is untouched by the other's Clear"
        );
    }

    /// `Keep` holds the numbers; `Clear` takes them away.
    ///
    /// This is the one decision in the refresh path that is not obvious, and it
    /// goes both ways for a reason: a network blink should not empty the bar, but
    /// a number the user is no longer entitled to see is describing a state that
    /// no longer exists.
    #[test]
    fn keep_holds_the_last_numbers_and_clear_takes_them_away() {
        let now = Utc::now();

        let mut store = AgentUsageStore::test_new();
        store.apply(Outcome::Windows(vec![a_window()]), nothing(), now);
        assert!(
            AgentUsageIndicator::has_anything_to_show(&store.source_states(), &all_agents_shown()),
            "a good read shows numbers"
        );

        store.apply(
            Outcome::Keep("the endpoint could not be reached".into()),
            nothing(),
            now,
        );
        assert!(
            AgentUsageIndicator::has_anything_to_show(&store.source_states(), &all_agents_shown()),
            "a transient failure must not empty the bar -- the panel carries the reason"
        );
        assert_eq!(
            store.source_at(0).reason.as_ref().map(|r| r.as_ref()),
            Some("the endpoint could not be reached"),
            "and the reason is kept verbatim for the panel to show"
        );

        store.apply(
            Outcome::Clear("no sign-in was found".into()),
            nothing(),
            now,
        );
        assert!(
            !AgentUsageIndicator::has_anything_to_show(&store.source_states(), &all_agents_shown()),
            "a number the user is no longer entitled to must go"
        );
        assert_eq!(
            store.source_at(0).fetched_at, None,
            "and the read time goes with it, or the panel would date absent data"
        );
    }

    /// Each source's failure vocabulary maps onto the same three outcomes.
    ///
    /// This is what lets the indicator stay ignorant of HTTP statuses and missing
    /// binaries alike — and the mapping is where a mistake would be invisible.
    #[test]
    fn each_source_maps_its_own_failures_onto_the_shared_outcomes() {
        assert!(matches!(
            Outcome::from(Err::<_, claude::Unavailable>(claude::Unavailable::Request(
                "blink".into()
            ))),
            Outcome::Keep(_)
        ));
        for cleared in [
            claude::Unavailable::RuntimeOverride,
            claude::Unavailable::NoCredentials,
            claude::Unavailable::UnsupportedPlan,
        ] {
            assert!(
                matches!(
                    Outcome::from(Err::<Vec<_>, _>(cleared.clone())),
                    Outcome::Clear(_)
                ),
                "{cleared:?} means the display is no longer warranted"
            );
        }

        assert!(matches!(
            Outcome::from(Err::<_, codex::Unavailable>(codex::Unavailable::Failed(
                "would not start".into()
            ))),
            Outcome::Keep(_),
        ));
        assert!(matches!(
            Outcome::from(Err::<Vec<_>, _>(codex::Unavailable::NotInstalled)),
            Outcome::Clear(_),
        ));
        assert!(
            matches!(
                Outcome::from(Err::<Vec<_>, _>(codex::Unavailable::Unreadable(
                    "odd".into()
                ))),
                Outcome::Clear(_),
            ),
            "a payload this build cannot read will not fix itself between ticks"
        );
    }

    /// The reasons this crate writes itself have to fit a 340px panel row next to
    /// the agent's name, and next to a countdown when there are stale numbers too.
    /// The row truncates rather than overflowing now, but a truncated sentence
    /// tells the reader nothing — so these stay short.
    ///
    /// The bound is the longest string that was already here when this was
    /// written. It exists because a 61-character one was added and overflowed the
    /// panel on sight, and writing it revealed a pre-existing 84-character one.
    ///
    /// **Scope, deliberately narrow.** This covers only the variants carrying a
    /// fixed string. `Unavailable::Request` and `codex::Unavailable::Failed`
    /// interpolate an OS error — `codex.rs` formats
    /// `"codex app-server would not start: {error}"`, and a routine
    /// `std::io::Error` message takes that past 70 characters. Those cannot be
    /// bounded at compile time, so what protects the row for them is the layout's
    /// `min_w_0` + `truncate`, not any promise about length. Do not read this test
    /// as covering them.
    #[test]
    fn fixed_reasons_fit_the_panel_row() {
        const BUDGET: usize = 48;

        let reasons = [
            Outcome::from(Err::<Vec<_>, _>(claude::Unavailable::RateLimited)),
            Outcome::from(Err::<Vec<_>, _>(claude::Unavailable::RuntimeOverride)),
            Outcome::from(Err::<Vec<_>, _>(claude::Unavailable::NoCredentials)),
            Outcome::from(Err::<Vec<_>, _>(claude::Unavailable::UnsupportedPlan)),
            Outcome::from(Err::<Vec<_>, _>(codex::Unavailable::NotInstalled)),
        ];

        for outcome in &reasons {
            let reason = match outcome {
                Outcome::Keep(reason) | Outcome::Clear(reason) => reason,
                Outcome::Windows(_) => continue,
            };
            assert!(
                reason.chars().count() <= BUDGET,
                "{reason:?} is {} characters; the panel row has room for about \
                 {BUDGET} beside the agent's name",
                reason.chars().count()
            );
        }
    }

    /// Regaining focus used to fetch unconditionally, so alt-tabbing in and out
    /// was one request per tab — against an endpoint this editor shares with the
    /// Claude Code CLI on one token. These are the conditions under which that
    /// immediate fetch may be skipped.
    #[test]
    fn an_attempt_made_recently_lets_activation_skip_asking_again() {
        let now = Utc::now();
        let mut store = AgentUsageStore::test_new();

        assert!(
            store.should_fetch(PollReason::Activation, now),
            "nothing has been asked yet — the first activation is the one the user \
             notices, and it must not be the one that is skipped"
        );

        store.set_last_polled_at(Some(now));
        assert!(!store.should_fetch(PollReason::Activation, now), "just asked");
        assert!(
            !store.should_fetch(PollReason::Activation, now + chrono::Duration::seconds(29)),
            "still inside the window"
        );
        assert!(
            store.should_fetch(PollReason::Activation, now + chrono::Duration::seconds(31)),
            "past the window, ask again"
        );
    }

    /// The throttle must key on the attempt, never on the answer.
    ///
    /// An earlier version asked "is every source's data fresh?" — which reads as
    /// the careful choice and is exactly backwards. `Outcome::Keep` never stamps a
    /// source, so a persistent 429 left the throttle permanently disengaged, and
    /// every activation during an outage then cost a whole retry chain instead of
    /// one request: the sustained-rate-limit case, the one this feature exists
    /// for, made several times worse rather than better.
    #[test]
    fn a_failing_source_does_not_disengage_the_throttle() {
        let now = Utc::now();
        let mut store = AgentUsageStore::test_new();
        store.set_last_polled_at(Some(now));

        store.apply(
            Outcome::from(Err::<Vec<_>, _>(claude::Unavailable::RateLimited)),
            Outcome::Keep("codex would not start".into()),
            now,
        );
        assert!(
            !store.should_fetch(PollReason::Activation, now),
            "a 429 is still an attempt; asking again immediately is what earned it"
        );
        assert_eq!(
            store.source_at(0).fetched_at, None,
            "and nothing about it made the data fresh — which is why the throttle \
             must not be asking that question"
        );
    }

    /// The common case, not an edge case: most users have no Codex CLI, so that
    /// source is `Clear` forever and its `fetched_at` is `None` forever. Keying
    /// the throttle on success would have meant it never engaged for them at all.
    #[test]
    fn a_source_that_will_never_answer_does_not_disengage_the_throttle() {
        let now = Utc::now();
        let mut store = AgentUsageStore::test_new();
        store.set_last_polled_at(Some(now));

        store.apply(
            Outcome::Windows(Vec::new()),
            Outcome::from(Err::<Vec<_>, _>(codex::Unavailable::NotInstalled)),
            now,
        );
        assert_eq!(
            store.source_at(1).fetched_at, None,
            "Clear nulls it, and an uninstalled CLI never un-nulls it"
        );
        assert!(
            !store.should_fetch(PollReason::Activation, now),
            "the throttle still works for a Claude-only user"
        );
    }

    /// Being asked too often says nothing about whether the numbers already on
    /// screen are still true, so they stay.
    #[test]
    fn rate_limiting_keeps_the_numbers_it_already_had() {
        let now = Utc::now();
        let mut store = AgentUsageStore::test_new();
        store.apply(
            Outcome::Windows(vec![UsageWindow {
                percent: 42,
                resets_at: None,
                label: None,
                kind: WindowKind::Session,
            }]),
            Outcome::Windows(Vec::new()),
            now,
        );

        store.apply(
            Outcome::from(Err::<Vec<_>, _>(claude::Unavailable::RateLimited)),
            Outcome::Windows(Vec::new()),
            now,
        );
        assert_eq!(
            store.source_at(0).windows.len(),
            1,
            "the 42% must survive a 429"
        );
        assert!(
            store
                .source_at(0)
                .reason
                .as_ref()
                .is_some_and(|reason| reason.contains("too often")),
            "and the tooltip must say why it is not fresh"
        );
    }

    /// One agent missing must not hide the other's numbers.
    ///
    /// The likely real-world state on most machines: Claude signed in, Codex not
    /// installed. Codex's absence has to be silent in the bar and explained in the
    /// panel, without touching Claude's row.
    #[test]
    fn one_agent_being_absent_does_not_hide_the_other() {
        let mut store = AgentUsageStore::test_new();
        store.apply(
            Outcome::Windows(vec![a_window()]),
            Outcome::Clear("the codex CLI is not installed".into()),
            Utc::now(),
        );

        assert!(
            AgentUsageIndicator::has_anything_to_show(&store.source_states(), &all_agents_shown()),
            "Claude still reports"
        );
        assert!(
            store.source_at(1).windows.is_empty(),
            "and Codex contributes no row rather than an empty icon"
        );
        assert_eq!(
            store.source_at(1).reason.as_ref().map(|r| r.as_ref()),
            Some("the codex CLI is not installed"),
            "the reason has to survive for the panel to explain the absence"
        );
        assert!(
            store.source_at(0).fetched_at.is_some(),
            "and Claude's read time is untouched by Codex failing"
        );
    }

    /// A successful read that reports no windows is nothing to show, not an error.
    #[test]
    fn a_read_with_no_windows_shows_nothing_without_complaining() {
        let mut store = AgentUsageStore::test_new();
        store.apply(Outcome::Windows(vec![a_window()]), nothing(), Utc::now());
        store.apply(Outcome::Windows(Vec::new()), nothing(), Utc::now());

        assert!(!AgentUsageIndicator::has_anything_to_show(&store.source_states(), &all_agents_shown()));
        assert!(
            store.source_at(0).reason.is_none(),
            "nothing went wrong, so nothing is reported as wrong"
        );
    }

    /// Applying always ends the in-flight state, however it turned out.
    ///
    /// Left set, the click would be disabled for good and the poll loop would skip
    /// every tick from then on — a single failure would silently stop all refreshes.
    #[test]
    fn applying_a_result_always_clears_the_in_flight_flag() {
        let now = Utc::now();
        for outcome in [
            Outcome::Windows(vec![a_window()]),
            Outcome::Windows(Vec::new()),
            Outcome::Keep("blink".into()),
            Outcome::Clear("gone".into()),
        ] {
            let mut store = AgentUsageStore::test_new();
            store.mark_fetching();
            store.apply(outcome, nothing(), now);
            assert!(
                !store.is_fetching(),
                "a stuck flag disables the click and mutes the poll loop for ever"
            );
        }
    }

    /// Whatever reaches the screen, it must not be credential-shaped.
    #[test]
    fn no_reason_reaching_the_screen_is_credential_shaped() {
        let mut store = AgentUsageStore::test_new();
        store.apply(
            Outcome::Clear("no Claude Code sign-in was found on this machine".into()),
            Outcome::Clear("the codex CLI is not installed".into()),
            Utc::now(),
        );

        let now = Utc::now();
        for source in &store.source_states() {
            // Through the panel's own renderer, because that is the only surface a
            // reason reaches now that the status bar has no tooltip.
            let shown = crate::usage_panel::UsagePanel::source_status(source, now);
            assert!(!shown.is_empty(), "a silent agent still says why");
            for forbidden in ["Bearer", "sk-", "accessToken", "Authorization"] {
                assert!(
                    !shown.contains(forbidden),
                    "this is user-visible text; `{forbidden}` has no business in it"
                );
            }
        }
    }

    /// An indicator holding nothing must not reserve space.
    ///
    /// The reason this store exists: a window full of workspaces is one reader.
    ///
    /// The status bar builds an indicator per `Workspace`, and this fork keeps a
    /// workspace per project inside one window. Eight of them used to mean eight
    /// poll loops against one endpoint on one token. They must now all resolve to
    /// the same store, because that is what makes it one loop.
    #[gpui::test]
    fn every_indicator_in_the_app_reads_one_store(cx: &mut gpui::TestAppContext) {
        // An empty root, so building the indicators does not also draw them:
        // drawing reads settings this crate's tests deliberately do without.
        let window = cx.add_window(|_, _| gpui::Empty);

        let stores: Vec<_> = (0..8)
            .map(|_| {
                window
                    .update(cx, |_, window, cx| {
                        cx.new(|cx| AgentUsageIndicator::new(window, cx))
                            .read(cx)
                            .store
                            .clone()
                    })
                    .expect("the window was just built")
            })
            .collect();

        let first = stores[0].entity_id();
        for (index, store) in stores.iter().enumerate() {
            assert_eq!(
                store.entity_id(),
                first,
                "indicator {index} built its own store, which is one more poll loop"
            );
        }
    }

    /// The status bar puts a gap between items, so an item that returns a flex
    /// box rather than a bare `div` shifts everything beside it while showing
    /// nothing — visible as a hole in the bar on any build where no agent is
    /// configured, which is most of them.
    #[gpui::test]
    fn an_indicator_with_no_data_shows_nothing(cx: &mut gpui::TestAppContext) {
        let store = cx.new(|_| AgentUsageStore::test_new());

        store.read_with(cx, |store, _| {
            assert!(
                !AgentUsageIndicator::has_anything_to_show(&store.source_states(), &all_agents_shown()),
                "a fresh indicator has no source and therefore nothing to say"
            );
        });
    }

    /// An agent that answered but reported no windows is still nothing to show.
    ///
    /// This is the case a naive `!usage.is_empty()` gets wrong: the vector has an
    /// entry, so the check passes, and the bar draws an agent icon with no number
    /// beside it.
    #[gpui::test]
    fn an_agent_with_no_windows_is_still_nothing_to_show(cx: &mut gpui::TestAppContext) {
        // A source that answered with no windows: the vector has an entry, so a
        // naive `!sources.is_empty()` would pass and draw an icon with no number.
        let store = cx.new(|_| {
            let mut store = AgentUsageStore::test_new();
            store.apply(Outcome::Windows(Vec::new()), nothing(), Utc::now());
            store
        });

        store.read_with(cx, |store, _| {
            assert!(
                !AgentUsageIndicator::has_anything_to_show(&store.source_states(), &all_agents_shown()),
                "an icon with no percentage beside it reads as 0%, which is a lie"
            );
        });
    }
}
