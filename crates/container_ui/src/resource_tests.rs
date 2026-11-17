//! Images, volumes and networks: the dialogs that create and change them, the
//! image's History tab, and a removal that says what still depends on it --
//! driven through the drawn panel over the in-process fake engine.
//!
//! Nothing here runs a docker command. The fake writes down every request it
//! was asked, so "the dialog sent exactly this" is read off its list, and a
//! small wrapper over it counts reads of the list and can refuse a write.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use container::backend::{BackendEvent, ContainerError};
use container::fake_backend::{FakeBackend, Request};
use container::{
    ContainerBackend, DestructivePlan, ImageLayer, Op, PortMapping, PortProtocol, Resource,
    ResourceAction, ResourceKind, RunSpec, RunState, VolumeMount,
};
use futures::channel::oneshot;
use futures::stream::BoxStream;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use workspace::Workspace;

use crate::confirm_modal::ConfirmDestructive;
use crate::container_panel::ContainerPanel;
use crate::container_panel_tests::{init_test, tab_in_workspace};
use crate::detail_tabs::DetailTab;
use crate::network_membership::{Membership, NetworkMembership};
use crate::resource_modals::FormModal;
use crate::row_menu::MenuItem;
use crate::run_container::RunContainer;
use crate::terminal::{TerminalIntent, spawn_in_terminal};

fn resource(kind: ResourceKind, id: &str, name: &str) -> Resource {
    Resource {
        kind,
        id: id.into(),
        name: name.into(),
        state: if kind == ResourceKind::Container {
            RunState::Running
        } else {
            RunState::Unknown
        },
        detail: Vec::new(),
        parent: None,
        compose: None,
    }
}

fn image(name: &str) -> Resource {
    resource(ResourceKind::Image, &format!("sha-{name}"), name)
}

fn volume(name: &str) -> Resource {
    resource(ResourceKind::Volume, name, name)
}

fn network(name: &str) -> Resource {
    resource(ResourceKind::Network, &format!("net-{name}"), name)
}

fn web() -> Resource {
    resource(ResourceKind::Container, "id-web", "web")
}

fn db() -> Resource {
    resource(ResourceKind::Container, "id-db", "db")
}

/// The fake, plus a count of how often the list was read and a refusal that can
/// be switched on for every write, so a test can tell "stays in the dialog" from
/// "refreshes the list".
struct Probe {
    inner: Arc<FakeBackend>,
    lists: AtomicUsize,
    refusal: Option<&'static str>,
    failing_usages: bool,
    /// What forcing a removal of a kind does, for the kinds the engine says it
    /// does something for.
    force_effects: Vec<(ResourceKind, &'static str)>,
    /// Holds the next write until the test lets it through, so a dialog can be
    /// closed while the engine is still working.
    write_gate: std::sync::Mutex<Option<oneshot::Receiver<()>>>,
}

impl Probe {
    fn over(inner: Arc<FakeBackend>) -> Self {
        Self {
            inner,
            lists: AtomicUsize::new(0),
            refusal: None,
            failing_usages: false,
            force_effects: Vec::new(),
            write_gate: std::sync::Mutex::new(None),
        }
    }

    /// Holds the next write until the returned sender fires.
    fn hold_next_write(&self) -> oneshot::Sender<()> {
        let (release, wait) = oneshot::channel();
        if let Ok(mut gate) = self.write_gate.lock() {
            *gate = Some(wait);
        }
        release
    }

    async fn gate(&self) {
        let held = self.write_gate.lock().ok().and_then(|mut gate| gate.take());
        if let Some(wait) = held {
            wait.await.ok();
        }
    }

    fn forcing(mut self, kind: ResourceKind, effect: &'static str) -> Self {
        self.force_effects.push((kind, effect));
        self
    }

    fn refusing(mut self, words: &'static str) -> Self {
        self.refusal = Some(words);
        self
    }

    fn failing_usages(mut self) -> Self {
        self.failing_usages = true;
        self
    }

