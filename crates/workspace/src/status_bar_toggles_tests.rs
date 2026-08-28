use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use fs::FakeFs;
use gpui::{
    App, AppContext as _, Empty, IntoElement, Render, TestAppContext, UpdateGlobal as _, Window,
};
use project::Project;
use settings::{Settings as _, SettingsStore, update_settings_file};

use super::*;
use crate::{StatusBarSettings, Workspace};

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let settings_store = settings::SettingsStore::test(cx);
        cx.set_global(settings_store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
    });
}

/// A minimal `StatusItemView`: nothing to render, nothing to react to an
/// active-pane change. Standing in for a real item so these tests exercise
/// only the registry/reconciler mechanism, not any concrete item's logic.
struct TestStatusItem;

impl Render for TestStatusItem {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl crate::StatusItemView for TestStatusItem {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn crate::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

fn always_shown(_: &App) -> bool {
    true
}

fn never_shown(_: &App) -> bool {
    false
}

fn build_test_status_item(
    status_bar: &mut StatusBar,
    rank: usize,
    window: &mut Window,
    cx: &mut Context<StatusBar>,
) {
    status_bar.insert_item_at_rank(
        StatusBarSide::Left,
        rank,
        cx.new(|_| TestStatusItem),
        window,
        cx,
    );
}

/// Proves the assertion the three `go_to_line` tests depend on: they never
/// run `initialize_workspace`, so they register no specs, and an empty spec
/// vector must never touch either item vector.
#[gpui::test]
async fn reconcile_with_no_specs_touches_nothing(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    let rank = workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.add_left_item(cx.new(|_| TestStatusItem), window, cx)
        })
    });

    // No spec was ever registered on this bar.
    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .status_bar()
            .update(cx, |status_bar, cx| status_bar.reconcile(window, cx));
    });

    workspace.read_with(cx, |workspace, cx| {
        assert!(
            workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "reconcile with no specs must not touch an existing item"
        );
    });
}

/// Covers the build-and-insert branch, and the idempotence success
/// criterion: reconciling twice with unchanged settings must not build the
/// same item a second time.
#[gpui::test]
async fn reconcile_builds_once_and_a_second_call_is_a_no_op(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    // Allocate a rank the normal way (add, then remove), so the spec starts
    // absent -- the shape a setting that is already off at launch produces.
    let rank = workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            let rank = status_bar.add_left_item(cx.new(|_| TestStatusItem), window, cx);
            status_bar.remove_item_by_rank(StatusBarSide::Left, rank, cx);
            rank
        })
    });

    let build_count = Rc::new(Cell::new(0usize));
    let spec = {
        let build_count = build_count.clone();
        StatusBarItemSpec {
            id: "test-item",
            label: "Test Item",
            side: StatusBarSide::Left,
            rank,
            is_shown: always_shown,
            set_shown: |_, _| {},
            build: Box::new(move |status_bar, rank, window, cx| {
                build_count.set(build_count.get() + 1);
                build_test_status_item(status_bar, rank, window, cx);
            }),
        }
    };

    workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.register_toggleable_item(spec);
            status_bar.reconcile(window, cx);
        });
    });

    assert_eq!(
        build_count.get(),
        1,
        "reconcile must build the absent, wanted item"
    );
    workspace.read_with(cx, |workspace, cx| {
        assert!(
            workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "the built item must be present at its registered rank"
        );
    });

    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .status_bar()
            .update(cx, |status_bar, cx| status_bar.reconcile(window, cx));
    });
    assert_eq!(
        build_count.get(),
        1,
        "reconciling twice with unchanged settings must not build a second time"
    );
}

/// Covers the remove branch: a present item whose setting reads false must
/// be dropped, and must never be built.
#[gpui::test]
async fn reconcile_removes_a_present_item_whose_setting_is_off(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    let rank = workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.add_left_item(cx.new(|_| TestStatusItem), window, cx)
        })
    });

    let build_count = Rc::new(Cell::new(0usize));
    let spec = {
        let build_count = build_count.clone();
        StatusBarItemSpec {
            id: "test-item",
            label: "Test Item",
            side: StatusBarSide::Left,
            rank,
            is_shown: never_shown,
            set_shown: |_, _| {},
            build: Box::new(move |status_bar, rank, window, cx| {
                build_count.set(build_count.get() + 1);
                build_test_status_item(status_bar, rank, window, cx);
            }),
        }
    };

    workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.register_toggleable_item(spec);
            status_bar.reconcile(window, cx);
        });
    });

    assert_eq!(build_count.get(), 0, "a hidden item must never be built");
    workspace.read_with(cx, |workspace, cx| {
        assert!(
            !workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "reconcile must remove a present item whose setting is off"
        );
    });
}

