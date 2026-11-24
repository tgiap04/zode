use std::time::Duration;

use gpui::{AppContext as _, TestAppContext, UpdateGlobal as _};
use reminders::{ReminderKind, Reminders, RemindersSettings};
use settings::{Settings as _, SettingsStore};

use crate::status_item::countdown;
use crate::{RemindersStatusItem, notification_id};

fn init(cx: &mut TestAppContext) -> gpui::Entity<Reminders> {
    cx.set_notifications_supported(true);
    cx.set_system_idle_time(Some(Duration::ZERO));
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        RemindersSettings::register(cx);
        reminders::init(cx);
        crate::init(cx);
        Reminders::global(cx).unwrap()
    })
}

fn enable_water_every_minute(cx: &mut TestAppContext) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                let reminders = content.reminders.get_or_insert_default();
                reminders.enabled = Some(true);
                reminders.water.get_or_insert_default().interval_minutes = Some(1);
                reminders.eye_break.get_or_insert_default().enabled = Some(false);
                reminders.stand_up.get_or_insert_default().enabled = Some(false);
            });
        });
    });
    cx.update(|cx| {
        Reminders::global(cx).unwrap().update(cx, |reminders, cx| {
            reminders.start_reminder(ReminderKind::Water, cx)
        })
    });
}

fn advance(cx: &mut TestAppContext, total: Duration) {
    let mut elapsed = Duration::ZERO;
    while elapsed < total {
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        elapsed += Duration::from_secs(1);
    }
}

#[gpui::test]
async fn a_due_reminder_posts_one_notification_per_kind_id(cx: &mut TestAppContext) {
    init(cx);
    enable_water_every_minute(cx);

    advance(cx, Duration::from_secs(60));
    let posted = cx.posted_notifications();
    assert_eq!(posted.len(), 1);
    assert_eq!(posted[0].id.as_ref(), "reminders.water");
    assert_eq!(posted[0].title.as_ref(), "Time for some water");

    advance(cx, Duration::from_secs(60));
    let posted = cx.posted_notifications();
    assert_eq!(posted.len(), 2);
    assert_eq!(
        posted[1].id, posted[0].id,
        "the same id, so the OS replaces the banner instead of stacking it"
    );
}

#[gpui::test]
async fn the_status_item_counts_the_focus_timer_down(cx: &mut TestAppContext) {
    let reminders = init(cx);
    enable_water_every_minute(cx);
    let item = cx.new(RemindersStatusItem::new);
    assert_eq!(item.read_with(cx, |item, cx| item.compute_label(cx)), None);

    reminders.update(cx, |reminders, cx| reminders.start_focus(cx));
    advance(cx, Duration::from_secs(61));
    assert_eq!(
        item.read_with(cx, |item, cx| item.compute_label(cx))
            .map(|label| label.to_string()),
        Some("Focus 23:59".to_string())
    );
}

#[test]
fn countdown_formats_minutes_and_hours() {
    assert_eq!(countdown(Duration::from_secs(5 * 60 + 7)), "05:07");
    assert_eq!(countdown(Duration::from_secs(3600 + 61)), "1:01:01");
    assert_eq!(countdown(Duration::ZERO), "00:00");
}

#[test]
fn every_due_event_has_its_own_notification_id() {
    let ids: Vec<_> = ReminderKind::ALL
        .into_iter()
        .map(|kind| notification_id(reminders::ReminderEvent::Due(kind)).unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "reminders.water",
            "reminders.eye_break",
            "reminders.stand_up"
        ]
    );
}

mod tray {
    use gpui::{
        Bounds, TestAppContext, TrayEdge, TrayIconAnchor, UpdateGlobal as _, point, px, size,
    };
    use settings::{Settings as _, SettingsStore};

    use crate::panel::step_minutes;
    use crate::tray::{PANEL_SIZE, TrayIconSetting};
    use crate::{panel_bounds, panel_is_open};

    fn anchor(x: f32, edge: TrayEdge) -> TrayIconAnchor {
        TrayIconAnchor {
            bounds: Bounds::new(point(px(x), px(0.)), size(px(30.), px(24.))),
            display_id: None,
            edge,
        }
    }