    fn write(&self) -> Result<(), ContainerError> {
        match self.refusal {
            Some(stderr) => Err(ContainerError::CommandFailed {
                program: "fake".into(),
                stderr: stderr.into(),
            }),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl ContainerBackend for Probe {
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
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.inner.list(kind).await
    }

    async fn act(
        &self,
        kind: ResourceKind,
        action: ResourceAction,
        id: &str,
    ) -> Result<(), ContainerError> {
        self.inner.act(kind, action, id).await
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

    fn force_effect(&self, kind: ResourceKind) -> Option<String> {
        self.force_effects
            .iter()
            .find(|(forced, _)| *forced == kind)
            .map(|(_, effect)| effect.to_string())
    }

    fn supports_usages(&self, kind: ResourceKind) -> bool {
        self.inner.supports_usages(kind)
    }

    async fn usages(
        &self,
        kind: ResourceKind,
        id: &str,
    ) -> Result<Option<Vec<Resource>>, ContainerError> {
        if self.failing_usages {
            return Err(ContainerError::NotAuthorized {
                detail: "usages refused".into(),
            });
        }
        self.inner.usages(kind, id).await
    }

    fn offers(&self, op: Op) -> bool {
        self.inner.offers(op)
    }

    fn pull_command(&self, reference: &str) -> Option<(String, Vec<String>)> {
        self.inner.pull_command(reference)
    }

    async fn tag(&self, source: &str, target: &str) -> Result<(), ContainerError> {
        self.write()?;
        self.inner.tag(source, target).await
    }

    async fn history(&self, id: &str) -> Result<Vec<ImageLayer>, ContainerError> {
        self.inner.history(id).await
    }

    fn run_command(&self, spec: &RunSpec) -> Result<(String, Vec<String>), ContainerError> {
        self.inner.run_command(spec)
    }

    async fn create_volume(
        &self,
        name: &str,
        driver: Option<&str>,
        labels: &[(String, String)],
    ) -> Result<(), ContainerError> {
        self.gate().await;
        self.write()?;
        self.inner.create_volume(name, driver, labels).await
    }

    async fn create_network(
        &self,
        name: &str,
        driver: Option<&str>,
        subnet: Option<&str>,
    ) -> Result<(), ContainerError> {
        self.write()?;
        self.inner.create_network(name, driver, subnet).await
    }

    async fn connect(
        &self,
        network: &str,
        container: &str,
        alias: Option<&str>,
    ) -> Result<(), ContainerError> {
        self.gate().await;
        self.write()?;
        self.inner.connect(network, container, alias).await
    }

    async fn disconnect(&self, network: &str, container: &str) -> Result<(), ContainerError> {
        self.gate().await;
        self.write()?;
        self.inner.disconnect(network, container).await
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
}

fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

fn press(selector: &str, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(leak(selector.to_string()))
        .unwrap_or_else(|| panic!("{selector} must be drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(leak(selector.to_string())).is_some()
}

async fn workspace_over(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    let fs = fs::FakeFs::new(cx.executor());
    let project = project::Project::test(fs, [], cx).await;
    let (multi_workspace, cx) =
        cx.add_window_view(|window, cx| workspace::MultiWorkspace::test_new(project, window, cx));
    let workspace = multi_workspace.read_with(cx, |multi, _| multi.workspace().clone());
    (workspace, cx)
}

/// A tab in a workspace over `backend`, on `kind`, drawn.
async fn panel_on(
    backend: Arc<dyn ContainerBackend>,
    kind: ResourceKind,
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<ContainerPanel>,
    &mut VisualTestContext,
) {
    init_test(cx);
    let (workspace, cx) = workspace_over(cx).await;
    let panel = tab_in_workspace(&workspace, vec![backend], cx);
    panel.update(cx, |panel, cx| {
        panel.choose_kind(kind, cx);
        // Construction is silent, and choosing the kind already on screen
        // asks nothing.
        panel.reload(cx);
    });
    draw(cx);
    (workspace, panel, cx)
}

fn the_modal<T: workspace::ModalView>(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Option<Entity<T>> {
    workspace.read_with(cx, |workspace, cx| workspace.active_modal::<T>(cx))
}

fn requests(fake: &FakeBackend) -> Vec<Request> {
    fake.requests()
}

fn lists(probe: &Arc<Probe>) -> usize {
    probe.lists.load(Ordering::SeqCst)
}

mod pull {
    use super::*;

    fn pull_requests(fake: &FakeBackend) -> Vec<String> {
        requests(fake)
            .into_iter()
            .filter_map(|request| match request {
                Request::PullCommand(reference) => Some(reference),
                _ => None,
            })
            .collect()
    }

    #[gpui::test]
    async fn the_header_button_opens_the_pull_dialog_and_a_bad_reference_goes_nowhere(
        cx: &mut TestAppContext,
    ) {
        let fake = Arc::new(FakeBackend::docker().holding(vec![image("nginx:1")]));
        let (workspace, _panel, cx) = panel_on(fake.clone(), ResourceKind::Image, cx).await;
        press("container-create", cx);
        let modal = the_modal::<FormModal>(&workspace, cx).expect("the pull dialog is open");

        for bad in ["--rm", "has space"] {
            modal.update_in(cx, |modal, window, cx| {
                modal.type_into(0, bad, window, cx);
                modal.submit(window, cx);
            });
            draw(cx);
            assert!(
                modal
                    .read_with(cx, |modal, cx| modal.problem_of(0, cx))
                    .is_some(),
                "{bad:?} says what is wrong beside the field"
            );
        }
        assert!(
            pull_requests(&fake).is_empty(),
            "a value that fails the form's own check never reaches the backend"
        );
        assert!(the_modal::<FormModal>(&workspace, cx).is_some());

        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(0, "nginx:1.27", window, cx);
            modal.submit(window, cx);
        });
        draw(cx);
        assert_eq!(pull_requests(&fake), ["nginx:1.27"]);
        assert!(
            the_modal::<FormModal>(&workspace, cx).is_none(),
            "a pull that started closes the dialog: its progress is in the terminal"
        );
    }

    #[gpui::test]
    async fn an_empty_reference_is_reported_only_once_somebody_submits(cx: &mut TestAppContext) {
        let fake = Arc::new(FakeBackend::docker());
        let (workspace, _panel, cx) = panel_on(fake.clone(), ResourceKind::Image, cx).await;
        press("container-create", cx);
        let modal = the_modal::<FormModal>(&workspace, cx).expect("open");
        modal.read_with(cx, |modal, cx| assert_eq!(modal.problem_of(0, cx), None));
        modal.update_in(cx, |modal, window, cx| modal.submit(window, cx));
        draw(cx);
        modal.read_with(cx, |modal, cx| {
            assert_eq!(modal.problem_of(0, cx).as_deref(), Some("Required"))
        });
        assert!(pull_requests(&fake).is_empty());
    }

    #[gpui::test]
    async fn an_engine_that_cannot_pull_has_no_button_and_no_menu_entry(cx: &mut TestAppContext) {
        let fake = Arc::new(FakeBackend::docker().not_offering(Op::ImagePull));
        let (_workspace, panel, cx) = panel_on(fake, ResourceKind::Image, cx).await;
        assert!(!drawn("container-create", cx));
        panel.read_with(cx, |panel, _| assert!(panel.creation().is_none()));
    }

    #[gpui::test]
    async fn the_button_follows_the_kind_on_screen(cx: &mut TestAppContext) {
        let fake = Arc::new(FakeBackend::docker());
        let (_workspace, panel, cx) = panel_on(fake, ResourceKind::Container, cx).await;
        assert!(!drawn("container-create", cx), "nothing to create here");
        for (kind, op) in [
            (ResourceKind::Image, Op::ImagePull),
            (ResourceKind::Volume, Op::VolumeCreate),
            (ResourceKind::Network, Op::NetworkCreate),
        ] {
            panel.update(cx, |panel, cx| panel.choose_kind(kind, cx));
            draw(cx);
            assert!(drawn("container-create", cx), "{kind:?}");
            panel.read_with(cx, |panel, _| {
                assert_eq!(panel.creation().map(|(op, _)| op), Some(op))
            });
        }
    }

    #[test]
    fn a_pull_and_a_run_are_terminals_of_their_own_per_name() {
        let args = vec!["run".to_string(), "--".to_string(), "nginx:1".to_string()];
        let task = spawn_in_terminal(
            TerminalIntent::Run,
            "docker",
            args.clone(),
            "web",
            ResourceKind::Container,
        );
        assert_eq!(task.label, "run: web");
        assert_eq!(task.command.as_deref(), Some("docker"));
        assert_eq!(task.args, args, "the words stay separate");
        let pull = spawn_in_terminal(
            TerminalIntent::Pull,
            "docker",
            vec!["pull".into()],
            "nginx:1",
            ResourceKind::Image,
        );
        assert_ne!(pull.id, task.id);
        assert_eq!(pull.label, "pull: nginx:1");
    }

    /// A terminal tab's label and the command line it prints are text on a
    /// screen that may be shared. The arguments that actually run are not text
    /// to read but words to execute, and stay exactly as built.
    #[test]
    fn a_terminal_task_masks_secrets_in_what_it_shows_and_not_in_what_it_runs() {
        let args: Vec<String> = [
            "run",
            "--env",
            "DB_PASSWORD=hunter2",
            "--api-key",
            "key-3",
            "nginx:1",
            "curl",
            "https://user:pw-4@host/x",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let task = spawn_in_terminal(
            TerminalIntent::Run,
            "docker",
            args.clone(),
            "web",
            ResourceKind::Container,
        );
        for leaked in ["hunter2", "key-3", "pw-4"] {
            assert!(
                !task.command_label.contains(leaked),
                "{leaked} is on the printed command line: {}",
                task.command_label
            );
            assert!(!task.label.contains(leaked) && !task.full_label.contains(leaked));
        }
        assert!(
            task.command_label.contains("DB_PASSWORD="),
            "it still reads as the command: {}",
            task.command_label
        );
        assert_eq!(task.args, args, "what runs is what was built");
    }
}

mod run {
    use super::*;

    async fn open_form(
        cx: &mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Arc<FakeBackend>,
        Entity<RunContainer>,
        &mut VisualTestContext,
    ) {
        let fake = Arc::new(FakeBackend::docker().holding(vec![
            image("nginx:1"),
            network("backend"),
            network("bad net"),
        ]));
        let (workspace, panel, cx) = panel_on(fake.clone(), ResourceKind::Image, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.run_menu_item(&MenuItem::RunImage, &[image("nginx:1")], window, cx)
        });
        draw(cx);
        let form = the_modal::<RunContainer>(&workspace, cx).expect("the run form is open");
        (workspace, fake, form, cx)
    }

    fn filled(form: &Entity<RunContainer>, cx: &mut VisualTestContext) {
        form.update_in(cx, |form, window, cx| {
            form.type_name("web-1", window, cx);
            form.add_port(window, cx);
            form.type_port(0, "8080", "80", window, cx);
            form.add_variable(window, cx);
            form.type_variable(0, "MODE", "value", window, cx);
            form.add_mount(window, cx);
            form.type_mount(0, "pgdata", "/data", window, cx);
            form.set_mount_read_only(0, true);
            form.choose_network(Some("backend".into()), cx);
            form.type_command("sh -c \"echo hi there\"", window, cx);
        });
        draw(cx);
    }

    fn last_run(fake: &FakeBackend) -> Option<RunSpec> {
        requests(fake)
            .into_iter()
            .rev()
            .find_map(|request| match request {
                Request::Run(spec) => Some(spec),
                _ => None,
            })
    }

    #[gpui::test]
    async fn the_preview_is_exactly_the_argument_list_the_backend_returns(cx: &mut TestAppContext) {
        let (_workspace, fake, form, cx) = open_form(cx).await;
        filled(&form, cx);
        let line = form
            .update(cx, |form, cx| form.previewed_line(cx))
            .expect("a command to show");
        assert_eq!(
            line,
            "fake run --detach --name web-1 --network backend --publish 8080:80/tcp \
             --env MODE=value --volume pgdata:/data:ro nginx:1 sh -c \"echo hi there\"",
            "the image is the row's own reference"
        );
        assert!(drawn("container-run-preview", cx));
        assert!(!requests(&fake).is_empty(), "the backend was asked");
    }

    #[gpui::test]
    async fn the_preview_masks_a_secret_variable_and_the_spec_that_runs_keeps_it(
        cx: &mut TestAppContext,
    ) {
        let (_workspace, fake, form, cx) = open_form(cx).await;
        form.update_in(cx, |form, window, cx| {
            form.add_variable(window, cx);
            form.type_variable(0, "DB_PASSWORD", "hunter2", window, cx);
            form.add_variable(window, cx);
            form.type_variable(1, "REGION", "eu", window, cx);
        });
        draw(cx);
        let line = form
            .update(cx, |form, cx| form.previewed_line(cx))
            .expect("a command to show");
        assert!(
            !line.contains("hunter2"),
            "the preview is on screen for anyone looking: {line}"
        );
        assert!(line.contains("DB_PASSWORD=********"), "{line}");
        assert!(line.contains("REGION=eu"), "a plain value is shown: {line}");

        press("container-run-submit", cx);
        assert_eq!(
            last_run(&fake).map(|spec| spec.env),
            Some(vec![
                ("DB_PASSWORD".to_string(), "hunter2".to_string()),
                ("REGION".to_string(), "eu".to_string()),
            ]),
            "what is run is what was typed"
        );
    }

    #[gpui::test]
    async fn the_preview_follows_every_change(cx: &mut TestAppContext) {
        let (_workspace, _fake, form, cx) = open_form(cx).await;
        let first = form.update(cx, |form, cx| form.previewed_line(cx));
        form.update(cx, |form, cx| form.set_remove_on_exit(true, cx));
        let second = form.update(cx, |form, cx| form.previewed_line(cx));
        assert_ne!(first, second);
        assert!(second.expect("a line").contains("--rm"));
        form.update(cx, |form, cx| form.set_detach(false, cx));
        let third = form.update(cx, |form, cx| form.previewed_line(cx));
        assert!(!third.expect("a line").contains("--detach"));
    }

    #[gpui::test]
    async fn the_networks_to_choose_from_are_the_engines_own(cx: &mut TestAppContext) {
        let (_workspace, _fake, form, cx) = open_form(cx).await;
        form.read_with(cx, |form, _| {
            assert_eq!(form.network_names(), ["backend", "bad net"])
        });
    }

    #[gpui::test]
    async fn run_sends_the_form_as_a_spec_and_closes_the_dialog(cx: &mut TestAppContext) {
        let (workspace, fake, form, cx) = open_form(cx).await;
        filled(&form, cx);
        press("container-run-submit", cx);
        assert_eq!(
            last_run(&fake),
            Some(RunSpec {
                image: "nginx:1".into(),
                name: Some("web-1".into()),
                ports: vec![PortMapping {
                    host: 8080,
                    container: 80,
                    protocol: PortProtocol::Tcp,
                }],
                env: vec![("MODE".into(), "value".into())],
                volumes: vec![VolumeMount {
                    source: "pgdata".into(),
                    target: "/data".into(),
                    read_only: true,
                }],
                network: Some("backend".into()),
                detach: true,
                remove_on_exit: false,
                command: vec!["sh".into(), "-c".into(), "echo hi there".into()],
            })
        );
        assert!(the_modal::<RunContainer>(&workspace, cx).is_none());
    }

    #[gpui::test]
    async fn a_bad_row_marks_itself_and_nothing_reaches_the_backend(cx: &mut TestAppContext) {
        let (workspace, fake, form, cx) = open_form(cx).await;
        form.update_in(cx, |form, window, cx| {
            form.add_port(window, cx);
            form.type_port(0, "99999", "80", window, cx);
        });
        draw(cx);
        let before = requests(&fake).len();
        let line = form.update(cx, |form, cx| form.previewed_line(cx));
        assert_eq!(line, None, "no command is shown for a spec that is wrong");
        form.update_in(cx, |form, window, cx| form.submit(window, cx));
        draw(cx);
        assert_eq!(
            requests(&fake).len(),
            before,
            "the backend was not asked about a port that is not one"
        );
        assert!(the_modal::<RunContainer>(&workspace, cx).is_some());

        for (host, container) in [("0", "80"), ("80", ""), ("x", "y")] {
            form.update_in(cx, |form, window, cx| {
                form.type_port(0, host, container, window, cx)
            });
            assert_eq!(
                form.update(cx, |form, cx| form.previewed_line(cx)),
                None,
                "{host:?} -> {container:?}"
            );
        }
        form.update_in(cx, |form, window, cx| {
            form.type_port(0, "1", "65535", window, cx)
        });
        assert!(
            form.update(cx, |form, cx| form.previewed_line(cx))
                .is_some()
        );
    }

    #[gpui::test]
    async fn rows_nobody_filled_in_are_left_out_and_removing_a_row_keeps_the_rest(
        cx: &mut TestAppContext,
    ) {
        let (_workspace, _fake, form, cx) = open_form(cx).await;
        form.update_in(cx, |form, window, cx| {
            form.add_port(window, cx);
            form.add_port(window, cx);
            form.add_variable(window, cx);
            form.add_mount(window, cx);
            form.type_port(1, "9090", "90", window, cx);
        });
        let line = form
            .update(cx, |form, cx| form.previewed_line(cx))
            .expect("blank rows are not errors");
        assert!(line.contains("--publish 9090:90/tcp"), "{line}");
        assert!(
            !line.contains("--env") && !line.contains("--volume"),
            "{line}"
        );

        form.update(cx, |form, cx| form.remove_port(0, cx));
        form.read_with(cx, |form, _| assert_eq!(form.port_count(), 1));
        let line = form
            .update(cx, |form, cx| form.previewed_line(cx))
            .expect("still a command");
        assert!(line.contains("--publish 9090:90/tcp"), "{line}");
    }

    #[gpui::test]
    async fn a_value_only_the_backend_refuses_is_said_inline_in_its_words(cx: &mut TestAppContext) {
        let (workspace, fake, form, cx) = open_form(cx).await;
        form.update(cx, |form, cx| {
            form.choose_network(Some("bad net".into()), cx)
        });
        draw(cx);
        let said = form
            .update(cx, |form, cx| form.preview_refusal(cx))
            .expect("the refusal is the preview");
        assert!(
            said.contains("network") && said.contains("bad net"),
            "{said}"
        );

        let before = requests(&fake).len();
        form.update_in(cx, |form, window, cx| form.submit(window, cx));
        draw(cx);
        assert_eq!(requests(&fake).len(), before);
        form.read_with(cx, |form, _| {
            assert!(form.error().is_some_and(|error| error.contains("bad net")))
        });
        assert!(drawn("container-run-error", cx));
        assert!(the_modal::<RunContainer>(&workspace, cx).is_some());
    }

    #[gpui::test]
    async fn a_name_that_could_be_an_option_is_refused_at_once(cx: &mut TestAppContext) {
        let (_workspace, _fake, form, cx) = open_form(cx).await;
        form.update_in(cx, |form, window, cx| {
            form.type_name("--privileged", window, cx)
        });
        assert_eq!(form.update(cx, |form, cx| form.previewed_line(cx)), None);
        form.update_in(cx, |form, window, cx| {
            form.type_name("", window, cx);
            form.type_command("echo \"oops", window, cx);
        });
        assert_eq!(
            form.update(cx, |form, cx| form.previewed_line(cx)),
            None,
            "an unclosed quote is not a command"
        );
    }

    #[gpui::test]
    async fn an_engine_that_cannot_run_has_no_entry(cx: &mut TestAppContext) {
        let fake = Arc::new(
            FakeBackend::docker()
                .holding(vec![image("nginx:1")])
                .not_offering(Op::ImageRun),
        );
        let (_workspace, panel, cx) = panel_on(fake, ResourceKind::Image, cx).await;
        let items = panel.read_with(cx, |panel, cx| {
            panel
                .row_menu_plan(&image("nginx:1"), cx)
                .expect("a menu")
                .1
        });
        assert!(!items.contains(&MenuItem::RunImage));
        assert!(items.contains(&MenuItem::TagImage));
    }
}

mod tag {
    use super::*;

    #[gpui::test]
    async fn tag_sends_the_images_id_and_the_new_reference_and_refreshes(cx: &mut TestAppContext) {
        let fake = Arc::new(FakeBackend::docker().holding(vec![image("nginx:1")]));
        let probe = Arc::new(Probe::over(fake.clone()));
        let (workspace, panel, cx) = panel_on(probe.clone(), ResourceKind::Image, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.run_menu_item(&MenuItem::TagImage, &[image("nginx:1")], window, cx)
        });
        draw(cx);
        let modal = the_modal::<FormModal>(&workspace, cx).expect("the tag dialog");

        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(0, "-bad", window, cx);
            modal.submit(window, cx);
        });
        draw(cx);
        assert!(requests(&fake).is_empty(), "refused before the backend");

        let before = lists(&probe);
        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(0, "myrepo/app:1.0", window, cx);
            modal.submit(window, cx);
        });
        draw(cx);
        assert_eq!(
            requests(&fake),
            [Request::Tag {
                source: "sha-nginx:1".into(),
                target: "myrepo/app:1.0".into(),
            }]
        );
        assert!(the_modal::<FormModal>(&workspace, cx).is_none());
        assert!(lists(&probe) > before, "the list is read again");
    }

