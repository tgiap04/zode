//! Acting on several rows at once, planning their removal, and confirming it,
//! over an engine the test can steer: its rows change under the panel, and its
//! commands wait until the test lets them finish.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use container::backend::{BackendEvent, ContainerError};
use container::fake_backend::FakeBackend;
use container::{
    ContainerBackend, DestructivePlan, Resource, ResourceAction, ResourceKind, RunState,
};
use futures::channel::oneshot;
use futures::stream::BoxStream;
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::confirm_modal::ConfirmDestructive;
use crate::container_panel::{Busy, ContainerPanel};
use crate::container_panel_tests::{init_test, tab_in_workspace};

fn container(name: &str, state: RunState) -> Resource {
    Resource {
        kind: ResourceKind::Container,
        id: format!("id-{name}"),
        name: name.into(),
        state,
        detail: vec![("Image", "nginx:1".into())],
        parent: None,
        compose: None,
    }
}

fn id(name: &str) -> String {
    format!("id-{name}")
}

/// A fake engine whose rows can be swapped after the panel read them, and whose
/// `act` and `removal_plan` can be held until the test releases them.
struct Steered {
    inner: FakeBackend,
    rows: Mutex<Vec<Resource>>,
    running: AtomicUsize,
    peak: AtomicUsize,
    finished: AtomicUsize,
    hold_acts: AtomicBool,
    act_releases: Mutex<Vec<oneshot::Sender<()>>>,
    hold_plans: AtomicBool,
    plan_releases: Mutex<Vec<oneshot::Sender<()>>>,
    plans_asked: AtomicUsize,
}

impl Steered {
    fn holding(rows: Vec<Resource>) -> Self {
        Self {
            inner: FakeBackend::docker().holding(rows.clone()),
            rows: Mutex::new(rows),
            running: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            finished: AtomicUsize::new(0),
            hold_acts: AtomicBool::new(false),
            act_releases: Mutex::new(Vec::new()),
            hold_plans: AtomicBool::new(false),
            plan_releases: Mutex::new(Vec::new()),
            plans_asked: AtomicUsize::new(0),
        }
    }

    fn set_state(&self, name: &str, state: RunState) {
        if let Ok(mut rows) = self.rows.lock() {
            for row in rows.iter_mut().filter(|row| row.name == name) {
                row.state = state;
            }
        }
    }

    fn remove_row(&self, name: &str) {
        if let Ok(mut rows) = self.rows.lock() {
            rows.retain(|row| row.name != name);
        }
    }

    fn release_acts(&self) {
        if let Ok(mut releases) = self.act_releases.lock() {
            for release in releases.drain(..) {
                release.send(()).ok();
            }
        }
    }

    fn release_plans(&self) {
        if let Ok(mut releases) = self.plan_releases.lock() {
            for release in releases.drain(..) {
                release.send(()).ok();
            }
        }
    }
}

#[async_trait]
impl ContainerBackend for Steered {
    fn kind(&self) -> container::BackendKind {
        self.inner.kind()
    }