/// Covers re-showing: an item removed by one reconcile pass must come back
/// at the exact same rank on a later pass, not appended to the end.
#[gpui::test]
async fn reconcile_rebuilds_a_hidden_item_at_its_original_rank(cx: &mut TestAppContext) {
    static SHOWN: AtomicBool = AtomicBool::new(true);
    fn toggled(_: &App) -> bool {
        SHOWN.load(Ordering::SeqCst)
    }

    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    let rank = workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.add_left_item(cx.new(|_| TestStatusItem), window, cx)
        })
    });

    let spec = StatusBarItemSpec {
        id: "test-item",
        label: "Test Item",
        side: StatusBarSide::Left,
        rank,
        is_shown: toggled,
        set_shown: |_, _| {},
        build: Box::new(build_test_status_item),
    };
    workspace.update_in(cx, |workspace, _, cx| {
        workspace.status_bar().update(cx, |status_bar, _| {
            status_bar.register_toggleable_item(spec)
        });
    });

    SHOWN.store(false, Ordering::SeqCst);
    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .status_bar()
            .update(cx, |status_bar, cx| status_bar.reconcile(window, cx));
    });
    workspace.read_with(cx, |workspace, cx| {
        assert!(
            !workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "the item must be removed while its setting is off"
        );
    });

    SHOWN.store(true, Ordering::SeqCst);
    workspace.update_in(cx, |workspace, window, cx| {
        workspace
            .status_bar()
            .update(cx, |status_bar, cx| status_bar.reconcile(window, cx));
    });
    workspace.read_with(cx, |workspace, cx| {
        assert!(
            workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "re-showing must rebuild the item back at its original rank"
        );
    });
}

/// Covers the loop over more than one spec: a single `reconcile` call must
/// act on every registered spec independently, not just the first.
#[gpui::test]
async fn reconcile_processes_every_registered_spec_independently(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    let (shown_rank, hidden_rank) = workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            let shown_rank = status_bar.add_left_item(cx.new(|_| TestStatusItem), window, cx);
            let hidden_rank = status_bar.add_left_item(cx.new(|_| TestStatusItem), window, cx);
            (shown_rank, hidden_rank)
        })
    });

    workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.register_toggleable_item(StatusBarItemSpec {
                id: "shown-item",
                label: "Shown Item",
                side: StatusBarSide::Left,
                rank: shown_rank,
                is_shown: always_shown,
                set_shown: |_, _| {},
                build: Box::new(build_test_status_item),
            });
            status_bar.register_toggleable_item(StatusBarItemSpec {
                id: "hidden-item",
                label: "Hidden Item",
                side: StatusBarSide::Left,
                rank: hidden_rank,
                is_shown: never_shown,
                set_shown: |_, _| {},
                build: Box::new(build_test_status_item),
            });
            status_bar.reconcile(window, cx);
        });
    });

    workspace.read_with(cx, |workspace, cx| {
        let status_bar = workspace.status_bar().read(cx);
        assert!(
            status_bar.contains_rank(StatusBarSide::Left, shown_rank),
            "the spec whose setting is on must stay present"
        );
        assert!(
            !status_bar.contains_rank(StatusBarSide::Left, hidden_rank),
            "the spec whose setting is off must be removed"
        );
    });
}

/// The commission's actual promise: a hidden item is **gone**, not merely
/// unrendered, so its `Task`s and `Subscription`s die with it and it stops
/// costing CPU.
///
/// Asserted through a `WeakEntity` rather than by checking the item left the
/// vector, because those two are not the same claim. A removal that leaves the
/// entity alive somewhere — a build closure that captured a strong handle, a
/// registered action holding one, an observer that outlives the bar — still
/// empties the vector and still polls forever. Only the failed upgrade rules
/// that out.
#[gpui::test]
async fn hiding_an_item_drops_its_entity(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs, [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    let (rank, weak_item) = workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            let item = cx.new(|_| TestStatusItem);
            let weak_item = item.downgrade();
            let rank = status_bar.add_left_item(item, window, cx);
            (rank, weak_item)
        })
    });

    // Still held by the bar, so the handle must still upgrade -- otherwise the
    // assertion below would pass for the wrong reason.
    assert!(
        weak_item.upgrade().is_some(),
        "a registered item must be alive before it is hidden"
    );

    let spec = StatusBarItemSpec {
        id: "test-item",
        label: "Test Item",
        side: StatusBarSide::Left,
        rank,
        is_shown: never_shown,
        set_shown: |_, _| {},
        build: Box::new(|status_bar, rank, window, cx| {
            build_test_status_item(status_bar, rank, window, cx);
        }),
    };

    workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.register_toggleable_item(spec);
            status_bar.reconcile(window, cx);
        });
    });

    assert!(
        weak_item.upgrade().is_none(),
        "hiding must drop the entity, not just remove it from the vector -- \
         something is still holding a strong handle to it"
    );
}