    #[gpui::test]
    async fn an_engine_refusal_stays_in_the_dialog_and_the_list_is_left_alone(
        cx: &mut TestAppContext,
    ) {
        let fake = Arc::new(FakeBackend::docker().holding(vec![image("nginx:1")]));
        let probe = Arc::new(Probe::over(fake).refusing("no such repository"));
        let (workspace, panel, cx) = panel_on(probe.clone(), ResourceKind::Image, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.run_menu_item(&MenuItem::TagImage, &[image("nginx:1")], window, cx)
        });
        draw(cx);
        let modal = the_modal::<FormModal>(&workspace, cx).expect("open");
        let before = lists(&probe);
        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(0, "myrepo/app:1.0", window, cx);
            modal.submit(window, cx);
        });
        draw(cx);
        modal.read_with(cx, |modal, _| {
            assert!(
                modal
                    .error()
                    .is_some_and(|error| error.contains("no such repository"))
            )
        });
        assert!(drawn("container-tag-error", cx));
        assert!(
            the_modal::<FormModal>(&workspace, cx).is_some(),
            "the value that was refused is still in front of the person"
        );
        assert_eq!(lists(&probe), before);

        let reloaded = panel.read_with(cx, |panel, _| panel.last_error.is_none());
        assert!(
            reloaded,
            "an engine's refusal belongs to the dialog, not the banner"
        );
    }
}

