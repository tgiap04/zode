//! The Stats tab: live resource usage of the open row.
//!
//! The backend answers one sample at a time (`docker stats --no-stream`), so
//! the "live" part is this module polling it. The polling is the part that has
//! to be right: it runs only while the tab is on screen, it is owned by the
//! open row so closing the row drops it, and what it remembers is a ring of
//! [`STATS_HISTORY`] samples -- the panel is left open all day.

use std::collections::VecDeque;
use std::time::Duration;

use container::{ContainerError, Resource, ResourceKind, Stats};
use gpui::{AnyElement, Task};
use ui::prelude::*;

use crate::container_panel::ContainerPanel;
use crate::detail_tabs::DetailTab;

/// Time between two samples. `docker stats --no-stream` itself takes about a
/// second to answer, so anything faster would queue commands behind each other.
pub(crate) const STATS_INTERVAL: Duration = Duration::from_secs(2);

/// Time between two samples once [`STATS_FAILURES_BEFORE_BACKOFF`] polls in a
/// row have failed: whatever is wrong -- the container is gone, the daemon is
/// down -- is not mended by asking again every two seconds.
pub(crate) const STATS_BACKOFF_INTERVAL: Duration = Duration::from_secs(10);

/// How many failed polls in a row slow the polling down to
/// [`STATS_BACKOFF_INTERVAL`]. One good answer ends it.
const STATS_FAILURES_BEFORE_BACKOFF: usize = 3;

/// How many samples the sparklines remember: two minutes at [`STATS_INTERVAL`].
pub(crate) const STATS_HISTORY: usize = 60;

/// A compose project's member containers change rarely, so they are looked up
/// again only every this many samples rather than with every one.
const MEMBER_REFRESH_TICKS: usize = 15;

/// What one sample contributes to a sparkline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Sample {
    pub(crate) cpu: Option<f64>,
    pub(crate) memory: Option<u64>,
}

impl From<&Stats> for Sample {
    fn from(stats: &Stats) -> Self {
        Self {
            cpu: stats.cpu_percent,
            memory: stats.memory_used,
        }
    }
}

#[derive(Default)]
pub(crate) struct StatsState {
    /// Oldest first, never longer than [`STATS_HISTORY`].
    pub(crate) history: VecDeque<Sample>,
    /// The last answer: one entry for a container or pod, one per member for a
    /// compose project.
    pub(crate) latest: Vec<Stats>,
    /// Whether any poll has finished yet, so "waiting" is told from "nothing".
    pub(crate) answered: bool,
    /// The engine said it has no metrics at all (`Ok(None)`). Polling stops, and
    /// is not started again for this row: asking again would get the same answer.
    pub(crate) unavailable: bool,
    /// Why the last poll failed. Set while the numbers on screen are older than
    /// the last attempt; cleared by the next poll that works.
    pub(crate) stale: Option<String>,
    /// Polls that failed one after another, ended by the first that works.
    failures: usize,
    /// The polling loop. Dropping it is what stops the polling.
    pub(crate) poll: Option<Task<()>>,
    /// Set by every frame that draws the tab, cleared by the loop each tick. A
    /// tick that finds it still clear means nothing drew the tab for a whole
    /// interval -- the pane is hidden or the window is gone -- and the loop ends.
    rendered: bool,
}

impl StatsState {
    pub(crate) fn push_sample(&mut self, sample: Sample) {
        if self.history.len() == STATS_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(sample);
    }

    pub(crate) fn stop_polling(&mut self) {
        self.poll = None;
    }

    /// How long to wait before the next poll.
    fn next_interval(&self) -> Duration {
        if self.failures >= STATS_FAILURES_BEFORE_BACKOFF {
            STATS_BACKOFF_INTERVAL
        } else {
            STATS_INTERVAL
        }
    }

    /// The loop's own check: whether the tab was drawn since the last one.
    /// Clears the mark, so the next tick needs a fresh frame to continue.
    fn take_rendered(&mut self) -> bool {
        std::mem::take(&mut self.rendered)
    }
}