    #[test]
    fn the_panel_opens_under_a_top_tray_centered_on_the_icon() {
        let display = Bounds::new(point(px(0.), px(0.)), size(px(1512.), px(982.)));
        let bounds = panel_bounds(&anchor(700., TrayEdge::Top), PANEL_SIZE, display);
        assert_eq!(bounds.origin, point(px(715. - 180.), px(30.)));
        assert_eq!(bounds.size, PANEL_SIZE);
    }

    #[test]
    fn the_panel_stays_inside_the_display() {
        let display = Bounds::new(point(px(0.), px(0.)), size(px(1512.), px(982.)));
        let right = panel_bounds(&anchor(1490., TrayEdge::Top), PANEL_SIZE, display);
        assert_eq!(right.origin.x, px(1512. - 360. - 6.));
        let left = panel_bounds(&anchor(0., TrayEdge::Top), PANEL_SIZE, display);
        assert_eq!(left.origin.x, px(6.));
    }

    #[test]
    fn a_bottom_tray_opens_the_panel_above_the_icon() {
        let display = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
        let icon = TrayIconAnchor {
            bounds: Bounds::new(point(px(1800.), px(1040.)), size(px(24.), px(24.))),
            display_id: None,
            edge: TrayEdge::Bottom,
        };
        let bounds = panel_bounds(&icon, PANEL_SIZE, display);
        assert_eq!(bounds.origin.y, px(1040. - 560. - 6.));
    }

    #[test]
    fn a_display_that_does_not_start_at_the_origin_keeps_the_panel_inside_it() {
        // A second monitor to the left of the primary one, in global coordinates.
        let display = Bounds::new(point(px(-1920.), px(0.)), size(px(1920.), px(1080.)));
        let icon = TrayIconAnchor {
            bounds: Bounds::new(point(px(-30.), px(1050.)), size(px(24.), px(24.))),
            display_id: None,
            edge: TrayEdge::Bottom,
        };
        let bounds = panel_bounds(&icon, PANEL_SIZE, display);
        assert_eq!(bounds.origin.x, px(-360. - 6.));
        assert!(bounds.origin.x >= display.origin.x);
    }

    #[test]
    fn steppers_move_by_five_minutes_and_stay_in_range() {
        assert_eq!(step_minutes(45, true), 50);
        assert_eq!(step_minutes(45, false), 40);
        assert_eq!(step_minutes(47, false), 45);
        assert_eq!(step_minutes(47, true), 50);
        assert_eq!(step_minutes(5, false), 1);
        assert_eq!(step_minutes(1, false), 1);
        assert_eq!(step_minutes(480, true), 480);
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            reminders::RemindersSettings::register(cx);
            TrayIconSetting::register(cx);
            reminders::init(cx);
            crate::init(cx);
        });
    }

    #[gpui::test]
    async fn the_tray_icon_click_toggles_the_panel(cx: &mut TestAppContext) {
        init(cx);
        cx.set_tray_icon_placement(Some(anchor(700., TrayEdge::Top)), true);
        // Only macOS draws the glyph as a template; other trays get the color icon.
        assert!(
            cx.tray_icon()
                .is_some_and(|icon| icon.template == cfg!(target_os = "macos"))
        );

        cx.simulate_tray_click();
        assert!(cx.update(|cx| panel_is_open(cx)));
        assert_eq!(cx.windows().len(), 1);

        cx.simulate_tray_click();
        assert!(!cx.update(|cx| panel_is_open(cx)));
        assert!(cx.windows().is_empty());
    }

    #[gpui::test]
    async fn a_click_outside_closes_the_panel(cx: &mut TestAppContext) {
        init(cx);
        cx.simulate_tray_click();
        assert!(cx.update(|cx| panel_is_open(cx)));

        cx.simulate_outside_mouse_down();
        assert!(!cx.update(|cx| panel_is_open(cx)));
    }

    #[gpui::test]
    async fn turning_the_tray_icon_off_removes_it(cx: &mut TestAppContext) {
        init(cx);
        assert!(cx.tray_icon().is_some());
        cx.update(|cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |content| content.tray_icon = Some(false));
            });
        });
        assert!(cx.tray_icon().is_none());
    }
}

