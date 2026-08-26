//! Tests for the aggregation, attribution, and formatting rules in this
//! crate. In their own file the way `keep_awake_tests` is: the rules here are
//! worth reading on their own, and everything is exercised through a scripted
//! [`FakeSampler`] with no OS access and no GPUI window except where the
//! setting itself is under test.

use gpui::{AppContext as _, TestAppContext, UpdateGlobal as _};
use settings::SettingsStore;

use super::*;

/// A stand-in process table: `(pid, parent, rss, cpu)`. `parent == 0` marks a
/// root with no tracked parent of its own.
#[derive(Default)]
struct FakeSampler {
    processes: Vec<(Pid, Pid, u64, Option<f32>)>,
    core_count: usize,
    descendants_calls: usize,
}

impl ProcessSampler for FakeSampler {
    fn descendants(&mut self, roots: &[Pid]) -> Vec<(Pid, Pid)> {
        self.descendants_calls += 1;
        let mut found = Vec::new();
        for &root in roots {
            let mut frontier = vec![root];
            while let Some(current) = frontier.pop() {
                for &(pid, parent, _, _) in &self.processes {
                    if parent == current {
                        found.push((pid, root));
                        frontier.push(pid);
                    }
                }
            }
        }
        found
    }

    fn sample(&mut self, pids: &[Pid]) -> Vec<(Pid, u64, Option<f32>)> {
        pids.iter()
            .filter_map(|&pid| {
                self.processes
                    .iter()
                    .find(|&&(candidate, ..)| candidate == pid)
                    .map(|&(pid, _, rss, cpu)| (pid, rss, cpu))
            })
            .collect()
    }

    fn core_count(&self) -> usize {
        self.core_count.max(1)
    }
}

fn roots(key: EntityId, roots: &[Pid]) -> FootprintRoots {
    FootprintRoots {
        key,
        label: "project".into(),
        roots: roots.to_vec(),
    }
}

#[gpui::test]
fn disjoint_trees_sum_only_their_own_processes(cx: &mut TestAppContext) {
    let a = cx.update(|cx| cx.new(|_| ()).entity_id());
    let b = cx.update(|cx| cx.new(|_| ()).entity_id());
    let mut sampler = FakeSampler {
        processes: vec![
            (1, 0, 100, None),
            (2, 1, 200, None), // child of root 1
            (10, 0, 300, None),
            (11, 10, 400, None), // child of root 10
        ],
        core_count: 1,
        ..Default::default()
    };
    let project_roots = [roots(a, &[1]), roots(b, &[10])];

    let result = collect(&mut sampler, &project_roots, true);

    let footprint_a = result.iter().find(|(key, _, _)| *key == a).unwrap().1;
    let footprint_b = result.iter().find(|(key, _, _)| *key == b).unwrap().1;
    assert_eq!(footprint_a.rss_bytes, Some(300));
    assert_eq!(footprint_b.rss_bytes, Some(700));
}

#[gpui::test]
fn a_pid_reachable_from_two_roots_is_counted_once(cx: &mut TestAppContext) {
    let a = cx.update(|cx| cx.new(|_| ()).entity_id());
    let b = cx.update(|cx| cx.new(|_| ()).entity_id());
    let mut sampler = FakeSampler {
        // pid 5 is reachable both as a child of root 1 and (after a
        // re-parent to a shell that is itself tracked) as a child of root 2.
        processes: vec![
            (1, 0, 100, None),
            (2, 0, 150, None),
            (5, 1, 50, None),
            (5, 2, 50, None),
        ],
        core_count: 1,
        ..Default::default()
    };
    let project_roots = [roots(a, &[1]), roots(b, &[2])];

    let result = collect(&mut sampler, &project_roots, true);
    let footprints = Footprints(
        result
            .into_iter()
            .map(|(key, footprint, _)| (key, "project".into(), footprint))
            .collect(),
    );

    let total_rss = footprints.combined().rss_bytes.unwrap();
    assert_eq!(
        total_rss, 300,
        "pid 5 must be counted once, not once per claiming root"
    );
}

#[gpui::test]
fn cpu_is_normalized_by_core_count(cx: &mut TestAppContext) {
    let a = cx.update(|cx| cx.new(|_| ()).entity_id());
    let mut sampler = FakeSampler {
        processes: vec![(1, 0, 100, Some(340.0))],
        core_count: 10,
        ..Default::default()
    };
    let project_roots = [roots(a, &[1])];

    let result = collect(&mut sampler, &project_roots, true);

    assert_eq!(result[0].1.cpu_percent, Some(34.0));
}

#[gpui::test]
fn cpu_over_100_percent_after_normalization_is_clamped(cx: &mut TestAppContext) {
    let a = cx.update(|cx| cx.new(|_| ()).entity_id());
    let mut sampler = FakeSampler {
        processes: vec![(1, 0, 100, Some(1400.0))],
        core_count: 10,
        ..Default::default()
    };
    let project_roots = [roots(a, &[1])];

    let result = collect(&mut sampler, &project_roots, true);

    assert_eq!(result[0].1.cpu_percent, Some(100.0));
}

#[gpui::test]
fn no_cpu_baseline_yet_reports_none_while_rss_is_real(cx: &mut TestAppContext) {
    let a = cx.update(|cx| cx.new(|_| ()).entity_id());
    let mut sampler = FakeSampler {
        processes: vec![(1, 0, 100, None)],
        core_count: 1,
        ..Default::default()
    };
    let project_roots = [roots(a, &[1])];

    let result = collect(&mut sampler, &project_roots, true);

    assert_eq!(result[0].1.rss_bytes, Some(100));
    assert_eq!(result[0].1.cpu_percent, None);

    let footprints = Footprints(vec![(a, "project".into(), result[0].1)]);
    let combined = footprints.combined();
    assert_eq!(combined.rss_bytes, Some(100));
    assert_eq!(combined.cpu_percent, None);
}

