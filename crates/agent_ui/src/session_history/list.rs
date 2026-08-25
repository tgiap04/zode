use crate::session_history::{
    panel::{AgentHistoryPanel, PANEL_GROUP},
    row,
};
use agent_sessions::{AgentKind, SessionSummary};
use collections::HashSet;
use gpui::{Context, Window};
use std::path::{Path, PathBuf};
use ui::{Divider, DividerColor, ScrollAxes, Scrollbars, Tooltip, WithScrollbar as _, prelude::*};

/// One line of the list. Three levels, outermost first.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Row {
    /// An agent, and every session it has anywhere in this project.
    AgentGroup {
        agent: AgentKind,
        count: usize,
        collapsed: bool,
    },
    /// A project directory, under the agent that ran there.
    ///
    /// Carries the agent as well as the path because the same path appears under
    /// every agent used in it, and the two must open and close independently.
    ProjectGroup {
        agent: AgentKind,
        path: PathBuf,
        label: String,
        count: usize,
        collapsed: bool,
    },
    Session(SessionSummary),
}

/// Turn the flat session list into the rows the panel draws.
///
/// Four rules, in order:
///
/// 1. **Only this project's sessions.** A session belongs if it ran inside one of
///    `roots` — the worktrees of the workspace this panel belongs to. Switching
///    project therefore switches the list, because a different workspace has
///    different roots.
/// 2. **Group by agent first**, so the outermost thing the eye lands on is which
///    tool the conversation was with.
/// 3. **Then by the session's own `cwd`.** Never by the encoded directory name
///    Claude stores them under: `/a/b-c` and `/a-b-c` both encode to `-a-b-c`, so
///    that name cannot be decoded back into a path.
/// 4. **Newest first at every level** — a group is as recent as its most recent
///    session, and that holds for agents as much as for projects.
///
/// An agent with nothing to show gets no header: an empty section is a row that
/// costs space and answers nothing.
///
/// Kept a free function over plain data so the rules can be tested without a
/// window, which is the only way to test them honestly.
pub(crate) fn rows(
    sessions: &[SessionSummary],
    roots: &[PathBuf],
    query: &str,
    collapsed_agents: &HashSet<AgentKind>,
    collapsed: &HashSet<(AgentKind, PathBuf)>,
) -> Vec<Row> {
    let query = query.trim().to_lowercase();

    // Two nested insertion-ordered lists rather than maps: `sessions` arrives
    // newest first, so first-seen order *is* newest-first order at both levels,
    // and a map would throw that away and need it sorted back.
    let mut agents: Vec<(AgentKind, Vec<(PathBuf, Vec<SessionSummary>)>)> = Vec::new();
    for session in sessions {
        if !session.is_within(roots) || !matches(session, &query) {
            continue;
        }
        let index = match agents.iter().position(|(agent, _)| *agent == session.agent) {
            Some(index) => index,
            None => {
                agents.push((session.agent, Vec::new()));
                agents.len() - 1
            }
        };
        let projects = &mut agents[index].1;
        match projects.iter_mut().find(|(cwd, _)| *cwd == session.cwd) {
            Some((_, found)) => found.push(session.clone()),
            None => projects.push((session.cwd.clone(), vec![session.clone()])),
        }
    }

    let mut out = Vec::new();
    for (agent, projects) in agents {
        let agent_collapsed = collapsed_agents.contains(&agent);
        out.push(Row::AgentGroup {
            agent,
            count: projects.iter().map(|(_, s)| s.len()).sum(),
            collapsed: agent_collapsed,
        });
        if agent_collapsed {
            continue;
        }
        for (cwd, sessions) in projects {
            let collapsed = collapsed.contains(&(agent, cwd.clone()));
            out.push(Row::ProjectGroup {
                agent,
                label: group_label(&cwd, roots),
                count: sessions.len(),
                path: cwd,
                collapsed,
            });
            if !collapsed {
                out.extend(sessions.into_iter().map(Row::Session));
            }
        }
    }
    out
}

