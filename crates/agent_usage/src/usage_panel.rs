//! The panel behind the status bar's usage figures.
//!
//! ```text
//! ┌─ Usage ────────── all agents  ⟳ ─┐
//! │ ┌─Detailed─┐ ┌ Compact ┐        │
//! ├──────────────────────────────────┤
//! │ ✳ Claude   Resets in 2h 58m     │
//! │   5h ▬▭ 11%  wk ▬▭ 11%  Fable 0%│
//! │ ◎ Codex    Refresh failed       │
//! └──────────────────────────────────┘
//! ```
//!
//! The panel and the status bar draw the same state, and the panel holds no copy
//! of it: it reads through a weak handle on the indicator. Two copies would
//! disagree the first time a poll landed between one render and the other, and
//! that disagreement would look like a bug in the numbers rather than in the
//! plumbing.
//!
//! What the panel says that the bar cannot: **why an agent is silent.** The bar
//! hides a source with nothing to report, which is right for a bar — but it means
//! the one question a user opens this panel to ask ("where did Codex go?") is
//! exactly the question the bar refuses to answer.

use std::collections::HashSet;

use chrono::{DateTime, FixedOffset, Local, Utc};
use gpui::{
    DismissEvent, EventEmitter, FocusHandle, Focusable, Subscription, WeakEntity,
    prelude::FluentBuilder as _,
};
use project::AgentId;
use settings::AgentUsageDisplay;
use ui::{ToggleButtonGroup, ToggleButtonGroupStyle, ToggleButtonSimple, Tooltip, prelude::*};
use workspace::{StatusBarSettings, item::Settings as _};

use crate::{
    AgentUsageIndicator, SourceState, UsageWindow, agent_display_name, agent_icon,
    format_countdown, most_constrained,
};

/// The width of a window's progress bar.
///
/// A fixed width rather than a flexible one: `flex_1` and `self_stretch` both
/// resolve to zero under a parent that is not a flex container on that axis, and
/// a bar of zero width is invisible without being an error anyone would notice.
const BAR_WIDTH: Pixels = px(28.);

/// The panel's own width, so the rows line up with each other rather than with
/// whatever the longest agent name happens to be.
const PANEL_WIDTH: Pixels = px(340.);

pub struct UsagePanel {
    indicator: WeakEntity<AgentUsageIndicator>,
    focus_handle: FocusHandle,
    /// Which agents have their per-window detail open.
    ///
    /// Panel state and deliberately not a setting: the panel is dismissed on the
    /// next click anywhere, so a disclosure that persisted would be remembering a
    /// gesture nobody made twice.
    expanded: HashSet<AgentId>,
    /// Closes the panel when focus leaves it.
    ///
    /// Without this the panel stays open over the editor until it is clicked again,
    /// which is what shipped first. `PopoverMenu` does not do it: its only mouse
    /// handler covers the *trigger*, so that clicking the button while the panel is
    /// open closes rather than re-opens it -- outside clicks are left to the
    /// content. `ContextMenu` handles them with exactly this subscription, set up in
    /// its own constructor, which is why menus dismiss and this panel did not.
    _blur: Subscription,
}

impl UsagePanel {
    pub fn new(
        indicator: WeakEntity<AgentUsageIndicator>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let _blur = cx.on_blur(&focus_handle, window, |_, _, cx| {
            cx.emit(DismissEvent);
        });

        Self {
            indicator,
            focus_handle,
            expanded: HashSet::default(),
            _blur,
        }
    }

    /// A reset instant as a wall-clock time the reader can check against their own.
    ///
    /// Takes the offset rather than reading the system zone, so this is assertable:
    /// a function that consulted the machine's timezone would pass here and fail on
    /// a CI box in another one.
    pub(crate) fn format_reset_at(at: DateTime<Utc>, offset: &FixedOffset) -> String {
        at.with_timezone(offset).format("%d/%m %H:%M").to_string()
    }

    /// One window, with room for its full name and an absolute reset time.
    ///
    /// The countdown is the more useful of the two at a glance, which is why the
    /// collapsed row carries it -- but a countdown cannot be checked against
    /// anything, and this is the view for checking.
    pub(crate) fn window_detail(window: &UsageWindow, offset: &FixedOffset) -> String {
        let when = match window.resets_at {
            Some(at) => format!("resets {}", Self::format_reset_at(at, offset)),
            // Said out loud rather than left blank: a model-scoped window genuinely
            // has no reset instant, and an empty column reads as missing data.
            None => "no reset window".to_string(),
        };

        let name = match &window.label {
            Some(label) => format!("{} · {label}", window.kind.long_name()),
            None => window.kind.long_name(),
        };

        format!("{name}  {}%  {when}", window.percent)
    }