mod bubble {
    use std::time::Duration;

    use gpui::{
        Bounds, TestAppContext, TrayEdge, TrayIconAnchor, UpdateGlobal as _, point, px, size,
    };
    use reminders::{ReminderEvent, ReminderKind, ReminderStyle, Reminders};
    use settings::{Settings as _, SettingsStore};

    use crate::{Delivery, bubble_is_open, delivery, pending, snooze};

    #[test]
    fn where_a_reminder_goes() {
        let (bubble, system) = (ReminderStyle::Bubble, ReminderStyle::System);
        assert_eq!(delivery(system, true, true, true), Delivery::System);
        assert_eq!(delivery(bubble, true, true, true), Delivery::Panel);
        assert_eq!(delivery(bubble, false, true, true), Delivery::Bubble);
        assert_eq!(
            delivery(bubble, false, false, true),
            Delivery::System,
            "icon hidden"
        );
        assert_eq!(
            delivery(bubble, false, true, false),
            Delivery::System,
            "icon not placed"
        );
    }

    pub(super) fn init(cx: &mut TestAppContext, tray_visible: bool) -> gpui::Entity<Reminders> {
        cx.set_notifications_supported(true);
        cx.set_system_idle_time(Some(Duration::ZERO));
        cx.set_tray_icon_placement(
            Some(TrayIconAnchor {
                bounds: Bounds::new(point(px(700.), px(0.)), size(px(30.), px(24.))),
                display_id: None,
                edge: TrayEdge::Top,
            }),
            tray_visible,
        );
        let reminders = cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            reminders::RemindersSettings::register(cx);
            crate::TrayIconSetting::register(cx);
            reminders::init(cx);
            crate::init(cx);
            Reminders::global(cx).unwrap()
        });
        cx.update(|cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |content| {
                    let reminders = content.reminders.get_or_insert_default();
                    reminders.enabled = Some(true);
                    reminders.water.get_or_insert_default().interval_minutes = Some(1);
                    reminders.eye_break.get_or_insert_default().enabled = Some(false);
                    reminders.stand_up.get_or_insert_default().enabled = Some(false);
                });
            });
        });
        reminders.update(cx, |reminders, cx| {
            reminders.start_reminder(ReminderKind::Water, cx)
        });
        reminders
    }

    pub(super) fn advance(cx: &mut TestAppContext, seconds: u64) {
        for _ in 0..seconds {
            cx.executor().advance_clock(Duration::from_secs(1));
            cx.run_until_parked();
        }
    }

    #[gpui::test]
    async fn a_visible_tray_icon_gets_a_bubble_instead_of_a_notification(cx: &mut TestAppContext) {
        init(cx, true);
        advance(cx, 60);
        assert!(cx.update(|cx| bubble_is_open(cx)));
        assert!(cx.posted_notifications().is_empty());
        assert_eq!(
            cx.update(|cx| pending(cx)),
            Some(ReminderEvent::Due(ReminderKind::Water))
        );
    }

    #[gpui::test]
    async fn a_hidden_tray_icon_falls_back_to_a_notification(cx: &mut TestAppContext) {
        init(cx, false);
        advance(cx, 60);
        assert!(!cx.update(|cx| bubble_is_open(cx)));
        assert_eq!(cx.posted_notifications().len(), 1);
    }

    #[gpui::test]
    async fn the_bubble_hides_itself_and_the_reminder_stays_pending(cx: &mut TestAppContext) {
        init(cx, true);
        advance(cx, 60);
        assert!(cx.update(|cx| bubble_is_open(cx)));
        advance(cx, 15);
        assert!(!cx.update(|cx| bubble_is_open(cx)));
        assert!(cx.update(|cx| pending(cx)).is_some());
    }

    #[gpui::test]
    async fn snooze_puts_the_reminder_off_by_the_snooze_length(cx: &mut TestAppContext) {
        let reminders = init(cx, true);
        advance(cx, 60);
        cx.update(|cx| snooze(cx));
        assert!(cx.update(|cx| pending(cx)).is_none());
        assert!(!cx.update(|cx| bubble_is_open(cx)));
        let now = cx.executor().now();
        let due = reminders.read_with(cx, |reminders, _| {
            reminders.schedule().next_due(ReminderKind::Water)
        });
        assert_eq!(due, Some(now + Duration::from_secs(5 * 60)));
    }
}

