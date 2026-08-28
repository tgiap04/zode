use std::sync::Arc;

use container::{
    BackendEvent, ContainerBackend, ContainerError, DestructivePlan, DockerBackend,
    KubernetesBackend, PruneScope, Resource, ResourceAction, ResourceKind,
};
use futures::StreamExt as _;
use futures::future::Either;
use std::collections::HashMap;
use std::time::Duration;

/// How often the list is re-read when nothing has said it changed.
///
/// Slow on purpose: this is a safety net under the event stream, not the primary
/// way the list stays right. Fast enough that a missed event is noticed within
/// half a minute; slow enough that a machine with the column open all day is not
/// running `docker ps` every second.
const REFRESH_TICK: Duration = Duration::from_secs(30);
use gpui::{App, Context, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Task, Window};
use ui::prelude::*;
use workspace::Workspace;
use workspace::dock::PanelEvent;

/// What the panel knows about the list right now.
///
/// `Failed` keeps the error rather than a message, so the view can tell
/// "not installed" from "installed but down" -- two states that need different
/// words and neither of which is a fault worth a dialog.
pub(crate) enum ListState {
    Loading,
    Ready(Vec<Resource>),
    Failed(ContainerError),
}

pub struct ContainerPanel {
    /// The width the view was last drawn at, when it is standing on its own.
    ///
    /// Measured rather than asked for: a tab is as wide as the pane holds and a
    /// window as wide as somebody dragged it, and neither number exists until
    /// the frame is laid out. Read one frame late, which is invisible at this
    /// scale -- the columns only change when the width crosses the breakpoint.
    pub(crate) measured_width: Option<Pixels>,
    pub(crate) focus_handle: FocusHandle,
    /// Every engine the panel can ask, in the order the switcher shows them.
    ///
    /// Offered unconditionally rather than probed at startup: probing means
    /// running a command per engine on every window open, which is exactly what
    /// `ContainerPanel::new` was fixed not to do. An engine that is not there
    /// says so in its own words when it is asked -- and "podman is not
    /// installed" is a better answer than a button that was never drawn.
    pub(crate) backends: Vec<Arc<dyn ContainerBackend>>,
    pub(crate) active_backend: usize,
    /// Which kind the list is showing.
    ///
    /// One kind at a time rather than every kind at once, because the kinds do
    /// not share columns: an image wants its size and age, a volume its driver
    /// and mount point, a container its ports. One table with columns common to
    /// all five would be wrong for all five. It is also four fewer commands per
    /// refresh.
    pub(crate) active_kind: ResourceKind,
    pub(crate) state: ListState,
    /// Held in a field rather than detached, so closing the column stops the
    /// command it had in flight. CLAUDE.md's concurrency rule -- a detached task
    /// here would leave a `docker` process behind every time the panel goes.
    pub(crate) load: Option<Task<()>>,
    /// Listening to the engine, and re-reading on a slow tick.
    ///
    /// Held in a field for the same reason as `load`, and it matters more here:
    /// this one owns a child process. Detached, every close of the column would
    /// leave a `docker events` behind.
    pub(crate) watch: Option<Task<()>>,
    /// Actions still running, by resource id, so a row can say "stopping" rather
    /// than sitting still.
    ///
    /// `docker stop` waits ten seconds for the process before killing it. A
    /// button that goes dead for ten seconds with no explanation reads as broken.
    pub(crate) in_flight: HashMap<String, ResourceAction>,
    /// The last action that failed, until it is dismissed or another runs.
    ///
    /// Kept and shown rather than logged: CLAUDE.md requires an async failure to
    /// reach the person who asked for it, and `stderr` is the only text that says
    /// *why* the engine refused.
    pub(crate) last_error: Option<ContainerError>,
    /// One per running action, keyed by an id private to this panel.
    ///
    /// A plain `Vec` here would only ever grow: the panel is meant to be left
    /// open all day, and GPUI's `Task` has no `is_finished()` to poll for
    /// completed entries to drop. Each spawned action instead removes its own
    /// entry as the last thing it does (see `act` and `destroy`), so the map
    /// only ever holds the actions genuinely still running.
    pub(crate) actions: HashMap<usize, Task<()>>,
    /// The id the next entry in `actions` is given.
    ///
    /// Monotonic rather than reused: reusing a finished id could let a
    /// straggling removal from an old action delete a new, unrelated one that
    /// was assigned the same slot.
    pub(crate) next_action_id: usize,
    /// The row that was clicked, with its logs, or `None` when the list is what
    /// is on screen.
    pub(crate) detail: Option<crate::detail::Detail>,
    /// Building the log terminal for `detail`.
    ///
    /// Held rather than detached, for the reason `load` and `watch` are: it ends
    /// in a child process running `logs -f`, and a detached build would keep
    /// producing one after the panel that asked is gone.
    pub(crate) logs_build: Option<Task<()>>,
    /// The workspace a terminal tab would open in.
    ///
    /// `None` for a floating window's panel: it is built in a different window's
    /// context on purpose (a `&Workspace` cannot be borrowed there alongside the
    /// `App` creating the entity), so there is no workspace to open a tab in and
    /// the buttons that would say otherwise are not drawn.
    pub(crate) workspace: Option<gpui::WeakEntity<Workspace>>,
}