/// Substring, case-insensitive, across everything the row shows as text.
///
/// Not fuzzy: the list is short and a fuzzy match that quietly reorders results
/// makes "why is that one still here" a harder question than it needs to be.
fn matches(session: &SessionSummary, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let haystacks = [
        session.title.to_lowercase(),
        session.preview.to_lowercase(),
        session.cwd.to_string_lossy().to_lowercase(),
        session.branch.clone().unwrap_or_default().to_lowercase(),
        session.model.clone().unwrap_or_default().to_lowercase(),
        session.agent.label().to_lowercase(),
    ];
    haystacks.iter().any(|haystack| haystack.contains(query))
}

/// The shortest tail of `cwd` that still says which directory this is.
///
/// A worktree root shows its own last component; anything deeper shows the path
/// relative to the root it sits in, so a subdirectory session reads as
/// `zode/crates/ui` rather than repeating the whole absolute path.
fn group_label(cwd: &Path, roots: &[PathBuf]) -> String {
    let root = roots
        .iter()
        .filter(|root| cwd.starts_with(root))
        .max_by_key(|root| root.components().count());
    let name = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    };
    match root {
        Some(root) if root == cwd => name(root),
        Some(root) => match cwd.strip_prefix(root) {
            Ok(rest) => format!("{}/{}", name(root), rest.to_string_lossy()),
            Err(_) => name(cwd),
        },
        None => name(cwd),
    }
}

impl Render for AgentHistoryPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (background, border) = {
            let colors = cx.theme().colors();
            (colors.panel_background, colors.border)
        };
        let roots = self.project_roots(cx);
        let query = self.query(cx);
        let rows = rows(
            &self.sessions,
            &roots,
            &query,
            &self.collapsed_agents,
            &self.collapsed_groups,
        );
        let unavailable = self.unavailable();

        v_flex()
            .id("agent-history-panel")
            .debug_selector(|| "agent-history-panel".into())
            .group(PANEL_GROUP)
            .key_context("AgentHistory")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(background)
            .child(self.render_search(cx))
            .child(Divider::horizontal().color(DividerColor::Border))
            .child(if rows.is_empty() {
                self.render_empty(&roots, &query, cx).into_any_element()
            } else {
                self.render_rows(rows, window, cx).into_any_element()
            })
            .children((!unavailable.is_empty()).then(|| {
                // Named, not hidden: half the feature is missing and the only
                // person who can fix it is the one reading this.
                v_flex()
                    .flex_none()
                    .p_2()
                    .gap_0p5()
                    .border_t_1()
                    .border_color(border)
                    .children(unavailable.into_iter().map(|(agent, reason)| {
                        Label::new(format!("{agent}: {reason}"))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                    }))
            }))
    }
}

