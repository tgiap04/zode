//! The open row's tabs, driven through the drawn panel over a fake engine.

use std::collections::VecDeque;
use std::sync::Arc;

use container::fake_backend::FakeBackend;
use container::resource::ComposeLabels;
use container::{
    ContainerBackend, ContainerError, InspectRow, InspectSection, Inspection, Resource,
    ResourceKind, RunState, Stats,
};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use parking_lot::Mutex;

use crate::container_panel::ContainerPanel;
use crate::container_panel_tests::init_test;
use crate::detail_inspect::InspectView;
use crate::detail_stats::{STATS_HISTORY, STATS_INTERVAL};
use crate::detail_tabs::{DetailTab, tabs_for};

fn resource(kind: ResourceKind, id: &str, state: RunState) -> Resource {
    Resource {
        kind,
        id: id.into(),
        name: format!("{id}-name"),
        state,
        detail: vec![("Image", "postgres:16".into())],
        parent: None,
        compose: None,
    }
}

fn member(id: &str, project: &str) -> Resource {
    Resource {
        compose: Some(ComposeLabels {
            project: project.into(),
            service: id.into(),
            working_dir: None,
            config_files: Vec::new(),
        }),
        ..resource(ResourceKind::Container, id, RunState::Running)
    }
}

fn usage(id: &str, cpu: f64) -> Stats {
    Stats {
        id: id.into(),
        name: format!("{id}-name"),
        cpu_percent: Some(cpu),
        memory_used: Some(1024 * 1024),
        memory_limit: Some(8 * 1024 * 1024),
        ..Stats::default()
    }
}

fn inspection(secret: &str) -> Inspection {
    Inspection {
        sections: vec![InspectSection {
            title: "Environment".into(),
            rows: vec![
                InspectRow::new("DB", "app"),
                InspectRow::secret("PASSWORD", secret),
            ],
        }],
        raw_json: "{\"Env\":[\"PASSWORD=********\"]}".into(),
        raw_json_revealed: format!("{{\"Env\":[\"PASSWORD={secret}\"]}}"),
    }
}

/// The fake, with a scripted `stats` that records what it was asked.
///
/// Everything else is the fake's own. Scripted answers are consumed first, in
/// order; when the script is empty the fake answers as it normally would.
pub(crate) struct Scripted {
    inner: FakeBackend,
    /// Offers neither inspection nor stats, so a kind with no logs opens nothing.
    bare: bool,
    script: Mutex<VecDeque<Result<Option<Vec<Stats>>, ContainerError>>>,
    asked: Mutex<Vec<Vec<String>>>,
}

impl Scripted {
    pub(crate) fn bare(inner: FakeBackend) -> Arc<Self> {
        Arc::new(Self {
            bare: true,
            ..Self::unwrapped(inner)
        })
    }

    fn new(inner: FakeBackend) -> Arc<Self> {
        Arc::new(Self::unwrapped(inner))
    }