pub enum ContainerPanelEvent {}

impl EventEmitter<ContainerPanelEvent> for ContainerPanel {}
impl EventEmitter<PanelEvent> for ContainerPanel {}

/// The engines a freshly opened view starts with.
///
/// `ContainerPanel::default_backends`, unless a test has replaced them. The two
/// are kept apart on purpose: the test that asserts the real engine list must
/// keep seeing the real list, so the replacement is applied here -- where a tab
/// is opened -- and not there.
pub(crate) fn engines_for_a_new_view(cx: &App) -> Vec<Arc<dyn ContainerBackend>> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(engines) = cx.try_global::<crate::EnginesForTest>() {
        return engines.0.clone();
    }
    let _ = cx;
    ContainerPanel::default_backends()
}

impl ContainerPanel {
    /// Builds the view and asks the engine.
    ///
    /// Asking here is safe now and was not before: this is only ever called when
    /// somebody opens the tab, whereas the panel this replaced was constructed by
    /// `initialize_panels` on every window open -- which ran `docker ps` once per
    /// window for a column nobody had opened. Nothing constructs this view
    /// unasked any more, so there is no `set_active` to wait for.
    pub fn new(workspace: &Workspace, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::build(engines_for_a_new_view(cx), cx);
        panel.workspace = Some(workspace.weak_handle());
        panel.reload(cx);
        panel.start_watching(cx);
        panel
    }

    /// The one place a view is built.
    ///
    /// There is one host now: an editor tab (or a window of its own, which lays
    /// itself out the same way). The column this used to also live in is gone --
    /// two places showing the same list was two places to keep in step, and the
    /// tab is where it was asked for.
    pub(crate) fn build(backends: Vec<Arc<dyn ContainerBackend>>, cx: &mut Context<Self>) -> Self {
        Self {
            measured_width: None,
            focus_handle: cx.focus_handle(),
            active_kind: backends
                .first()
                .and_then(|backend| backend.supported_kinds().first().copied())
                .unwrap_or(ResourceKind::Container),
            backends,
            active_backend: 0,
            state: ListState::Loading,
            load: None,
            watch: None,
            in_flight: HashMap::default(),
            last_error: None,
            actions: HashMap::default(),
            next_action_id: 0,
            detail: None,
            logs_build: None,
            workspace: None,
        }
    }

    /// The engines a fresh panel offers.
    ///
    /// All three, unconditionally, rather than probed. Probing means running a
    /// command per engine on every window open, which is what
    /// `ContainerPanel::new` was fixed not to do, and an engine that is not
    /// installed already answers clearly for itself -- "podman is not installed"
    /// is more use than a button that was never drawn and cannot be asked about.
    ///
    /// Deliberately NOT read from the `remote.use_podman` setting. That setting
    /// governs which engine a *dev container* is built with on a remote host;
    /// this panel is local-only, and reusing it here would tie two unrelated
    /// choices together.
    ///
    /// Docker first because it is what most people have.
    pub(crate) fn default_backends() -> Vec<Arc<dyn ContainerBackend>> {
        vec![
            Arc::new(DockerBackend::docker()),
            Arc::new(DockerBackend::podman()),
            Arc::new(KubernetesBackend::new()),
        ]
    }

    pub(crate) fn backend(&self) -> Option<&Arc<dyn ContainerBackend>> {
        self.backends.get(self.active_backend)
    }

    /// Switches engine and asks the new one.
    ///
    /// Resets to `Loading` rather than keeping the old list on screen: the two
    /// engines answer different questions, and leaving Docker's containers
    /// visible under a Kubernetes heading would be a lie for as long as the
    /// command took.
    pub(crate) fn choose_backend(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.backends.len() || index == self.active_backend {
            return;
        }
        self.active_backend = index;
        self.state = ListState::Loading;
        // The open row belonged to the old engine. Keeping it would leave its
        // logs following a container the panel is no longer even asking about.
        self.close_detail(cx);
        // The kinds do not carry over: Kubernetes has no word for a volume, and
        // leaving `active_kind` on one would ask the new engine a question it
        // cannot answer. Falls to whatever the new engine lists first.
        if let Some(backend) = self.backends.get(index)
            && !backend.supported_kinds().contains(&self.active_kind)
        {
            self.active_kind = backend
                .supported_kinds()
                .first()
                .copied()
                .unwrap_or(ResourceKind::Container);
        }
        self.reload(cx);
        // The old engine's listener would go on waking the panel about a list it
        // is no longer showing.
        self.start_watching(cx);
        cx.notify();
    }