// Phase 05 -- the tests below drive the same registry/reconciler through the
// path a real right-click actually takes: write a setting, `run_until_parked`,
// and let the `observe_global_in::<SettingsStore>` subscription in
// `StatusBar::new` call `reconcile` on its own. Every test above this line
// calls `reconcile` directly and proves the reconciler itself is correct;
// these prove the *wiring* to it is -- that nobody has to call `reconcile`
// by hand for a settings change to take effect, which is the actual claim a
// right-click (or a hand-edited `settings.json`) depends on.
//
// They read and write real `StatusBarSettings` fields rather than the
// `AtomicBool`-backed fakes above, because an `is_shown` that isn't backed by
// an actual setting can't react to a real settings write in the first place.

fn cursor_position_button_shown(cx: &App) -> bool {
    StatusBarSettings::get_global(cx).cursor_position_button
}

fn set_cursor_position_button_shown(content: &mut SettingsContent, on: bool) {
    content
        .status_bar
        .get_or_insert_default()
        .cursor_position_button = Some(on);
}

fn active_language_button_shown(cx: &App) -> bool {
    StatusBarSettings::get_global(cx).active_language_button
}

fn set_active_language_button_shown(content: &mut SettingsContent, on: bool) {
    content
        .status_bar
        .get_or_insert_default()
        .active_language_button = Some(on);
}

fn claude_usage_button_shown(cx: &App) -> bool {
    StatusBarSettings::get_global(cx).claude_usage_button
}

fn set_claude_usage_button_shown(content: &mut SettingsContent, on: bool) {
    content
        .status_bar
        .get_or_insert_default()
        .claude_usage_button = Some(on);
}

fn cursor_position_spec(rank: usize) -> StatusBarItemSpec {
    StatusBarItemSpec {
        id: "cursor-position",
        label: "Cursor Position",
        side: StatusBarSide::Left,
        rank,
        is_shown: cursor_position_button_shown,
        set_shown: set_cursor_position_button_shown,
        build: Box::new(build_test_status_item),
    }
}

/// The commission's actual promise, driven the way a right-click (or a
/// hand-edited `settings.json`) really drives it: nobody here ever calls
/// `reconcile` -- only `update_settings_file` and `run_until_parked`.
#[gpui::test]
async fn flipping_a_setting_off_through_settings_drops_the_items_entity(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs.clone(), [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    let (rank, weak_item) = workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            let item = cx.new(|_| TestStatusItem);
            let weak_item = item.downgrade();
            let rank = status_bar.add_left_item(item, window, cx);
            status_bar.register_toggleable_item(cursor_position_spec(rank));
            (rank, weak_item)
        })
    });

    assert!(
        weak_item.upgrade().is_some(),
        "must still be alive before the setting flips"
    );

    cx.update(|_window, cx| {
        update_settings_file(fs, cx, |content, _| {
            content
                .status_bar
                .get_or_insert_default()
                .cursor_position_button = Some(false);
        });
    });
    cx.run_until_parked();

    assert!(
        weak_item.upgrade().is_none(),
        "the settings observer registered in StatusBar::new must reconcile on its own -- \
         nothing in this test ever called reconcile() directly"
    );
    workspace.read_with(cx, |workspace, cx| {
        assert!(
            !workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "the item must also be gone from the rank index, not just deallocated"
        );
    });
}

/// A minimal `StatusItemView` distinguishable from `TestStatusItem` by type,
/// so `position_of_item::<T>()` can name this exact item among several on
/// the same side -- `TestStatusItem` alone can't, since that lookup matches
/// by type and every plain `TestStatusItem` looks identical to it.
struct TestStatusItemB;

