//! Everything the floating window can be asked to hold, and the two menus that
//! offer it.
//!
//! Cut out of `render.rs` once that file passed ~700 lines, per the note it
//! left behind for whoever did it. What stayed there is the window chrome
//! (title bar, grips, drags); what moved here is the one list three surfaces
//! read -- the window's `+`, the tab bar's `+`, and the empty state -- plus the
//! two menus built from it.

use std::sync::LazyLock;

use gpui::{Anchor, WeakEntity};
use ui::prelude::*;
use ui::{ContextMenu, ContextMenuEntry, PopoverMenu, Tooltip};

use crate::host::FloatingPane;

/// Everything the window can be asked to hold.
///
/// One list, read by both the `+` menu and the empty state. Two lists would be
/// two places to add the next entry, and the one used less would be the one that
/// fell behind.
#[derive(Clone, Copy)]
pub(crate) enum Entry {
    Terminal,
    NewNote,
    OpenNote,
    Agent(&'static str, IconName, &'static str),
    Database,
    /// Index into the engine list, plus its icon and label. Mirrors `Entry::Agent`
    /// deliberately: this crate must not learn the words "Docker" or "Kubernetes".
    Engine(usize, IconName, &'static str),
    /// A second, independent `GitPanel` -- see `crate::panel_item::PanelItem`
    /// for what that costs against the one the dock already shows.
    Git,
    /// A second, independent `ProjectPanel`. See `Entry::Git`.
    ProjectFiles,
    /// A second, independent `DebugPanel`. See `Entry::Git`.
    Debugger,
}

/// Built once, not on every render: `render_menu` and `render_empty_state`
/// both call `Entry::all()` on every frame, and `Entry` is `Copy`, so there is
/// nothing to gain from a fresh `Vec` each time.
static ENTRIES: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    let mut entries = vec![Entry::Terminal, Entry::NewNote, Entry::OpenNote];
    entries.extend(
        agent_ui::agent_marks().map(|(agent, icon, label)| Entry::Agent(agent, icon, label)),
    );
    entries.push(Entry::Database);
    entries.extend(
        container_ui::engine_marks().map(|(index, icon, label)| Entry::Engine(index, icon, label)),
    );
    entries.push(Entry::Git);
    entries.push(Entry::ProjectFiles);
    entries.push(Entry::Debugger);
    entries
});

impl Entry {
    pub(crate) fn all() -> &'static [Entry] {
        &ENTRIES
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Entry::Terminal => "New Terminal",
            Entry::NewNote => "New Markdown Note",
            Entry::OpenNote => "Open Markdown Note",
            Entry::Agent(_, _, label) => label,
            Entry::Database => "Database",
            Entry::Engine(_, _, label) => label,
            Entry::Git => "Git Panel",
            Entry::ProjectFiles => "Project Panel",
            Entry::Debugger => "Debug Panel",
        }
    }

    pub(crate) fn icon(self) -> IconName {
        match self {
            Entry::Terminal => IconName::Terminal,
            Entry::NewNote => IconName::Notepad,
            Entry::OpenNote => IconName::FileMarkdown,
            Entry::Agent(_, icon, _) => icon,
            Entry::Database => IconName::Database,
            Entry::Engine(_, icon, _) => icon,
            Entry::Git => IconName::GitBranch,
            Entry::ProjectFiles => IconName::Folder,
            Entry::Debugger => IconName::Debug,
        }
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            Entry::Terminal => "floating-pane-new-terminal",
            Entry::NewNote => "floating-pane-new-note",
            Entry::OpenNote => "floating-pane-open-note",
            Entry::Agent(agent, _, _) => agent,
            Entry::Database => "floating-pane-database",
            // The label ("Docker", "Podman", "Kubernetes") is already `'static`
            // and unique among the three, so it doubles as the element id --
            // the same trick `Entry::Agent` plays with the agent's own id.
            Entry::Engine(_, _, label) => label,
            Entry::Git => "floating-pane-git-panel",
            Entry::ProjectFiles => "floating-pane-project-panel",
            Entry::Debugger => "floating-pane-debug-panel",
        }
    }

    /// Whether a separator belongs above this entry.
    ///
    /// The agents are a different kind of thing from the three above them, and
    /// the first of them is where the list changes subject; `Database` opens a
    /// second such change, into what this window can hold that isn't a tab of
    /// running work. `Git` opens a third: a second, independent copy of a dock
    /// panel, which is its own kind of thing again -- see `Entry::Git`'s doc
    /// comment for what sets it apart from the two groups above it.
    fn opens_a_group(self) -> bool {
        matches!(self, Entry::Agent(agent, _, _) if agent_ui::agent_marks().next().is_some_and(|(first, _, _)| first == agent))
            || matches!(self, Entry::Database)
            || matches!(self, Entry::Git)
    }

    /// The heading drawn under this entry's separator, if it wants one.
    ///
    /// Only the agent group gets a heading. `Database` opens its own group but
    /// also fronts for the engine rows behind it, and "Agent" copied onto that
    /// group would misname three-quarters of it -- so it gets a bare separator
    /// instead of an inherited label that only fits the first row.
    fn group_header(self) -> Option<&'static str> {
        match self {
            Entry::Agent(agent, _, _)
                if agent_ui::agent_marks()
                    .next()
                    .is_some_and(|(first, _, _)| first == agent) =>
            {
                Some("Agent")
            }
            _ => None,
        }
    }

    pub(crate) fn run(
        self,
        pane: &mut FloatingPane,
        window: &mut Window,
        cx: &mut Context<FloatingPane>,
    ) {
        match self {
            Entry::Terminal => pane.open_terminal(window, cx),
            Entry::NewNote => pane.new_markdown_note(window, cx),
            Entry::OpenNote => pane.open_markdown_note(window, cx),
            Entry::Agent(agent, _, _) => pane.open_agent(agent, window, cx),
            Entry::Database => pane.open_database(window, cx),
            Entry::Engine(index, _, _) => pane.open_containers(index, window, cx),
            Entry::Git => pane.open_git_panel(window, cx),
            Entry::ProjectFiles => pane.open_project_panel(window, cx),
            Entry::Debugger => pane.open_debug_panel(window, cx),
        }
    }
}

