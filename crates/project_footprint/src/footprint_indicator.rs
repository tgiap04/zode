//! The status-bar entity: one poll loop at two cadences, gated by the setting
//! and the window's focus, driving a badge and a per-project popover.
//!
//! Two nested rhythms share one timer -- 3s narrow, every 10th tick a 30s
//! discovery -- so the expensive full-process enumeration and the cheap
//! narrow refresh can never interleave on the same sampler (phase 03). The
//! foreground half of every tick only reads entity state (no syscalls); the
//! sampler itself is touched only from `cx.background_spawn`.
//!
//! The gating/cadence/rendering decisions and the popover builder live in
//! `footprint_popover.rs`, split out purely to keep this file under the
//! 200-line guidance -- this file is the poll loop and the entity itself.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui::{
    Anchor, Context, Entity, IntoElement, Render, SharedString, Subscription, Task, WeakEntity,
    Window, div,
};
use settings::SettingsStore;
use ui::prelude::*;
use ui::{ButtonLike, PopoverMenu, Tooltip};
use workspace::{MultiWorkspace, StatusItemView, Workspace};

use super::footprint_popover::{
    CPU_ICON, RSS_ICON, build_popover, collect_roots, footprint_parts, footprints_render_the_same,
    is_discovery_tick, merge_known_pids, wants_polling,
};
use super::{
    Footprints, Pid, ProcessSampler, ProjectFootprint, ProjectFootprintSetting,
    SysinfoProcessSampler, collect,
};

/// The narrow cadence; the discovery cadence is a multiple of this, decided
/// by `is_discovery_tick` in `footprint_popover.rs`.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// The status-bar entity. See the module doc for the two-cadence poll loop.
pub struct ProjectFootprintIndicator {
    multi_workspace: Option<WeakEntity<MultiWorkspace>>,
    /// CPU deltas depend on a sampler that outlives one tick, but only the
    /// background executor may ever touch it -- **never lock this on the
    /// foreground**, or a frame stalls for the 12-15ms discovery pass. The
    /// poll loop is the only task that locks it, so the lock is uncontended
    /// by construction.
    sampler: Arc<Mutex<Box<dyn ProcessSampler>>>,
    footprints: Footprints,
    /// The PIDs each project was last known to own, descendants included.
    /// `collect` is pure, so on a narrow tick it can only attribute the PIDs it
    /// is handed -- without this, 9 of every 10 ticks would measure root
    /// processes only and the badge would sawtooth as every agent's child tree
    /// dropped out and came back. Bounded by the tracked projects' process
    /// count and pruned to the live project set on every pass.
    known_pids: HashMap<gpui::EntityId, Vec<Pid>>,
    ticks_since_discovery: usize,
    window_active: bool,
    /// Dropping this cancels the loop. The single owner of its existence is
    /// `sync_polling`; nothing else creates or clears it.
    poll: Option<Task<()>>,
    _settings: Subscription,
    _activation: Subscription,
}

