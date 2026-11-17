//! "Run Container...": an image started with the ports, variables, mounts and
//! network somebody asked for, in a terminal.
//!
//! The command is the backend's. What the form holds is a [`RunSpec`]; the line
//! shown under the fields is exactly the argument list the backend returns for
//! it, asked again whenever the spec changes, and a value the backend refuses is
//! said there in the backend's own words. Run opens that same list in a terminal
//! -- never a string handed to a shell.

use std::sync::Arc;

use container::{
    ContainerBackend, ContainerError, PortMapping, PortProtocol, Resource, ResourceKind, RunSpec,
    VolumeMount, mask_secrets_in_text,
};
use gpui::{
    App, ClickEvent, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, ScrollHandle,
    Task, WeakEntity, Window,
};
use ui::{
    Checkbox, ContextMenu, KeyBinding, Modal, ModalFooter, ModalHeader, PopoverMenu, Section,
    ToggleState, prelude::*,
};

use crate::container_panel::ContainerPanel;
use crate::resource_form::{
    Field, Rule, check_env_key, check_mount_path, check_name, check_port, command_line, error_line,
    field_view, parse_port, split_command,
};
use crate::resource_modals::image_reference;
use crate::terminal::TerminalIntent;

struct PortRow {
    host: Field,
    container: Field,
    protocol: PortProtocol,
}

struct EnvRow {
    key: Field,
    value: Field,
}

struct MountRow {
    source: Field,
    target: Field,
    read_only: bool,
}

/// What the line under the fields says.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Preview {
    /// Something above is still wrong, and says what.
    Incomplete,
    Command(String),
    /// The backend refused a value, in its own words.
    Refused(String),
}

pub(crate) struct RunContainer {
    reference: String,
    backend: Arc<dyn ContainerBackend>,
    panel: WeakEntity<ContainerPanel>,
    name: Field,
    ports: Vec<PortRow>,
    env: Vec<EnvRow>,
    mounts: Vec<MountRow>,
    command: Field,
    network: Option<String>,
    networks: Vec<String>,
    detach: bool,
    remove_on_exit: bool,
    /// The spec the preview was last made for, so the backend is asked again
    /// only when something changed.
    previewed: Option<(Result<RunSpec, Vec<String>>, Preview)>,
    attempted: bool,
    error: Option<String>,
    networks_load: Option<Task<()>>,
    scroll: ScrollHandle,
    focus_handle: FocusHandle,
}

impl EventEmitter<DismissEvent> for RunContainer {}

impl workspace::ModalView for RunContainer {}

impl Focusable for RunContainer {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name.input.focus_handle(cx)
    }
}

/// Says what the field does with what is typed: quotes keep a space inside a
/// word, and a backslash is just a backslash -- `a\ b` is two words, not one.
const COMMAND_PLACEHOLDER: &str = "replaces the image's own; quotes group words, no \\ escapes";

fn blank(rule: Rule, placeholder: &str, window: &mut Window, cx: &mut App) -> Field {
    Field::new("", placeholder, "", rule, window, cx)
}

fn any_text(_: &str) -> Result<(), String> {
    Ok(())
}

impl PortRow {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            host: blank(Rule::optional(check_port), "8080", window, cx),
            container: blank(Rule::optional(check_port), "80", window, cx),
            protocol: PortProtocol::Tcp,
        }
    }

    /// `Ok(None)` for a row nobody filled in.
    fn read(&self, cx: &App) -> Result<Option<PortMapping>, String> {
        let (host, container) = (self.host.text(cx), self.container.text(cx));
        if host.is_empty() && container.is_empty() {
            return Ok(None);
        }
        for text in [&host, &container] {
            check_port(text)?;
        }
        match (parse_port(&host), parse_port(&container)) {
            (Some(host), Some(container)) => Ok(Some(PortMapping {
                host,
                container,
                protocol: self.protocol,
            })),
            _ => Err("A port is a number from 1 to 65535".to_string()),
        }
    }
}

impl EnvRow {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            key: blank(Rule::optional(check_env_key), "KEY", window, cx),
            value: blank(Rule::optional(any_text), "value", window, cx),
        }
    }

    fn read(&self, cx: &App) -> Result<Option<(String, String)>, String> {
        // The value is read as typed: a variable can hold leading spaces.
        let value = self.value.input.read(cx).text(cx);
        let key = self.key.text(cx);
        if key.is_empty() && value.is_empty() {
            return Ok(None);
        }
        if key.is_empty() {
            return Err("A variable needs a name".to_string());
        }
        check_env_key(&key)?;
        Ok(Some((key, value)))
    }
}