    /// How the header describes what is being shown.
    ///
    /// "all agents" is a claim, so it is only made when it is true. With one agent
    /// switched off the panel still shows that agent's row — the panel is where you
    /// go to see what the bar is hiding — but the header must not call that "all".
    pub(crate) fn scope_label(settings: &StatusBarSettings) -> &'static str {
        match (settings.claude_usage_button, settings.codex_usage_button) {
            (true, true) => "all agents",
            (true, false) => "Claude only",
            (false, true) => "Codex only",
            (false, false) => "none on the bar",
        }
    }

    /// A source's one-line status, beside its name.
    ///
    /// The countdown of whichever window is closest to running out, because that
    /// is the number that matters at a glance -- or the reason there is no number,
    /// which is the whole point of the panel.
    pub(crate) fn source_status(source: &SourceState, now: DateTime<Utc>) -> String {
        if source.windows.is_empty() {
            return source
                .reason
                .as_ref()
                .map(|reason| reason.to_string())
                // No windows and no reason means the first read has not landed.
                .unwrap_or_else(|| "Reading…".into());
        }

        let countdown = most_constrained(&source.windows, now)
            .and_then(|window| window.resets_at)
            .and_then(|at| (at - now).to_std().ok())
            .and_then(format_countdown);

        match (countdown, &source.reason) {
            // Numbers *and* a complaint: the numbers are real but stale, and
            // saying only one of the two would be a half-truth either way.
            (Some(countdown), Some(reason)) => format!("Resets in {countdown} · {reason}"),
            (Some(countdown), None) => format!("Resets in {countdown}"),
            (None, Some(reason)) => reason.to_string(),
            (None, None) => String::new(),
        }
    }

    /// The short tag that names one window on the collapsed row.
    ///
    /// **The same precedence as the status bar, deliberately:** a window with a
    /// reset instant is named by its kind, and only a window *without* one is
    /// named by its label. `crate::render_window` decides the bar's text the same
    /// way, and the two must agree — the panel exists to explain the bar, so a
    /// panel that names a window differently is worse than no panel.
    ///
    /// Shipped wrong once in each direction. First the kind won unconditionally,
    /// so a model-scoped weekly window read `wk` and lost `Fable` — two adjacent
    /// rows both tagged `wk`. Fixing that by letting the label win
    /// *unconditionally* was also wrong, and reachable: `codex::parse_windows`
    /// clones one account-level `limitName` onto **both** of its windows, so two
    /// Codex rows that each have their own countdown would both have taken the
    /// same label. Checking `resets_at` first is what makes it agree with the bar
    /// in every shape, not just in the one the recorded fixture happens to have.
    pub(crate) fn window_tag(window: &UsageWindow) -> String {
        match (&window.resets_at, &window.label) {
            (None, Some(label)) => label.to_string(),
            _ => window.kind.short_tag(),
        }
    }

    fn render_window(window: &UsageWindow, cx: &App) -> AnyElement {
        let tag = Self::window_tag(window);

        h_flex()
            .gap_1()
            .when(!tag.is_empty(), |row| {
                row.child(
                    // Truncated because the tag slot no longer holds only
                    // `short_tag()`'s two or three characters: a label reaches it
                    // too, and a label is whatever the vendor called the thing.
                    Label::new(tag)
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                )
            })
            .child(
                // Two nested divs and an explicit width, because the filled part
                // is a fraction of a known width -- there is nothing here for a
                // flex rule to divide up.
                div()
                    .w(BAR_WIDTH)
                    .h(px(4.))
                    .rounded_full()
                    .bg(cx.theme().colors().element_background)
                    .child(
                        div()
                            .h_full()
                            .w(BAR_WIDTH * (f32::from(window.percent) / 100.))
                            .rounded_full()
                            .bg(cx.theme().colors().text_muted),
                    ),
            )
            .child(Label::new(format!("{}%", window.percent)).size(LabelSize::Small))
            .into_any_element()
    }

    fn render_source(
        &self,
        source: &SourceState,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = self.expanded.contains(&source.agent);
        let agent = source.agent.clone();
        let offset = *Local::now().offset();

        let summary = source
            .windows
            .iter()
            .map(|window| Self::render_window(window, cx))
            .collect::<Vec<_>>();

        // The detail is built from the same windows, said at length: full name,
        // exact percentage, and an absolute instant instead of a countdown.
        let detail = source
            .windows
            .iter()
            .map(|window| {
                Label::new(Self::window_detail(window, &offset))
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        v_flex()
            .w_full()
            .gap_0p5()
            .child(
                h_flex()
                    .id(SharedString::from(format!("usage-row-{}", agent.as_ref())))
                    .w_full()
                    .gap_1p5()
                    .cursor_pointer()
                    .child(Icon::new(agent_icon(&source.agent)).size(IconSize::Small))
                    .child(Label::new(agent_display_name(&source.agent)))
                    .child(
                        Label::new(Self::source_status(source, now))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        div().flex_1().flex().justify_end().child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                        ),
                    )
                    // Expanding is reading what is already in hand, so it fires no
                    // request -- opening the detail must never be a reason to
                    // touch the network or spawn a process.
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded.remove(&agent) {
                            this.expanded.insert(agent.clone());
                        }
                        cx.notify();
                    })),
            )
            // A silent agent has no summary line at all rather than an empty one:
            // its row is the name and the reason, which is a complete answer.
            .when(!expanded && !summary.is_empty(), |column| {
                column.child(h_flex().pl_5().gap_2().children(summary))
            })
            .when(expanded, |column| {
                column.child(
                    v_flex()
                        .pl_5()
                        .gap_0p5()
                        // Expanded with nothing to expand: the reason is already on
                        // the row above, and repeating it would be noise -- but
                        // saying nothing at all would read as a broken control.
                        .when(detail.is_empty(), |column| {
                            column.child(
                                Label::new("No windows reported.")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                        })
                        .children(detail),
                )
            })
            .into_any_element()
    }

    fn render_display_switch(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = match StatusBarSettings::get_global(cx).agent_usage_display {
            AgentUsageDisplay::Detailed => 0,
            AgentUsageDisplay::Compact => 1,
        };

        ToggleButtonGroup::single_row(
            "agent-usage-display",
            [
                ToggleButtonSimple::new(
                    "Detailed",
                    cx.listener(|_, _, _, cx| {
                        Self::set_display(Self::display_for_click(false), cx);
                    }),
                ),
                ToggleButtonSimple::new(
                    "Compact",
                    cx.listener(|_, _, _, cx| {
                        Self::set_display(Self::display_for_click(true), cx);
                    }),
                ),
            ],
        )
        .label_size(LabelSize::Small)
        .style(ToggleButtonGroupStyle::Outlined)
        .full_width()
        .selected_index(selected)
        .into_any_element()
    }

    /// The value the toggle writes for a given click.
    ///
    /// Split out from the write so the decision is testable: the write needs a
    /// settings store and a filesystem, and neither is what is interesting here.
    pub(crate) fn display_for_click(compact_clicked: bool) -> AgentUsageDisplay {
        if compact_clicked {
            AgentUsageDisplay::Compact
        } else {
            AgentUsageDisplay::Detailed
        }
    }

    /// Writes the choice to the user's settings file.
    ///
    /// A setting rather than panel state, because the thing it changes is the
    /// status bar -- which outlives the panel by definition, and would be a
    /// pointless toggle if it reverted on the next launch.
    fn set_display(display: AgentUsageDisplay, cx: &mut App) {
        settings::update_settings_file(<dyn fs::Fs>::global(cx), cx, move |content, _| {
            content
                .status_bar
                .get_or_insert_default()
                .agent_usage_display = Some(display);
        });
    }
}