impl ProjectFootprintIndicator {
    /// `workspace` and `handle` are the same workspace twice over,
    /// deliberately -- see `KeepAwake::new`'s doc comment for the reason.
    /// This runs from `initialize_workspace`, itself inside an `observe_new`
    /// on `Workspace`, so the entity is mid-update and `handle.read(cx)`
    /// would panic with "cannot read workspace::Workspace while it is
    /// already being updated" -- a prior session lost ~30 tests to exactly
    /// this. The borrow already in hand is the only safe way to read the
    /// existing state. `handle` is accepted (and left unused) purely to keep
    /// this constructor's shape identical to `KeepAwake::new`'s, so a future
    /// reader who needs to subscribe to this workspace's events has the
    /// non-reading handle already in scope instead of being tempted to add
    /// `handle.read(cx)`.
    pub fn new(
        workspace: &Workspace,
        _handle: &Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let multi_workspace = workspace.multi_workspace().cloned();

        // Fires on *any* settings change, not only this one. Accepted
        // deliberately: `sync_polling` is idempotent, and the cost of a
        // needless restart is one extra discovery pass 3s later. Filtering to
        // this single key would mean tracking its previous value by hand.
        let settings = cx.observe_global::<SettingsStore>(|this, cx| this.sync_polling(cx));
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.window_active = window.is_window_active();
            this.sync_polling(cx);
        });

        let mut this = Self {
            multi_workspace,
            sampler: Arc::new(Mutex::new(
                Box::new(SysinfoProcessSampler::new()) as Box<dyn ProcessSampler>
            )),
            footprints: Footprints::default(),
            known_pids: HashMap::new(),
            ticks_since_discovery: 0,
            window_active: window.is_window_active(),
            poll: None,
            _settings: settings,
            _activation: activation,
        };
        this.sync_polling(cx);
        this
    }

    /// (Re)starts or stops the poll loop so it matches `wants_polling`. The
    /// single place `poll` is ever assigned, and always unconditionally --
    /// never merely `is_none()`-guarded -- so a task that ended itself
    /// (see the loop's own `wants_polling` recheck below) can never leave a
    /// stale, finished task blocking a legitimate restart.
    fn sync_polling(&mut self, cx: &mut Context<Self>) {
        let wanted = wants_polling(ProjectFootprintSetting::is_enabled(cx), self.window_active);
        self.poll = None;
        if !wanted {
            return;
        }
        self.ticks_since_discovery = 0;
        // A restart's first tick is a discovery tick, which rebuilds the ledger
        // from real roots anyway; dropping it here means a window that was
        // inactive for hours cannot resurface PIDs that have long since exited.
        self.known_pids.clear();
        let sampler = self.sampler.clone();
        self.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;

                let Ok(Some((roots, discover))) = this.update(cx, |this, cx| {
                    if !wants_polling(ProjectFootprintSetting::is_enabled(cx), this.window_active) {
                        // Never touches `self.poll` here: dropping the task
                        // that is currently running this closure, from
                        // inside itself, is the bug `sync_polling`'s comment
                        // warns about. `sync_polling` running from the
                        // settings/activation observers is what actually
                        // stops polling; this is only a defensive exit.
                        return None;
                    }
                    let discover = is_discovery_tick(this.ticks_since_discovery);
                    this.ticks_since_discovery = this.ticks_since_discovery.wrapping_add(1);
                    let mut roots = collect_roots(this.multi_workspace.as_ref(), cx);
                    if !discover {
                        // Narrow tick: re-offer the last discovery's descendants
                        // as roots so they stay attributed. A discovery tick
                        // deliberately passes the real roots alone, so the walk
                        // rebuilds the set from truth and PIDs that have exited
                        // fall out instead of lingering.
                        merge_known_pids(&mut roots, &this.known_pids);
                    }
                    Some((roots, discover))
                }) else {
                    break;
                };

                if roots.iter().all(|project| project.roots.is_empty()) {
                    // No tracked project owns a child process: nothing a
                    // background pass could discover or sample, so none is
                    // started -- the common case in a freshly opened window.
                    let footprints = Footprints(
                        roots
                            .into_iter()
                            .map(|project| {
                                (project.key, project.label, ProjectFootprint::default())
                            })
                            .collect(),
                    );
                    if this
                        .update(cx, |this, cx| {
                            // Same "replaced wholesale, never merged" rule as
                            // the normal path below: every project has zero
                            // roots this tick, so nothing is tracked, and a
                            // stale entry left behind here would be a PID this
                            // sampler is no longer even looking at.
                            this.known_pids.clear();
                            this.apply(footprints, cx)
                        })
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }

                let sampler = sampler.clone();
                let (roots, per_project) = cx
                    .background_spawn(async move {
                        // Poisoning would mean the loop's own previous pass
                        // panicked mid-lock; recovering rather than
                        // panicking here keeps one bad tick from wedging the
                        // indicator for the rest of the session. No project
                        // identity is in scope to log by index here, so this
                        // is deliberately silent beyond the warning itself.
                        let mut sampler = sampler.lock().unwrap_or_else(|poisoned| {
                            log::warn!("project_footprint: sampler mutex was poisoned; recovering");
                            poisoned.into_inner()
                        });
                        let per_project = collect(&mut **sampler, &roots, discover);
                        (roots, per_project)
                    })
                    .await;

                let mut known_pids = HashMap::with_capacity(per_project.len());
                let footprints = Footprints(
                    roots
                        .into_iter()
                        .zip(per_project)
                        .map(|(project, (_, footprint, pids))| {
                            known_pids.insert(project.key, pids);
                            (project.key, project.label, footprint)
                        })
                        .collect(),
                );
                if this
                    .update(cx, |this, cx| {
                        // Replaced wholesale, never merged: a project that
                        // closed between ticks must not keep an entry, or this
                        // becomes a map that only ever grows.
                        this.known_pids = known_pids;
                        this.apply(footprints, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// Applies one pass's results, notifying only when what is actually
    /// drawn on screen changed.
    fn apply(&mut self, footprints: Footprints, cx: &mut Context<Self>) {
        let changed = !footprints_render_the_same(&self.footprints, &footprints);
        self.footprints = footprints;
        if changed {
            cx.notify();
        }
    }
}

impl Render for ProjectFootprintIndicator {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let combined = self.footprints.combined();
        if combined.rss_bytes.is_none() && combined.cpu_percent.is_none() {
            // Renders nothing rather than reserving space: a status-bar item
            // returning a flex box while empty shifts every neighbour over.
            return div();
        }

        let count = self.footprints.0.len();
        let (rss, cpu) = footprint_parts(&combined);
        // Spells out which half is which: the two icons are stand-ins (this
        // repo ships no CPU or memory glyph), so the words live here rather
        // than relying on a reader to decode a bolt and a database.
        let tooltip: SharedString = format!(
            "RAM {rss} and CPU {cpu} — language servers and terminals for {count} project{}",
            if count == 1 { "" } else { "s" }
        )
        .into();
        let footprints = self.footprints.clone();

        div().child(
            PopoverMenu::new("project-footprint")
                .menu(move |window, cx| Some(build_popover(footprints.clone(), window, cx)))
                // Above and left-aligned: this item sits on the bar at the
                // bottom of the window, so anywhere below it is off-screen.
                .anchor(Anchor::BottomLeft)
                .trigger_with_tooltip(
                    ButtonLike::new("project-footprint-trigger")
                        .style(ButtonStyle::Subtle)
                        .child(
                            h_flex()
                                .gap_1p5()
                                .child(
                                    h_flex()
                                        .gap_0p5()
                                        .child(
                                            Icon::new(RSS_ICON)
                                                .size(IconSize::XSmall)
                                                .color(Color::Muted),
                                        )
                                        .child(
                                            Label::new(rss)
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .gap_0p5()
                                        .child(
                                            Icon::new(CPU_ICON)
                                                .size(IconSize::XSmall)
                                                .color(Color::Muted),
                                        )
                                        .child(
                                            Label::new(cpu)
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        ),
                                ),
                        ),
                    Tooltip::text(tooltip),
                ),
        )
    }
}

impl StatusItemView for ProjectFootprintIndicator {
    /// Nothing to do: which buffer is active does not change what any
    /// project's processes are doing.
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn workspace::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}