impl MountRow {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            source: blank(
                Rule::optional(check_mount_path),
                "volume or /host/path",
                window,
                cx,
            ),
            target: blank(
                Rule::optional(check_mount_path),
                "/container/path",
                window,
                cx,
            ),
            read_only: false,
        }
    }

    fn read(&self, cx: &App) -> Result<Option<VolumeMount>, String> {
        let (source, target) = (self.source.text(cx), self.target.text(cx));
        if source.is_empty() && target.is_empty() {
            return Ok(None);
        }
        if source.is_empty() || target.is_empty() {
            return Err("A mount needs both a source and a target".to_string());
        }
        check_mount_path(&source)?;
        check_mount_path(&target)?;
        Ok(Some(VolumeMount {
            source,
            target,
            read_only: self.read_only,
        }))
    }
}

impl RunContainer {
    pub(crate) fn new(
        image: &Resource,
        backend: Arc<dyn ContainerBackend>,
        panel: WeakEntity<ContainerPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut form = Self {
            reference: image_reference(image).to_string(),
            backend,
            panel,
            name: Field::new(
                "Name (optional)",
                "my-container",
                "",
                Rule::optional(check_name),
                window,
                cx,
            ),
            ports: Vec::new(),
            env: Vec::new(),
            mounts: Vec::new(),
            command: Field::new(
                "Command (optional)",
                COMMAND_PLACEHOLDER,
                "",
                Rule::optional(|text| split_command(text).map(|_| ())),
                window,
                cx,
            ),
            network: None,
            networks: Vec::new(),
            detach: true,
            remove_on_exit: false,
            previewed: None,
            attempted: false,
            error: None,
            networks_load: None,
            scroll: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        };
        form.load_networks(cx);
        form
    }

    /// The networks there are to choose from. A failure leaves the list empty:
    /// the default network still works, and a refusal here is not worth a
    /// dialog of its own.
    fn load_networks(&mut self, cx: &mut Context<Self>) {
        if !self
            .backend
            .supported_kinds()
            .contains(&ResourceKind::Network)
        {
            return;
        }
        let backend = self.backend.clone();
        self.networks_load = Some(cx.spawn(async move |this, cx| {
            let listed = backend.list(ResourceKind::Network).await;
            let applied = this.update(cx, |this, cx| {
                this.networks_load = None;
                match listed {
                    Ok(networks) => {
                        this.networks = networks.into_iter().map(|row| row.name).collect();
                    }
                    Err(error) => log::debug!("could not list networks for the run form: {error}"),
                }
                cx.notify();
            });
            if let Err(error) = applied {
                log::debug!("dialog closed before the networks arrived: {error}");
            }
        }));
    }

    pub(crate) fn add_port(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ports.push(PortRow::new(window, cx));
        cx.notify();
    }

    pub(crate) fn add_variable(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.env.push(EnvRow::new(window, cx));
        cx.notify();
    }