    fn unwrapped(inner: FakeBackend) -> Self {
        Self {
            bare: false,
            inner,
            script: Mutex::new(VecDeque::new()),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn then(&self, answer: Result<Option<Vec<Stats>>, ContainerError>) {
        self.script.lock().push_back(answer);
    }

    fn polls(&self) -> usize {
        self.asked.lock().len()
    }
}

#[async_trait::async_trait]
impl ContainerBackend for Scripted {
    fn kind(&self) -> container::BackendKind {
        self.inner.kind()
    }
    fn supported_kinds(&self) -> &'static [ResourceKind] {
        self.inner.supported_kinds()
    }
    fn supported_actions(&self, kind: ResourceKind) -> &'static [container::ResourceAction] {
        self.inner.supported_actions(kind)
    }
    async fn list(&self, kind: ResourceKind) -> Result<Vec<Resource>, ContainerError> {
        self.inner.list(kind).await
    }
    async fn act(
        &self,
        kind: ResourceKind,
        action: container::ResourceAction,
        id: &str,
    ) -> Result<(), ContainerError> {
        self.inner.act(kind, action, id).await
    }
    fn watch(&self) -> Option<futures::stream::BoxStream<'static, container::BackendEvent>> {
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
    async fn destroy(&self, plan: &container::DestructivePlan) -> Result<(), ContainerError> {
        self.inner.destroy(plan).await
    }
    fn exec_command(&self, kind: ResourceKind, id: &str) -> Option<(String, Vec<String>)> {
        self.inner.exec_command(kind, id)
    }
    fn supports_inspect(&self, kind: ResourceKind) -> bool {
        !self.bare && self.inner.supports_inspect(kind)
    }
    async fn inspect(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Inspection>, ContainerError> {
        self.inner.inspect(kind, id).await
    }
    fn supports_stats(&self) -> bool {
        !self.bare
    }
    async fn stats(&self, ids: &[String]) -> Result<Option<Vec<Stats>>, ContainerError> {
        self.asked.lock().push(ids.to_vec());
        let scripted = self.script.lock().pop_front();
        match scripted {
            Some(answer) => answer,
            None => self.inner.stats(ids).await,
        }
    }
    fn supports_usages(&self, kind: ResourceKind) -> bool {
        self.inner.supports_usages(kind)
    }
    async fn usages(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Vec<Resource>>, ContainerError> {
        self.inner.usages(kind, id).await
    }
}

fn panel_over(
    backend: Arc<dyn ContainerBackend>,
    cx: &mut TestAppContext,
) -> (Entity<ContainerPanel>, &mut VisualTestContext) {
    init_test(cx);
    let (panel, cx) = cx.add_window_view(|_window, cx| ContainerPanel::with_backend(backend, cx));
    cx.run_until_parked();
    (panel, cx)
}

fn open(
    panel: &Entity<ContainerPanel>,
    kind: ResourceKind,
    row: Resource,
    cx: &mut VisualTestContext,
) {
    panel.update_in(cx, |panel, window, cx| {
        panel.choose_kind(kind, cx);
        panel.open_detail(row, window, cx);
    });
    cx.run_until_parked();
}

/// One drawn frame, as the panel gets while it is on screen.
fn draw(cx: &mut VisualTestContext) {
    cx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(800.)));
    cx.run_until_parked();
}

/// One polling interval during which the tab is on screen.
fn drawn_interval(cx: &mut VisualTestContext) {
    draw(cx);
    cx.executor().advance_clock(STATS_INTERVAL);
    cx.run_until_parked();
}

fn samples(panel: &Entity<ContainerPanel>, cx: &mut VisualTestContext) -> usize {
    panel.read_with(cx, |panel, _| {
        panel
            .detail
            .as_ref()
            .map_or(0, |detail| detail.stats.history.len())
    })
}

fn polling(panel: &Entity<ContainerPanel>, cx: &mut VisualTestContext) -> bool {
    panel.read_with(cx, |panel, _| {
        panel
            .detail
            .as_ref()
            .is_some_and(|detail| detail.stats.poll.is_some())
    })
}

mod tabs {
    use super::*;

    #[test]
    fn the_tabs_are_the_backends_answers_per_kind() {
        let fake = FakeBackend::docker();
        let tabs = |kind| tabs_for(&fake, kind);
        assert_eq!(
            tabs(ResourceKind::Container),
            vec![DetailTab::Logs, DetailTab::Inspect, DetailTab::Stats]
        );
        for kind in [ResourceKind::Volume, ResourceKind::Network] {
            assert_eq!(tabs(kind), vec![DetailTab::Inspect], "{kind:?} has no logs");
        }
        assert_eq!(
            tabs(ResourceKind::Image),
            vec![DetailTab::Inspect, DetailTab::History],
            "an image has no logs, and has the layers it was built from"
        );
        assert_eq!(
            tabs(ResourceKind::ComposeProject),
            vec![DetailTab::Stats],
            "a project has no inspection and no logs of its own"
        );
    }