mod bubble_buttons {
    use gpui::TestAppContext;

    use super::bubble::*;
    use crate::bubble_is_open;

    #[gpui::test]
    async fn done_pressed_inside_the_bubble_closes_it(cx: &mut TestAppContext) {
        init(cx, true);
        advance(cx, 60);
        let handle = cx
            .update(|cx| cx.windows().into_iter().next())
            .expect("the bubble window");
        cx.update(|cx| {
            handle
                .update(cx, |_, window, cx| {
                    crate::bubble::acknowledge_from(window, cx)
                })
                .unwrap()
        });
        cx.run_until_parked();
        assert!(!cx.update(|cx| bubble_is_open(cx)));
        assert!(cx.windows().is_empty(), "the bubble window is gone");
    }
}

mod tray_reclick {
    use gpui::TestAppContext;

    use crate::panel_is_open;

    /// Clicking the tray icon while the panel is open is itself a click outside
    /// the panel (on macOS the menu bar is another process; elsewhere the panel
    /// loses focus), and that arrives first. The icon click right after it must
    /// not reopen what the user meant to close.
    #[gpui::test]
    async fn clicking_the_icon_again_closes_the_panel_even_after_an_outside_click(
        cx: &mut TestAppContext,
    ) {
        super::tray_init(cx);
        cx.simulate_tray_click();
        assert!(cx.update(|cx| panel_is_open(cx)));

        cx.simulate_outside_mouse_down();
        cx.simulate_tray_click();
        assert!(
            !cx.update(|cx| panel_is_open(cx)),
            "the icon click that caused the outside click reopened the panel"
        );
        assert!(cx.windows().is_empty());

        // A later click, well after that close, opens it again.
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(1));
        cx.simulate_tray_click();
        assert!(cx.update(|cx| panel_is_open(cx)));
    }
}

fn tray_init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        theme_settings::init(theme::LoadThemes::JustBase, cx);
        RemindersSettings::register(cx);
        crate::TrayIconSetting::register(cx);
        reminders::init(cx);
        crate::init(cx);
    });
}

mod ring {
    use std::time::Duration;

    use crate::panel::phase_progress;

    #[test]
    fn the_ring_fills_as_the_phase_runs_out() {
        let length = Duration::from_secs(25 * 60);
        assert_eq!(phase_progress(length, length), 0.);
        assert!((phase_progress(length / 2, length) - 0.5).abs() < 1e-6);
        assert_eq!(phase_progress(Duration::ZERO, length), 1.);
        assert_eq!(phase_progress(Duration::from_secs(10), Duration::ZERO), 0.);
        assert_eq!(
            phase_progress(length * 2, length),
            0.,
            "a phase shortened after it started never draws a negative arc"
        );
    }
}

mod user_actions {
    use std::time::Duration;

    use gpui::TestAppContext;

    use crate::pending;

    /// Starting or skipping the focus timer by hand is not news to the user who
    /// just did it: no notice, no notification.
    #[gpui::test]
    async fn starting_the_focus_timer_by_hand_announces_nothing(cx: &mut TestAppContext) {
        let reminders = super::bubble::init(cx, true);
        reminders.update(cx, |reminders, cx| reminders.start_focus(cx));
        cx.run_until_parked();
        reminders.update(cx, |reminders, cx| reminders.skip_focus_phase(cx));
        cx.run_until_parked();
        assert!(cx.update(|cx| pending(cx)).is_none());
        assert!(cx.posted_notifications().is_empty());

        // The phase that ends on its own is still announced.
        for _ in 0..(6 * 60) {
            cx.executor().advance_clock(Duration::from_secs(1));
            cx.run_until_parked();
        }
        assert!(cx.update(|cx| pending(cx)).is_some());
    }
}

mod leaving_the_panel {
    use gpui::{AppContext as _, TestAppContext};

    use crate::panel::leave_panel_and_dispatch;

    gpui::actions!(test_panel, [OpensSomething]);