impl Render for TestStatusItemB {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl crate::StatusItemView for TestStatusItemB {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn crate::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

/// A third, distinct marker type -- see `TestStatusItemB`'s doc comment.
struct TestStatusItemC;

impl Render for TestStatusItemC {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl crate::StatusItemView for TestStatusItemC {
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn crate::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

/// Covers the reordering claim across more than one item on both sides at
/// once, which `reconcile_rebuilds_a_hidden_item_at_its_original_rank` above
/// does not: three items -- two on the left, one on the right -- hidden
/// together in a single settings write and shown again in a second one, must
/// land back at their original flat positions rather than at the end of
/// whichever side rebuilt them last.
#[gpui::test]
async fn hiding_and_reshowing_three_scattered_items_preserves_order(cx: &mut TestAppContext) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs.clone(), [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            let rank_a = status_bar.add_left_item(cx.new(|_| TestStatusItem), window, cx);
            status_bar.register_toggleable_item(StatusBarItemSpec {
                id: "a",
                label: "A",
                side: StatusBarSide::Left,
                rank: rank_a,
                is_shown: active_language_button_shown,
                set_shown: set_active_language_button_shown,
                build: Box::new(|status_bar, rank, window, cx| {
                    status_bar.insert_item_at_rank(
                        StatusBarSide::Left,
                        rank,
                        cx.new(|_| TestStatusItem),
                        window,
                        cx,
                    );
                }),
            });

            let rank_b = status_bar.add_left_item(cx.new(|_| TestStatusItemB), window, cx);
            status_bar.register_toggleable_item(StatusBarItemSpec {
                id: "b",
                label: "B",
                side: StatusBarSide::Left,
                rank: rank_b,
                is_shown: cursor_position_button_shown,
                set_shown: set_cursor_position_button_shown,
                build: Box::new(|status_bar, rank, window, cx| {
                    status_bar.insert_item_at_rank(
                        StatusBarSide::Left,
                        rank,
                        cx.new(|_| TestStatusItemB),
                        window,
                        cx,
                    );
                }),
            });

            let rank_c = status_bar.add_right_item(cx.new(|_| TestStatusItemC), window, cx);
            status_bar.register_toggleable_item(StatusBarItemSpec {
                id: "c",
                label: "C",
                side: StatusBarSide::Right,
                rank: rank_c,
                is_shown: claude_usage_button_shown,
                set_shown: set_claude_usage_button_shown,
                build: Box::new(|status_bar, rank, window, cx| {
                    status_bar.insert_item_at_rank(
                        StatusBarSide::Right,
                        rank,
                        cx.new(|_| TestStatusItemC),
                        window,
                        cx,
                    );
                }),
            });
        });
    });

    let starting_positions = workspace.read_with(cx, |workspace, cx| {
        let status_bar = workspace.status_bar().read(cx);
        (
            status_bar.position_of_item::<TestStatusItem>(),
            status_bar.position_of_item::<TestStatusItemB>(),
            status_bar.position_of_item::<TestStatusItemC>(),
        )
    });
    // Not pinned to a literal tuple: `Workspace::test_new` calls the same
    // `Workspace::new` production code that seeds the bar with the three
    // dock `PanelButtons` (one left, two right) before this test registers
    // anything, so the absolute flat positions depend on that unrelated
    // detail. What this test actually cares about -- A before B before C,
    // all three present -- does not.
    let (start_a, start_b, start_c) = starting_positions;
    assert!(
        start_a.is_some() && start_b.is_some() && start_c.is_some(),
        "all three must be present to start: {starting_positions:?}"
    );
    assert!(
        start_a < start_b && start_b < start_c,
        "A must precede B (both left, registered in that order), which must precede C \
         (right): {starting_positions:?}"
    );

    cx.update(|_window, cx| {
        update_settings_file(fs.clone(), cx, |content, _| {
            let status_bar = content.status_bar.get_or_insert_default();
            status_bar.active_language_button = Some(false);
            status_bar.cursor_position_button = Some(false);
            status_bar.claude_usage_button = Some(false);
        });
    });
    cx.run_until_parked();

    workspace.read_with(cx, |workspace, cx| {
        let status_bar = workspace.status_bar().read(cx);
        assert!(status_bar.position_of_item::<TestStatusItem>().is_none());
        assert!(status_bar.position_of_item::<TestStatusItemB>().is_none());
        assert!(status_bar.position_of_item::<TestStatusItemC>().is_none());
    });