    #[gpui::test]
    async fn an_image_row_opens_on_inspect(cx: &mut TestAppContext) {
        let image = resource(ResourceKind::Image, "sha256:1", RunState::Unknown);
        let fake = FakeBackend::docker()
            .holding(vec![image.clone()])
            .inspecting("sha256:1", inspection("x"));
        let (panel, cx) = panel_over(Arc::new(fake), cx);

        panel.read_with(cx, |panel, _| assert!(panel.rows_open_a_detail()));
        panel.update(cx, |panel, cx| panel.choose_kind(ResourceKind::Image, cx));
        panel.read_with(cx, |panel, _| {
            assert!(
                panel.rows_open_a_detail(),
                "an image has Inspect, so its row opens"
            )
        });

        open(&panel, ResourceKind::Image, image, cx);
        panel.read_with(cx, |panel, _| {
            let detail = panel.detail.as_ref().expect("the image opened");
            assert_eq!(detail.tab, DetailTab::Inspect);
            assert!(matches!(detail.inspect.inspection, Some(Ok(_))));
        });
    }

    #[gpui::test]
    async fn a_container_opens_on_logs_and_a_tab_it_lacks_is_refused(cx: &mut TestAppContext) {
        let fake = FakeBackend::docker()
            .holding(vec![
                resource(ResourceKind::Container, "c0ffee", RunState::Running),
                resource(ResourceKind::Image, "sha256:1", RunState::Unknown),
            ])
            .inspecting("sha256:1", inspection("x"));
        let (panel, cx) = panel_over(Arc::new(fake), cx);

        open(
            &panel,
            ResourceKind::Container,
            resource(ResourceKind::Container, "c0ffee", RunState::Running),
            cx,
        );
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                panel.detail.as_ref().map(|detail| detail.tab),
                Some(DetailTab::Logs)
            );
        });

        open(
            &panel,
            ResourceKind::Image,
            resource(ResourceKind::Image, "sha256:1", RunState::Unknown),
            cx,
        );
        panel.update_in(cx, |panel, window, cx| {
            panel.select_detail_tab(DetailTab::Logs, window, cx);
            panel.select_detail_tab(DetailTab::Stats, window, cx);
        });
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                panel.detail.as_ref().map(|detail| detail.tab),
                Some(DetailTab::Inspect),
                "an image has neither Logs nor Stats"
            );
        });
    }
}

mod inspect {
    use super::*;

    fn container_on_inspect(
        cx: &mut TestAppContext,
    ) -> (Entity<ContainerPanel>, &mut VisualTestContext) {
        let (panel, cx) = panel_over(Arc::new(FakeBackend::docker()), cx);
        open(
            &panel,
            ResourceKind::Container,
            resource(ResourceKind::Container, "c0ffee", RunState::Running),
            cx,
        );
        panel.update_in(cx, |panel, window, cx| {
            panel.select_detail_tab(DetailTab::Inspect, window, cx);
        });
        cx.run_until_parked();
        draw(cx);
        (panel, cx)
    }

    fn raw_text(panel: &Entity<ContainerPanel>, cx: &mut VisualTestContext) -> String {
        panel.read_with(cx, |panel, cx| {
            let editor = panel
                .detail
                .as_ref()
                .and_then(|detail| detail.inspect.raw_editor.clone())
                .expect("the raw view has an editor");
            editor.read(cx).text(cx)
        })
    }

    #[gpui::test]
    async fn the_sections_draw_and_a_secret_starts_concealed(cx: &mut TestAppContext) {
        let (panel, cx) = container_on_inspect(cx);
        panel.read_with(cx, |panel, _| {
            let state = &panel.detail.as_ref().expect("open").inspect;
            assert!(matches!(state.inspection, Some(Ok(_))));
            assert!(state.revealed.is_empty(), "nothing is revealed on arrival");
        });
        assert!(cx.debug_bounds("container-inspect-row-1-1").is_some());
    }