/// The one list, as menu entries.
///
/// Shared so the window's own `+`, the tab bar's `+` and the empty state can
/// never offer three different sets. That was the whole reason `Entry` exists.
pub(crate) fn entry_items(mut menu: ContextMenu, this: &WeakEntity<FloatingPane>) -> ContextMenu {
    for entry in Entry::all() {
        if entry.opens_a_group() {
            menu = menu.separator();
            if let Some(header) = entry.group_header() {
                menu = menu.header(header);
            }
        }
        let this = this.clone();
        menu = menu.item(
            ContextMenuEntry::new(entry.label())
                .icon(entry.icon())
                .handler(move |window, cx| {
                    this.update(cx, |pane, cx| entry.run(pane, window, cx)).ok();
                }),
        );
    }
    menu
}

/// The `+` at the end of this window's tab bar.
///
/// A pane draws one by default, and the default offers New File, New Terminal
/// and the agents as *workspace* actions -- which resolve against the active
/// pane of the editor. This pane is not one of those, so every entry on that
/// menu opened in the editor behind the window. It only showed once a tab
/// existed, because until then the empty state is what fills the pane.
pub(crate) fn tab_bar_menu(this: WeakEntity<FloatingPane>) -> AnyElement {
    // Wrapped so a test can tell this `+` from the one in the window's title
    // bar: `IconButton` names its debug selector after the icon, and both are
    // a plus.
    h_flex()
        .debug_selector(|| "floating-pane-tab-bar-add".into())
        .child(menu_for(this))
        .into_any_element()
}

fn menu_for(this: WeakEntity<FloatingPane>) -> AnyElement {
    PopoverMenu::new("floating-pane-tab-bar-menu")
        .trigger_with_tooltip(
            IconButton::new("plus", IconName::Plus).icon_size(IconSize::Small),
            Tooltip::text("New\u{2026}"),
        )
        .anchor(Anchor::TopRight)
        .menu(move |window, cx| {
            let this = this.clone();
            Some(ContextMenu::build(window, cx, move |menu, _, _| {
                let menu = entry_items(menu, &this);
                // Not in `Entry`/`entry_items`: that list also feeds the empty
                // state, and splitting a window with nothing in it would
                // produce two empty halves.
                menu.separator()
                    .submenu_with_icon("Split", IconName::Split, move |menu, _, _| {
                        crate::render::split_entries(menu, &this)
                    })
            }))
        })
        .into_any_element()
}