    /// A footer button closes the panel and runs an action that builds things
    /// (Settings, a new window). Built while the panel's window is still the one
    /// being updated, they were recorded as the panel's, and flushing them after
    /// the panel was gone panicked: "window not found".
    #[gpui::test]
    async fn a_footer_action_that_builds_entities_outlives_the_panel(cx: &mut TestAppContext) {
        super::tray_init(cx);
        // Something with an `observe_new` observer, as a workspace or an editor
        // has: only those make the flush look for the window it was built in.
        struct Built;
        cx.update(|cx| {
            cx.observe_new(|_: &mut Built, _, _| {}).detach();
            cx.on_action(|_: &OpensSomething, cx| {
                let _built = cx.new(|_| Built);
            });
        });
        cx.simulate_tray_click();
        let panel = cx
            .update(|cx| cx.windows().into_iter().next())
            .expect("the panel window");

        // As on macOS when the panel is used from the menu bar while another
        // application is in front: no Zode window is active, so the action runs
        // at once rather than through a window.
        gpui::VisualTestContext::from_window(panel, cx).deactivate_window();
        assert!(cx.update(|cx| cx.active_window()).is_none());

        cx.update(|cx| {
            panel
                .update(cx, |_, window, cx| {
                    leave_panel_and_dispatch(Box::new(OpensSomething), window, cx)
                })
                .unwrap()
        });
        cx.run_until_parked();
        assert!(cx.windows().is_empty());
    }
}

mod follows_the_app_background {
    use gpui::{TestAppContext, UpdateGlobal as _, WindowBackgroundAppearance};
    use settings::SettingsStore;

    use crate::{panel_is_open, surface_fill, window_background};

    /// The panel takes the app's own window background: the theme's appearance
    /// for its window (an opaque one left transparent for the rounded corners)
    /// and the app's surface opacity for its fill.
    #[gpui::test]
    async fn the_panel_follows_the_window_opacity_and_vibrancy_settings(cx: &mut TestAppContext) {
        super::tray_init(cx);
        assert_eq!(
            cx.update(|cx| window_background(cx)),
            WindowBackgroundAppearance::Transparent,
            "an opaque app background keeps the panel's corners rounded"
        );
        assert_eq!(cx.update(|cx| surface_fill(cx)).a, 1.);

        cx.simulate_tray_click();
        assert!(cx.update(|cx| panel_is_open(cx)));
        cx.update(|cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |content| {
                    content.theme.window_opacity = Some(settings::WindowOpacity(0.5));
                    content.theme.window_vibrancy = Some(true);
                });
            });
        });
        cx.run_until_parked();

        assert_eq!(
            cx.update(|cx| window_background(cx)),
            WindowBackgroundAppearance::Vibrancy
        );
        // Like the editor's windows, the panel keeps solid surfaces and fades as
        // a whole: a tint per surface would hide the material and the image.
        assert_eq!(cx.update(|cx| surface_fill(cx)).a, 1.);
        let panel = cx.update(|cx| cx.windows().into_iter().next()).unwrap();
        let expected = cx.update(|cx| {
            use settings::Settings as _;
            theme_settings::ThemeSettings::get_global(cx).background_opacity()
        });
        let content_opacity = cx.update(|cx| {
            panel
                .update(cx, |_, window, _| window.content_opacity())
                .unwrap()
        });
        assert_eq!(content_opacity, expected);
        let applied = cx.update(|cx| {
            panel
                .update(cx, |_, window, _| window.background_appearance())
                .unwrap()
        });
        assert_eq!(
            applied,
            WindowBackgroundAppearance::Vibrancy,
            "the open panel picks the change up without being reopened"
        );
    }
}

mod queued_reminders {
    use gpui::{TestAppContext, UpdateGlobal as _};
    use reminders::{ReminderEvent, ReminderKind};
    use settings::SettingsStore;

    use super::bubble::{advance, init};
    use crate::{acknowledge, bubble_is_open, pending};