    #[gpui::test]
    async fn a_reveal_is_per_row_and_a_refresh_puts_the_mask_back(cx: &mut TestAppContext) {
        let (panel, cx) = container_on_inspect(cx);
        panel.update(cx, |panel, cx| panel.toggle_row_reveal(1, 1, cx));
        panel.read_with(cx, |panel, _| {
            let revealed = &panel.detail.as_ref().expect("open").inspect.revealed;
            assert!(revealed.contains(&(1, 1)) && revealed.len() == 1);
        });

        panel.update_in(cx, |panel, window, cx| panel.load_inspect(window, cx));
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert!(
                panel
                    .detail
                    .as_ref()
                    .expect("open")
                    .inspect
                    .revealed
                    .is_empty(),
                "reveals are not kept across a refresh"
            );
        });
    }

    #[gpui::test]
    async fn the_raw_json_is_masked_until_show_secrets_and_is_masked_again_after(
        cx: &mut TestAppContext,
    ) {
        let (panel, cx) = container_on_inspect(cx);
        panel.update_in(cx, |panel, window, cx| {
            panel.set_inspect_view(InspectView::RawJson, window, cx)
        });
        let masked = raw_text(&panel, cx);
        assert!(masked.contains("********") && !masked.contains("fake-not-real"));

        panel.update_in(cx, |panel, window, cx| {
            panel.toggle_show_secrets(window, cx)
        });
        assert!(raw_text(&panel, cx).contains("fake-not-real"));

        panel.update_in(cx, |panel, window, cx| {
            panel.toggle_show_secrets(window, cx)
        });
        assert!(!raw_text(&panel, cx).contains("fake-not-real"));

        panel.update_in(cx, |panel, window, cx| {
            panel.toggle_show_secrets(window, cx);
            panel.load_inspect(window, cx);
        });
        cx.run_until_parked();
        assert!(
            !raw_text(&panel, cx).contains("fake-not-real"),
            "a refresh conceals the JSON again"
        );
        panel.read_with(cx, |panel, _| {
            assert!(!panel.detail.as_ref().expect("open").inspect.show_secrets);
        });
    }

    #[gpui::test]
    async fn a_failed_inspection_is_shown_inline_with_a_retry(cx: &mut TestAppContext) {
        let image = resource(ResourceKind::Image, "unknown", RunState::Unknown);
        let fake = FakeBackend::docker().holding(vec![image.clone()]);
        let (panel, cx) = panel_over(Arc::new(fake), cx);
        open(&panel, ResourceKind::Image, image, cx);
        draw(cx);
        panel.read_with(cx, |panel, _| {
            let state = &panel.detail.as_ref().expect("open").inspect;
            assert!(matches!(state.inspection, Some(Err(_))));
        });
        assert!(cx.debug_bounds("container-inspect-error").is_some());
    }

    #[gpui::test]
    async fn used_by_lists_the_users_and_a_click_opens_one(cx: &mut TestAppContext) {
        let volume = resource(ResourceKind::Volume, "data", RunState::Unknown);
        let fake = FakeBackend::docker()
            .holding(vec![
                resource(ResourceKind::Container, "c0ffee", RunState::Running),
                volume.clone(),
            ])
            .inspecting("data", inspection("x"))
            .used_by(
                "data",
                vec![resource(
                    ResourceKind::Container,
                    "c0ffee",
                    RunState::Running,
                )],
            );
        let (panel, cx) = panel_over(Arc::new(fake), cx);
        open(&panel, ResourceKind::Volume, volume, cx);
        draw(cx);
        let users = panel.read_with(cx, |panel, _| {
            match &panel.detail.as_ref().expect("open").inspect.used_by {
                Some(Ok(rows)) => rows.clone(),
                other => panic!("expected the users, got {other:?}"),
            }
        });
        assert_eq!(users.len(), 1);
        assert!(cx.debug_bounds("container-inspect-used-by").is_some());

        panel.update_in(cx, |panel, window, cx| {
            panel.open_user_of_detail(users[0].clone(), window, cx)
        });
        cx.run_until_parked();
        panel.read_with(cx, |panel, _| {
            assert_eq!(panel.active_kind, ResourceKind::Container);
            assert_eq!(
                panel
                    .detail
                    .as_ref()
                    .map(|detail| detail.resource.id.as_str()),
                Some("c0ffee")
            );
        });
    }
}

