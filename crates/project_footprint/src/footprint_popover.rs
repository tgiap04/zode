//! The pure, cx-light half of the indicator: gating and cadence decisions,
//! rendering the numbers to text, gathering one tick's PIDs and labels from
//! the tracked workspaces, and building the popover. Split out of
//! `footprint_indicator.rs` purely to keep that file under the 200-line
//! guidance -- the poll loop it drives is what would otherwise push it over.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{App, Entity, EntityId, SharedString, WeakEntity, Window};
use project::{Project, ProjectGroupKey, path_suffix};
use ui::{ContextMenu, IconName};
use util::disambiguate::compute_disambiguation_details;
use workspace::MultiWorkspace;

use super::{FootprintRoots, Footprints, Pid, ProjectFootprint, format_cpu, format_rss};

/// Every `DISCOVERY_EVERY`th tick is a discovery pass (10 x 3s = 30s), so one
/// timer drives both rhythms and they can never interleave on the sampler.
const DISCOVERY_EVERY: usize = 10;

/// Whether polling should be running at all. Off when the setting is off, or
/// when the window is not active: an unfocused window's footer is not being
/// read, and a background editor must not wake every 3s for it.
pub(crate) fn wants_polling(enabled: bool, window_active: bool) -> bool {
    enabled && window_active
}

/// Whether `ticks_since_discovery` names a discovery tick. A pure modulo, so
/// the counter never needs resetting on its own -- restarting the loop resets
/// it to 0 so re-activation always begins with a fresh discovery pass.
pub(crate) fn is_discovery_tick(ticks_since_discovery: usize) -> bool {
    ticks_since_discovery.is_multiple_of(DISCOVERY_EVERY)
}

/// The text drawn for one project's numbers, or the badge's combined total --
/// "not measured" where a value is `None`, never the lie of `"0 B"`. A
/// remote/guest project, or a sampler with no CPU baseline yet, has nothing
/// to report; that is not the same as reporting zero.
pub(crate) fn render_project_line(footprint: &ProjectFootprint) -> SharedString {
    let (rss, cpu) = footprint_parts(footprint);
    format!("RAM {rss} \u{b7} CPU {cpu}").into()
}

/// The memory and CPU halves separately, so the badge can put an icon in front
/// of each while `render_project_line` keeps producing the single string that
/// change-detection and the popover rows compare.
pub(crate) fn footprint_parts(footprint: &ProjectFootprint) -> (SharedString, SharedString) {
    let rss = footprint
        .rss_bytes
        .map(format_rss)
        .unwrap_or_else(|| "not measured".into());
    let cpu = footprint
        .cpu_percent
        .map(format_cpu)
        .unwrap_or_else(|| "not measured".into());
    (rss, cpu)
}

/// The icon that stands for memory. Neither this nor [`CPU_ICON`] is a
/// purpose-built glyph -- this repo ships no CPU or memory-module icon among
/// its 265 -- so the badge's tooltip and the popover rows carry the words
/// "RAM" and "CPU" as well, rather than leaving the icons to carry the meaning
/// alone.
pub(crate) const RSS_ICON: IconName = IconName::Database;

/// The icon that stands for CPU. See [`RSS_ICON`] for why it is a bolt.
pub(crate) const CPU_ICON: IconName = IconName::BoltOutlined;

/// The label/rss/cpu text a `Footprints` actually draws, one entry per row.
/// Change-detection compares *this*, not the raw `ProjectFootprint` values --
/// comparing raw floats would notify on every fractional CPU jiggle the
/// rounded, on-screen text can never show.
fn rendered_lines(footprints: &Footprints) -> Vec<(EntityId, SharedString, SharedString)> {
    footprints
        .0
        .iter()
        .map(|(key, label, footprint)| (*key, label.clone(), render_project_line(footprint)))
        .collect()
}

/// Whether applying `next` in place of `current` would change anything drawn
/// on screen. A free function, tested without an entity or a window, the same
/// trick `keep_awake`'s `needs_rereading` uses.
pub(crate) fn footprints_render_the_same(current: &Footprints, next: &Footprints) -> bool {
    rendered_lines(current) == rendered_lines(next)
}

/// Foreground half of a tick: every tracked project's root PIDs and display
/// label. No syscalls -- `child_process_root_pids` only reads
/// already-maintained entity state, so this is safe to call every tick.
/// Re-offers the PIDs each project was last known to own as additional roots,
/// for the narrow ticks where `collect` cannot rediscover them. Extends rather
/// than replaces, so a terminal opened since the last discovery pass is still
/// measured (as its own root) instead of waiting up to 30s to appear.
///
/// De-duplication is not done here: `collect`'s `claim` already credits a PID
/// to exactly one project the first time it is seen, so a PID arriving both as
/// a real root and as a remembered one is counted once.
pub(crate) fn merge_known_pids(roots: &mut [FootprintRoots], known: &HashMap<EntityId, Vec<Pid>>) {
    for project in roots.iter_mut() {
        if let Some(remembered) = known.get(&project.key) {
            project.roots.extend_from_slice(remembered);
        }
    }
}

pub(crate) fn collect_roots(
    multi_workspace: Option<&WeakEntity<MultiWorkspace>>,
    cx: &App,
) -> Vec<FootprintRoots> {
    let Some(multi_workspace) = multi_workspace.and_then(|weak| weak.upgrade()) else {
        return Vec::new();
    };
    let projects: Vec<Entity<Project>> = multi_workspace
        .read(cx)
        .workspaces()
        .map(|workspace| workspace.read(cx).project().clone())
        .collect();

    // The sidebar's own disambiguation path (`project_list.rs`), reused
    // rather than a second path-shortening rule invented here.
    let keys: Vec<ProjectGroupKey> = projects
        .iter()
        .map(|project| project.read(cx).project_group_key(cx))
        .collect();
    let mut all_paths: Vec<PathBuf> = keys
        .iter()
        .flat_map(|key| key.path_list().paths().iter().cloned())
        .collect();
    all_paths.sort();
    all_paths.dedup();
    let details =
        compute_disambiguation_details(&all_paths, |path, detail| path_suffix(path, detail));
    let path_detail_map: HashMap<PathBuf, usize> = all_paths.into_iter().zip(details).collect();

    projects
        .iter()
        .zip(keys.iter())
        .map(|(project, key)| FootprintRoots {
            key: project.entity_id(),
            label: key.display_name(&path_detail_map),
            roots: project
                .read(cx)
                .child_process_root_pids(cx)
                .into_iter()
                .map(|pid| pid.as_u32())
                .collect(),
        })
        .collect()
}

/// The popover: one row per tracked project. Built fresh on every open -- a
/// menu kept between opens would show the previous answer.
pub(crate) fn build_popover(
    footprints: Footprints,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ContextMenu> {
    ContextMenu::build(window, cx, move |mut menu, _window, _cx| {
        if footprints.0.is_empty() {
            return menu.label("No tracked projects");
        }
        for (_, label, footprint) in &footprints.0 {
            menu = menu.label(format!("{label}  {}", render_project_line(footprint)));
        }
        menu
    })
}