    cx.update(|_window, cx| {
        update_settings_file(fs, cx, |content, _| {
            let status_bar = content.status_bar.get_or_insert_default();
            status_bar.active_language_button = Some(true);
            status_bar.cursor_position_button = Some(true);
            status_bar.claude_usage_button = Some(true);
        });
    });
    cx.run_until_parked();

    let ending_positions = workspace.read_with(cx, |workspace, cx| {
        let status_bar = workspace.status_bar().read(cx);
        (
            status_bar.position_of_item::<TestStatusItem>(),
            status_bar.position_of_item::<TestStatusItemB>(),
            status_bar.position_of_item::<TestStatusItemC>(),
        )
    });

    assert_eq!(
        ending_positions, starting_positions,
        "hiding all three together and showing them again must return each to its original \
         flat position, not the end of whichever side rebuilt last"
    );
}

/// The claim the module doc comment makes: the settings observer is the
/// *only* path that adds or removes an item, so it must not matter whether a
/// setting changed via `update_settings_file` (what the menu's click handler
/// in `status_bar_toggles::append_side_rows` calls) or via a direct write to
/// the `SettingsStore` (the shape a hand-edited `settings.json` takes once
/// `SettingsStore` has parsed it, without the file round-trip). Both must
/// reconcile to the identical bar state.
#[gpui::test]
async fn writing_settings_directly_or_via_update_settings_file_reconcile_identically(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs.clone(), [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    let rank = workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            let rank = status_bar.add_left_item(cx.new(|_| TestStatusItem), window, cx);
            status_bar.register_toggleable_item(cursor_position_spec(rank));
            rank
        })
    });

    // The "menu" path.
    cx.update(|_window, cx| {
        update_settings_file(fs, cx, |content, _| {
            content
                .status_bar
                .get_or_insert_default()
                .cursor_position_button = Some(false);
        });
    });
    cx.run_until_parked();
    workspace.read_with(cx, |workspace, cx| {
        assert!(
            !workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "the update_settings_file path must remove the item"
        );
    });

    // The "hand-edited settings.json" path: a direct write to the
    // `SettingsStore`, bypassing `update_settings_file`'s fs round-trip.
    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                content
                    .status_bar
                    .get_or_insert_default()
                    .cursor_position_button = Some(true);
            });
        });
    });
    cx.run_until_parked();
    workspace.read_with(cx, |workspace, cx| {
        assert!(
            workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "a direct settings write must rebuild the item exactly like update_settings_file did"
        );
    });

    cx.update(|_window, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                content
                    .status_bar
                    .get_or_insert_default()
                    .cursor_position_button = Some(false);
            });
        });
    });
    cx.run_until_parked();
    workspace.read_with(cx, |workspace, cx| {
        assert!(
            !workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "the direct-write path alone must reach the identical removed outcome the \
             update_settings_file path reached above"
        );
    });
}

/// The guarantee that keeps `go_to_line`'s three
/// `.expect("missing cursor position item")` tests from turning into panics:
/// an item added directly (the shape `go_to_line` uses today, outside this
/// registry) must be untouched by a settings change when no spec references
/// it -- the reconciler only ever acts on the ranks a spec named, and here
/// none are registered at all.
#[gpui::test]
async fn flipping_cursor_position_button_with_no_specs_registered_leaves_the_item_in_place(
    cx: &mut TestAppContext,
) {
    init_test(cx);
    let fs = FakeFs::new(cx.executor());
    let project = Project::test(fs.clone(), [], cx).await;
    let (workspace, cx) = cx.add_window_view(|window, cx| Workspace::test_new(project, window, cx));

    let rank = workspace.update_in(cx, |workspace, window, cx| {
        workspace.status_bar().update(cx, |status_bar, cx| {
            status_bar.add_left_item(cx.new(|_| TestStatusItem), window, cx)
        })
    });

    // No spec is ever registered on this bar -- the empty-vector case.
    cx.update(|_window, cx| {
        update_settings_file(fs, cx, |content, _| {
            content
                .status_bar
                .get_or_insert_default()
                .cursor_position_button = Some(false);
        });
    });
    cx.run_until_parked();

    workspace.read_with(cx, |workspace, cx| {
        assert!(
            workspace
                .status_bar()
                .read(cx)
                .contains_rank(StatusBarSide::Left, rank),
            "an empty spec vector must not touch an item that no spec names, even when the \
             setting it would have read changes"
        );
    });
}