    fn supported_kinds(&self) -> &'static [ResourceKind] {
        self.inner.supported_kinds()
    }

    fn supported_actions(&self, kind: ResourceKind) -> &'static [ResourceAction] {
        self.inner.supported_actions(kind)
    }

    async fn list(&self, kind: ResourceKind) -> Result<Vec<Resource>, ContainerError> {
        Ok(self
            .rows
            .lock()
            .map(|rows| {
                rows.iter()
                    .filter(|row| row.kind == kind)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn act(
        &self,
        kind: ResourceKind,
        action: ResourceAction,
        id: &str,
    ) -> Result<(), ContainerError> {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        if self.hold_acts.load(Ordering::SeqCst) {
            let (release, wait) = oneshot::channel();
            if let Ok(mut releases) = self.act_releases.lock() {
                releases.push(release);
            }
            wait.await.ok();
        }
        let outcome = self.inner.act(kind, action, id).await;
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.finished.fetch_add(1, Ordering::SeqCst);
        outcome
    }

    fn watch(&self) -> Option<BoxStream<'static, BackendEvent>> {
        None
    }

    fn logs_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        self.inner.logs_command(kind, id)
    }

    async fn prune_targets(
        &self,
        scope: container::PruneScope,
    ) -> Option<Result<Vec<Resource>, ContainerError>> {
        self.inner.prune_targets(scope).await
    }

    async fn destroy(&self, plan: &DestructivePlan) -> Result<(), ContainerError> {
        self.inner.destroy(plan).await
    }

    fn exec_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        self.inner.exec_command(kind, id)
    }

    fn supports_inspect(&self, kind: ResourceKind) -> bool {
        self.inner.supports_inspect(kind)
    }

    fn offers(&self, op: container::Op) -> bool {
        self.inner.offers(op)
    }

    async fn removal_plan(
        &self,
        kind: ResourceKind,
        targets: Vec<Resource>,
    ) -> Result<Option<DestructivePlan>, ContainerError> {
        self.plans_asked.fetch_add(1, Ordering::SeqCst);
        if self.hold_plans.load(Ordering::SeqCst) {
            let (release, wait) = oneshot::channel();
            if let Ok(mut releases) = self.plan_releases.lock() {
                releases.push(release);
            }
            wait.await.ok();
        }
        self.inner.removal_plan(kind, targets).await
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn panel_over(
    steered: Arc<Steered>,
    cx: &mut TestAppContext,
) -> (Entity<ContainerPanel>, &mut VisualTestContext) {
    init_test(cx);
    let engine: Arc<dyn ContainerBackend> = steered;
    let (panel, cx) = cx.add_window_view(|_window, cx| ContainerPanel::with_backend(engine, cx));
    panel.update(cx, |panel, cx| panel.reload(cx));
    draw(cx);
    (panel, cx)
}

fn acted(steered: &Steered) -> Vec<(ResourceAction, String)> {
    let mut acted = steered.inner.acted();
    acted.sort_by(|a, b| a.1.cmp(&b.1));
    acted
}

fn many(count: usize) -> Vec<Resource> {
    (0..count)
        .map(|index| container(&format!("c{index:02}"), RunState::Running))
        .collect()
}

mod acting {
    use super::*;

    /// The menu was opened over a row that was running. By the time an entry is
    /// pressed it has stopped: Restart on the stale row would start a container
    /// somebody had just stopped.
    #[gpui::test]
    async fn an_action_uses_the_state_the_list_has_now_not_the_one_the_menu_saw(
        cx: &mut TestAppContext,
    ) {
        let steered = Arc::new(Steered::holding(vec![
            container("web", RunState::Running),
            container("api", RunState::Running),
        ]));
        let (panel, cx) = panel_over(steered.clone(), cx);
        let seen_by_the_menu = vec![
            container("web", RunState::Running),
            container("api", RunState::Running),
        ];

        steered.set_state("web", RunState::Stopped);
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);

        panel.update(cx, |panel, cx| {
            panel.act_many(ResourceAction::Restart, &seen_by_the_menu, cx)
        });
        draw(cx);
        assert_eq!(
            acted(&steered),
            [(ResourceAction::Restart, id("api"))],
            "web stopped in the meantime, so it is not restarted"
        );
    }

    #[gpui::test]
    async fn a_row_that_is_gone_is_skipped_without_an_error(cx: &mut TestAppContext) {
        let steered = Arc::new(Steered::holding(vec![
            container("web", RunState::Running),
            container("api", RunState::Running),
        ]));
        let (panel, cx) = panel_over(steered.clone(), cx);
        let seen_by_the_menu = vec![
            container("web", RunState::Running),
            container("api", RunState::Running),
        ];

        steered.remove_row("web");
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);

        panel.update(cx, |panel, cx| {
            panel.act_many(ResourceAction::Stop, &seen_by_the_menu, cx)
        });
        draw(cx);
        assert_eq!(acted(&steered), [(ResourceAction::Stop, id("api"))]);
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.last_error.is_none(),
                "a row that left is not an error to report: {:?}",
                panel.last_error
            )
        });
    }

    #[gpui::test]
    async fn a_bulk_action_runs_a_few_at_a_time_and_every_row_still_gets_its_turn(
        cx: &mut TestAppContext,
    ) {
        let rows = many(10);
        let steered = Arc::new(Steered::holding(rows.clone()));
        let (panel, cx) = panel_over(steered.clone(), cx);
        steered.hold_acts.store(true, Ordering::SeqCst);

        panel.update(cx, |panel, cx| {
            panel.act_many(ResourceAction::Stop, &rows, cx)
        });
        draw(cx);
        assert_eq!(
            steered.running.load(Ordering::SeqCst),
            4,
            "ten rows do not start ten processes at once"
        );

        for _ in 0..20 {
            if steered.finished.load(Ordering::SeqCst) == rows.len() {
                break;
            }
            steered.release_acts();
            draw(cx);
        }
        assert_eq!(steered.finished.load(Ordering::SeqCst), rows.len());
        assert_eq!(
            steered.peak.load(Ordering::SeqCst),
            4,
            "never more than four ran together"
        );
        assert_eq!(acted(&steered).len(), 10, "and each row was acted on once");
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.in_flight.is_empty(),
                "every row is idle again: {:?}",
                panel.in_flight
            )
        });
    }

    #[gpui::test]
    async fn a_failing_row_in_a_bulk_action_does_not_stop_the_rest(cx: &mut TestAppContext) {
        let rows = vec![
            container("a", RunState::Running),
            container("b", RunState::Running),
            container("c", RunState::Running),
        ];
        let steered = Arc::new(Steered {
            inner: FakeBackend::docker()
                .holding(rows.clone())
                .refusing_actions_on(&id("b")),
            ..Steered::holding(rows.clone())
        });
        let (panel, cx) = panel_over(steered.clone(), cx);
        panel.update(cx, |panel, cx| {
            panel.act_many(ResourceAction::Stop, &rows, cx)
        });
        draw(cx);
        assert_eq!(
            acted(&steered),
            [
                (ResourceAction::Stop, id("a")),
                (ResourceAction::Stop, id("c"))
            ]
        );
        panel.read_with(cx, |panel, _| {
            assert!(
                panel
                    .last_error
                    .as_ref()
                    .is_some_and(|error| error.to_string().contains("refused")),
                "the refusal is shown"
            );
            assert!(panel.in_flight.is_empty());
        });
    }
}