mod stats {
    use super::*;

    fn docker_with_usage() -> Arc<Scripted> {
        Scripted::new(FakeBackend::docker().with_stats(vec![usage("c0ffee", 12.0)]))
    }

    fn open_stats(
        backend: Arc<Scripted>,
        cx: &mut TestAppContext,
    ) -> (Entity<ContainerPanel>, &mut VisualTestContext) {
        let (panel, cx) = panel_over(backend, cx);
        open(
            &panel,
            ResourceKind::Container,
            resource(ResourceKind::Container, "c0ffee", RunState::Running),
            cx,
        );
        panel.update_in(cx, |panel, window, cx| {
            panel.select_detail_tab(DetailTab::Stats, window, cx);
        });
        cx.run_until_parked();
        (panel, cx)
    }

    #[gpui::test]
    async fn polling_starts_with_the_tab_and_keeps_sampling_while_it_is_drawn(
        cx: &mut TestAppContext,
    ) {
        let backend = docker_with_usage();
        let (panel, cx) = open_stats(backend.clone(), cx);
        assert_eq!(backend.polls(), 1, "the first sample is not delayed");
        for _ in 0..3 {
            drawn_interval(cx);
        }
        assert_eq!(backend.polls(), 4);
        assert_eq!(samples(&panel, cx), 4);
        assert!(polling(&panel, cx));
        assert!(cx.debug_bounds("container-stats-card-CPU").is_some());
    }

    /// A window that can stop drawing the panel, the way a pane does when another
    /// tab comes forward.
    struct Host {
        panel: Entity<ContainerPanel>,
        shown: bool,
    }

    impl gpui::Render for Host {
        fn render(
            &mut self,
            _window: &mut gpui::Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::prelude::*;
            gpui::div()
                .size_full()
                .when(self.shown, |element| element.child(self.panel.clone()))
        }
    }

    #[gpui::test]
    async fn polling_stops_when_nothing_draws_the_tab_and_resumes_when_it_does(
        cx: &mut TestAppContext,
    ) {
        init_test(cx);
        let backend = docker_with_usage();
        let engine: Arc<dyn ContainerBackend> = backend.clone();
        let (host, cx) = cx.add_window_view(|_window, cx| Host {
            panel: cx.new(|cx| ContainerPanel::with_backend(engine, cx)),
            shown: true,
        });
        let panel = host.read_with(cx, |host, _| host.panel.clone());
        cx.run_until_parked();
        open(
            &panel,
            ResourceKind::Container,
            resource(ResourceKind::Container, "c0ffee", RunState::Running),
            cx,
        );
        panel.update_in(cx, |panel, window, cx| {
            panel.select_detail_tab(DetailTab::Stats, window, cx);
        });
        cx.run_until_parked();
        drawn_interval(cx);
        assert!(polling(&panel, cx));

        host.update(cx, |host, cx| {
            host.shown = false;
            cx.notify();
        });
        cx.run_until_parked();
        for _ in 0..4 {
            cx.executor().advance_clock(STATS_INTERVAL);
            cx.run_until_parked();
        }
        let polled = backend.polls();
        assert!(!polling(&panel, cx), "an undrawn tab must not keep polling");
        for _ in 0..3 {
            cx.executor().advance_clock(STATS_INTERVAL);
            cx.run_until_parked();
        }
        assert_eq!(backend.polls(), polled, "no command runs while hidden");

        host.update(cx, |host, cx| {
            host.shown = true;
            cx.notify();
        });
        cx.run_until_parked();
        assert!(polling(&panel, cx), "drawing the tab starts it again");
        assert!(backend.polls() > polled);
    }