mod create {
    use super::*;

    #[gpui::test]
    async fn a_volume_is_created_with_its_name_and_optional_driver(cx: &mut TestAppContext) {
        let fake = Arc::new(FakeBackend::docker());
        let probe = Arc::new(Probe::over(fake.clone()));
        let (workspace, _panel, cx) = panel_on(probe.clone(), ResourceKind::Volume, cx).await;
        press("container-create", cx);
        let modal = the_modal::<FormModal>(&workspace, cx).expect("the volume dialog");

        modal.update_in(cx, |modal, window, cx| modal.submit(window, cx));
        draw(cx);
        modal.read_with(cx, |modal, cx| {
            assert_eq!(modal.problem_of(0, cx).as_deref(), Some("Required"))
        });
        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(0, "-v", window, cx);
            modal.type_into(1, "local", window, cx);
            modal.submit(window, cx);
        });
        draw(cx);
        assert!(
            requests(&fake).is_empty(),
            "a name cannot start with a dash"
        );

        let before = lists(&probe);
        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(0, "pgdata", window, cx);
            modal.type_into(1, "", window, cx);
            modal.submit(window, cx);
        });
        draw(cx);
        assert_eq!(
            requests(&fake),
            [Request::CreateVolume {
                name: "pgdata".into(),
                driver: None,
                labels: Vec::new(),
            }],
            "an empty driver is none, not an empty word"
        );
        assert!(the_modal::<FormModal>(&workspace, cx).is_none());
        assert!(lists(&probe) > before);
    }

    #[gpui::test]
    async fn a_network_is_created_with_name_driver_and_subnet(cx: &mut TestAppContext) {
        let fake = Arc::new(FakeBackend::docker());
        let (workspace, _panel, cx) = panel_on(fake.clone(), ResourceKind::Network, cx).await;
        press("container-create", cx);
        let modal = the_modal::<FormModal>(&workspace, cx).expect("the network dialog");

        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(0, "backend", window, cx);
            modal.type_into(2, "10.5.0.0", window, cx);
        });
        draw(cx);
        modal.read_with(cx, |modal, cx| {
            assert!(modal.problem_of(2, cx).is_some(), "a subnet has a prefix")
        });
        modal.update_in(cx, |modal, window, cx| modal.submit(window, cx));
        draw(cx);
        assert!(requests(&fake).is_empty());

        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(1, "bridge", window, cx);
            modal.type_into(2, "10.5.0.0/16", window, cx);
            modal.submit(window, cx);
        });
        draw(cx);
        assert_eq!(
            requests(&fake),
            [Request::CreateNetwork {
                name: "backend".into(),
                driver: Some("bridge".into()),
                subnet: Some("10.5.0.0/16".into()),
            }]
        );
        assert!(the_modal::<FormModal>(&workspace, cx).is_none());
    }

    #[gpui::test]
    async fn a_refused_create_stays_in_the_dialog(cx: &mut TestAppContext) {
        let fake = Arc::new(FakeBackend::docker());
        let probe =
            Arc::new(Probe::over(fake).refusing("network with name backend already exists"));
        let (workspace, _panel, cx) = panel_on(probe.clone(), ResourceKind::Network, cx).await;
        press("container-create", cx);
        let modal = the_modal::<FormModal>(&workspace, cx).expect("open");
        let before = lists(&probe);
        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(0, "backend", window, cx);
            modal.submit(window, cx);
        });
        draw(cx);
        modal.read_with(cx, |modal, _| {
            assert!(
                modal
                    .error()
                    .is_some_and(|error| error.contains("already exists"))
            )
        });
        assert!(the_modal::<FormModal>(&workspace, cx).is_some());
        assert_eq!(lists(&probe), before);
    }

    /// Opens the create-volume dialog, fills in a name and submits it with the
    /// engine held, then closes the dialog. Returns what is needed to let the
    /// engine finish.
    async fn volume_dialog_closed_mid_call(
        probe: Arc<Probe>,
        cx: &mut TestAppContext,
    ) -> (
        Entity<ContainerPanel>,
        oneshot::Sender<()>,
        &mut VisualTestContext,
    ) {
        let (workspace, panel, cx) = panel_on(probe.clone(), ResourceKind::Volume, cx).await;
        press("container-create", cx);
        let modal = the_modal::<FormModal>(&workspace, cx).expect("the volume dialog");
        let release = probe.hold_next_write();
        modal.update_in(cx, |modal, window, cx| {
            modal.type_into(0, "pgdata", window, cx);
            modal.submit(window, cx);
        });
        draw(cx);
        modal.update(cx, |_, cx| cx.emit(gpui::DismissEvent));
        draw(cx);
        assert!(
            the_modal::<FormModal>(&workspace, cx).is_none(),
            "the dialog is closed while the engine is still working"
        );
        (panel, release, cx)
    }

    #[gpui::test]
    async fn closing_the_dialog_does_not_abandon_a_call_the_engine_is_making(
        cx: &mut TestAppContext,
    ) {
        let fake = Arc::new(FakeBackend::docker());
        let probe = Arc::new(Probe::over(fake.clone()));
        let (_panel, release, cx) = volume_dialog_closed_mid_call(probe.clone(), cx).await;
        let before = lists(&probe);
        release.send(()).expect("the call is waiting");
        draw(cx);
        assert_eq!(
            requests(&fake),
            [Request::CreateVolume {
                name: "pgdata".into(),
                driver: None,
                labels: Vec::new(),
            }],
            "the volume was created after all"
        );
        assert!(lists(&probe) > before, "and the list shows it");
    }

    #[gpui::test]
    async fn a_refusal_that_arrives_after_the_dialog_closed_is_shown_on_the_panel(
        cx: &mut TestAppContext,
    ) {
        let fake = Arc::new(FakeBackend::docker());
        let probe = Arc::new(Probe::over(fake).refusing("volume pgdata already exists"));
        let (panel, release, cx) = volume_dialog_closed_mid_call(probe, cx).await;
        release.send(()).expect("the call is waiting");
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert!(
                panel
                    .last_error
                    .as_ref()
                    .is_some_and(|error| error.to_string().contains("already exists")),
                "the answer is not dropped with the dialog: {:?}",
                panel.last_error
            )
        });
    }

    #[gpui::test]
    async fn the_empty_space_menu_offers_the_creation_of_the_kind_on_screen(
        cx: &mut TestAppContext,
    ) {
        let fake = Arc::new(FakeBackend::docker());
        let (workspace, panel, cx) = panel_on(fake, ResourceKind::Volume, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.deploy_panel_menu(gpui::point(gpui::px(5.), gpui::px(5.)), window, cx)
        });
        draw(cx);
        let menu = panel
            .read_with(cx, |panel, _| {
                panel.context_menu.as_ref().map(|(menu, _, _)| menu.clone())
            })
            .expect("a menu");
        menu.update_in(cx, |menu, window, cx| {
            menu.select_first(&menu::SelectFirst, window, cx);
            menu.select_next(&menu::SelectNext, window, cx);
            menu.confirm(&menu::Confirm, window, cx);
        });
        draw(cx);
        assert!(
            the_modal::<FormModal>(&workspace, cx).is_some(),
            "the second entry is Create Volume"
        );
    }
}