mod removing {
    use super::*;

    #[gpui::test]
    async fn a_row_the_filters_hide_cannot_be_planned_even_when_asked_for_by_id(
        cx: &mut TestAppContext,
    ) {
        let steered = Arc::new(Steered::holding(vec![
            container("web", RunState::Running),
            container("db", RunState::Stopped),
        ]));
        let (panel, cx) = panel_over(steered, cx);
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        draw(cx);

        let hidden = panel
            .update(cx, |panel, cx| panel.plan_removal_of(&[id("db")], cx))
            .await
            .expect("planning");
        assert!(
            hidden.is_none(),
            "db is not on screen, so it is not planned"
        );
        let shown = panel
            .update(cx, |panel, cx| {
                panel.plan_removal_of(&[id("db"), id("web")], cx)
            })
            .await
            .expect("planning")
            .expect("web is on screen");
        let names: Vec<&str> = shown
            .targets()
            .iter()
            .map(|target| target.name.as_str())
            .collect();
        assert_eq!(names, ["web"]);
    }

    /// The open row's own Remove button is above the list: the container may
    /// have stopped, under a running-only filter, from that very view.
    #[gpui::test]
    async fn the_open_row_can_be_removed_even_when_the_filters_now_hide_it(
        cx: &mut TestAppContext,
    ) {
        let steered = Arc::new(Steered::holding(vec![
            container("web", RunState::Running),
            container("api", RunState::Running),
        ]));
        let (panel, cx) = panel_over(steered.clone(), cx);
        panel.update(cx, |panel, cx| panel.toggle_running_only(cx));
        panel.update_in(cx, |panel, window, cx| {
            let web = panel
                .visible_resources(cx)
                .into_iter()
                .find(|row| row.name == "web")
                .expect("web is shown while it runs");
            panel.open_detail(web, window, cx)
        });
        steered.set_state("web", RunState::Stopped);
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);