impl ContainerPanel {
    /// Starts polling unless it is already running, cannot work, or the open
    /// row is not on the Stats tab.
    ///
    /// Called when the tab is chosen and from every frame that draws it, so a
    /// loop that ended while the pane was hidden starts again when it returns.
    pub(crate) fn ensure_stats_polling(&mut self, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        if detail.tab != DetailTab::Stats || detail.stats.poll.is_some() || detail.stats.unavailable
        {
            return;
        }
        detail.stats.rendered = true;
        let resource = detail.resource.clone();
        detail.stats.poll = Some(cx.spawn(async move |this, cx| {
            let mut members: Vec<String> = Vec::new();
            let mut tick = 0usize;
            loop {
                let outcome = if resource.kind == ResourceKind::ComposeProject {
                    sample_project(backend.as_ref(), &resource, &mut members, tick).await
                } else {
                    backend.stats(std::slice::from_ref(&resource.id)).await
                };
                let wait = this
                    .update(cx, |this, cx| this.apply_stats(outcome, cx))
                    .ok()
                    .flatten();
                let Some(wait) = wait else {
                    return;
                };
                cx.background_executor().timer(wait).await;
                let still_shown = this
                    .update(cx, |this, _| match this.detail.as_mut() {
                        Some(detail) if detail.tab == DetailTab::Stats => {
                            let shown = detail.stats.take_rendered();
                            if !shown {
                                // The task is dropped from inside its own body; it
                                // returns right after, so it is never polled again.
                                detail.stats.stop_polling();
                            }
                            shown
                        }
                        _ => false,
                    })
                    .unwrap_or(false);
                if !still_shown {
                    return;
                }
                tick += 1;
            }
        }));
    }

    /// Folds one poll's answer into the open row, and says how long to wait
    /// before the next. `None` ends the loop: the row is gone, or the engine has
    /// nothing to say.
    fn apply_stats(
        &mut self,
        outcome: Result<Option<Vec<Stats>>, ContainerError>,
        cx: &mut Context<Self>,
    ) -> Option<Duration> {
        let detail = self.detail.as_mut()?;
        let single = detail.resource.kind != ResourceKind::ComposeProject;
        let id = detail.resource.id.clone();
        let state = &mut detail.stats;
        let mut keep_going = true;
        match outcome {
            Ok(Some(mut all)) => {
                state.answered = true;
                state.stale = None;
                state.failures = 0;
                if single {
                    // An absent id is a gap, not a zero: a stopped container has
                    // no usage, and a flat line at zero would claim it does.
                    let sample = all
                        .iter()
                        .find(|stats| stats.id == id)
                        .map(Sample::from)
                        .unwrap_or_default();
                    state.push_sample(sample);
                } else {
                    all.sort_by(|left, right| left.name.cmp(&right.name));
                }
                state.latest = all;
            }
            Ok(None) => {
                state.answered = true;
                state.unavailable = true;
                state.latest.clear();
                state.stop_polling();
                keep_going = false;
            }
            // One removed container can fail the whole call, and the next poll
            // may be fine: keep the numbers, mark them old, keep polling -- more
            // slowly once it keeps failing.
            Err(error) => {
                state.stale = Some(error.to_string());
                state.failures += 1;
            }
        }
        let wait = keep_going.then(|| state.next_interval());
        cx.notify();
        wait
    }

    /// The Stats tab: numbers and sparklines for one row, or a table for a
    /// compose project's containers.
    pub(crate) fn render_stats(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // Marked before anything else: the loop reads this to know the tab is on
        // screen, and starts it again if it had stopped while hidden.
        if let Some(detail) = self.detail.as_mut() {
            detail.stats.rendered = true;
        }
        self.ensure_stats_polling(cx);
        let Some(detail) = self.detail.as_ref() else {
            return div().into_any_element();
        };
        let state = &detail.stats;
        let resource = &detail.resource;

        let body = if state.unavailable {
            crate::render::note(
                Some(IconName::Info),
                "This engine reports no metrics for it. A cluster needs a metrics server.",
            )
        } else if !state.answered {
            crate::render::note(None, "Waiting for the first sample\u{2026}")
        } else if resource.kind == ResourceKind::ComposeProject {
            render_project_table(state, cx)
        } else if state.latest.is_empty() {
            crate::render::note(
                Some(IconName::Server),
                "No sample for it -- metrics exist only while it is running.",
            )
        } else {
            render_single(state, cx)
        };

        v_flex()
            .id("container-stats")
            .debug_selector(|| "container-stats".into())
            .size_full()
            .overflow_y_scroll()
            .children(state.stale.as_ref().map(|reason| stale_banner(reason, cx)))
            .child(body)
            .into_any_element()
    }
}

/// A compose project's members' usage in one call.
///
/// Members are the containers carrying the project's compose label, looked up
/// through the backend's own list so no engine's vocabulary is spoken here.
async fn sample_project(
    backend: &dyn container::ContainerBackend,
    project: &Resource,
    members: &mut Vec<String>,
    tick: usize,
) -> Result<Option<Vec<Stats>>, ContainerError> {
    if tick.is_multiple_of(MEMBER_REFRESH_TICKS) {
        let listed = backend.list(ResourceKind::Container).await?;
        *members = listed
            .into_iter()
            .filter(|row| {
                row.compose
                    .as_ref()
                    .is_some_and(|labels| labels.project == project.name)
            })
            .map(|row| row.id)
            .collect();
    }
    backend.stats(members).await
}