impl Focusable for UsagePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for UsagePanel {}

impl Render for UsagePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Utc::now();
        let scope = Self::scope_label(StatusBarSettings::get_global(cx));
        let indicator = self.indicator.clone();
        let fetching = self
            .indicator
            .read_with(cx, |indicator, _| indicator.is_fetching())
            .unwrap_or(false);

        // Read once and clone, rather than holding the borrow across the render:
        // reaching back into another entity mid-render is what this crate's
        // neighbours have repeatedly paid for.
        let sources = self
            .indicator
            .read_with(cx, |indicator, _| indicator.source_snapshot())
            .unwrap_or_default();

        v_flex()
            // Focus has to land somewhere for `on_blur` to ever fire, and
            // `PopoverMenu` focuses this view's handle on the frame after it opens.
            .track_focus(&self.focus_handle)
            // A `ManagedView` is drawn straight into a deferred layer with nothing
            // behind it, so without this the panel is transparent and the editor,
            // the git log and the sidebar all read through it -- which looks like
            // washed-out text rather than like a missing background. `elevation_2`
            // is what `ContextMenu` uses for the same job.
            .elevation_2(cx)
            .w(PANEL_WIDTH)
            .p_2()
            .gap_1p5()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(Label::new("Usage"))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(Label::new(scope).size(LabelSize::Small).color(Color::Muted))
                            .child(
                                IconButton::new("agent-usage-refresh", IconName::ArrowCircle)
                                    .icon_size(IconSize::Small)
                                    .icon_color(if fetching {
                                        Color::Accent
                                    } else {
                                        Color::Muted
                                    })
                                    .tooltip(Tooltip::text("Read the quota again"))
                                    .on_click(move |_, window, cx| {
                                        indicator
                                            .update(cx, |indicator, cx| {
                                                indicator.refresh_now(window, cx);
                                            })
                                            .ok();
                                    }),
                            ),
                    ),
            )
            .child(self.render_display_switch(cx))
            .children(
                sources
                    .iter()
                    .map(|source| self.render_source(source, now, cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Outcome, WindowKind};
    use project::AgentId;

    fn settings() -> StatusBarSettings {
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
        }
    }

    fn source_with(windows: Vec<UsageWindow>, reason: Option<&str>) -> SourceState {
        let mut source = SourceState::new(AgentId::new(project::CODEX_AGENT_ID.to_string()));
        let outcome = match (windows.is_empty(), reason) {
            (true, reason) => Outcome::Clear(reason.unwrap_or("nothing").to_string().into()),
            (false, _) => Outcome::Windows(windows),
        };
        source.apply(outcome, Utc::now());
        source
    }

    /// "all agents" is a claim, and the header only makes it when it holds.
    ///
    /// With one agent switched off, the panel still shows both rows -- it is where
    /// you go to see what the bar is hiding -- so a header that still said "all"
    /// would be describing the panel correctly and the bar wrongly.
    #[test]
    fn the_header_only_claims_all_agents_when_both_are_on() {
        let mut only_claude = settings();
        only_claude.codex_usage_button = false;
        let mut only_codex = settings();
        only_codex.claude_usage_button = false;
        let mut neither = settings();
        neither.claude_usage_button = false;
        neither.codex_usage_button = false;

        assert_eq!(UsagePanel::scope_label(&settings()), "all agents");
        assert_eq!(UsagePanel::scope_label(&only_claude), "Claude only");
        assert_eq!(UsagePanel::scope_label(&only_codex), "Codex only");
        assert_eq!(UsagePanel::scope_label(&neither), "none on the bar");
    }

    /// A silent agent's row carries the reason, which is the one thing the status
    /// bar cannot say.
    ///
    /// The bar hides a source with nothing to report. That makes "where did Codex
    /// go?" precisely the question it refuses to answer -- so the panel must.
    #[test]
    fn a_silent_agent_still_gets_a_row_and_it_says_why() {
        let source = source_with(Vec::new(), Some("the codex CLI is not installed"));

        assert_eq!(
            UsagePanel::source_status(&source, Utc::now()),
            "the codex CLI is not installed"
        );
    }

    /// Before the first read lands there is neither a number nor a complaint, and
    /// the row says so rather than showing an empty gap.
    #[test]
    fn an_agent_that_has_not_answered_yet_says_it_is_reading() {
        let source = SourceState::new(AgentId::new(project::CODEX_AGENT_ID.to_string()));
        assert_eq!(UsagePanel::source_status(&source, Utc::now()), "Reading…");
    }

    /// The status counts down the window closest to running out, not the first one.
    #[test]
    fn the_status_counts_down_the_window_that_will_stop_you() {
        let now: DateTime<Utc> = "2026-08-21T11:00:00+00:00".parse().unwrap();
        let source = source_with(
            vec![
                UsageWindow {
                    percent: 4,
                    resets_at: Some("2026-08-21T12:00:00+00:00".parse().unwrap()),
                    label: None,
                    kind: WindowKind::Session,
                },
                UsageWindow {
                    percent: 91,
                    resets_at: Some("2026-08-21T13:58:00+00:00".parse().unwrap()),
                    label: None,
                    kind: WindowKind::Weekly,
                },
            ],
            None,
        );

        assert_eq!(
            UsagePanel::source_status(&source, now),
            "Resets in 2h 58m",
            "the 91% window, not the 4% one listed first"
        );
    }

    /// Stale numbers and a reason are both reported, not one or the other.
    ///
    /// `Keep` means the figures are real but the last read failed. Showing only the
    /// countdown hides the failure; showing only the failure throws away good
    /// numbers. Both is the only honest answer.
    #[test]
    fn numbers_that_are_stale_report_the_countdown_and_the_complaint() {
        let now: DateTime<Utc> = "2026-08-21T11:00:00+00:00".parse().unwrap();
        let mut source = source_with(
            vec![UsageWindow {
                percent: 40,
                resets_at: Some("2026-08-21T12:00:00+00:00".parse().unwrap()),
                label: None,
                kind: WindowKind::Session,
            }],
            None,
        );
        source.apply(Outcome::Keep("the request timed out".into()), now);

        let status = UsagePanel::source_status(&source, now);
        assert!(status.contains("Resets in 1h 0m"), "{status}");
        assert!(status.contains("the request timed out"), "{status}");
    }

    /// A window with no reset instant says so, rather than leaving the column
    /// blank.
    ///
    /// Blank reads as data that went missing. "no reset window" is the true answer
    /// for a model-scoped window, and it is a different statement.
    #[test]
    fn a_window_with_no_reset_says_so_out_loud() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let scoped = UsageWindow {
            percent: 0,
            resets_at: None,
            label: Some("Fable".into()),
            kind: WindowKind::Weekly,
        };

        let detail = UsagePanel::window_detail(&scoped, &utc);
        assert_eq!(detail, "Weekly · Fable  0%  no reset window");
        assert!(
            !detail.contains("resets "),
            "nothing that reads as a reset time"
        );
    }

    /// The detail names the window in full and dates it absolutely.
    ///
    /// The collapsed row shows `wk` and a countdown; this one shows `Weekly` and an
    /// instant, because a countdown cannot be checked against a clock and this is
    /// the view for checking.
    #[test]
    fn the_detail_names_the_window_and_dates_it() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let weekly = UsageWindow {
            percent: 11,
            resets_at: Some("2026-08-27T09:00:00+00:00".parse().unwrap()),
            label: None,
            kind: WindowKind::Weekly,
        };

        assert_eq!(
            UsagePanel::window_detail(&weekly, &utc),
            "Weekly  11%  resets 27/08 09:00"
        );
    }

    /// The instant is rendered in the offset it is given, not in UTC and not in
    /// whatever zone the test machine happens to sit in.
    #[test]
    fn the_reset_instant_follows_the_offset_it_is_given() {
        let at: DateTime<Utc> = "2026-08-21T23:30:00+00:00".parse().unwrap();

        assert_eq!(
            UsagePanel::format_reset_at(at, &FixedOffset::east_opt(0).unwrap()),
            "21/08 23:30"
        );
        assert_eq!(
            UsagePanel::format_reset_at(at, &FixedOffset::east_opt(7 * 3600).unwrap()),
            "22/08 06:30",
            "seven hours east crosses into the next day, and the date must follow"
        );
    }

    /// An unrecognised kind still gets a readable name in the detail.
    ///
    /// `short_tag` is empty for `Unknown` because there is no honest two-letter
    /// answer — but a detail row with a blank name would look broken, so
    /// `long_name` falls back to a word.
    #[test]
    fn an_unknown_kind_still_reads_as_something() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let odd = UsageWindow {
            percent: 61,
            resets_at: None,
            label: None,
            kind: WindowKind::Unknown,
        };

        assert_eq!(
            UsagePanel::window_detail(&odd, &utc),
            "Window  61%  no reset window"
        );
    }

    /// A model-scoped window is tagged with the model, not with `wk`.
    ///
    /// Shipped wrong once: the tag preferred the kind, so the weekly allowance and
    /// the Fable slice of it both rendered as `wk` — two identical tags on adjacent
    /// rows, and the model name gone.
    #[test]
    fn a_model_scoped_window_is_tagged_with_the_model() {
        let scoped = UsageWindow {
            percent: 0,
            resets_at: None,
            label: Some("Fable".into()),
            kind: WindowKind::Weekly,
        };

        assert_eq!(UsagePanel::window_tag(&scoped), "Fable");
        assert_ne!(
            UsagePanel::window_tag(&scoped),
            "wk",
            "the kind must not win over a name the source actually gave"
        );
    }

    /// A window with no name falls back to its kind.
    #[test]
    fn an_unnamed_window_is_tagged_with_its_kind() {
        let weekly = UsageWindow {
            percent: 12,
            resets_at: Some("2026-08-27T09:00:00+00:00".parse().unwrap()),
            label: None,
            kind: WindowKind::Weekly,
        };
        assert_eq!(UsagePanel::window_tag(&weekly), "wk");
    }

    /// A window with both a countdown and a name is tagged by its kind.
    ///
    /// The shape no fixture had, and the one that made "label always wins" wrong:
    /// `codex::parse_windows` clones a single account-level `limitName` onto both
    /// of its windows, so two rows that each have their own `resetsAt` would both
    /// have been tagged with the same string — the exact collision the previous
    /// fix was for, moved to the other agent.
    #[test]
    fn a_window_with_both_a_countdown_and_a_name_is_tagged_by_its_kind() {
        let both = UsageWindow {
            percent: 42,
            resets_at: Some("2026-08-27T09:00:00+00:00".parse().unwrap()),
            label: Some("Codex Cloud".into()),
            kind: WindowKind::Span(std::time::Duration::from_secs(43_200 * 60)),
        };

        assert_eq!(UsagePanel::window_tag(&both), "30d");
        assert_ne!(
            UsagePanel::window_tag(&both),
            "Codex Cloud",
            "a shared account-level name must not become two identical row tags"
        );
    }

    /// Two Codex windows sharing one `limitName` still read as two distinct rows.
    ///
    /// This is the collision itself, asserted end to end rather than as a rule.
    #[test]
    fn two_windows_sharing_one_name_do_not_collide() {
        let shared: SharedString = "Codex Cloud".into();
        let primary = UsageWindow {
            percent: 10,
            resets_at: Some("2026-08-22T09:00:00+00:00".parse().unwrap()),
            label: Some(shared.clone()),
            kind: WindowKind::Span(std::time::Duration::from_secs(300 * 60)),
        };
        let secondary = UsageWindow {
            percent: 40,
            resets_at: Some("2026-09-20T09:00:00+00:00".parse().unwrap()),
            label: Some(shared),
            kind: WindowKind::Span(std::time::Duration::from_secs(43_200 * 60)),
        };

        assert_ne!(
            UsagePanel::window_tag(&primary),
            UsagePanel::window_tag(&secondary),
            "two rows must not read the same"
        );
    }

    /// The panel and the status bar must not disagree about one window.
    ///
    /// They render the same data through different code, so they can drift. The bar
    /// showed `0% used Fable` while the panel showed `wk 0%` — same window, two
    /// answers, and the panel's was the less informative one.
    /// Covers **every** combination of `resets_at` and `label`, not just the one
    /// the recorded fixture happens to have. Testing only the no-countdown case is
    /// what let "label always wins" look correct.
    #[test]
    fn the_panel_and_the_status_bar_name_a_window_the_same_way() {
        let now: DateTime<Utc> = "2026-08-21T11:00:00+00:00".parse().unwrap();
        let reset: DateTime<Utc> = "2026-08-21T12:00:00+00:00".parse().unwrap();

        let cases = [
            // The Fable row: a name and no countdown. The bar prints the name, so
            // the panel must tag it with the name.
            ("named, no countdown", None, Some("Fable")),
            // A countdown and a name at once -- Codex's shared `limitName`. The bar
            // prints the countdown and never the name, so the panel must not tag it
            // with the name either.
            ("named, with countdown", Some(reset), Some("Codex Cloud")),
            ("unnamed, with countdown", Some(reset), None),
        ];

        for (what, resets_at, label) in cases {
            let window = UsageWindow {
                percent: 12,
                resets_at,
                label: label.map(SharedString::from),
                kind: WindowKind::Weekly,
            };

            let bar = crate::render_window(&window, now);
            let tag = UsagePanel::window_tag(&window);

            if let Some(label) = label {
                // The decisive assertion: whichever of the two the panel shows, the
                // bar must be showing the same one.
                assert_eq!(
                    tag == label,
                    bar.contains(label),
                    "{what}: bar says {bar:?}, panel tags it {tag:?} -- they disagree \
                     about whether this window is called {label:?}"
                );
            }
            assert!(
                !tag.is_empty(),
                "{what}: every window gets some tag on the collapsed row"
            );
        }
    }

    /// The toggle writes the setting the click names, and nothing else.
    #[test]
    fn each_half_of_the_toggle_writes_its_own_value() {
        assert_eq!(
            UsagePanel::display_for_click(false),
            AgentUsageDisplay::Detailed
        );
        assert_eq!(
            UsagePanel::display_for_click(true),
            AgentUsageDisplay::Compact
        );
    }
}