        let planned = panel
            .update(cx, |panel, cx| {
                assert!(
                    panel
                        .visible_resources(cx)
                        .iter()
                        .all(|row| row.name != "web"),
                    "the filter hides web now"
                );
                panel.plan_removal_of(&[id("web"), id("api")], cx)
            })
            .await
            .expect("planning")
            .expect("something to remove");
        let names: Vec<&str> = planned
            .targets()
            .iter()
            .map(|target| target.name.as_str())
            .collect();
        assert_eq!(names, ["web", "api"], "the open row, and the one in view");
    }

    #[gpui::test]
    async fn the_rows_being_planned_say_so_and_a_second_delete_waits_for_the_first(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let steered = Arc::new(Steered::holding(vec![
            container("web", RunState::Running),
            container("api", RunState::Running),
        ]));
        let engine: Arc<dyn ContainerBackend> = steered.clone();
        let panel = tab_in_workspace(&workspace, vec![engine], cx);
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);
        steered.hold_plans.store(true, Ordering::SeqCst);

        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal_of(vec![id("web"), id("api")], window, cx)
        });
        draw(cx);
        panel.read_with(cx, |panel, _| {
            for name in ["web", "api"] {
                assert_eq!(
                    panel.busy_for(&id(name)),
                    Some(Busy::Planning),
                    "{name} is being looked at, and says so"
                );
            }
        });

        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal_of(vec![id("web")], window, cx)
        });
        draw(cx);
        assert_eq!(
            steered.plans_asked.load(Ordering::SeqCst),
            1,
            "a second Delete on the same row asks the engine nothing more"
        );

        steered.release_plans();
        draw(cx);
        let modal = workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<ConfirmDestructive>(cx)
            })
            .expect("the one confirmation opens");
        modal.read_with(cx, |modal, _| assert_eq!(modal.plan().targets().len(), 2));
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.in_flight.is_empty(),
                "the rows are free again once the dialog is up"
            )
        });
    }

    #[gpui::test]
    async fn a_plan_that_fails_frees_its_rows(cx: &mut TestAppContext) {
        let steered = Arc::new(Steered::holding(many(2)));
        let (panel, cx) = panel_over(steered.clone(), cx);
        steered.hold_plans.store(true, Ordering::SeqCst);
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal_of(vec![id("c00")], window, cx)
        });
        draw(cx);
        panel.read_with(cx, |panel, _| assert!(panel.busy_for(&id("c00")).is_some()));

        // The panel has no workspace here, so nothing opens: the rows must
        // still be released.
        steered.release_plans();
        draw(cx);
        panel.read_with(cx, |panel, _| assert!(panel.in_flight.is_empty()));
    }
}

mod confirming {
    use super::*;

    /// Plans are built for one engine. A confirmation answered after the panel
    /// moved to another must not run them there: the ids could name something
    /// else, or nothing.
    #[gpui::test]
    async fn a_removal_confirmed_after_the_engine_changed_destroys_nothing(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let docker =
            Arc::new(FakeBackend::docker().holding(vec![container("web", RunState::Running)]));
        let podman = Arc::new(FakeBackend::empty(
            container::BackendKind::Podman,
            &[ResourceKind::Container],
        ));
        let engines: Vec<Arc<dyn ContainerBackend>> = vec![docker.clone(), podman.clone()];
        let panel = tab_in_workspace(&workspace, engines, cx);
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);

        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal(&id("web"), window, cx)
        });
        draw(cx);
        let modal = workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<ConfirmDestructive>(cx)
            })
            .expect("the dialog is open");

        panel.update(cx, |panel, cx| panel.choose_backend(1, cx));
        draw(cx);

        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("web", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);

        assert!(
            docker.destroyed().is_empty(),
            "the panel is no longer on docker, so docker is not asked either"
        );
        assert!(
            podman.destroyed().is_empty(),
            "and the plan was built for another engine than podman"
        );
        panel.read_with(cx, |panel, _| {
            let said = panel
                .last_error
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default();
            assert!(
                said.contains("engine"),
                "the person is told why nothing happened: {said:?}"
            );
            assert!(panel.in_flight.is_empty());
        });
    }

    #[gpui::test]
    async fn a_removal_confirmed_on_the_same_engine_goes_through(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = fs::FakeFs::new(cx.executor());
        let project = project::Project::test(fs, [], cx).await;
        let (multi_workspace, cx) = cx
            .add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let docker =
            Arc::new(FakeBackend::docker().holding(vec![container("web", RunState::Running)]));
        let engines: Vec<Arc<dyn ContainerBackend>> = vec![docker.clone()];
        let panel = tab_in_workspace(&workspace, engines, cx);
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);

        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal(&id("web"), window, cx)
        });
        draw(cx);
        let modal = workspace
            .read_with(cx, |workspace, cx| {
                workspace.active_modal::<ConfirmDestructive>(cx)
            })
            .expect("the dialog is open");
        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("web", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);
        assert_eq!(docker.destroyed().len(), 1);
    }
}