fn stale_banner(reason: &str, cx: &mut Context<ContainerPanel>) -> AnyElement {
    h_flex()
        .debug_selector(|| "container-stats-stale".into())
        .flex_none()
        .gap_1p5()
        .px_3()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().colors().border)
        .child(
            Icon::new(IconName::Warning)
                .size(IconSize::Small)
                .color(Color::Warning),
        )
        .child(
            Label::new(format!("Out of date -- the last update failed: {reason}"))
                .size(LabelSize::Small)
                .color(Color::Warning),
        )
        .into_any_element()
}

fn render_single(state: &StatsState, cx: &mut Context<ContainerPanel>) -> AnyElement {
    let Some(stats) = state.latest.first() else {
        return div().into_any_element();
    };
    let cpu_series: Vec<Option<f64>> = state.history.iter().map(|sample| sample.cpu).collect();
    let memory_series: Vec<Option<f64>> = state
        .history
        .iter()
        .map(|sample| sample.memory.map(|bytes| bytes as f64))
        .collect();
    let memory_scale = stats
        .memory_limit
        .map(|limit| limit as f64)
        .unwrap_or_else(|| series_peak(&memory_series));
    let cpu_scale = series_peak(&cpu_series).max(10.0);

    h_flex()
        .p_3()
        .gap_3()
        .flex_wrap()
        .items_start()
        .child(stat_card(
            "CPU",
            percent_text(stats.cpu_percent),
            Some(sparkline("container-stats-cpu", &cpu_series, cpu_scale, cx)),
            cx,
        ))
        .child(stat_card(
            "Memory",
            memory_text(stats),
            Some(sparkline(
                "container-stats-memory",
                &memory_series,
                memory_scale,
                cx,
            )),
            cx,
        ))
        .child(stat_card(
            "Network I/O",
            format!(
                "\u{2193} {}   \u{2191} {}",
                bytes_text(stats.net_rx),
                bytes_text(stats.net_tx)
            ),
            None,
            cx,
        ))
        .child(stat_card(
            "Block I/O",
            format!(
                "read {}   write {}",
                bytes_text(stats.block_read),
                bytes_text(stats.block_write)
            ),
            None,
            cx,
        ))
        .child(stat_card(
            "PIDs",
            stats
                .pids
                .map_or_else(|| "\u{2014}".to_string(), |pids| pids.to_string()),
            None,
            cx,
        ))
        .into_any_element()
}

fn stat_card(
    title: &'static str,
    value: String,
    chart: Option<AnyElement>,
    cx: &mut Context<ContainerPanel>,
) -> AnyElement {
    v_flex()
        .debug_selector(move || format!("container-stats-card-{title}"))
        .w(px(240.))
        .p_2()
        .gap_1()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().colors().border)
        .child(Label::new(title).size(LabelSize::Small).color(Color::Muted))
        .child(Label::new(value))
        .children(chart)
        .into_any_element()
}

/// Bars, newest on the right. A missing sample leaves a gap rather than a bar.
fn sparkline(
    id: &'static str,
    series: &[Option<f64>],
    scale: f64,
    cx: &mut Context<ContainerPanel>,
) -> AnyElement {
    const HEIGHT: f32 = 32.0;
    let colour = cx.theme().colors().text_accent;
    h_flex()
        .debug_selector(move || id.to_string())
        .h(px(HEIGHT))
        .w_full()
        .gap(px(1.))
        .items_end()
        .justify_end()
        .children(bar_fractions(series, scale).into_iter().map(|fraction| {
            let bar = div().w(px(3.));
            match fraction {
                Some(fraction) => bar
                    .h(px((HEIGHT * fraction).max(1.0)))
                    .bg(colour)
                    .into_any_element(),
                None => bar.into_any_element(),
            }
        }))
        .into_any_element()
}

fn render_project_table(state: &StatsState, cx: &mut Context<ContainerPanel>) -> AnyElement {
    if state.latest.is_empty() {
        return crate::render::note(
            Some(IconName::Server),
            "No running containers in this project.",
        );
    }
    let border = cx.theme().colors().border;
    let total_cpu: f64 = state
        .latest
        .iter()
        .filter_map(|stats| stats.cpu_percent)
        .sum();
    let total_memory: u64 = state
        .latest
        .iter()
        .filter_map(|stats| stats.memory_used)
        .sum();

    let row = |name: String, cpu: String, memory: String, share: String, muted: bool| {
        let colour = if muted { Color::Muted } else { Color::Default };
        h_flex()
            .px_3()
            .py_1()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .child(
                div().flex_1().min_w_0().child(
                    Label::new(name)
                        .size(LabelSize::Small)
                        .color(colour)
                        .truncate(),
                ),
            )
            .child(cell(cpu, 80., colour))
            .child(cell(memory, 180., colour))
            .child(cell(share, 70., colour))
    };

    v_flex()
        .debug_selector(|| "container-stats-table".into())
        .child(row(
            "Container".into(),
            "CPU".into(),
            "Memory".into(),
            "Mem %".into(),
            true,
        ))
        .children(state.latest.iter().map(|stats| {
            row(
                stats.name.clone(),
                percent_text(stats.cpu_percent),
                memory_used_text(stats),
                memory_share_text(stats),
                false,
            )
        }))
        .child(row(
            "Total".into(),
            format!("{total_cpu:.1}%"),
            bytes_text(Some(total_memory)),
            String::new(),
            true,
        ))
        .into_any_element()
}

