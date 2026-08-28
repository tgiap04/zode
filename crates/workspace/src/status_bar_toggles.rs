//! The toggle registry and the settings reconciler that adds or removes
//! status-bar items to match the settings in force.
//!
//! The menu (a later phase) never touches the bar directly -- a click writes
//! a setting through `update_settings_file` and returns. `StatusBar::reconcile`
//! is the *only* code path that ever adds or removes a toggleable item, so
//! hand-editing `settings.json` produces exactly the same result as using the
//! menu. Hiding an item means dropping its `Entity`, not hiding it at render
//! time -- that is what actually stops its `Task`s and `Subscription`s from
//! running.
//!
//! `workspace` cannot read the settings behind three of the fifteen items --
//! they live in crates that depend on `workspace`, so reading them here would
//! be a dependency cycle. `StatusBarItemSpec::is_shown`/`set_shown` therefore
//! take the crate-agnostic `&App`/`&mut SettingsContent` rather than a
//! `workspace`-owned settings type; the caller that fills the registry
//! supplies the specific getter and setter per item.

use gpui::{App, Context, Entity, Window};
use settings::{SettingsContent, update_settings_file};
use ui::{ContextMenu, IconPosition};

use crate::status_bar::StatusBar;
pub use crate::status_bar::StatusBarSide;

/// Builds the item and inserts it at `rank`. Boxed because each item's
/// constructor has a different signature and captures different handles --
/// a `WeakEntity<Workspace>` only, never a strong one, or hiding an item
/// would not free what this feature is actually about.
pub type StatusBarItemBuilder =
    Box<dyn Fn(&mut StatusBar, usize, &mut Window, &mut Context<StatusBar>)>;

/// One entry in the toggle registry: everything the reconciler needs to
/// decide whether an item belongs on the bar, plus what the menu needs to
/// draw and write its row.
pub struct StatusBarItemSpec {
    /// Stable key for tests and logs.
    pub id: &'static str,
    /// The menu row.
    pub label: &'static str,
    pub side: StatusBarSide,
    /// Captured at registration time from the rank `add_left_item`/
    /// `add_right_item` returned for this item; never recomputed, so a
    /// rebuilt item returns to its original slot instead of the end of its
    /// group.
    pub rank: usize,
    pub is_shown: fn(&App) -> bool,
    /// Writes the opposite of what `is_shown` returned, to the root
    /// settings struct `update_settings_file`'s closure receives.
    pub set_shown: fn(&mut SettingsContent, bool),
    pub build: StatusBarItemBuilder,
}

impl StatusBar {
    /// Registers a spec so `reconcile` can act on it. Called once per item,
    /// right after that item was first added to the bar, so `spec.rank` is
    /// the rank `add_left_item`/`add_right_item` just returned rather than a
    /// guess at it.
    pub fn register_toggleable_item(&mut self, spec: StatusBarItemSpec) {
        self.specs.push(spec);
    }

    /// Diffs every registered spec's desired visibility against what the bar
    /// currently shows, and takes exactly one of the build-and-insert or
    /// remove branches per spec -- never both, and neither when the two
    /// already agree. That is what makes this safe to call on every settings
    /// change rather than only the ones that matter, and safe to call twice
    /// in a row with nothing changed in between.
    pub fn reconcile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Taken out of `self` for the pass: a spec's `build` closure needs
        // `&mut self`, and calling it while still holding a borrow of
        // `self.specs` (to read that same closure) is exactly the aliasing
        // the borrow checker refuses. Restored below regardless of branch.
        let specs = std::mem::take(&mut self.specs);
        for spec in &specs {
            let wants_shown = (spec.is_shown)(cx);
            let present = self.contains_rank(spec.side, spec.rank);
            match (wants_shown, present) {
                (true, false) => (spec.build)(self, spec.rank, window, cx),
                (false, true) => {
                    self.remove_item_by_rank(spec.side, spec.rank, cx);
                }
                (true, true) | (false, false) => {}
            }
        }
        self.specs = specs;
    }
}