mod membership {
    use super::*;

    fn world() -> Arc<FakeBackend> {
        Arc::new(
            FakeBackend::docker()
                .holding(vec![network("backend"), web(), db()])
                .used_by("net-backend", vec![db()]),
        )
    }

    async fn open(
        mode: Membership,
        backend: Arc<dyn ContainerBackend>,
        cx: &mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<NetworkMembership>,
        &mut VisualTestContext,
    ) {
        let (workspace, panel, cx) = panel_on(backend, ResourceKind::Network, cx).await;
        let item = match mode {
            Membership::Connect => MenuItem::ConnectContainer,
            Membership::Disconnect => MenuItem::DisconnectContainer,
        };
        panel.update_in(cx, |panel, window, cx| {
            panel.run_menu_item(&item, &[network("backend")], window, cx)
        });
        draw(cx);
        let modal = the_modal::<NetworkMembership>(&workspace, cx).expect("the membership dialog");
        (workspace, modal, cx)
    }

    #[gpui::test]
    async fn connect_offers_only_containers_that_are_not_on_the_network(cx: &mut TestAppContext) {
        let (_workspace, modal, cx) = open(Membership::Connect, world(), cx).await;
        modal.read_with(cx, |modal, _| {
            assert_eq!(modal.choice_names(), Some(vec!["web".to_string()]))
        });
    }