    /// Back from a pause or from being away, several reminders can come due in
    /// the same tick. Each one reaches the user, one after another.
    #[gpui::test]
    async fn reminders_due_together_are_shown_one_after_another(cx: &mut TestAppContext) {
        let reminders = init(cx, true);
        cx.update(|cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |content| {
                    let eye_break = content
                        .reminders
                        .get_or_insert_default()
                        .eye_break
                        .get_or_insert_default();
                    eye_break.enabled = Some(true);
                    eye_break.interval_minutes = Some(1);
                });
            });
        });
        reminders.update(cx, |reminders, cx| {
            reminders.start_reminder(ReminderKind::EyeBreak, cx)
        });

        advance(cx, 60);
        assert_eq!(
            cx.update(|cx| pending(cx)),
            Some(ReminderEvent::Due(ReminderKind::Water))
        );

        cx.update(|cx| acknowledge(cx));
        cx.run_until_parked();
        assert_eq!(
            cx.update(|cx| pending(cx)),
            Some(ReminderEvent::Due(ReminderKind::EyeBreak))
        );
        assert!(
            cx.update(|cx| bubble_is_open(cx)),
            "the next reminder gets its own bubble"
        );

        cx.update(|cx| acknowledge(cx));
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| pending(cx)), None);
        assert!(!cx.update(|cx| bubble_is_open(cx)));
    }
}

mod panel_window {
    use std::time::Duration;

    use gpui::TestAppContext;
    use reminders::ReminderKind;

    use crate::RemindersPanel;
    use crate::panel_is_open;

    /// A window manager's close button removes the panel without going through
    /// Zode's own close; the next reminder must not be sent to a panel that is
    /// gone.
    #[gpui::test]
    async fn a_panel_closed_by_the_window_manager_is_forgotten(cx: &mut TestAppContext) {
        super::tray_init(cx);
        cx.simulate_tray_click();
        assert!(cx.update(|cx| panel_is_open(cx)));

        let window = cx.windows()[0];
        cx.update(|cx| {
            window
                .update(cx, |_, window, _| window.remove_window())
                .unwrap()
        });
        cx.run_until_parked();
        assert!(!cx.update(|cx| panel_is_open(cx)));
    }

    /// The rings move with the seconds while something counts down, and the
    /// panel stays still -- no redraws -- when nothing does.
    #[gpui::test]
    async fn the_open_panel_redraws_each_second_only_while_something_counts_down(
        cx: &mut TestAppContext,
    ) {
        let reminders = super::bubble::init(cx, true);
        cx.simulate_tray_click();
        let window = cx.windows()[0];
        let panel = cx.update(|cx| {
            window
                .downcast::<RemindersPanel>()
                .expect("the panel window")
                .update(cx, |_, _, cx| cx.entity())
                .unwrap()
        });
        let redraws = std::rc::Rc::new(std::cell::Cell::new(0));
        let _subscription = cx.update(|cx| {
            let redraws = redraws.clone();
            cx.observe(&panel, move |_, _| redraws.set(redraws.get() + 1))
        });

        super::bubble::advance(cx, 10);
        assert!(
            redraws.get() >= 9,
            "{} redraws in ten seconds",
            redraws.get()
        );

        reminders.update(cx, |reminders, cx| {
            reminders.stop_reminder(ReminderKind::Water, cx)
        });
        cx.run_until_parked();
        redraws.set(0);
        cx.executor().advance_clock(Duration::from_secs(10));
        cx.run_until_parked();
        assert_eq!(redraws.get(), 0);
    }
}

mod steppers {
    use std::sync::Arc;

    use fs::{FakeFs, Fs};
    use gpui::TestAppContext;
    use reminders::ReminderKind;

    use crate::panel::step_interval;

    /// Two quick presses both count, even when the second comes before the
    /// first has been written and read back.
    #[gpui::test]
    async fn quick_presses_each_move_the_interval(cx: &mut TestAppContext) {
        let fs = FakeFs::new(cx.executor());
        super::tray_init(cx);
        cx.update(|cx| <dyn Fs>::set_global(fs.clone() as Arc<dyn Fs>, cx));

        cx.update(|cx| {
            step_interval(ReminderKind::Water, 45, true, cx);
            step_interval(ReminderKind::Water, 45, true, cx);
        });
        cx.run_until_parked();

        let written = fs.load(paths::settings_file()).await.unwrap();
        assert!(
            written.contains("\"interval_minutes\": 55"),
            "settings file after two presses: {written}"
        );
    }
}