#[gpui::test]
fn a_project_with_no_local_pids_reports_nothing_measured(cx: &mut TestAppContext) {
    let remote = cx.update(|cx| cx.new(|_| ()).entity_id());
    let local = cx.update(|cx| cx.new(|_| ()).entity_id());
    let mut sampler = FakeSampler {
        processes: vec![(1, 0, 500, Some(10.0))],
        core_count: 1,
        ..Default::default()
    };
    let project_roots = [roots(remote, &[]), roots(local, &[1])];

    let result = collect(&mut sampler, &project_roots, true);

    let remote_footprint = result.iter().find(|(key, _, _)| *key == remote).unwrap().1;
    assert_eq!(remote_footprint.rss_bytes, None);
    assert_eq!(remote_footprint.cpu_percent, None);

    let footprints = Footprints(
        result
            .into_iter()
            .map(|(key, footprint, _)| (key, "project".into(), footprint))
            .collect(),
    );
    assert_eq!(
        footprints.combined().rss_bytes,
        Some(500),
        "the remote project with nothing measured must not drag the total to a lower Some(_)"
    );
}

#[gpui::test]
fn discover_false_skips_the_enumeration_and_returns_what_it_attributed(cx: &mut TestAppContext) {
    let a = cx.update(|cx| cx.new(|_| ()).entity_id());
    let mut sampler = FakeSampler {
        processes: vec![(1, 0, 100, None), (2, 1, 50, None)],
        core_count: 1,
        ..Default::default()
    };
    let project_roots = [roots(a, &[1])];

    let discovered = collect(&mut sampler, &project_roots, true);
    assert_eq!(sampler.descendants_calls, 1);
    assert_eq!(
        discovered[0].1.rss_bytes,
        Some(150),
        "a discovery pass must include the descendant"
    );

    // The attributed PIDs come back so the caller can re-offer them, which is
    // the contract `ProjectFootprintIndicator` relies on.
    let mut attributed = discovered[0].2.clone();
    attributed.sort();
    assert_eq!(attributed, vec![1, 2]);

    // Handed back as roots, a narrow pass reproduces the same total without a
    // second enumeration. This is the regression guard: reverting the caller to
    // pass bare roots here drops the child and yields Some(100).
    let narrow_roots = [roots(a, &attributed)];
    let reused = collect(&mut sampler, &narrow_roots, false);
    assert_eq!(
        sampler.descendants_calls, 1,
        "discover: false must not call descendants again"
    );
    assert_eq!(
        reused[0].1.rss_bytes,
        Some(150),
        "re-offering the discovered PIDs must preserve the descendant's memory"
    );
}

#[gpui::test]
fn a_narrow_pass_given_only_roots_loses_the_descendants(cx: &mut TestAppContext) {
    // The defect this feature shipped with for one review cycle, pinned so it
    // cannot come back quietly: `collect` is pure, so bare roots on a narrow
    // tick measure the root process alone.
    let a = cx.update(|cx| cx.new(|_| ()).entity_id());
    let mut sampler = FakeSampler {
        processes: vec![(1, 0, 100, None), (2, 1, 50, None)],
        core_count: 1,
        ..Default::default()
    };
    let project_roots = [roots(a, &[1])];

    let result = collect(&mut sampler, &project_roots, false);
    assert_eq!(result[0].1.rss_bytes, Some(100));
}

#[gpui::test]
fn format_rss_reports_one_decimal_above_a_megabyte() {
    assert_eq!(format_rss(1_610_612_736).to_string(), "1.5 GB");
    assert_eq!(format_rss(1_572_864).to_string(), "1.5 MB");
    assert_eq!(format_rss(3072).to_string(), "3 KB");
}

#[gpui::test]
fn format_cpu_reports_a_rounded_integer_percent() {
    assert_eq!(format_cpu(34.4).to_string(), "34%");
    assert_eq!(format_cpu(99.6).to_string(), "100%");
}

/// Installs a settings store so `ProjectFootprintSetting` resolves from a
/// real value rather than falling back. Mirrors `keep_awake_tests`'s
/// `init_settings`. See the comment on `from_settings` for why that impl
/// falls back rather than panicking on a missing default for now: `default.json`
/// does not carry `project_footprint_indicator` until the crate is wired into
/// `crates/zed` (phase 05), so the value resolved here is that fallback, not
/// a value read from `default.json`.
fn init_settings(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        ProjectFootprintSetting::register(cx);
    });
}

fn set_enabled(cx: &mut TestAppContext, enabled: bool) {
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |content| {
                content.project_footprint_indicator = Some(enabled);
            });
        });
    });
}

#[gpui::test]
fn is_enabled_defaults_true_with_no_settings_store(cx: &mut TestAppContext) {
    cx.update(|cx| assert!(ProjectFootprintSetting::is_enabled(cx)));
}

#[gpui::test]
fn is_enabled_follows_the_settings_store_once_installed(cx: &mut TestAppContext) {
    init_settings(cx);
    cx.update(|cx| assert!(ProjectFootprintSetting::is_enabled(cx)));

    set_enabled(cx, false);
    cx.update(|cx| assert!(!ProjectFootprintSetting::is_enabled(cx)));

    set_enabled(cx, true);
    cx.update(|cx| assert!(ProjectFootprintSetting::is_enabled(cx)));
}