    #[gpui::test]
    async fn disconnect_offers_only_the_containers_that_are_on_it(cx: &mut TestAppContext) {
        let (_workspace, modal, cx) = open(Membership::Disconnect, world(), cx).await;
        modal.read_with(cx, |modal, _| {
            assert_eq!(modal.choice_names(), Some(vec!["db".to_string()]))
        });
    }

    #[gpui::test]
    async fn connect_needs_a_choice_and_sends_the_network_container_and_alias(
        cx: &mut TestAppContext,
    ) {
        let fake = world();
        let (workspace, modal, cx) = open(Membership::Connect, fake.clone(), cx).await;

        modal.update(cx, |modal, cx| modal.submit(cx));
        draw(cx);
        assert!(requests(&fake).is_empty(), "nothing chosen, nothing sent");
        modal.read_with(cx, |modal, _| assert!(modal.error().is_some()));

        press("container-membership-choice:0", cx);
        modal.update_in(cx, |modal, window, cx| modal.type_alias("-bad", window, cx));
        modal.update(cx, |modal, cx| modal.submit(cx));
        draw(cx);
        assert!(
            requests(&fake).is_empty(),
            "an alias cannot start with a dash"
        );

        modal.update_in(cx, |modal, window, cx| {
            modal.type_alias("cache", window, cx)
        });
        press("container-membership-submit", cx);
        assert_eq!(
            requests(&fake),
            [Request::Connect {
                network: "net-backend".into(),
                container: "id-web".into(),
                alias: Some("cache".into()),
            }]
        );
        assert!(the_modal::<NetworkMembership>(&workspace, cx).is_none());
    }

    #[gpui::test]
    async fn disconnect_names_the_container_and_the_network_and_refreshes(cx: &mut TestAppContext) {
        let probe = Arc::new(Probe::over(world()));
        let fake = probe.inner.clone();
        let (workspace, modal, cx) = open(Membership::Disconnect, probe.clone(), cx).await;
        let before = lists(&probe);
        modal.update(cx, |modal, cx| modal.choose(0, cx));
        press("container-membership-submit", cx);
        assert_eq!(
            requests(&fake),
            [Request::Disconnect {
                network: "net-backend".into(),
                container: "id-db".into(),
            }]
        );
        assert!(the_modal::<NetworkMembership>(&workspace, cx).is_none());
        assert!(lists(&probe) > before);
    }

    #[gpui::test]
    async fn a_refused_disconnect_stays_in_the_dialog(cx: &mut TestAppContext) {
        let probe = Arc::new(Probe::over(world()).refusing("container is not connected"));
        let (workspace, modal, cx) = open(Membership::Disconnect, probe.clone(), cx).await;
        let before = lists(&probe);
        modal.update(cx, |modal, cx| modal.choose(0, cx));
        modal.update(cx, |modal, cx| modal.submit(cx));
        draw(cx);
        modal.read_with(cx, |modal, _| {
            assert!(
                modal
                    .error()
                    .is_some_and(|error| error.contains("not connected"))
            )
        });
        assert!(the_modal::<NetworkMembership>(&workspace, cx).is_some());
        assert_eq!(lists(&probe), before);
    }

    #[gpui::test]
    async fn closing_the_dialog_mid_disconnect_still_finishes_and_says_what_happened(
        cx: &mut TestAppContext,
    ) {
        let probe = Arc::new(Probe::over(world()).refusing("container is not connected"));
        let (workspace, modal, cx) = open(Membership::Disconnect, probe.clone(), cx).await;
        let release = probe.hold_next_write();
        modal.update(cx, |modal, cx| modal.choose(0, cx));
        modal.update(cx, |modal, cx| modal.submit(cx));
        draw(cx);
        modal.update(cx, |_, cx| cx.emit(gpui::DismissEvent));
        draw(cx);
        assert!(the_modal::<NetworkMembership>(&workspace, cx).is_none());
        // The test's own handle is the last thing keeping the dialog alive.
        drop(modal);

        release.send(()).expect("the call is waiting");
        draw(cx);
        let said = workspace.read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<ContainerPanel>(cx)
                .next()
                .and_then(|panel| panel.read(cx).last_error.as_ref().map(ToString::to_string))
        });
        assert!(
            said.is_some_and(|said| said.contains("not connected")),
            "the refusal is on the panel, not lost with the dialog"
        );
    }

    #[gpui::test]
    async fn a_failed_lookup_is_said_in_the_dialog_and_offers_nothing(cx: &mut TestAppContext) {
        let probe = Arc::new(Probe::over(world()).failing_usages());
        let (_workspace, modal, cx) = open(Membership::Disconnect, probe, cx).await;
        modal.read_with(cx, |modal, _| {
            assert!(
                modal
                    .load_failure()
                    .is_some_and(|said| said.contains("usages refused"))
            );
            assert_eq!(modal.choice_names(), None);
        });
        assert!(drawn("container-membership-failure", cx));
    }

    #[gpui::test]
    async fn an_engine_without_networks_connecting_has_no_menu_entries(cx: &mut TestAppContext) {
        let fake = Arc::new(
            FakeBackend::docker()
                .holding(vec![network("backend")])
                .not_offering(Op::NetworkConnect),
        );
        let (_workspace, panel, cx) = panel_on(fake, ResourceKind::Network, cx).await;
        let items = panel.read_with(cx, |panel, cx| {
            panel
                .row_menu_plan(&network("backend"), cx)
                .expect("a menu")
                .1
        });
        assert!(!items.contains(&MenuItem::ConnectContainer));
        assert!(!items.contains(&MenuItem::DisconnectContainer));
        assert!(items.contains(&MenuItem::Remove));
    }

    #[gpui::test]
    async fn a_network_menu_and_an_image_menu_carry_their_own_entries(cx: &mut TestAppContext) {
        let fake =
            Arc::new(FakeBackend::docker().holding(vec![network("backend"), image("nginx:1")]));
        let (_workspace, panel, cx) = panel_on(fake, ResourceKind::Network, cx).await;
        let network_items = panel.read_with(cx, |panel, cx| {
            panel
                .row_menu_plan(&network("backend"), cx)
                .expect("a menu")
                .1
        });
        assert!(network_items.contains(&MenuItem::ConnectContainer));
        assert!(network_items.contains(&MenuItem::DisconnectContainer));
        assert!(!network_items.contains(&MenuItem::RunImage));

        panel.update(cx, |panel, cx| panel.choose_kind(ResourceKind::Image, cx));
        draw(cx);
        let image_items = panel.read_with(cx, |panel, cx| {
            panel
                .row_menu_plan(&image("nginx:1"), cx)
                .expect("a menu")
                .1
        });
        for wanted in [
            MenuItem::RunImage,
            MenuItem::TagImage,
            MenuItem::ImageHistory,
        ] {
            assert!(image_items.contains(&wanted), "{wanted:?}");
        }
        assert!(!image_items.contains(&MenuItem::ConnectContainer));
    }
}

