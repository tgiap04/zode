//! The agents of one branch, drawn inside its card.
//!
//! Inside, not beneath: the card's border is what says "these belong to this
//! branch", and a list that starts after the border closes says the opposite.
//! That is why a branch row carries its agents rather than the tree emitting
//! them as rows of their own.

use std::sync::Arc;
use std::time::SystemTime;

use gpui::{AnyElement, ClickEvent};
use ui::{Indicator, Tooltip, prelude::*};

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::tree::{AgentEntry, RowKey};

impl BranchPanel {
    /// The disclosure line and, when open, the agents under it.
    ///
    /// `None` when the branch has none: a control that opens on nothing reads
    /// as broken.
    pub(super) fn render_agents(
        &self,
        ix: usize,
        agents: &Arc<[AgentEntry]>,
        expanded: bool,
        toggle_key: Option<RowKey>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if agents.is_empty() {
            return None;
        }
        let count = agents.len();

        Some(
            v_flex()
                .w_full()
                .gap_0p5()
                .child(
                    h_flex()
                        .id(("branch-agents", ix))
                        .w_full()
                        .justify_between()
                        .child(
                            Label::new(if count == 1 {
                                "1 agent".to_string()
                            } else {
                                format!("{count} agents")
                            })
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                        )
                        .child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                        )
                        .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                            // Without this the click reaches the card too and
                            // checks the branch out -- opening a list is not
                            // asking to switch to it.
                            cx.stop_propagation();
                            if let Some(key) = toggle_key.clone() {
                                panel.toggle_row(key, cx);
                            }
                        })),
                )
                .when(expanded, |this| {
                    this.children(
                        agents
                            .iter()
                            .enumerate()
                            .map(|(nth, entry)| self.render_agent(ix, nth, entry, cx)),
                    )
                })
                .into_any_element(),
        )
    }

    fn render_agent(
        &self,
        ix: usize,
        nth: usize,
        entry: &AgentEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let running = entry.is_running();
        let label = entry.label().clone();
        let tooltip = label.clone();
        let age = entry.updated_at().map(compact_age);
        let agent = entry.agent().clone();
        let entry = entry.clone();

        h_flex()
            .id(("agent", ix * 1000 + nth))
            .w_full()
            .gap_1p5()
            .pl_1()
            .py_0p5()
            .rounded_sm()
            .hover(|style| style.bg(cx.theme().colors().element_hover))
            .child(Indicator::dot().color(if running {
                Color::Success
            } else {
                Color::Muted
            }))
            // The vendor's own mark in the vendor's own colour, so a glance at
            // the list says which agent as well as which session.
            .child(
                Icon::new(agent_ui::agent_icon(&agent))
                    .size(IconSize::XSmall)
                    .color(Color::Custom(agent_ui::agent_color(&agent))),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Label::new(label)
                        .size(LabelSize::XSmall)
                        .truncate()
                        .color(if running {
                            Color::Default
                        } else {
                            Color::Muted
                        }),
                ),
            )
            .when_some(age, |this, age| {
                this.child(
                    Label::new(age)
                        .size(LabelSize::XSmall)
                        .color(Color::Disabled),
                )
            })
            .tooltip(move |_, cx| Tooltip::simple(tooltip.clone(), cx))
            .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                panel.open_agent(&entry, window, cx);
            }))
            .into_any_element()
    }

    /// Jumps to a running agent's tab, or brings a finished session back.
    ///
    /// Resuming goes through `agent_ui::resume_session`, the same call the
    /// history panel makes: the rules about what may be resumed -- the agent
    /// must support it, the working directory must still exist -- belong to the
    /// operation, not to whichever surface asked for it.
    pub(crate) fn open_agent(
        &mut self,
        entry: &AgentEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };

        match entry {
            AgentEntry::Running { view, .. } => {
                let Some(view) = view.upgrade() else {
                    return;
                };
                workspace.update(cx, |workspace, cx| {
                    workspace.activate_item(&view, true, true, window, cx);
                });
            }
            AgentEntry::Past { id, .. } => {
                let Some(store) = self.session_store.clone() else {
                    return;
                };
                let index = store.read(cx).index().clone();
                let Some(session) = index.find(id) else {
                    return;
                };
                agent_ui::resume_session(
                    &workspace,
                    session,
                    agent_sessions::Fork::Continue,
                    window,
                    cx,
                );
            }
        }
    }
}

/// How long ago, in the space a 280px panel can spare.
///
/// `time_format::TimestampFormat::Relative` says "3 hours ago", which is right
/// in a list with room for it and wrong at the end of a row that already
/// carries a session title. Deliberately coarse: nobody reads this to the
/// minute, and the exact time is in the tooltip of the row it belongs to.
pub(crate) fn compact_age(at: SystemTime) -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(at) else {
        // A timestamp in the future is a clock that moved, not a session from
        // tomorrow. "now" is the least wrong thing to say about it.
        return "now".to_string();
    };
    let seconds = elapsed.as_secs();

    match seconds {
        ..60 => "now".to_string(),
        ..3_600 => format!("{}m", seconds / 60),
        ..86_400 => format!("{}h", seconds / 3_600),
        ..604_800 => format!("{}d", seconds / 86_400),
        _ => format!("{}w", seconds / 604_800),
    }
}

#[cfg(test)]
mod tests {
    use super::compact_age;
    use std::time::{Duration, SystemTime};

    fn ago(secs: u64) -> String {
        compact_age(SystemTime::now() - Duration::from_secs(secs))
    }

    #[test]
    fn under_a_minute_reads_as_now() {
        assert_eq!(ago(0), "now");
        assert_eq!(ago(59), "now");
    }

    #[test]
    fn the_unit_changes_at_each_boundary() {
        assert_eq!(ago(60), "1m");
        assert_eq!(ago(3_599), "59m");
        assert_eq!(ago(3_600), "1h");
        assert_eq!(ago(86_399), "23h");
        assert_eq!(ago(86_400), "1d");
        assert_eq!(ago(604_799), "6d");
        assert_eq!(ago(604_800), "1w");
    }

    /// A clock that jumped backwards must not produce a panic or a wrapped
    /// number -- `duration_since` fails rather than returning a negative.
    #[test]
    fn a_timestamp_in_the_future_reads_as_now() {
        assert_eq!(
            compact_age(SystemTime::now() + Duration::from_secs(3_600)),
            "now"
        );
    }
}