impl AgentHistoryPanel {
    fn render_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        h_flex()
            .flex_none()
            .w_full()
            .px_2()
            .py_1p5()
            .gap_1p5()
            .child(
                Icon::new(IconName::MagnifyingGlass)
                    .size(IconSize::Small)
                    .color(Color::Muted),
            )
            .child(div().flex_1().child(self.filter_editor.clone()))
            .children(self.loading.then(|| {
                Label::new("Reading…")
                    .size(LabelSize::XSmall)
                    .color(Color::Muted)
            }))
            .children((!self.loading).then(|| {
                IconButton::new("agent-history-refresh", IconName::ArrowCircle)
                    .icon_size(IconSize::Small)
                    .tooltip(|_window, cx| Tooltip::simple("Refresh", cx))
                    .on_click(cx.listener(|this, _, _window, cx| this.refresh(cx)))
            }))
            .bg(colors.panel_background)
    }

    fn render_empty(
        &self,
        roots: &[PathBuf],
        query: &str,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // Three different nothings, and telling them apart is the whole value of
        // this element: no project open, nothing matched, or genuinely no history.
        let message = if roots.is_empty() {
            "Open a project to see its agent sessions"
        } else if !query.trim().is_empty() {
            "No sessions match"
        } else if self.loading {
            "Reading sessions…"
        } else {
            "No agent sessions for this project yet"
        };
        v_flex()
            .flex_1()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Icon::new(IconName::Astroid)
                    .size(IconSize::XLarge)
                    .color(Color::Muted),
            )
            .child(Label::new(message).color(Color::Muted))
    }

    /// A variable-height list inside a column parent.
    ///
    /// Two things here are load-bearing and were each learned the hard way:
    ///
    /// - **`gpui::list`, not `uniform_list`.** A group header and a session row
    ///   are different heights. `uniform_list` measures its first item and spaces
    ///   every row by that, so a mixed list draws its rows on top of one another —
    ///   visible only on a painted frame.
    /// - **A column parent.** The list has no intrinsic height, and `flex_1` under
    ///   a row parent resolves to zero rows drawn with nothing to say so.
    fn render_rows(
        &mut self,
        rows: Vec<Row>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // The state carries the row count across frames; resetting it whenever the
        // count changes is what keeps scroll position sane while the list is
        // filtered.
        if self.list_state.item_count() != rows.len() {
            self.list_state.reset(rows.len());
        }
        let rows = std::rc::Rc::new(rows);
        v_flex()
            .flex_1()
            .min_h_0()
            .debug_selector(|| "agent-history-list".into())
            .child(
                gpui::list(
                    self.list_state.clone(),
                    cx.processor(move |this, ix: usize, _window, cx| match &rows[ix] {
                        Row::AgentGroup {
                            agent,
                            count,
                            collapsed,
                        } => row::render_agent_group(ix, *agent, *count, *collapsed, cx)
                            .into_any_element(),
                        Row::ProjectGroup {
                            agent,
                            path,
                            label,
                            count,
                            collapsed,
                        } => row::render_group(ix, *agent, path, label, *count, *collapsed, cx)
                            .into_any_element(),
                        Row::Session(session) => {
                            // Only rows the list actually renders pay for their
                            // numbers.
                            this.ensure_counts(session, cx);
                            row::render_session(ix, session, this, cx).into_any_element()
                        }
                    }),
                )
                .size_full(),
            )
            .custom_scrollbars(Scrollbars::new(ScrollAxes::Vertical), window, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_sessions::{AgentKind, SessionSummary};
    use std::{
        sync::Arc,
        time::{Duration, UNIX_EPOCH},
    };

    fn session(id: &str, cwd: &str, title: &str, secs: u64) -> SessionSummary {
        agent_session(id, cwd, title, secs, AgentKind::Claude)
    }

    fn agent_session(
        id: &str,
        cwd: &str,
        title: &str,
        secs: u64,
        agent: AgentKind,
    ) -> SessionSummary {
        SessionSummary {
            id: Arc::from(id),
            agent,
            title: title.to_string(),
            preview: format!("preview of {title}"),
            preview_speaker: None,
            cwd: PathBuf::from(cwd),
            branch: Some("main".into()),
            model: Some("claude-opus-5".into()),
            updated_at: UNIX_EPOCH + Duration::from_secs(secs),
            log_path: None,
            log_bytes: 0,
        }
    }

    /// Newest first, as `list_all` hands them over.
    fn all() -> Vec<SessionSummary> {
        vec![
            session("a", "/w/zode", "Newest here", 300),
            session("b", "/w/zode/crates/ui", "Deeper", 200),
            session("c", "/w/other", "Another project", 100),
            session("d", "/w/zode", "Older here", 50),
        ]
    }

    #[test]
    fn only_sessions_of_the_open_project_are_listed() {
        let roots = vec![PathBuf::from("/w/zode")];
        let listed = rows(&all(), &roots, "", &HashSet::default(), &HashSet::default());
        let sessions: Vec<&str> = listed
            .iter()
            .filter_map(|row| match row {
                Row::Session(session) => Some(&*session.id),
                _ => None,
            })
            .collect();
        assert_eq!(
            sessions,
            vec!["a", "d", "b"],
            "the other project's session must not appear, \
             and each group keeps its own newest-first order"
        );

        // The other project, opened instead, shows the other session — this is
        // what makes switching project switch the list.
        let other = rows(
            &all(),
            &[PathBuf::from("/w/other")],
            "",
            &HashSet::default(),
            &HashSet::default(),
        );
        let sessions: Vec<&str> = other
            .iter()
            .filter_map(|row| match row {
                Row::Session(session) => Some(&*session.id),
                _ => None,
            })
            .collect();
        assert_eq!(sessions, vec!["c"]);
    }

    #[test]
    fn with_no_project_open_nothing_is_listed() {
        assert!(rows(&all(), &[], "", &HashSet::default(), &HashSet::default()).is_empty());
    }

    #[test]
    fn groups_carry_a_label_and_a_count() {
        let roots = vec![PathBuf::from("/w/zode")];
        let rows = rows(&all(), &roots, "", &HashSet::default(), &HashSet::default());
        let groups: Vec<(String, usize)> = rows
            .iter()
            .filter_map(|row| match row {
                Row::ProjectGroup { label, count, .. } => Some((label.clone(), *count)),
                _ => None,
            })
            .collect();
        assert_eq!(
            groups,
            vec![("zode".to_string(), 2), ("zode/crates/ui".to_string(), 1)],
            "a subdirectory session reads relative to the root it sits in"
        );
    }

    #[test]
    fn a_collapsed_group_keeps_its_header_and_drops_its_sessions() {
        let roots = vec![PathBuf::from("/w/zode")];
        let mut collapsed = HashSet::default();
        collapsed.insert((AgentKind::Claude, PathBuf::from("/w/zode")));
        let rows = rows(&all(), &roots, "", &HashSet::default(), &collapsed);
        assert_eq!(
            rows.len(),
            4,
            "the agent header, two project headers, and the one uncollapsed session"
        );
        assert!(matches!(
            &rows[1],
            Row::ProjectGroup {
                collapsed: true,
                count: 2,
                ..
            }
        ));
    }

    /// Agents are the outer level, and an agent header counts every session
    /// underneath it rather than just its first project's.
    #[test]
    fn agents_are_the_outer_level_and_carry_the_total() {
        let roots = vec![PathBuf::from("/w/zode")];
        let sessions = vec![
            agent_session("a", "/w/zode", "Claude newest", 400, AgentKind::Claude),
            agent_session("b", "/w/zode/crates/ui", "Codex", 300, AgentKind::Codex),
            agent_session("c", "/w/zode", "Claude older", 200, AgentKind::Claude),
            agent_session("d", "/w/zode", "Copilot", 100, AgentKind::Copilot),
        ];
        let listed = rows(
            &sessions,
            &roots,
            "",
            &HashSet::default(),
            &HashSet::default(),
        );

        let agents: Vec<(AgentKind, usize)> = listed
            .iter()
            .filter_map(|row| match row {
                Row::AgentGroup { agent, count, .. } => Some((*agent, *count)),
                _ => None,
            })
            .collect();
        assert_eq!(
            agents,
            vec![
                (AgentKind::Claude, 2),
                (AgentKind::Codex, 1),
                (AgentKind::Copilot, 1),
            ],
            "newest-first between agents, and each total counts its whole subtree"
        );
        assert!(
            matches!(&listed[0], Row::AgentGroup { .. }),
            "the first row is an agent, not a project"
        );
    }

    /// The trap this grouping sets: the same project appears under every agent
    /// the user ran there. Keying collapse by path alone would make closing it
    /// under one agent close it under the other.
    #[test]
    fn closing_a_project_under_one_agent_leaves_it_open_under_another() {
        let roots = vec![PathBuf::from("/w/zode")];
        let sessions = vec![
            agent_session("a", "/w/zode", "Claude here", 400, AgentKind::Claude),
            agent_session("b", "/w/zode", "Codex here", 300, AgentKind::Codex),
        ];
        let mut collapsed = HashSet::default();
        collapsed.insert((AgentKind::Claude, PathBuf::from("/w/zode")));

        let listed = rows(&sessions, &roots, "", &HashSet::default(), &collapsed);
        let shown: Vec<&str> = listed
            .iter()
            .filter_map(|row| match row {
                Row::Session(session) => Some(&*session.id),
                _ => None,
            })
            .collect();
        assert_eq!(
            shown,
            vec!["b"],
            "Claude's /w/zode is closed; Codex's /w/zode is a different section \
             and must still be open"
        );
    }

    #[test]
    fn a_collapsed_agent_hides_its_projects_and_its_sessions() {
        let roots = vec![PathBuf::from("/w/zode")];
        let sessions = vec![
            agent_session("a", "/w/zode", "Claude", 400, AgentKind::Claude),
            agent_session(
                "b",
                "/w/zode/crates/ui",
                "Claude deeper",
                300,
                AgentKind::Claude,
            ),
            agent_session("c", "/w/zode", "Codex", 200, AgentKind::Codex),
        ];
        let mut agents = HashSet::default();
        agents.insert(AgentKind::Claude);

        let listed = rows(&sessions, &roots, "", &agents, &HashSet::default());
        assert!(matches!(
            &listed[0],
            Row::AgentGroup {
                agent: AgentKind::Claude,
                collapsed: true,
                count: 2,
                ..
            }
        ));
        assert!(
            !listed.iter().any(|row| matches!(
                row,
                Row::ProjectGroup {
                    agent: AgentKind::Claude,
                    ..
                }
            )),
            "a closed agent shows none of its projects"
        );
        assert_eq!(
            listed
                .iter()
                .filter(|row| matches!(row, Row::Session(_)))
                .count(),
            1,
            "only Codex's session is left"
        );
    }

    /// A filter that empties an agent must take its header with it: a section
    /// that says "Codex 0" answers nothing and costs a row.
    #[test]
    fn an_agent_with_nothing_matching_gets_no_header() {
        let roots = vec![PathBuf::from("/w/zode")];
        let sessions = vec![
            agent_session("a", "/w/zode", "findable", 400, AgentKind::Claude),
            agent_session("b", "/w/zode", "invisible", 300, AgentKind::Codex),
        ];
        let listed = rows(
            &sessions,
            &roots,
            "findable",
            &HashSet::default(),
            &HashSet::default(),
        );
        let agents: Vec<AgentKind> = listed
            .iter()
            .filter_map(|row| match row {
                Row::AgentGroup { agent, .. } => Some(*agent),
                _ => None,
            })
            .collect();
        assert_eq!(agents, vec![AgentKind::Claude]);
    }

    #[test]
    fn the_query_matches_every_field_the_row_shows() {
        let roots = vec![PathBuf::from("/w/zode")];
        let hits = |query: &str| {
            rows(
                &all(),
                &roots,
                query,
                &HashSet::default(),
                &HashSet::default(),
            )
            .iter()
            .filter(|row| matches!(row, Row::Session(_)))
            .count()
        };
        assert_eq!(hits("newest"), 1, "title");
        assert_eq!(hits("PREVIEW OF Deeper"), 1, "preview, case-insensitively");
        assert_eq!(hits("crates"), 1, "cwd");
        assert_eq!(hits("opus"), 3, "model");
        assert_eq!(hits("claude"), 3, "agent name");
        assert_eq!(hits("nothing at all"), 0);
    }
}
