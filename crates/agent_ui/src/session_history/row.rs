use crate::session_history::panel::{AgentHistoryPanel, CountState};
use agent_sessions::{AgentKind, Fork, SessionSummary};
use gpui::{Anchor, Context, FontWeight, Window};
use std::path::{Path, PathBuf};
use ui::{
    Chip, ContextMenu, ContextMenuEntry, Disclosure, IconName, ListItem, PopoverMenu,
    PopoverMenuHandle, Tab, Tooltip, prelude::*,
};
use util::ResultExt as _;

/// An agent group's header: disclosure, vendor mark, name, and total.
///
/// The glyph comes from `crate::agent_icon` rather than a match of its own — the
/// rail, the tab, the session row and this header must all draw the same mark,
/// and a fourth copy of the mapping is a fourth place to forget.
pub(crate) fn render_agent_group(
    ix: usize,
    agent: AgentKind,
    count: usize,
    collapsed: bool,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let colors = cx.theme().colors();
    // The surface and rule are the header's own; `ListItem` brings the height,
    // the spacing and the click target.
    div()
        .w_full()
        .bg(colors.elevated_surface_background)
        .border_b_1()
        .border_color(colors.border)
        .child(
            ListItem::new(("agent-history-agent", ix))
                .height(Tab::content_height(cx))
                .selectable(false)
                .start_slot(
                    h_flex()
                        .gap_1()
                        .child(Disclosure::new(
                            ("agent-history-agent-toggle", ix),
                            !collapsed,
                        ))
                        .child(
                            Icon::new(crate::agent_icon(agent.builtin_agent_id()))
                                .size(IconSize::Small),
                        ),
                )
                .child(Label::new(agent.label()).weight(FontWeight::MEDIUM))
                .end_slot(
                    Label::new(count.to_string())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
                .on_click(cx.listener(move |this, _, _window, cx| {
                    if !this.collapsed_agents.remove(&agent) {
                        this.collapsed_agents.insert(agent);
                    }
                    cx.notify();
                })),
        )
}

/// The inner header: one project, under the agent that ran there.
pub(crate) fn render_group(
    ix: usize,
    agent: AgentKind,
    path: &Path,
    label: &str,
    count: usize,
    collapsed: bool,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let colors = cx.theme().colors();
    let path = path.to_path_buf();
    div()
        .w_full()
        .bg(colors.elevated_surface_background)
        .border_b_1()
        .border_color(colors.border)
        .child(
            ListItem::new(("agent-history-group", ix))
                .height(Tab::content_height(cx))
                .selectable(false)
                // Indented past the agent header it belongs to.
                .indent_level(1)
                .start_slot(Disclosure::new(
                    ("agent-history-group-toggle", ix),
                    !collapsed,
                ))
                .child(Label::new(label.to_string()))
                .end_slot(
                    Label::new(count.to_string())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
                .on_click(cx.listener(move |this, _, _window, cx| {
                    // Keyed by (agent, path): closing a project under one agent
                    // must not close the same project under another.
                    let key = (agent, path.clone());
                    if !this.collapsed_groups.remove(&key) {
                        this.collapsed_groups.insert(key);
                    }
                    cx.notify();
                })),
        )
}

/// One session: what it was, who ran it, and what can be done with it.
pub(crate) fn render_session(
    ix: usize,
    session: &SessionSummary,
    open: bool,
    panel: &mut AgentHistoryPanel,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let colors = cx.theme().colors();
    let expanded = panel.expanded_rows.contains(&session.id);
    let alive = panel.cwd_alive(&session.cwd);
    let can_fork = panel.can_fork(session);
    let roots = panel.project_roots(cx);
    let menu_handle = panel.menu_handle(&session.id);
    let title = crate::SessionNames::try_global(cx)
        .and_then(|names| names.read(cx).name(&session.id).cloned())
        .unwrap_or_else(|| session.title.clone().into());

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
        .relative()
        .hover(|style| style.bg(colors.element_hover))
        .child(
            h_flex()
                .w_full()
                .gap_1p5()
                .child(
                    // Through `agent_icon` rather than a match of its own: the
                    // rail, the tab and this row must draw the same vendor
                    // mark, and three copies of the mapping is three places to
                    // forget when an agent is added.
                    leading_slot().child(
                        Icon::new(crate::agent_icon(session.agent.builtin_agent_id()))
                            .size(IconSize::Small),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .child(Label::new(title).single_line().color(Color::Default)),
                )
                .child(render_controls(
                    ix,
                    session,
                    open,
                    alive,
                    can_fork,
                    expanded,
                    menu_handle,
                    cx,
                )),
        )
        .child(
            // Everything under the title lines up with it, not with the mark.
            h_flex().w_full().gap_1p5().child(leading_slot()).child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(render_preview(session, expanded))
                    .child(render_facts(session, panel))
                    .child(render_badges(session, &roots, alive, open, cx))
                    .children(expanded.then(|| render_details(session, cx))),
            ),
        )
        // Drawn over the row's padding, so it takes no width from the content and
        // an open row lines up with the rest.
        .children(open.then(|| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(px(2.))
                .bg(Color::Accent.color(cx))
        }))
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

/// The fixed-width column that holds the vendor mark, and that the lines under
/// the title leave empty so they start where the title does.
fn leading_slot() -> Div {
    h_flex().size_4().flex_none().justify_center()
}

fn row_group(ix: usize) -> SharedString {
    SharedString::from(format!("agent-history-row-{ix}"))
}

/// The hover toolbar, the menu behind the ellipsis, and the disclosure that
/// stays visible to say the row opens.
fn render_controls(
    ix: usize,
    session: &SessionSummary,
    open: bool,
    alive: bool,
    can_fork: bool,
    expanded: bool,
    menu_handle: PopoverMenuHandle<ContextMenu>,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    let group = row_group(ix);
    h_flex()
        .flex_none()
        .gap_0p5()
        .children(open.then(|| {
            // Wrapped only so a test can find the button's bounds and click it.
            div()
                .debug_selector(move || format!("agent-history-go-to-tab:{ix}"))
                .child(
                    IconButton::new(("go-to-tab", ix), IconName::ArrowUpRight)
                        .icon_size(IconSize::Small)
                        .visible_on_hover(group.clone())
                        .tooltip(Tooltip::text("Go to Tab"))
                        .on_click(cx.listener({
                            let agent = project::AgentId::new(session.agent.builtin_agent_id());
                            let id = session.id.clone();
                            move |_, _, window, cx| {
                                let agent = agent.clone();
                                let id = id.clone();
                                // Deferred: this handler runs with the panel leased,
                                // and bringing a workspace forward reads the panel
                                // back through the dock that holds it.
                                window.defer(cx, move |window, cx| {
                                    // The tab may have closed since the row was drawn.
                                    if let Some(view) = crate::agent_tabs::open_tab(&agent, &id, cx)
                                    {
                                        crate::agent_tabs::go_to_tab(&view, window, cx);
                                    }
                                });
                            }
                        })),
                )
        }))
        .child(
            IconButton::new(("resume", ix), IconName::PlayFilled)
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
                .visible_on_hover(group.clone())
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
            // Wrapped only so a test can find the trigger's bounds and click it;
            // `IconButton` has no probe of its own.
            div()
                .debug_selector(move || format!("agent-history-menu:{ix}"))
                .child(render_menu(
                    ix,
                    session,
                    alive,
                    can_fork,
                    menu_handle,
                    group,
                    cx,
                )),
        )
        // A click on this reaches the row's own handler, which is the toggle: the
        // disclosure only has to be there to show the state, and to be a target.
        .child(Disclosure::new(("agent-history-expand", ix), expanded))
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
    menu_handle: PopoverMenuHandle<ContextMenu>,
    group: SharedString,
    cx: &mut Context<AgentHistoryPanel>,
) -> impl IntoElement {
    // Weak: the handle that stores this menu's builder is a field of the panel,
    // so a strong handle captured here would have the panel own itself.
    let panel = cx.weak_entity();
    let session = session.clone();
    // Hovering the row is what reveals the trigger, and opening the menu moves
    // the pointer off the row. Without the exception the trigger would vanish
    // out from under the menu it just opened.
    let menu_open = menu_handle.is_deployed();
    PopoverMenu::new(("agent-history-menu", ix))
        .with_handle(menu_handle)
        .menu(move |window, cx| {
            let panel = panel.clone();
            let session = session.clone();
            Some(ContextMenu::build(
                window,
                cx,
                move |mut menu, _window, _cx| {
                    let has_log = session.log_path.is_some();
                    // Every entry runs against the panel entity, deferred past this
                    // click by `WeakEntity::update` — the panel is not leased here,
                    // but the handler runs later and must not assume it still is,
                    // or that it still exists.
                    let add = |menu: ContextMenu,
                               label: &'static str,
                               icon: IconName,
                               enabled: bool,
                               action: fn(
                        &mut AgentHistoryPanel,
                        &SessionSummary,
                        &mut Window,
                        &mut Context<AgentHistoryPanel>,
                    )| {
                        let panel = panel.clone();
                        let session = session.clone();
                        menu.item(
                            ContextMenuEntry::new(label)
                                .icon(icon)
                                .disabled(!enabled)
                                .handler(move |window, cx| {
                                    panel
                                        .update(cx, |panel, cx| action(panel, &session, window, cx))
                                        .log_err();
                                }),
                        )
                    };

                    menu = add(
                        menu,
                        "Resume in Worktree",
                        IconName::PlayFilled,
                        alive,
                        |panel, session, window, cx| {
                            panel.resume(session, Fork::Continue, window, cx)
                        },
                    );
                    menu = add(
                        menu,
                        "Continue in New Session…",
                        IconName::Plus,
                        alive && can_fork,
                        |panel, session, window, cx| panel.resume(session, Fork::New, window, cx),
                    );
                    menu = add(
                        menu,
                        "Copy Resume Command",
                        IconName::Copy,
                        true,
                        |panel, session, _window, cx| panel.copy_resume_command(session, cx),
                    );
                    menu = menu.separator();
                    menu = add(
                        menu,
                        "Open Log",
                        IconName::FileCode,
                        has_log,
                        |panel, session, window, cx| panel.open_log(session, window, cx),
                    );
                    menu = add(
                        menu,
                        "Reveal Log",
                        IconName::Folder,
                        has_log,
                        |panel, session, _window, cx| panel.reveal_log(session, cx),
                    );
                    menu = add(
                        menu,
                        "Open Working Directory",
                        IconName::Folder,
                        alive,
                        |panel, session, _window, cx| panel.open_working_directory(session, cx),
                    );
                    menu = menu.separator();
                    menu = add(
                        menu,
                        "Copy Session ID",
                        IconName::Copy,
                        true,
                        |panel, session, _window, cx| panel.copy(session.id.to_string(), cx),
                    );
                    menu = add(
                        menu,
                        "Copy Log Path",
                        IconName::Copy,
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
                        IconName::Trash,
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
                .when(!menu_open, |button| button.visible_on_hover(group))
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

/// Agent, message counts, when, and which model — the row of secondary facts.
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
            Label::new(session.agent.label())
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .children(messages.map(|messages| {
            Label::new(messages)
                .size(LabelSize::Small)
                .color(Color::Muted)
        }))
        .children(subagents.map(|subagents| {
            Label::new(subagents)
                .size(LabelSize::Small)
                .color(Color::Muted)
        }))
        .child(Label::new(when).size(LabelSize::Small).color(Color::Muted))
        .children(
            session
                .model
                .clone()
                .map(|model| Label::new(model).size(LabelSize::Small).color(Color::Muted)),
        )
        .child(div().flex_1())
}

/// Which worktree this session belongs to, and which branch it was on.
fn render_badges(
    session: &SessionSummary,
    roots: &[PathBuf],
    alive: bool,
    open: bool,
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
        // The worktree badge is outlined with no fill and the branch badge is
        // filled with no rule, so each overrides one of the chip's defaults. The
        // chip also sets the label in the buffer font.
        .children(open.then(|| {
            Chip::new("Open")
                .label_color(Color::Accent)
                .bg_color(gpui::transparent_black())
                .border_color(Color::Accent.color(cx))
        }))
        .children(worktree.map(|(label, color)| {
            Chip::new(label)
                .label_color(color)
                .bg_color(gpui::transparent_black())
        }))
        .children(session.branch.clone().map(|branch| {
            Chip::new(branch)
                .truncate()
                .icon(IconName::GitBranch)
                .bg_color(colors.element_background)
                .border_color(gpui::transparent_black())
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
