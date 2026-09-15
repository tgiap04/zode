//! The agents of one branch, drawn inside its card.
//!
//! Inside, not beneath: the card's border is what says "these belong to this
//! branch", and a list that starts after the border closes says the opposite.
//! That is why a branch row carries its agents rather than the tree emitting
//! them as rows of their own.

use std::sync::Arc;
use std::time::SystemTime;

use gpui::{AnyElement, ClickEvent};
use ui::{CommonAnimationExt as _, Indicator, Tooltip, prelude::*};

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::tree::{AgentActivity, AgentEntry, RowKey, SubagentKey};

/// One subagent, under the session that spawned it.
///
/// The same spinner the session's own row uses, because it is the same fact
/// about a different worker. The dot beside a finished one is muted rather than
/// green: a finished subagent is not waiting for you, it has already handed its
/// answer back to the session above it, and green is the colour this panel uses
/// for "this one wants you".
fn render_subagent(
    id: usize,
    kind: SharedString,
    description: SharedString,
    running: bool,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap_1p5()
        .pl_2()
        .py_0p5()
        .child(if running {
            Icon::new(IconName::LoadCircle)
                .size(IconSize::XSmall)
                .color(Color::Accent)
                // Keyed, not caller-located: every subagent row in the panel
                // shares this call site, and one id for all of them is one
                // animation state for all of them.
                .with_keyed_rotate_animation(("subagent-spinner", id), 1)
                .into_any_element()
        } else {
            Indicator::dot().color(Color::Muted).into_any_element()
        })
        .child(Label::new(kind).size(LabelSize::XSmall).color(if running {
            Color::Default
        } else {
            Color::Muted
        }))
        .child(
            div().flex_1().min_w_0().child(
                Label::new(description)
                    .size(LabelSize::XSmall)
                    .truncate()
                    .color(Color::Disabled),
            ),
        )
}

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
        let activity = entry.activity(cx);
        let label = entry.label(cx);
        let tooltip = label.clone();
        let age = entry.updated_at().map(compact_age);
        let agent = entry.agent().clone();
        // Built before the entry is moved into the handlers below, and drawn
        // under the row rather than inside it: the session's own line is one
        // click target, and a disclosure nested inside it would put a second
        // one in the middle of it.
        let subagents = self.render_subagents(ix, nth, entry, cx);
        let entry = entry.clone();

        let row = h_flex()
            .id(("agent", ix * 1000 + nth))
            .w_full()
            .gap_1p5()
            .pl_1()
            .py_0p5()
            .rounded_sm()
            .hover(|style| style.bg(cx.theme().colors().element_hover))
            // A spinner only while it is answering: "still going" is a moving
            // fact and a static mark says it no better than the words beside
            // it, but a spinner that never stops says nothing at all. A green
            // dot is the one worth scanning for -- that agent is done and
            // waiting for you.
            .child(match activity {
                AgentActivity::Responding => Icon::new(IconName::LoadCircle)
                    .size(IconSize::XSmall)
                    .color(Color::Accent)
                    // Keyed, not caller-located: every running agent in the
                    // list shares this call site, and one id for all of them
                    // is one animation state for all of them.
                    .with_keyed_rotate_animation(("agent-spinner", ix * 1000 + nth), 1)
                    .into_any_element(),
                AgentActivity::Ready => Indicator::dot().color(Color::Success).into_any_element(),
                AgentActivity::Gone => Indicator::dot().color(Color::Muted).into_any_element(),
            })
            // The vendor's own mark in the vendor's own colour, so a glance at
            // the list says which agent as well as which session.
            .child(
                Icon::new(agent_ui::agent_icon(&agent))
                    .size(IconSize::XSmall)
                    .color(Color::Custom(agent_ui::agent_color(&agent))),
            )
            .child(div().flex_1().min_w_0().child(
                Label::new(label).size(LabelSize::XSmall).truncate().color(
                    if activity == AgentActivity::Gone {
                        Color::Muted
                    } else {
                        Color::Default
                    },
                ),
            ))
            .when_some(age, |this, age| {
                this.child(
                    Label::new(age)
                        .size(LabelSize::XSmall)
                        .color(Color::Disabled),
                )
            })
            .when(!self.menu_is_open(), |this| {
                this.tooltip(move |_, cx| Tooltip::simple(tooltip.clone(), cx))
            })
            .on_click(cx.listener({
                let entry = entry.clone();
                move |panel, _: &ClickEvent, window, cx| {
                    panel.open_agent(&entry, window, cx);
                }
            }))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |panel, event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let menu = panel.agent_context_menu(&entry, window, cx);
                    panel.open_context_menu(menu, event.position, window, cx);
                }),
            );

        v_flex()
            .w_full()
            .child(row)
            .children(subagents)
            .into_any_element()
    }

    /// The subagents of one session: a disclosure, and the rows under it when
    /// open.
    ///
    /// `None` when the session spawned none, or when its agent keeps no record
    /// of them — only Claude writes a sidecar naming each one, so Codex,
    /// Copilot and opencode rows carry nothing here rather than an empty
    /// disclosure that never opens onto anything.
    fn render_subagents(
        &self,
        ix: usize,
        nth: usize,
        entry: &AgentEntry,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let key = entry.subagent_key()?;
        // Cheap clones: both labels are `Arc<str>` precisely because this runs
        // again on every animation frame while one of them is spinning.
        let rows: Vec<(SharedString, SharedString, bool)> = match entry {
            AgentEntry::Open { view, .. } => {
                let view = view.upgrade()?;
                let view = view.read(cx);
                view.subagents()
                    .iter()
                    .map(|subagent| {
                        (
                            SharedString::from(subagent.kind.clone()),
                            SharedString::from(subagent.description.clone()),
                            view.subagent_is_running(subagent),
                        )
                    })
                    .collect()
            }
            AgentEntry::Past { id, .. } => {
                let tracker = self.past_subagents.get(id)?;
                tracker
                    .subagents()
                    .iter()
                    .map(|subagent| {
                        (
                            SharedString::from(subagent.kind.clone()),
                            SharedString::from(subagent.description.clone()),
                            tracker.is_running(subagent),
                        )
                    })
                    .collect()
            }
        };
        if rows.is_empty() {
            return None;
        }

        let expanded = self.expanded_subagents.contains(&key);
        let count = rows.len();

        Some(
            v_flex()
                .w_full()
                .pl_3()
                .gap_0p5()
                .child(
                    h_flex()
                        .id(("subagents", ix * 1000 + nth))
                        .w_full()
                        .gap_1()
                        .child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                        )
                        .child(
                            Label::new(if count == 1 {
                                "1 subagent".to_string()
                            } else {
                                format!("{count} subagents")
                            })
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                        )
                        .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                            // Two ancestors would otherwise act on this: the
                            // agent row opens the session, and the card behind
                            // it checks the branch out. Opening a list asks for
                            // neither.
                            cx.stop_propagation();
                            panel.toggle_subagents(key.clone(), cx);
                        })),
                )
                .when(expanded, |this| {
                    this.children(rows.into_iter().enumerate().map(
                        |(at, (kind, description, running))| {
                            render_subagent(
                                ix * 100_000 + nth * 100 + at,
                                kind,
                                description,
                                running,
                            )
                        },
                    ))
                })
                .into_any_element(),
        )
    }

    /// Opens or closes one session's subagent list, reading a finished
    /// session's the first time it is asked for.
    pub(crate) fn toggle_subagents(&mut self, key: SubagentKey, cx: &mut Context<Self>) {
        if !self.expanded_subagents.remove(&key) {
            self.expanded_subagents.insert(key);
        }
        cx.notify();
    }

    /// Reads a finished session's subagents, once.
    ///
    /// An open tab answers for itself and never reaches this — its own tracker
    /// is already following the session it is showing.
    pub(crate) fn read_past_subagents(
        &mut self,
        agent: &SharedString,
        id: &Arc<str>,
        cx: &mut Context<Self>,
    ) {
        if self.past_subagents.contains_key(id) || self._subagent_reads.contains_key(id) {
            return;
        }
        let Some(provider) =
            agent_ui::provider_for_agent(&project::AgentId::new(agent.to_string()))
        else {
            return;
        };
        let id_key = id.clone();
        let session_id = SharedString::from(id.to_string());
        let id = id.clone();
        let read = cx.spawn(async move |panel, cx| {
            let pass = cx
                .background_spawn(async move {
                    agent_ui::SubagentTracker::scan(&provider, None, session_id, 0)
                })
                .await;
            panel
                .update(cx, |panel, cx| {
                    let mut tracker = agent_ui::SubagentTracker::default();
                    tracker.apply(pass);
                    panel.past_subagents.insert(id, tracker);
                    cx.notify();
                })
                // A failed update means the panel is gone, and with it the
                // handle that was holding this task -- there is nobody left to
                // tell.
                .ok();
        });
        self._subagent_reads.insert(id_key, read);
    }

    /// The right-click menu on an agent.
    ///
    /// Deleting is offered only for a session with no tab open on it. A tab
    /// may still be writing its transcript, and one that has finished is still
    /// showing what would be removed out from under it.
    fn agent_context_menu(
        &self,
        entry: &AgentEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<ui::ContextMenu> {
        let panel = cx.entity();
        let entry = entry.clone();

        ui::ContextMenu::build(window, cx, move |menu, _window, _cx| {
            let open = panel.clone();
            let open_entry = entry.clone();
            let delete = panel.clone();
            let delete_entry = entry.clone();
            let has_tab = entry.is_open();

            menu.item(
                ui::ContextMenuEntry::new(if has_tab { "Go to Tab" } else { "Resume" })
                    .icon(if has_tab {
                        IconName::ArrowUpRight
                    } else {
                        IconName::PlayOutlined
                    })
                    .icon_position(ui::IconPosition::Start)
                    .handler(move |window, cx| {
                        open.update(cx, |panel, cx| panel.open_agent(&open_entry, window, cx));
                    }),
            )
            .separator()
            .item(
                ui::ContextMenuEntry::new("Delete Session\u{2026}")
                    .icon(IconName::Trash)
                    .icon_position(ui::IconPosition::Start)
                    .icon_color(Color::Error)
                    .disabled(has_tab)
                    .handler(move |window, cx| {
                        delete.update(cx, |panel, cx| {
                            panel.delete_agent_session(&delete_entry, window, cx)
                        });
                    }),
            )
        })
    }

    /// Moves one session's transcript to the trash, after asking.
    ///
    /// Through `agent_ui::delete_session`, the same call the history panel
    /// makes: what a delete takes and what it warns about belong to the
    /// operation, not to whichever surface asked for it.
    fn delete_agent_session(
        &mut self,
        entry: &AgentEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let AgentEntry::Past { id, .. } = entry else {
            return;
        };
        let (Some(workspace), Some(store)) = (self.workspace.upgrade(), self.session_store.clone())
        else {
            return;
        };
        let index = store.read(cx).index().clone();
        let Some(session) = index.find(id) else {
            return;
        };

        // The store is shared, so removing the entry there is what refreshes
        // this panel -- no callback needed.
        agent_ui::delete_session(&workspace, session, window, cx);
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
            AgentEntry::Open { view, .. } => {
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

/// How long ago, in the space a 360px panel can spare.
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