    /// The kinds the active engine has a word for.
    pub(crate) fn available_kinds(&self) -> &'static [ResourceKind] {
        self.backend()
            .map(|backend| backend.supported_kinds())
            .unwrap_or(&[])
    }

    /// Listens for changes, and re-reads on a slow tick regardless.
    ///
    /// Both, not one: an event can be missed (the daemon restarts, the stream
    /// breaks) and a missed event leaves the list quietly wrong, which is worse
    /// than late. An engine with no event stream at all -- `watch()` answering
    /// `None` -- is carried entirely by the tick.
    ///
    /// Assigning over `self.watch` drops the previous task, so switching engine
    /// or kind replaces the listener rather than stacking a second one.
    pub(crate) fn start_watching(&mut self, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        self.watch = Some(cx.spawn(async move |this, cx| {
            // `Option` rather than a fused stream: an engine may have no stream
            // at all, and one that ends must not be polled again. Both cases are
            // then the same case.
            let mut events = backend.watch();
            loop {
                let mut stale = None;
                match events.as_mut() {
                    Some(stream) => {
                        let tick = cx.background_executor().timer(REFRESH_TICK);
                        match futures::future::select(stream.next(), tick).await {
                            Either::Left((Some(BackendEvent::Stale { kind }), _)) => {
                                stale = Some(kind);
                            }
                            // The stream ended: the daemon went away, or the
                            // process died. Stop listening rather than restarting
                            // it in a loop -- that is how a dead daemon becomes a
                            // spawn storm. The tick carries it from here.
                            Either::Left((None, _)) => events = None,
                            // The tick won. Re-read anyway; that is the point of
                            // having one.
                            Either::Right(((), _)) => {}
                        }
                    }
                    None => cx.background_executor().timer(REFRESH_TICK).await,
                }

                let alive = this
                    .update(cx, |this, cx| {
                        // An event for a kind nobody is looking at is not a reason
                        // to run a command.
                        if stale.is_none_or(|kind| kind == this.active_kind) {
                            this.reload(cx);
                        }
                    })
                    .is_ok();
                if !alive {
                    return;
                }
            }
        }));
    }

    /// What may be done to the kind on screen.
    pub(crate) fn available_actions(&self) -> &'static [ResourceAction] {
        self.backend()
            .map(|backend| backend.supported_actions(self.active_kind))
            .unwrap_or(&[])
    }

    /// Runs an action and lets the engine tell the panel what happened.
    ///
    /// Deliberately does **not** set the new state itself. `docker start`
    /// returning says the command was accepted, not that the container is up --
    /// it may exit a moment later. The list is re-read instead, and until then
    /// the row says what is being attempted.
    pub(crate) fn act(&mut self, action: ResourceAction, id: String, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        if !self.available_actions().contains(&action) {
            return;
        }
        let kind = self.active_kind;
        self.in_flight.insert(id.clone(), action);
        self.last_error = None;
        cx.notify();

        let action_id = self.next_action_id;
        self.next_action_id += 1;
        self.actions.insert(
            action_id,
            cx.spawn(async move |this, cx| {
                let outcome = backend.act(kind, action, &id).await;
                if let Err(error) = this.update(cx, |this, cx| {
                    this.in_flight.remove(&id);
                    match outcome {
                        Ok(()) => {}
                        Err(error) => this.last_error = Some(error),
                    }
                    // Whether it worked or not: the engine is the only thing
                    // that knows the resulting state.
                    this.reload(cx);
                    cx.notify();
                }) {
                    log::debug!("panel closed before its action finished: {error}");
                }
                // Removing our own entry from inside the task's own body is
                // safe here: this is the task's last statement, so it is not
                // being polled again, and the removal only drops the (now
                // redundant) `Task` handle keeping it alive. If the panel is
                // already gone, there is nothing left to remove from.
                if let Err(error) = this.update(cx, |this, _cx| {
                    this.actions.remove(&action_id);
                }) {
                    log::debug!("panel closed before its action could self-remove: {error}");
                }
            }),
        );
    }

    /// Whether the kind on screen can be removed at all.
    pub(crate) fn removable(&self) -> bool {
        container::resource::is_removable(self.active_kind)
            && self.backend().is_some_and(|backend| {
                backend.kind() != container::BackendKind::Kubernetes
                    || self.active_kind == ResourceKind::Pod
            })
    }

    /// Builds the plan for removing one row, or `None` when there is nothing to
    /// remove.
    ///
    /// The **only** way a removal starts. It returns a plan and runs nothing:
    /// whoever holds the plan still has to get it past the confirmation.
    pub(crate) fn plan_removal(&self, id: &str) -> Option<DestructivePlan> {
        if !self.removable() {
            return None;
        }
        let ListState::Ready(resources) = &self.state else {
            return None;
        };
        let target = resources.iter().find(|resource| resource.id == id)?;
        DestructivePlan::remove(self.active_kind, vec![target.clone()])
    }

    /// Goes and finds what a prune would delete, then builds the plan.
    ///
    /// Asynchronous because the finding is: `docker system prune` has no
    /// `--dry-run`, so the list has to be gathered with real commands before
    /// anybody can be asked about it.
    pub(crate) fn plan_prune(
        &self,
        scope: PruneScope,
        cx: &mut Context<Self>,
    ) -> Task<Option<DestructivePlan>> {
        let Some(backend) = self.backend().cloned() else {
            return Task::ready(None);
        };
        cx.background_spawn(async move {
            match backend.prune_targets(scope).await {
                Some(Ok(targets)) => DestructivePlan::prune(scope, targets),
                Some(Err(error)) => {
                    log::warn!("could not work out what a prune would delete: {error}");
                    None
                }
                None => None,
            }
        })
    }

    /// Carries out a plan whose confirmation has already been given.
    ///
    /// Takes a plan and nothing else, so it cannot be called about something
    /// that was never enumerated and shown.
    pub(crate) fn destroy(&mut self, plan: DestructivePlan, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        self.last_error = None;
        cx.notify();
        let action_id = self.next_action_id;
        self.next_action_id += 1;
        self.actions.insert(
            action_id,
            cx.spawn(async move |this, cx| {
                let outcome = backend.destroy(&plan).await;
                if let Err(error) = this.update(cx, |this, cx| {
                    if let Err(error) = outcome {
                        this.last_error = Some(error);
                    }
                    this.reload(cx);
                    cx.notify();
                }) {
                    log::debug!("panel closed before its destroy finished: {error}");
                }
                // See the matching comment in `act`: removing our own map
                // entry as the very last statement is safe.
                if let Err(error) = this.update(cx, |this, _cx| {
                    this.actions.remove(&action_id);
                }) {
                    log::debug!("panel closed before its destroy could self-remove: {error}");
                }
            }),
        );
    }

    pub(crate) fn dismiss_error(&mut self, cx: &mut Context<Self>) {
        self.last_error = None;
        cx.notify();
    }

    pub(crate) fn choose_kind(&mut self, kind: ResourceKind, cx: &mut Context<Self>) {
        if kind == self.active_kind || !self.available_kinds().contains(&kind) {
            return;
        }
        self.active_kind = kind;
        // Same reason as switching engine: a volume list left on screen under an
        // "Images" heading is a lie for as long as the command takes.
        self.state = ListState::Loading;
        self.close_detail(cx);
        self.reload(cx);
        cx.notify();
    }

    /// A column panel over a backend chosen by the caller, for tests that must
    /// not reach a real engine.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_backend(backend: Arc<dyn ContainerBackend>, cx: &mut Context<Self>) -> Self {
        Self::build(vec![backend], cx)
    }

    /// A column panel over several backends, for tests of the switcher.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_backends(backends: Vec<Arc<dyn ContainerBackend>>, cx: &mut Context<Self>) -> Self {
        Self::build(backends, cx)
    }

    pub async fn load(
        workspace: gpui::WeakEntity<Workspace>,
        mut cx: gpui::AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        workspace.update_in(&mut cx, |workspace, window, cx| {
            cx.new(|cx| ContainerPanel::new(workspace, window, cx))
        })
    }

    /// Asks the engine again, replacing whatever the last answer was.
    ///
    /// Assigning over `self.load` drops the previous task, so a second reload
    /// while the first is still running cancels it rather than racing it into
    /// `state`.
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        let kind = self.active_kind;
        self.load = Some(cx.spawn(async move |this, cx| {
            let listed = backend.list(kind).await;
            let state = match listed {
                Ok(resources) => ListState::Ready(resources),
                Err(error) => ListState::Failed(error),
            };
            this.update(cx, |this, cx| {
                if let ListState::Ready(resources) = &state {
                    this.refresh_detail(resources);
                }
                this.state = state;
                cx.notify();
            })
            .ok();
        }));
    }
}

impl Focusable for ContainerPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