fn cell(text: String, width: f32, colour: Color) -> impl IntoElement {
    div()
        .w(px(width))
        .flex_none()
        .child(Label::new(text).size(LabelSize::Small).color(colour))
}

/// `value / scale` for every sample, clamped to `0..=1`; `None` where there is
/// no sample. A zero or unreadable scale yields flat bars, never a division.
pub(crate) fn bar_fractions(series: &[Option<f64>], scale: f64) -> Vec<Option<f32>> {
    series
        .iter()
        .map(|value| {
            value.map(|value| {
                if scale > 0.0 && scale.is_finite() {
                    (value / scale).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                }
            })
        })
        .collect()
}

fn series_peak(series: &[Option<f64>]) -> f64 {
    series.iter().flatten().copied().fold(0.0, f64::max)
}

pub(crate) fn bytes_text(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return "\u{2014}".to_string();
    };
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(crate) fn percent_text(percent: Option<f64>) -> String {
    percent.map_or_else(
        || "\u{2014}".to_string(),
        |percent| format!("{percent:.1}%"),
    )
}

/// How much of its limit a container uses, when both are known and the limit
/// is not zero.
pub(crate) fn memory_percent(stats: &Stats) -> Option<f64> {
    let (used, limit) = (stats.memory_used?, stats.memory_limit?);
    (limit > 0).then(|| used as f64 / limit as f64 * 100.0)
}

fn memory_share_text(stats: &Stats) -> String {
    percent_text(memory_percent(stats))
}

fn memory_used_text(stats: &Stats) -> String {
    match (stats.memory_used, stats.memory_limit) {
        (Some(_), Some(_)) => format!(
            "{} / {}",
            bytes_text(stats.memory_used),
            bytes_text(stats.memory_limit)
        ),
        _ => bytes_text(stats.memory_used),
    }
}

fn memory_text(stats: &Stats) -> String {
    match memory_percent(stats) {
        Some(percent) => format!("{} ({percent:.1}%)", memory_used_text(stats)),
        None => memory_used_text(stats),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_never_grows_past_its_cap_and_drops_the_oldest() {
        let mut state = StatsState::default();
        for step in 0..(STATS_HISTORY + 25) {
            state.push_sample(Sample {
                cpu: Some(step as f64),
                memory: None,
            });
        }
        assert_eq!(state.history.len(), STATS_HISTORY);
        assert_eq!(
            state.history.front().and_then(|sample| sample.cpu),
            Some(25.0),
            "the oldest samples are the ones to go"
        );
        assert_eq!(
            state.history.back().and_then(|sample| sample.cpu),
            Some((STATS_HISTORY + 24) as f64)
        );
    }

    #[test]
    fn bars_are_clamped_and_gaps_stay_gaps() {
        let bars = bar_fractions(&[Some(50.0), None, Some(500.0), Some(-1.0)], 100.0);
        assert_eq!(bars, vec![Some(0.5), None, Some(1.0), Some(0.0)]);
        assert_eq!(bar_fractions(&[Some(5.0)], 0.0), vec![Some(0.0)]);
        assert_eq!(bar_fractions(&[Some(5.0)], f64::NAN), vec![Some(0.0)]);
    }

    #[test]
    fn bytes_read_in_binary_units_and_unknown_is_a_dash() {
        assert_eq!(bytes_text(Some(0)), "0 B");
        assert_eq!(bytes_text(Some(1023)), "1023 B");
        assert_eq!(bytes_text(Some(64 * 1024 * 1024)), "64.0 MiB");
        assert_eq!(bytes_text(Some(1536)), "1.5 KiB");
        assert_eq!(bytes_text(None), "\u{2014}");
    }

    #[test]
    fn memory_percent_needs_both_halves_and_a_nonzero_limit() {
        let stats = |used, limit| Stats {
            memory_used: used,
            memory_limit: limit,
            ..Stats::default()
        };
        assert_eq!(memory_percent(&stats(Some(50), Some(200))), Some(25.0));
        assert_eq!(memory_percent(&stats(Some(50), Some(0))), None);
        assert_eq!(memory_percent(&stats(None, Some(200))), None);
        assert_eq!(memory_percent(&stats(Some(50), None)), None);
    }
}