/// `docker images` prints one row per tag, and every tag of an image shares its
/// id. Anything keyed by the id alone treats two rows as one.
mod tags {
    use super::*;
    use container::resource::row_key;
    use gpui::Modifiers;

    fn tag(image_id: &str, name: &str) -> Resource {
        Resource {
            kind: ResourceKind::Image,
            id: image_id.into(),
            name: name.into(),
            state: RunState::Unknown,
            detail: Vec::new(),
            parent: None,
            compose: None,
        }
    }

    /// Two tags of one image, and an unrelated one after them.
    fn shelf() -> Vec<Resource> {
        vec![
            tag("sha-nginx", "nginx:latest"),
            tag("sha-nginx", "nginx:1.27"),
            tag("sha-redis", "redis:7"),
        ]
    }

    fn panel_of_images(
        cx: &mut TestAppContext,
    ) -> (Entity<ContainerPanel>, &mut VisualTestContext) {
        let (panel, cx) = panel_over(Arc::new(Steered::holding(shelf())), cx);
        panel.update(cx, |panel, cx| {
            panel.choose_kind(ResourceKind::Image, cx);
            panel.reload(cx);
        });
        draw(cx);
        (panel, cx)
    }

    fn names(rows: Vec<Resource>) -> Vec<String> {
        rows.into_iter().map(|row| row.name).collect()
    }

    fn selected(panel: &Entity<ContainerPanel>, cx: &mut VisualTestContext) -> Vec<String> {
        panel.read_with(cx, |panel, cx| names(panel.selected_resources(cx)))
    }

    fn cursor_on(panel: &Entity<ContainerPanel>, cx: &mut VisualTestContext) -> Option<String> {
        panel.read_with(cx, |panel, cx| {
            let cursor = panel.selection.cursor()?;
            panel
                .visible_resources(cx)
                .into_iter()
                .find(|row| row_key(row) == cursor)
                .map(|row| row.name)
        })
    }

    #[gpui::test]
    async fn the_keyboard_cursor_walks_through_every_tag_of_an_image(cx: &mut TestAppContext) {
        let (panel, cx) = panel_of_images(cx);
        let mut walked = Vec::new();
        for _ in 0..3 {
            panel.update(cx, |panel, cx| panel.move_cursor(1, cx));
            walked.push(cursor_on(&panel, cx));
        }
        assert_eq!(
            walked,
            [
                Some("nginx:latest".to_string()),
                Some("nginx:1.27".to_string()),
                Some("redis:7".to_string()),
            ],
            "the cursor does not stick on the first row of a shared id"
        );
        panel.update(cx, |panel, cx| panel.move_cursor(-1, cx));
        assert_eq!(cursor_on(&panel, cx).as_deref(), Some("nginx:1.27"));
    }

    #[gpui::test]
    async fn selecting_one_tag_leaves_its_siblings_alone(cx: &mut TestAppContext) {
        let (panel, cx) = panel_of_images(cx);
        let second = shelf()[1].clone();
        panel.update_in(cx, |panel, window, cx| {
            panel.click_row(&second, Modifiers::secondary_key(), window, cx)
        });
        assert_eq!(selected(&panel, cx), ["nginx:1.27"]);
    }

