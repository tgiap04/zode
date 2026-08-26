//! Tests for the free functions `footprint_popover.rs` carries the poll
//! loop's decisions in -- gating, cadence, and change-detection -- none of
//! which need a window or a real entity to exercise. What is left uncovered
//! (the loop itself, `Render`, activation gating) is named in the phase's
//! Success Criteria and needs a real `Workspace`/`MultiWorkspace`, which this
//! crate does not depend on.

use gpui::{AppContext as _, EntityId, TestAppContext};

use std::collections::HashMap;

use crate::footprint_popover::{
    footprints_render_the_same, is_discovery_tick, merge_known_pids, render_project_line,
    wants_polling,
};
use crate::{FootprintRoots, Footprints, ProjectFootprint};

/// A stand-in identity for a tracked project. Only its `EntityId` matters to
/// these functions, the same convention `keep_awake_tests`' `tab` helper
/// uses.
fn project_id(cx: &mut TestAppContext) -> EntityId {
    cx.update(|cx| cx.new(|_| ()).entity_id())
}

/// Polling runs only with the setting on *and* the window active -- every
/// other combination stops it, matching the idle policy in the phase docs.
#[test]
fn polling_requires_the_setting_and_an_active_window() {
    assert!(wants_polling(true, true));
    assert!(!wants_polling(false, true));
    assert!(!wants_polling(true, false));
    assert!(!wants_polling(false, false));
}

/// Every 10th tick, starting at 0, is a discovery pass; every other tick is
/// the narrow refresh.
#[test]
fn every_tenth_tick_is_a_discovery_tick() {
    assert!(
        is_discovery_tick(0),
        "the loop always starts with discovery"
    );
    for tick in 1..=9 {
        assert!(!is_discovery_tick(tick), "tick {tick} is narrow-only");
    }
    assert!(is_discovery_tick(10));
}

/// A remote/guest project, or one with no CPU baseline yet, reports "not
/// measured" -- never "0 B", which would claim something the sampler never
/// saw.
#[test]
fn an_unmeasured_footprint_says_so_rather_than_claiming_zero() {
    let unmeasured = ProjectFootprint::default();
    let rendered = render_project_line(&unmeasured);
    assert!(
        rendered.contains("not measured"),
        "{rendered} should read as unmeasured, not as a zero reading"
    );
    assert!(
        !rendered.contains("0 B"),
        "{rendered} must not claim zero RSS"
    );
}

/// Change-detection compares the *rendered* text, so applying an identical
/// result is reported unchanged (no `cx.notify()`), while a differing RSS is
/// reported changed.
#[gpui::test]
fn change_detection_follows_the_rendered_text(cx: &mut TestAppContext) {
    let key = project_id(cx);
    let label: gpui::SharedString = "demo".into();

    let current = Footprints(vec![(
        key,
        label.clone(),
        ProjectFootprint {
            rss_bytes: Some(10 * 1024 * 1024),
            cpu_percent: Some(12.0),
        },
    )]);
    let identical = current.clone();
    assert!(
        footprints_render_the_same(&current, &identical),
        "an identical result must not be reported as a change"
    );

    let differing_rss = Footprints(vec![(
        key,
        label,
        ProjectFootprint {
            rss_bytes: Some(20 * 1024 * 1024),
            cpu_percent: Some(12.0),
        },
    )]);
    assert!(
        !footprints_render_the_same(&current, &differing_rss),
        "a different rendered RSS must be reported as a change"
    );
}

#[gpui::test]
fn remembered_pids_are_re_offered_as_roots_on_a_narrow_tick(cx: &mut TestAppContext) {
    // The regression guard for the defect where narrow ticks measured root
    // processes only, so the badge shed every agent's descendant tree for 27 of
    // every 30 seconds and jumped back for 3.
    let key = project_id(cx);
    let mut roots = vec![FootprintRoots {
        key,
        label: "project".into(),
        roots: vec![10],
    }];
    let known = HashMap::from([(key, vec![10, 11, 12])]);

    merge_known_pids(&mut roots, &known);

    let mut offered = roots[0].roots.clone();
    offered.sort();
    offered.dedup();
    assert_eq!(
        offered,
        vec![10, 11, 12],
        "the descendants found by the last discovery pass must still be offered"
    );
}

#[gpui::test]
fn a_project_with_nothing_remembered_keeps_its_own_roots(cx: &mut TestAppContext) {
    let key = project_id(cx);
    let mut roots = vec![FootprintRoots {
        key,
        label: "project".into(),
        roots: vec![7],
    }];

    merge_known_pids(&mut roots, &HashMap::new());

    assert_eq!(
        roots[0].roots,
        vec![7],
        "a project with no remembered PIDs must be left exactly as collected"
    );
}
