use crate::session_history::{
    actions::cwd_exists,
    panel::{AgentHistoryPanel, CountState},
};
use agent_sessions::{Fork, SessionSummary};
use gpui::{Anchor, Context, Window};
use std::path::{Path, PathBuf};
use ui::{ContextMenu, ContextMenuEntry, IconName, PopoverMenu, Tooltip, prelude::*};

/// A project group's header: name, how many sessions, and a disclosure.
pub(crate) fn render_group(
    ix: usize,
    path: &Path,
    label: &str,
    count: usize,
    collapsed: bool,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let colors = cx.theme().colors();
    let path = path.to_path_buf();
    h_flex()
        .id(("agent-history-group", ix))
        .w_full()
        .px_2()
        .py_1()
        .gap_1()
        .justify_between()
        .bg(colors.elevated_surface_background)
        .border_b_1()
        .border_color(colors.border)
        .child(
            h_flex()
                .gap_1()
                .child(
                    Icon::new(if collapsed {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .size(IconSize::XSmall)
                    .color(Color::Muted),
                )
                .child(Label::new(label.to_string()).size(LabelSize::Small)),
        )
        .child(
            Label::new(count.to_string())
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
        .on_click(cx.listener(move |this, _, _window, cx| {
            if !this.collapsed_groups.remove(&path) {
                this.collapsed_groups.insert(path.clone());
            }
            cx.notify();
        }))
}

/// One session: what it was, who ran it, and what can be done with it.
pub(crate) fn render_session(
    ix: usize,
    session: &SessionSummary,
    panel: &mut AgentHistoryPanel,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let colors = cx.theme().colors();
    let expanded = panel.expanded_rows.contains(&session.id);
    let alive = cwd_exists(session);
    let can_fork = panel.can_fork(session);
    let roots = panel.project_roots(cx);

    v_flex()
        .id(("agent-history-session", ix))
        .debug_selector(move || format!("agent-history-row:{ix}"))
        .group(row_group(ix))
        .w_full()
        .px_2()
        .py_1p5()
        .gap_1()
        .border_b_1()
        .border_color(colors.border_variant)
        .hover(|style| style.bg(colors.element_hover))
        .child(
            h_flex()
                .w_full()
                .gap_1()
                .justify_between()
                .child(
                    div().flex_1().overflow_hidden().child(
                        Label::new(session.title.clone())
                            .single_line()
                            .color(Color::Default),
                    ),
                )
                .child(render_controls(ix, session, alive, can_fork, expanded, cx)),
        )
        .child(render_preview(session, expanded))
        .child(render_facts(session, panel))
        .child(render_badges(session, &roots, alive, cx))
        .children(expanded.then(|| render_details(session, cx)))
        .on_click(cx.listener({
            let id = session.id.clone();
            move |this, _, _window, cx| {
                if !this.expanded_rows.remove(&id) {
                    this.expanded_rows.insert(id.clone());
                }
                cx.notify();
            }
        }))
}

fn row_group(ix: usize) -> SharedString {
    SharedString::from(format!("agent-history-row-{ix}"))
}

/// The hover toolbar and the menu behind the ellipsis.
fn render_controls(
    ix: usize,
    session: &SessionSummary,
    alive: bool,
    can_fork: bool,
    expanded: bool,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let group = row_group(ix);
    h_flex()
        .flex_none()
        .gap_0p5()
        .child(
            IconButton::new(("resume", ix), IconName::PlayOutlined)
                .icon_size(IconSize::Small)
                .disabled(!alive)
                .visible_on_hover(group.clone())
                .tooltip(move |_window, cx| {
                    Tooltip::simple(
                        if alive {
                            "Resume in Worktree"
                        } else {
                            "Resume in Worktree — the directory is gone"
                        },
                        cx,
                    )
                })
                .on_click(cx.listener({
                    let session = session.clone();
                    move |this, _, window, cx| {
                        this.resume(&session, Fork::Continue, window, cx);
                    }
                })),
        )
        .child(
            IconButton::new(("fork", ix), IconName::Plus)
                .icon_size(IconSize::Small)
                .disabled(!alive || !can_fork)
                .visible_on_hover(group)
                // Two different reasons this can be off, and the tooltip must name
                // the one that applies: a dead directory first, since that is
                // checked first.
                .tooltip(move |_window, cx| {
                    Tooltip::simple(
                        match (alive, can_fork) {
                            (true, true) => "Continue in New Session…",
                            (false, _) => "Continue in New Session — the directory is gone",
                            (true, false) => "Continue in New Session — Codex has no fork",
                        },
                        cx,
                    )
                })
                .on_click(cx.listener({
                    let session = session.clone();
                    move |this, _, window, cx| {
                        this.resume(&session, Fork::New, window, cx);
                    }
                })),
        )
        .child(
            Icon::new(if expanded {
                IconName::ChevronUp
            } else {
                IconName::ChevronDown
            })
            .size(IconSize::Small)
            .color(Color::Muted),
        )
        .child(
            // Wrapped only so a test can find the trigger's bounds and click it;
            // `IconButton` has no probe of its own.
            div()
                .debug_selector(move || format!("agent-history-menu:{ix}"))
                .child(render_menu(ix, session, alive, can_fork, cx)),
        )
}

/// The ellipsis menu.
///
/// `PopoverMenu` opens it on a **left** click, which is the only way anyone tries
/// to open an ellipsis. `right_click_menu` here left the button silent: with no
/// `on_click` on the trigger, `ButtonLike` never calls `stop_propagation`
/// (`button_like.rs:766`), so the click bubbled to the row and toggled its
/// expansion instead. One wrapper, two symptoms.
///
/// The menu itself is a `ContextMenu` rather than a view of our own: a bespoke
/// view behind `PopoverMenu` gets neither a background nor dismissal on an
/// outside click, both of which `ContextMenu` handles itself.
///
/// Entries that cannot work are drawn disabled rather than left out, so the menu
/// has the same shape every time and says why a thing is unavailable.
fn render_menu(
    ix: usize,
    session: &SessionSummary,
    alive: bool,
    can_fork: bool,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let panel = cx.entity();
    let session = session.clone();
    PopoverMenu::new(("agent-history-menu", ix))
        .menu(move |window, cx| {
            let panel = panel.clone();
            let session = session.clone();
            Some(ContextMenu::build(
                window,
                cx,
                move |mut menu, _window, _cx| {
                    let has_log = session.log_path.is_some();
                    // Every entry runs against the panel entity, deferred past this
                    // click by `Entity::update` — the panel is not leased here, but
                    // the handler runs later and must not assume it still is.
                    let add = |menu: ContextMenu,
                               label: &'static str,
                               icon: Option<IconName>,
                               enabled: bool,
                               action: fn(
                        &mut AgentHistoryPanel,
                        &SessionSummary,
                        &mut Window,
                        &mut Context<AgentHistoryPanel>,
                    )| {
                        let panel = panel.clone();
                        let session = session.clone();
                        let mut entry = ContextMenuEntry::new(label).disabled(!enabled).handler(
                            move |window, cx| {
                                panel.update(cx, |panel, cx| action(panel, &session, window, cx));
                            },
                        );
                        if let Some(icon) = icon {
                            entry = entry.icon(icon);
                        }
                        menu.item(entry)
                    };

                    menu = add(
                        menu,
                        "Resume in Worktree",
                        Some(IconName::PlayOutlined),
                        alive,
                        |panel, session, window, cx| {
                            panel.resume(session, Fork::Continue, window, cx)
                        },
                    );
                    menu = add(
                        menu,
                        "Continue in New Session…",
                        Some(IconName::Plus),
                        alive && can_fork,
                        |panel, session, window, cx| panel.resume(session, Fork::New, window, cx),
                    );
                    menu = add(
                        menu,
                        "Copy Resume Command",
                        Some(IconName::Copy),
                        true,
                        |panel, session, _window, cx| panel.copy_resume_command(session, cx),
                    );
                    menu = menu.separator();
                    menu = add(
                        menu,
                        "Open Log",
                        Some(IconName::FileCode),
                        has_log,
                        |panel, session, window, cx| panel.open_log(session, window, cx),
                    );
                    menu = add(
                        menu,
                        "Reveal Log",
                        Some(IconName::Folder),
                        has_log,
                        |panel, session, _window, cx| panel.reveal_log(session, cx),
                    );
                    menu = add(
                        menu,
                        "Open Working Directory",
                        Some(IconName::Folder),
                        alive,
                        |panel, session, _window, cx| panel.open_working_directory(session, cx),
                    );
                    menu = menu.separator();
                    menu = add(
                        menu,
                        "Copy Session ID",
                        None,
                        true,
                        |panel, session, _window, cx| panel.copy(session.id.to_string(), cx),
                    );
                    menu = add(
                        menu,
                        "Copy Log Path",
                        None,
                        has_log,
                        |panel, session, _window, cx| {
                            if let Some(path) = &session.log_path {
                                panel.copy(path.display().to_string(), cx);
                            }
                        },
                    );
                    menu = menu.separator();
                    add(
                        menu,
                        "Delete",
                        Some(IconName::Trash),
                        true,
                        |panel, session, window, cx| panel.delete(session, window, cx),
                    )
                },
            ))
        })
        // Docked right, the ellipsis sits at the panel's right edge: drop the menu
        // below it and let it grow leftwards, into the window rather than off it.
        .anchor(Anchor::TopRight)
        .attach(Anchor::BottomRight)
        .trigger(
            IconButton::new(("agent-history-menu-trigger", ix), IconName::Ellipsis)
                .icon_size(IconSize::Small)
                .tooltip(Tooltip::text("More…")),
        )
}

fn render_preview(session: &SessionSummary, expanded: bool) -> impl IntoElement {
    let speaker = session
        .preview_speaker
        .map(|speaker| format!("{}: ", speaker.label()))
        .unwrap_or_default();
    div()
        .w_full()
        .child(
            Label::new(format!("{speaker}{}", session.preview))
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        // Two lines' worth of the last message, and the whole thing once the row
        // is expanded. Measured in rems so it follows the UI font rather than
        // pinning a pixel height that only suits one size.
        .when(!expanded, |this| this.max_h(rems(2.6)).overflow_hidden())
}

/// Agent, message counts, when, and which model — the row of small print.
fn render_facts(session: &SessionSummary, panel: &AgentHistoryPanel) -> impl IntoElement {
    let counts = panel.counts.get(&session.id).copied();
    let messages = match counts {
        // `…` while the scan runs. A zero here would read as an empty session.
        Some(CountState::Pending) | None => Some("… msgs".to_string()),
        Some(CountState::Ready(counts)) => {
            counts.messages.map(|messages| format!("{messages} msgs"))
        }
    };
    let subagents = match counts {
        Some(CountState::Ready(counts)) if counts.subagents > 0 => {
            Some(format!("{} subagents", counts.subagents))
        }
        _ => None,
    };
    // Relative ("15 minutes ago"), in the reader's own timezone.
    let when = time_format::format_localized_timestamp(
        time::OffsetDateTime::from(session.updated_at),
        time::OffsetDateTime::now_utc(),
        time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
        time_format::TimestampFormat::Relative,
    );

    h_flex()
        .w_full()
        .gap_1p5()
        .child(
            // Through `agent_icon` rather than a match of its own: the rail, the
            // tab and this row must draw the same vendor mark, and three copies
            // of the mapping is three places to forget when an agent is added.
            Icon::new(crate::agent_icon(session.agent.builtin_agent_id())).size(IconSize::XSmall),
        )
        .child(
            Label::new(session.agent.label())
                .size(LabelSize::XSmall)
                .color(Color::Muted),
        )
        .children(messages.map(|messages| {
            Label::new(messages)
                .size(LabelSize::XSmall)
                .color(Color::Muted)
        }))
        .children(subagents.map(|subagents| {
            Label::new(subagents)
                .size(LabelSize::XSmall)
                .color(Color::Muted)
        }))
        .child(Label::new(when).size(LabelSize::XSmall).color(Color::Muted))
        .children(session.model.clone().map(|model| {
            Label::new(model)
                .size(LabelSize::XSmall)
                .color(Color::Muted)
        }))
        .child(div().flex_1())
}

/// Which worktree this session belongs to, and which branch it was on.
fn render_badges(
    session: &SessionSummary,
    roots: &[PathBuf],
    alive: bool,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let colors = cx.theme().colors();
    let worktree = if !alive {
        Some(("Unavailable worktree", Color::Warning))
    } else if session.is_within(roots) {
        Some(("Current worktree", Color::Muted))
    } else {
        None
    };
    h_flex()
        .w_full()
        .gap_1()
        .children(worktree.map(|(label, color)| {
            div()
                .px_1()
                .rounded_sm()
                .border_1()
                .border_color(colors.border)
                .child(Label::new(label).size(LabelSize::XSmall).color(color))
        }))
        .children(session.branch.clone().map(|branch| {
            h_flex()
                .px_1()
                .gap_0p5()
                .rounded_sm()
                .bg(colors.element_background)
                .child(Icon::new(IconName::GitBranch).size(IconSize::XSmall))
                .child(Label::new(branch).size(LabelSize::XSmall))
        }))
}

/// The expanded half of a row: everything a person needs to identify a session
/// precisely, rather than approximately.
fn render_details(
    session: &SessionSummary,
    _cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let mut lines = vec![format!("Session {}", session.id)];
    lines.push(format!("Directory {}", session.cwd.display()));
    if let Some(log) = &session.log_path {
        lines.push(format!(
            "Log {} ({})",
            log.display(),
            crate::session_history::actions::format_bytes(session.log_bytes)
        ));
    }
    v_flex().w_full().gap_0p5().children(
        lines
            .into_iter()
            .map(|line| Label::new(line).size(LabelSize::XSmall).color(Color::Muted)),
    )
}