    #[gpui::test]
    async fn deactivating_the_tab_stops_the_polling_at_once(cx: &mut TestAppContext) {
        use workspace::item::Item as _;
        let (panel, cx) = open_stats(docker_with_usage(), cx);
        assert!(polling(&panel, cx));
        panel.update_in(cx, |panel, window, cx| panel.deactivated(window, cx));
        assert!(!polling(&panel, cx));
    }

    #[gpui::test]
    async fn leaving_the_tab_or_closing_the_row_drops_the_polling(cx: &mut TestAppContext) {
        let backend = docker_with_usage();
        let (panel, cx) = open_stats(backend.clone(), cx);
        panel.update_in(cx, |panel, window, cx| {
            panel.select_detail_tab(DetailTab::Inspect, window, cx);
        });
        assert!(!polling(&panel, cx), "another tab is not sampled for");

        panel.update_in(cx, |panel, window, cx| {
            panel.select_detail_tab(DetailTab::Stats, window, cx);
        });
        cx.run_until_parked();
        assert!(polling(&panel, cx));

        panel.update(cx, |panel, cx| panel.close_detail(cx));
        let polled = backend.polls();
        for _ in 0..3 {
            drawn_interval(cx);
        }
        assert_eq!(backend.polls(), polled, "a closed row is not sampled for");
    }

    #[gpui::test]
    async fn a_failed_poll_marks_the_numbers_stale_and_polling_goes_on(cx: &mut TestAppContext) {
        let backend = docker_with_usage();
        backend.then(Ok(Some(vec![usage("c0ffee", 40.0)])));
        backend.then(Err(ContainerError::CommandFailed {
            program: "docker".into(),
            stderr: "no such container".into(),
        }));
        let (panel, cx) = open_stats(backend, cx);
        let latest_cpu = |panel: &Entity<ContainerPanel>, cx: &mut VisualTestContext| {
            panel.read_with(cx, |panel, _| {
                let state = &panel.detail.as_ref().expect("open").stats;
                (
                    state.stale.clone(),
                    state.latest.first().and_then(|s| s.cpu_percent),
                )
            })
        };
        assert_eq!(latest_cpu(&panel, cx), (None, Some(40.0)));

        drawn_interval(cx);
        let (stale, cpu) = latest_cpu(&panel, cx);
        assert!(stale.is_some_and(|reason| reason.contains("no such container")));
        assert_eq!(cpu, Some(40.0), "the last good numbers stay on screen");
        assert!(polling(&panel, cx));
        draw(cx);
        assert!(cx.debug_bounds("container-stats-stale").is_some());

        drawn_interval(cx);
        assert_eq!(
            latest_cpu(&panel, cx),
            (None, Some(12.0)),
            "a good poll clears it"
        );
    }

    fn refusal() -> ContainerError {
        ContainerError::CommandFailed {
            program: "docker".into(),
            stderr: "no such container".into(),
        }
    }

    /// A container that was removed answers every poll with an error. Asking
    /// every two seconds for as long as the tab stays open is a process spawned
    /// at that rate for nothing.
    #[gpui::test]
    async fn after_three_failures_in_a_row_polling_slows_down_and_a_good_answer_ends_it(
        cx: &mut TestAppContext,
    ) {
        let backend = docker_with_usage();
        for _ in 0..4 {
            backend.then(Err(refusal()));
        }
        let (panel, cx) = open_stats(backend.clone(), cx);
        drawn_interval(cx);
        drawn_interval(cx);
        assert_eq!(backend.polls(), 3, "the first three come at the usual pace");

        drawn_interval(cx);
        assert_eq!(
            backend.polls(),
            3,
            "after three failures the next try is not two seconds away"
        );
        draw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(10) - STATS_INTERVAL);
        cx.run_until_parked();
        assert_eq!(backend.polls(), 4, "but it is still tried, ten seconds on");
        assert!(polling(&panel, cx));