    pub(crate) fn add_mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mounts.push(MountRow::new(window, cx));
        cx.notify();
    }

    pub(crate) fn remove_port(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.ports.len() {
            self.ports.remove(index);
            cx.notify();
        }
    }

    pub(crate) fn remove_variable(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.env.len() {
            self.env.remove(index);
            cx.notify();
        }
    }

    pub(crate) fn remove_mount(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.mounts.len() {
            self.mounts.remove(index);
            cx.notify();
        }
    }

    pub(crate) fn choose_network(&mut self, network: Option<String>, cx: &mut Context<Self>) {
        self.network = network;
        cx.notify();
    }

    pub(crate) fn set_detach(&mut self, detach: bool, cx: &mut Context<Self>) {
        self.detach = detach;
        cx.notify();
    }

    pub(crate) fn set_remove_on_exit(&mut self, remove: bool, cx: &mut Context<Self>) {
        self.remove_on_exit = remove;
        cx.notify();
    }

    /// The spec the fields describe, or what is wrong with them. Rows nobody
    /// filled in are not part of it.
    fn read_spec(&self, cx: &App) -> Result<RunSpec, Vec<String>> {
        let mut problems = Vec::new();
        let mut keep = |problem: Option<String>| problems.extend(problem);
        keep(self.name.problem(true, cx));
        keep(self.command.problem(true, cx));
        let mut spec = RunSpec {
            image: self.reference.clone(),
            name: Some(self.name.text(cx)).filter(|text| !text.is_empty()),
            network: self.network.clone(),
            detach: self.detach,
            remove_on_exit: self.remove_on_exit,
            command: split_command(&self.command.text(cx)).unwrap_or_default(),
            ..RunSpec::default()
        };
        let mut errors = Vec::new();
        for row in &self.ports {
            match row.read(cx) {
                Ok(port) => spec.ports.extend(port),
                Err(error) => errors.push(error),
            }
        }
        for row in &self.env {
            match row.read(cx) {
                Ok(pair) => spec.env.extend(pair),
                Err(error) => errors.push(error),
            }
        }
        for row in &self.mounts {
            match row.read(cx) {
                Ok(mount) => spec.volumes.extend(mount),
                Err(error) => errors.push(error),
            }
        }
        problems.extend(errors);
        if problems.is_empty() {
            Ok(spec)
        } else {
            Err(problems)
        }
    }

    /// The line the backend would run for what is typed now, asked again only
    /// when the spec is not the one the last answer was for.
    fn preview(&mut self, cx: &App) -> Preview {
        let wanted = self.read_spec(cx);
        if let Some((spec, preview)) = &self.previewed
            && *spec == wanted
        {
            return preview.clone();
        }
        let preview = match &wanted {
            Err(_) => Preview::Incomplete,
            Ok(spec) => match self.backend.run_command(spec) {
                // Masked: the line is on screen, and a variable typed above can be
                // a password. What runs is the spec, not this text.
                Ok((program, args)) => {
                    Preview::Command(mask_secrets_in_text(&command_line(&program, &args)))
                }
                Err(ContainerError::Invalid { detail }) => Preview::Refused(detail),
                Err(error) => Preview::Refused(error.to_string()),
            },
        };
        self.previewed = Some((wanted, preview.clone()));
        preview
    }

    pub(crate) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.attempted = true;
        let spec = match self.read_spec(cx) {
            Ok(spec) => spec,
            Err(_) => {
                cx.notify();
                return;
            }
        };
        let (program, args) = match self.backend.run_command(&spec) {
            Ok(command) => command,
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let label = spec.name.unwrap_or_else(|| self.reference.clone());
        self.panel
            .update(cx, |panel, cx| {
                panel.open_command_terminal(
                    TerminalIntent::Run,
                    ResourceKind::Container,
                    program,
                    args,
                    label,
                    window,
                    cx,
                )
            })
            .ok();
        cx.emit(DismissEvent);
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    #[cfg(test)]
    pub(crate) fn type_name(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.name.set_text(text, window, cx);
    }

    #[cfg(test)]
    pub(crate) fn type_command(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.command.set_text(text, window, cx);
    }

    #[cfg(test)]
    pub(crate) fn type_port(
        &mut self,
        row: usize,
        host: &str,
        container: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self.ports.get(row) {
            row.host.set_text(host, window, cx);
            row.container.set_text(container, window, cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn type_variable(
        &mut self,
        row: usize,
        key: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self.env.get(row) {
            row.key.set_text(key, window, cx);
            row.value.set_text(value, window, cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn type_mount(
        &mut self,
        row: usize,
        source: &str,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(row) = self.mounts.get_mut(row) {
            row.source.set_text(source, window, cx);
            row.target.set_text(target, window, cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_mount_read_only(&mut self, row: usize, read_only: bool) {
        if let Some(row) = self.mounts.get_mut(row) {
            row.read_only = read_only;
        }
    }

    #[cfg(test)]
    pub(crate) fn previewed_line(&mut self, cx: &App) -> Option<String> {
        match self.preview(cx) {
            Preview::Command(line) => Some(line),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn preview_refusal(&mut self, cx: &App) -> Option<String> {
        match self.preview(cx) {
            Preview::Refused(detail) => Some(detail),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn network_names(&self) -> &[String] {
        &self.networks
    }

    #[cfg(test)]
    pub(crate) fn port_count(&self) -> usize {
        self.ports.len()
    }
}

fn section_header(
    title: &'static str,
    id: &'static str,
    on_add: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    h_flex()
        .justify_between()
        .child(Label::new(title).size(LabelSize::Small).color(Color::Muted))
        .child(
            IconButton::new(id, IconName::Plus)
                .icon_size(IconSize::XSmall)
                .tooltip(move |_window, cx| ui::Tooltip::simple(format!("Add {title}"), cx))
                .on_click(on_add),
        )
}

fn row_error(problem: Option<String>) -> Option<impl IntoElement> {
    problem.map(error_line)
}

impl Render for RunContainer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = self.preview(cx);
        let focus_handle = self.focus_handle(cx);
        let this = cx.entity().downgrade();

        let ports = self.ports.iter().enumerate().map(|(index, row)| {
            let protocol = row.protocol;
            v_flex()
                .id(("container-run-port", index))
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_1()
                        .child(div().flex_1().child(row.host.input.clone()))
                        .child(Label::new(":").color(Color::Muted))
                        .child(div().flex_1().child(row.container.input.clone()))
                        .child(
                            Button::new(
                                ("container-run-port-protocol", index),
                                match protocol {
                                    PortProtocol::Tcp => "tcp",
                                    PortProtocol::Udp => "udp",
                                },
                            )
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(
                                move |this, _, _window, cx| {
                                    if let Some(row) = this.ports.get_mut(index) {
                                        row.protocol = match row.protocol {
                                            PortProtocol::Tcp => PortProtocol::Udp,
                                            PortProtocol::Udp => PortProtocol::Tcp,
                                        };
                                        cx.notify();
                                    }
                                },
                            )),
                        )
                        .child(
                            IconButton::new(("container-run-port-remove", index), IconName::Close)
                                .icon_size(IconSize::XSmall)
                                .tooltip(|_window, cx| ui::Tooltip::simple("Remove", cx))
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.remove_port(index, cx)
                                })),
                        ),
                )
                .children(row_error(row.read(cx).err()))
        });

        let variables = self.env.iter().enumerate().map(|(index, row)| {
            v_flex()
                .id(("container-run-variable", index))
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_1()
                        .child(div().flex_1().child(row.key.input.clone()))
                        .child(Label::new("=").color(Color::Muted))
                        .child(div().flex_1().child(row.value.input.clone()))
                        .child(
                            IconButton::new(
                                ("container-run-variable-remove", index),
                                IconName::Close,
                            )
                            .icon_size(IconSize::XSmall)
                            .tooltip(|_window, cx| ui::Tooltip::simple("Remove", cx))
                            .on_click(cx.listener(
                                move |this, _, _window, cx| this.remove_variable(index, cx),
                            )),
                        ),
                )
                .children(row_error(row.read(cx).err()))
        });

        let mounts = self.mounts.iter().enumerate().map(|(index, row)| {
            let this = this.clone();
            v_flex()
                .id(("container-run-mount", index))
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_1()
                        .child(div().flex_1().child(row.source.input.clone()))
                        .child(Label::new("\u{2192}").color(Color::Muted))
                        .child(div().flex_1().child(row.target.input.clone()))
                        .child(
                            Checkbox::new(
                                ("container-run-mount-ro", index),
                                if row.read_only {
                                    ToggleState::Selected
                                } else {
                                    ToggleState::Unselected
                                },
                            )
                            .label("ro")
                            .label_size(LabelSize::Small)
                            .on_click(move |state, _window, cx| {
                                let read_only = *state == ToggleState::Selected;
                                this.update(cx, |this, cx| {
                                    if let Some(row) = this.mounts.get_mut(index) {
                                        row.read_only = read_only;
                                        cx.notify();
                                    }
                                })
                                .ok();
                            }),
                        )
                        .child(
                            IconButton::new(("container-run-mount-remove", index), IconName::Close)
                                .icon_size(IconSize::XSmall)
                                .tooltip(|_window, cx| ui::Tooltip::simple("Remove", cx))
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.remove_mount(index, cx)
                                })),
                        ),
                )
                .children(row_error(row.read(cx).err()))
        });

        let chosen_network = self
            .network
            .clone()
            .unwrap_or_else(|| "Default".to_string());
        let network_choices = self.networks.clone();
        let picker = {
            let this = this.clone();
            PopoverMenu::new("container-run-network-picker")
                .trigger(
                    Button::new("container-run-network", chosen_network)
                        .label_size(LabelSize::Small)
                        .end_icon(
                            Icon::new(IconName::ChevronDown)
                                .size(IconSize::Small)
                                .color(Color::Muted),
                        ),
                )
                .menu(move |window, cx| {
                    let this = this.clone();
                    let choices = network_choices.clone();
                    Some(ContextMenu::build(window, cx, move |mut menu, _, _| {
                        let none = this.clone();
                        menu = menu.entry("Default", None, move |_window, cx| {
                            none.update(cx, |form, cx| form.choose_network(None, cx))
                                .ok();
                        });
                        for name in choices {
                            let this = this.clone();
                            let chosen = name.clone();
                            menu = menu.entry(name, None, move |_window, cx| {
                                this.update(cx, |form, cx| {
                                    form.choose_network(Some(chosen.clone()), cx)
                                })
                                .ok();
                            });
                        }
                        menu
                    }))
                })
        };

        let detach = {
            let this = this.clone();
            Checkbox::new(
                "container-run-detach",
                if self.detach {
                    ToggleState::Selected
                } else {
                    ToggleState::Unselected
                },
            )
            .label("Run in the background")
            .label_size(LabelSize::Small)
            .on_click(move |state, _window, cx| {
                let detach = *state == ToggleState::Selected;
                this.update(cx, |form, cx| form.set_detach(detach, cx)).ok();
            })
        };
        let remove = {
            let this = this.clone();
            Checkbox::new(
                "container-run-remove-on-exit",
                if self.remove_on_exit {
                    ToggleState::Selected
                } else {
                    ToggleState::Unselected
                },
            )
            .label("Remove when it exits")
            .label_size(LabelSize::Small)
            .on_click(move |state, _window, cx| {
                let remove = *state == ToggleState::Selected;
                this.update(cx, |form, cx| form.set_remove_on_exit(remove, cx))
                    .ok();
            })
        };

        let preview_line = match &preview {
            Preview::Incomplete => Label::new("Fix what is marked above to see the command.")
                .size(LabelSize::Small)
                .color(Color::Muted)
                .into_any_element(),
            Preview::Command(line) => Label::new(line.clone())
                .size(LabelSize::Small)
                .buffer_font(cx)
                .into_any_element(),
            Preview::Refused(detail) => error_line(detail.clone()).into_any_element(),
        };
        let can_run = matches!(preview, Preview::Command(_));

        div()
            .key_context("ContainerForm")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.submit(window, cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .occlude()
            .debug_selector(|| "container-run-dialog".into())
            .w(rems(36.))
            .max_h(rems(44.))
            .child(
                Modal::new("container-run", Some(self.scroll.clone()))
                    .header(ModalHeader::new().child(
                        Headline::new(format!("Run {}", self.reference)).size(HeadlineSize::Small),
                    ))
                    .section(
                        Section::new().child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .child(field_view(
                                    &self.name,
                                    self.attempted,
                                    cx,
                                    "container-run-name".to_string(),
                                ))
                                .child(section_header("Ports", "container-run-add-port", {
                                    cx.listener(|this, _, window, cx| this.add_port(window, cx))
                                }))
                                .children(ports)
                                .child(section_header(
                                    "Environment",
                                    "container-run-add-variable",
                                    cx.listener(|this, _, window, cx| {
                                        this.add_variable(window, cx)
                                    }),
                                ))
                                .children(variables)
                                .child(section_header(
                                    "Volumes",
                                    "container-run-add-mount",
                                    cx.listener(|this, _, window, cx| this.add_mount(window, cx)),
                                ))
                                .children(mounts)
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Label::new("Network")
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        )
                                        .child(picker),
                                )
                                .child(h_flex().gap_4().child(detach).child(remove))
                                .child(field_view(
                                    &self.command,
                                    self.attempted,
                                    cx,
                                    "container-run-command".to_string(),
                                ))
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            Label::new("Will run")
                                                .size(LabelSize::Small)
                                                .color(Color::Muted),
                                        )
                                        .child(
                                            div()
                                                .debug_selector(|| "container-run-preview".into())
                                                .p_1p5()
                                                .rounded_md()
                                                .bg(cx.theme().colors().editor_background)
                                                .child(preview_line),
                                        ),
                                )
                                .children(self.error.clone().map(|error| {
                                    div()
                                        .debug_selector(|| "container-run-error".into())
                                        .child(error_line(error))
                                })),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(Button::new("container-run-cancel", "Cancel").on_click(
                                    cx.listener(|this, _: &ClickEvent, _window, cx| {
                                        this.cancel(cx)
                                    }),
                                ))
                                .child(
                                    div()
                                        .debug_selector(|| "container-run-submit".into())
                                        .child(
                                            Button::new("container-run-go", "Run")
                                                .style(ButtonStyle::Filled)
                                                .disabled(!can_run)
                                                .key_binding(
                                                    KeyBinding::for_action_in(
                                                        &menu::Confirm,
                                                        &focus_handle,
                                                        cx,
                                                    )
                                                    .size(rems_from_px(12.)),
                                                )
                                                .on_click(cx.listener(
                                                    |this, _: &ClickEvent, window, cx| {
                                                        this.submit(window, cx)
                                                    },
                                                )),
                                        ),
                                ),
                        ),
                    ),
            )
    }
}

impl ContainerPanel {
    pub(crate) fn open_run_dialog(
        &mut self,
        image: Resource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        let Some(workspace) = self.workspace.clone() else {
            log::warn!("no workspace to show a dialog in");
            return;
        };
        let panel = cx.entity().downgrade();
        workspace
            .update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    RunContainer::new(&image, backend, panel, window, cx)
                });
            })
            .ok();
    }
}