    #[gpui::test]
    async fn shift_extends_over_rows_that_share_an_id(cx: &mut TestAppContext) {
        let (panel, cx) = panel_of_images(cx);
        let rows = shelf();
        panel.update_in(cx, |panel, window, cx| {
            panel.click_row(&rows[1], Modifiers::secondary_key(), window, cx);
            panel.click_row(&rows[2], Modifiers::shift(), window, cx);
        });
        assert_eq!(selected(&panel, cx), ["nginx:1.27", "redis:7"]);
        panel.update_in(cx, |panel, window, cx| {
            panel.click_row(&rows[0], Modifiers::shift(), window, cx)
        });
        assert_eq!(
            selected(&panel, cx),
            ["nginx:latest", "nginx:1.27"],
            "from the anchor at the second tag up to the first"
        );
    }

    #[gpui::test]
    async fn select_all_takes_each_tag_once(cx: &mut TestAppContext) {
        let (panel, cx) = panel_of_images(cx);
        panel.update(cx, |panel, cx| panel.select_all_rows(cx));
        assert_eq!(
            selected(&panel, cx),
            ["nginx:latest", "nginx:1.27", "redis:7"]
        );
    }

    #[gpui::test]
    async fn removing_one_tag_plans_that_row_and_not_its_siblings(cx: &mut TestAppContext) {
        let (panel, cx) = panel_of_images(cx);
        let second = shelf()[1].clone();
        let plan = panel
            .update(cx, |panel, cx| {
                panel.plan_removal_of(&[row_key(&second)], cx)
            })
            .await
            .expect("planning")
            .expect("one row to remove");
        assert_eq!(
            plan.targets()
                .iter()
                .map(|target| target.name.as_str())
                .collect::<Vec<_>>(),
            ["nginx:1.27"],
            "the other tag of the same image was not asked about"
        );
    }

    #[gpui::test]
    async fn only_the_tag_being_removed_is_busy(cx: &mut TestAppContext) {
        let (panel, cx) = panel_of_images(cx);
        let rows = shelf();
        let plan = DestructivePlan::remove(ResourceKind::Image, vec![rows[1].clone()])
            .expect("an image is removable");
        panel.update(cx, |panel, cx| {
            panel.destroy_here(plan, cx);
            assert_eq!(panel.busy_for(&row_key(&rows[1])), Some(Busy::Removing));
            assert_eq!(
                panel.busy_for(&row_key(&rows[0])),
                None,
                "its sibling keeps its buttons"
            );
        });
    }

    #[gpui::test]
    async fn two_tags_open_as_two_different_rows(cx: &mut TestAppContext) {
        let (panel, cx) = panel_of_images(cx);
        let rows = shelf();
        panel.update_in(cx, |panel, window, cx| {
            panel.open_detail(rows[0].clone(), window, cx);
            panel.open_detail(rows[1].clone(), window, cx);
        });
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                panel
                    .detail
                    .as_ref()
                    .map(|detail| detail.resource.name.as_str()),
                Some("nginx:1.27"),
                "the second tag is not mistaken for the one already open"
            )
        });
        // A list that arrives while it is open must not swap it for a sibling.
        panel.update(cx, |panel, cx| panel.reload(cx));
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                panel
                    .detail
                    .as_ref()
                    .map(|detail| detail.resource.name.as_str()),
                Some("nginx:1.27")
            )
        });
    }

    #[gpui::test]
    async fn clicking_the_second_tag_of_an_image_selects_only_it(cx: &mut TestAppContext) {
        let (panel, cx) = panel_of_images(cx);
        // An image row opens its detail on a plain click, so the selection is
        // built the way the keyboard and the modifier click build it.
        let rows = shelf();
        let bounds = |row: &Resource, cx: &mut VisualTestContext| {
            let selector: &'static str =
                Box::leak(format!("container-row:{}", row_key(row)).into_boxed_str());
            cx.debug_bounds(selector)
                .unwrap_or_else(|| panic!("the row of {} is drawn", row.name))
        };
        let (first, second) = (bounds(&rows[0], cx), bounds(&rows[1], cx));
        assert_ne!(first, second, "each tag has a row of its own");
        cx.simulate_click(
            gpui::point(second.left() + gpui::px(10.), second.center().y),
            Modifiers::secondary_key(),
        );
        draw(cx);
        assert_eq!(selected(&panel, cx), ["nginx:1.27"]);
    }
}