        // That fourth answer was the last failure scripted; the next is good.
        draw(cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(10));
        cx.run_until_parked();
        assert_eq!(backend.polls(), 5);
        drawn_interval(cx);
        assert_eq!(
            backend.polls(),
            6,
            "one good answer and it is back to the usual pace"
        );
    }

    #[gpui::test]
    async fn a_row_that_left_the_list_is_not_sampled_for_any_more(cx: &mut TestAppContext) {
        let backend = docker_with_usage();
        let (panel, cx) = panel_over(backend.clone(), cx);
        open(
            &panel,
            ResourceKind::Container,
            resource(ResourceKind::Container, "ghost", RunState::Running),
            cx,
        );
        panel.update_in(cx, |panel, window, cx| {
            panel.select_detail_tab(DetailTab::Stats, window, cx)
        });
        cx.run_until_parked();
        assert!(polling(&panel, cx));

        // The list that arrives has no such row.
        panel.update(cx, |panel, cx| panel.reload(cx));
        cx.run_until_parked();
        assert!(!polling(&panel, cx));
        panel.read_with(cx, |panel, _| assert!(panel.detail.is_none()));
        let polled = backend.polls();
        for _ in 0..3 {
            drawn_interval(cx);
        }
        assert_eq!(backend.polls(), polled);
    }

    #[gpui::test]
    async fn an_engine_without_metrics_says_so_and_stops_asking(cx: &mut TestAppContext) {
        let backend = docker_with_usage();
        backend.then(Ok(None));
        let (panel, cx) = open_stats(backend.clone(), cx);
        for _ in 0..3 {
            drawn_interval(cx);
        }
        assert_eq!(backend.polls(), 1);
        assert!(!polling(&panel, cx));
        panel.read_with(cx, |panel, _| {
            assert!(panel.detail.as_ref().expect("open").stats.unavailable);
        });
    }

    #[gpui::test]
    async fn the_history_is_a_capped_ring_under_real_polling(cx: &mut TestAppContext) {
        let backend = docker_with_usage();
        let (panel, cx) = open_stats(backend, cx);
        for _ in 0..(STATS_HISTORY + 5) {
            drawn_interval(cx);
        }
        assert_eq!(samples(&panel, cx), STATS_HISTORY);
    }

    #[gpui::test]
    async fn a_compose_project_samples_only_its_own_containers(cx: &mut TestAppContext) {
        // A project's name is its id, as the engine lists it.
        let project = Resource {
            name: "shop".into(),
            ..resource(ResourceKind::ComposeProject, "shop", RunState::Running)
        };
        let fake = FakeBackend::docker()
            .holding(vec![
                member("web-1", "shop"),
                member("db-1", "shop"),
                member("other-1", "elsewhere"),
                project.clone(),
            ])
            .with_stats(vec![
                usage("web-1", 5.0),
                usage("db-1", 9.0),
                usage("other-1", 1.0),
            ]);
        let backend = Scripted::new(fake);
        let (panel, cx) = panel_over(backend.clone(), cx);
        open(&panel, ResourceKind::ComposeProject, project, cx);
        draw(cx);

        let mut asked = backend.asked.lock().first().cloned().expect("it polled");
        asked.sort();
        assert_eq!(asked, vec!["db-1".to_string(), "web-1".to_string()]);
        panel.read_with(cx, |panel, _| {
            let detail = panel.detail.as_ref().expect("open");
            assert_eq!(detail.tab, DetailTab::Stats);
            let names: Vec<&str> = detail
                .stats
                .latest
                .iter()
                .map(|s| s.name.as_str())
                .collect();
            assert_eq!(names, vec!["db-1-name", "web-1-name"], "sorted by name");
        });
        assert!(cx.debug_bounds("container-stats-table").is_some());
    }
}