mod history {
    use super::*;

    fn layers() -> Vec<ImageLayer> {
        vec![
            ImageLayer {
                created_by: "CMD [\"nginx\"]".into(),
                size: "0B".into(),
                created: "2 days ago".into(),
            },
            ImageLayer {
                created_by: "RUN apt-get install nginx".into(),
                size: "60MB".into(),
                created: "2 days ago".into(),
            },
        ]
    }

    #[gpui::test]
    async fn the_history_entry_opens_the_image_on_its_history_tab_and_lists_the_layers(
        cx: &mut TestAppContext,
    ) {
        let fake = Arc::new(
            FakeBackend::docker()
                .holding(vec![image("nginx:1")])
                .with_history("sha-nginx:1", layers()),
        );
        let (_workspace, panel, cx) = panel_on(fake.clone(), ResourceKind::Image, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.run_menu_item(&MenuItem::ImageHistory, &[image("nginx:1")], window, cx)
        });
        draw(cx);
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                panel.detail.as_ref().map(|detail| detail.tab),
                Some(DetailTab::History)
            )
        });
        assert!(drawn("container-history-layer-0", cx));
        assert!(drawn("container-history-layer-1", cx));
        assert_eq!(
            requests(&fake),
            [Request::History("sha-nginx:1".into())],
            "asked once, by id"
        );
    }

    fn leaky_layers() -> Vec<ImageLayer> {
        vec![
            ImageLayer {
                created_by: "/bin/sh -c #(nop)  ENV DB_PASSWORD=hunter2".into(),
                size: "0B".into(),
                created: "2 days ago".into(),
            },
            ImageLayer {
                created_by: "RUN apt-get install nginx".into(),
                size: "60MB".into(),
                created: "2 days ago".into(),
            },
        ]
    }

    async fn history_of_leaky_image(
        cx: &mut TestAppContext,
    ) -> (Entity<ContainerPanel>, &mut VisualTestContext) {
        let fake = Arc::new(
            FakeBackend::docker()
                .holding(vec![image("nginx:1")])
                .with_history("sha-nginx:1", leaky_layers()),
        );
        let (_workspace, panel, cx) = panel_on(fake, ResourceKind::Image, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_detail_on(image("nginx:1"), Some(DetailTab::History), window, cx)
        });
        draw(cx);
        (panel, cx)
    }

    fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    #[gpui::test]
    async fn a_layer_that_holds_a_secret_has_an_eye_and_one_without_has_none(
        cx: &mut TestAppContext,
    ) {
        let (_panel, cx) = history_of_leaky_image(cx).await;
        assert!(drawn("container-history-reveal-0", cx));
        assert!(
            !drawn("container-history-reveal-1", cx),
            "nothing to hide, nothing to reveal"
        );
    }

    #[gpui::test]
    async fn copying_a_layer_copies_what_the_screen_shows_and_the_eye_shows_the_rest(
        cx: &mut TestAppContext,
    ) {
        let (_panel, cx) = history_of_leaky_image(cx).await;
        assert!(drawn("container-history-copy-0", cx));
        press("container-history-copy-0", cx);
        let copied = clipboard(cx).expect("something was copied");
        assert!(
            !copied.contains("hunter2"),
            "the mask is what is on screen, so the mask is what is copied: {copied}"
        );

        assert!(drawn("container-history-reveal-0", cx));
        press("container-history-reveal-0", cx);
        press("container-history-copy-0", cx);
        assert!(
            clipboard(cx).is_some_and(|copied| copied.contains("DB_PASSWORD=hunter2")),
            "revealed, the real command is what is copied"
        );
    }

    #[gpui::test]
    async fn a_refresh_puts_every_layer_back_under_its_mask(cx: &mut TestAppContext) {
        let (panel, cx) = history_of_leaky_image(cx).await;
        assert!(drawn("container-history-reveal-0", cx));
        press("container-history-reveal-0", cx);
        panel.update_in(cx, |panel, window, cx| panel.load_history(window, cx));
        draw(cx);
        assert!(drawn("container-history-copy-0", cx));
        press("container-history-copy-0", cx);
        assert!(
            clipboard(cx).is_some_and(|copied| !copied.contains("hunter2")),
            "a reveal is not kept across a refresh"
        );
    }

    #[gpui::test]
    async fn an_image_with_other_tabs_gets_history_beside_inspect(cx: &mut TestAppContext) {
        let fake = Arc::new(
            FakeBackend::docker()
                .holding(vec![image("nginx:1")])
                .with_history("sha-nginx:1", layers()),
        );
        let (_workspace, panel, cx) = panel_on(fake, ResourceKind::Image, cx).await;
        panel.read_with(cx, |panel, _| {
            assert_eq!(
                panel.detail_tabs(),
                [DetailTab::Inspect, DetailTab::History]
            )
        });
        panel.update(cx, |panel, cx| panel.choose_kind(ResourceKind::Volume, cx));
        panel.read_with(cx, |panel, _| {
            assert!(!panel.detail_tabs().contains(&DetailTab::History))
        });
    }

    #[gpui::test]
    async fn a_failed_history_says_why_in_the_tab(cx: &mut TestAppContext) {
        let fake = Arc::new(FakeBackend::docker().holding(vec![image("nginx:1")]));
        let (_workspace, panel, cx) = panel_on(fake, ResourceKind::Image, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.open_detail_on(image("nginx:1"), Some(DetailTab::History), window, cx)
        });
        draw(cx);
        assert!(drawn("container-history-error", cx));
    }

    #[gpui::test]
    async fn an_engine_without_history_has_no_tab_and_no_entry(cx: &mut TestAppContext) {
        let fake = Arc::new(
            FakeBackend::docker()
                .holding(vec![image("nginx:1")])
                .not_offering(Op::ImageHistory),
        );
        let (_workspace, panel, cx) = panel_on(fake, ResourceKind::Image, cx).await;
        panel.read_with(cx, |panel, cx| {
            assert!(!panel.detail_tabs().contains(&DetailTab::History));
            let items = panel.row_menu_plan(&image("nginx:1"), cx).expect("menu").1;
            assert!(!items.contains(&MenuItem::ImageHistory));
        });
    }
}

mod removal {
    use super::*;

    fn world() -> Arc<FakeBackend> {
        Arc::new(
            FakeBackend::docker()
                .holding(vec![
                    volume("pgdata"),
                    volume("cache"),
                    volume("spare"),
                    web(),
                ])
                .used_by("pgdata", vec![web()])
                .used_by("cache", vec![db()]),
        )
    }