/// A read-only, `Copy` snapshot of one spec's menu-relevant fields --
/// `label`/`side`/`is_shown`/`set_shown` -- without the boxed `build`
/// closure `StatusBarItemSpec` carries. `build` is irrelevant to the menu
/// and not `Clone`, so the whole spec can't be captured into the `'static`
/// closure `build_item_menu` returns; this can.
#[derive(Clone, Copy)]
struct ToggleMenuRow {
    label: &'static str,
    side: StatusBarSide,
    is_shown: fn(&App) -> bool,
    set_shown: fn(&mut SettingsContent, bool),
}

impl From<&StatusBarItemSpec> for ToggleMenuRow {
    fn from(spec: &StatusBarItemSpec) -> Self {
        Self {
            label: spec.label,
            side: spec.side,
            is_shown: spec.is_shown,
            set_shown: spec.set_shown,
        }
    }
}

/// Builds the right-click menu's `.menu()` callback from the specs
/// registered on the bar at render time.
///
/// Returns a closure rather than the `Entity<ContextMenu>` itself: the menu
/// must be built fresh on every open (never cached), and `RightClickMenu`
/// enforces that shape by calling this closure itself at open time rather
/// than accepting a pre-built entity.
pub(crate) fn build_item_menu(
    specs: &[StatusBarItemSpec],
) -> impl Fn(&mut Window, &mut App) -> Entity<ContextMenu> + 'static {
    let rows: Vec<ToggleMenuRow> = specs.iter().map(ToggleMenuRow::from).collect();
    move |window, cx| {
        // The outer closure is `Fn`, callable more than once across
        // separate opens, so it can't move `rows` into the inner `FnOnce`
        // outright -- each open gets its own clone to consume instead.
        let rows = rows.clone();
        ContextMenu::build(window, cx, move |menu, _window, cx| {
            let has_left = rows.iter().any(|row| row.side == StatusBarSide::Left);
            let has_right = rows.iter().any(|row| row.side == StatusBarSide::Right);
            let menu = append_side_rows(menu, "Left", StatusBarSide::Left, &rows, cx);
            let menu = if has_left && has_right {
                menu.separator()
            } else {
                menu
            };
            append_side_rows(menu, "Right", StatusBarSide::Right, &rows, cx)
        })
    }
}

/// Appends `header` plus every row belonging to `side`, or nothing at all
/// if that side is empty -- an empty header with no rows under it would be
/// worse than no header.
///
/// Built with `ContextMenu::toggleable_entry` rather than the lower-level
/// `ContextMenuEntry::new(..).toggleable(..).icon(..)` builder:
/// `toggleable_entry` hardcodes its pushed entry's `icon` field to `None`,
/// so it cannot hit the bug recorded in `agent_usage::status_bar_items`'s
/// doc comment, where that lower-level builder let a row's `.icon(..)`
/// silently replace the very checkmark `.toggleable(..)` had just asked
/// for. A menu whose whole job is to say what is on cannot spend its tick
/// slot on decoration.
fn append_side_rows(
    mut menu: ContextMenu,
    header: &'static str,
    side: StatusBarSide,
    rows: &[ToggleMenuRow],
    cx: &App,
) -> ContextMenu {
    let side_rows: Vec<&ToggleMenuRow> = rows.iter().filter(|row| row.side == side).collect();
    if side_rows.is_empty() {
        return menu;
    }
    menu = menu.header(header);
    for row in side_rows {
        let is_shown = row.is_shown;
        let set_shown = row.set_shown;
        menu = menu.toggleable_entry(
            row.label,
            is_shown(cx),
            IconPosition::Start,
            // No action: the row commits on click alone, not through a
            // keybinding.
            None,
            move |_window, cx| {
                // Re-read rather than invert a value captured when the menu
                // was built: that reading is as old as the menu, and a
                // handler that flipped it would write the setting the user
                // had two states ago.
                let now_shown = is_shown(cx);
                update_settings_file(<dyn fs::Fs>::global(cx), cx, move |content, _| {
                    set_shown(content, !now_shown);
                });
            },
        );
    }
    menu
}

#[cfg(test)]
#[path = "status_bar_toggles_tests.rs"]
mod tests;