    fn the_dialog(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<ConfirmDestructive> {
        the_modal::<ConfirmDestructive>(workspace, cx).expect("the removal dialog is open")
    }

    #[gpui::test]
    async fn a_volume_in_use_lists_who_uses_it_before_anything_is_asked(cx: &mut TestAppContext) {
        let fake = world();
        let (workspace, panel, cx) = panel_on(fake.clone(), ResourceKind::Volume, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal("pgdata", window, cx)
        });
        draw(cx);
        let modal = the_dialog(&workspace, cx);
        modal.read_with(cx, |modal, _| {
            let plan = modal.plan();
            assert!(plan.has_dependents());
            assert_eq!(plan.dependents()[0].target_id, "pgdata");
            assert_eq!(plan.dependents()[0].containers[0].name, "web");
        });
        assert!(drawn("confirm-destructive-dependents", cx));
        assert!(fake.destroyed().is_empty());
    }

    const FORCING_A_VOLUME: &str = "Removes the volume even while a container still mounts it.";

    /// An engine that says forcing the removal of a volume does something.
    fn forcing(fake: &Arc<FakeBackend>) -> Arc<Probe> {
        Arc::new(Probe::over(fake.clone()).forcing(ResourceKind::Volume, FORCING_A_VOLUME))
    }

    #[gpui::test]
    async fn the_tick_is_offered_with_what_the_engine_says_forcing_does(cx: &mut TestAppContext) {
        let fake = world();
        let (workspace, panel, cx) = panel_on(forcing(&fake), ResourceKind::Volume, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal("pgdata", window, cx)
        });
        draw(cx);
        let modal = the_dialog(&workspace, cx);
        assert!(drawn("confirm-destructive-force-tick", cx));
        assert!(drawn("confirm-destructive-force-effect", cx));
        modal.read_with(cx, |modal, _| {
            assert_eq!(modal.force_effect(), Some(FORCING_A_VOLUME))
        });
    }

    /// Forcing a removal the engine would refuse just the same is a promise the
    /// dialog cannot keep: the person agrees to a forced removal and gets an
    /// error instead.
    #[gpui::test]
    async fn no_tick_is_offered_for_a_kind_where_forcing_does_nothing(cx: &mut TestAppContext) {
        let fake = world();
        let engine = Arc::new(Probe::over(fake));
        let (workspace, panel, cx) = panel_on(engine, ResourceKind::Volume, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal("pgdata", window, cx)
        });
        draw(cx);
        let modal = the_dialog(&workspace, cx);
        modal.read_with(cx, |modal, _| assert!(modal.plan().has_dependents()));
        assert!(
            drawn("confirm-destructive-dependents", cx),
            "who uses it is still said"
        );
        assert!(!drawn("confirm-destructive-force-tick", cx));
        modal.update_in(cx, |modal, window, cx| modal.set_force(true, window, cx));
        modal.read_with(cx, |modal, _| assert!(!modal.force_wanted()));
    }

    #[gpui::test]
    async fn forcing_takes_the_word_force_and_the_plan_that_leaves_is_forced(
        cx: &mut TestAppContext,
    ) {
        let fake = world();
        let (workspace, panel, cx) = panel_on(forcing(&fake), ResourceKind::Volume, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal("pgdata", window, cx)
        });
        draw(cx);
        let modal = the_dialog(&workspace, cx);

        modal.update_in(cx, |modal, window, cx| {
            modal.set_force(true, window, cx);
            modal.type_confirmation("pgdata", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);
        assert!(
            fake.destroyed().is_empty(),
            "the name is not the answer to a forced removal"
        );
        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("force", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);
        let destroyed = fake.destroyed();
        assert_eq!(destroyed.len(), 1);
        assert!(destroyed[0].is_forced());
        assert_eq!(destroyed[0].targets()[0].name, "pgdata");
    }

    #[gpui::test]
    async fn without_forcing_the_plan_that_leaves_is_the_plain_one(cx: &mut TestAppContext) {
        let fake = world();
        let (workspace, panel, cx) = panel_on(forcing(&fake), ResourceKind::Volume, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal("pgdata", window, cx)
        });
        draw(cx);
        let modal = the_dialog(&workspace, cx);
        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("force", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);
        assert!(fake.destroyed().is_empty(), "force is not the name");
        modal.update_in(cx, |modal, window, cx| {
            modal.type_confirmation("pgdata", window, cx);
            modal.press_remove(window, cx);
        });
        draw(cx);
        assert!(!fake.destroyed()[0].is_forced());
    }

    #[gpui::test]
    async fn a_volume_nobody_uses_offers_no_force(cx: &mut TestAppContext) {
        let fake = world();
        let (workspace, panel, cx) = panel_on(forcing(&fake), ResourceKind::Volume, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal("spare", window, cx)
        });
        draw(cx);
        let modal = the_dialog(&workspace, cx);
        assert!(!drawn("confirm-destructive-dependents", cx));
        modal.update_in(cx, |modal, window, cx| modal.set_force(true, window, cx));
        modal.read_with(cx, |modal, _| assert!(!modal.force_wanted()));
    }

    #[gpui::test]
    async fn a_bulk_removal_carries_the_dependents_of_every_row(cx: &mut TestAppContext) {
        let fake = world();
        let (workspace, panel, cx) = panel_on(fake, ResourceKind::Volume, cx).await;
        let ids = vec![
            "pgdata".to_string(),
            "cache".to_string(),
            "spare".to_string(),
        ];
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal_of(ids, window, cx)
        });
        draw(cx);
        let modal = the_dialog(&workspace, cx);
        modal.read_with(cx, |modal, _| {
            let plan = modal.plan();
            assert_eq!(plan.targets().len(), 3);
            let with_users: Vec<&str> = plan
                .dependents()
                .iter()
                .map(|entry| entry.target_id.as_str())
                .collect();
            assert_eq!(with_users, ["pgdata", "cache"], "spare has none to list");
        });
    }

    #[gpui::test]
    async fn a_lookup_that_fails_is_an_error_and_not_a_plan_that_says_nobody_uses_it(
        cx: &mut TestAppContext,
    ) {
        let probe = Arc::new(Probe::over(world()).failing_usages());
        let (workspace, panel, cx) = panel_on(probe, ResourceKind::Volume, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal("pgdata", window, cx)
        });
        draw(cx);
        assert!(the_modal::<ConfirmDestructive>(&workspace, cx).is_none());
        panel.read_with(cx, |panel, _| {
            assert!(
                panel
                    .last_error
                    .as_ref()
                    .is_some_and(|error| error.to_string().contains("usages refused"))
            )
        });
    }

    #[gpui::test]
    async fn containers_are_removed_with_no_lookup_of_users(cx: &mut TestAppContext) {
        let fake = world();
        let (workspace, panel, cx) = panel_on(fake, ResourceKind::Container, cx).await;
        panel.update_in(cx, |panel, window, cx| {
            panel.start_removal("id-web", window, cx)
        });
        draw(cx);
        let modal = the_dialog(&workspace, cx);
        modal.read_with(cx, |modal, _| assert!(!modal.plan().has_dependents()));
    }
}
