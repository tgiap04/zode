//! The small forms: pull an image, tag one, create a volume or a network.
//!
//! One dialog shape for all four -- a few labelled inputs, checked as they are
//! typed, and a button -- because they differ only in which fields they ask for
//! and what pressing the button does. What an engine says in refusal stays in the
//! dialog under the fields, where the value that caused it is still in front of
//! the person; the dialog closes only once the engine has said yes.

use container::{ContainerBackend, ContainerError, Op, Resource, ResourceKind};
use gpui::{
    App, ClickEvent, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, Task, WeakEntity,
    Window,
};
use std::sync::Arc;
use ui::{KeyBinding, Modal, ModalFooter, ModalHeader, Section, prelude::*};

use crate::container_panel::ContainerPanel;
use crate::resource_form::{
    Field, Rule, check_name, check_reference, check_subnet, error_line, field_view,
};
use crate::terminal::TerminalIntent;

/// Runs once the values pass the form's own checks. The task's `Err` is shown in
/// the dialog, or on the panel when the dialog has been closed by then; `Ok`
/// closes it.
type Submit = Box<dyn Fn(Vec<String>, &mut Window, &mut App) -> Task<Result<(), ContainerError>>>;

pub(crate) struct FormModal {
    /// Prefix of every debug selector the dialog draws.
    id: &'static str,
    title: String,
    note: Option<String>,
    submit_label: &'static str,
    fields: Vec<Field>,
    panel: WeakEntity<ContainerPanel>,
    submit: Submit,
    /// Whether the list is read again once the engine said yes. Not for a pull,
    /// which only starts one in a terminal.
    reload_on_success: bool,
    attempted: bool,
    error: Option<String>,
    /// The engine being asked. Closing the dialog does not drop the question --
    /// see the `Drop` impl below.
    pending: Option<Task<()>>,
    focus_handle: FocusHandle,
}

impl EventEmitter<DismissEvent> for FormModal {}

/// A dialog closed while the engine is still working leaves the call running to
/// its end: a volume half-created is worse than a volume created, and an answer
/// nobody sees is worse than either. The task then finds the dialog gone and
/// hands the answer to the panel instead (see `submit`).
impl Drop for FormModal {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.detach();
        }
    }
}

impl workspace::ModalView for FormModal {}

impl Focusable for FormModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.fields.first() {
            Some(field) => field.input.focus_handle(cx),
            None => self.focus_handle.clone(),
        }
    }
}

impl FormModal {
    fn new(
        id: &'static str,
        title: impl Into<String>,
        submit_label: &'static str,
        panel: WeakEntity<ContainerPanel>,
        fields: Vec<Field>,
        submit: Submit,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            note: None,
            submit_label,
            fields,
            panel,
            submit,
            reload_on_success: true,
            attempted: false,
            error: None,
            pending: None,
            focus_handle: cx.focus_handle(),
        }
    }

    fn noting(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    fn without_reload(mut self) -> Self {
        self.reload_on_success = false;
        self
    }

    fn problems(&self, cx: &App) -> Vec<String> {
        self.fields
            .iter()
            .filter_map(|field| field.problem(true, cx))
            .collect()
    }

    pub(crate) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Checked here and not only by the button's state: Enter must meet the
        // same gate.
        if self.pending.is_some() {
            return;
        }
        self.attempted = true;
        if !self.problems(cx).is_empty() {
            cx.notify();
            return;
        }
        let values = self.fields.iter().map(|field| field.text(cx)).collect();
        self.error = None;
        let running = (self.submit)(values, window, cx);
        let panel = self.panel.clone();
        let reload_on_success = self.reload_on_success;
        self.pending = Some(cx.spawn(async move |this, cx| {
            let outcome = running.await;
            let shown = this.update(cx, |this, cx| {
                this.pending = None;
                match &outcome {
                    Ok(()) => cx.emit(DismissEvent),
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            });
            // The panel is told after the dialog has settled, not from inside
            // its update: a reload reads the panel, which may be reading this.
            let dialog_was_there = shown.is_ok();
            let told = panel.update(cx, |panel, cx| match outcome {
                Ok(()) if reload_on_success => panel.reload(cx),
                Ok(()) => {}
                // The dialog already said so, where the value that caused it is
                // still in front of the person.
                Err(_) if dialog_was_there => {}
                Err(error) => {
                    panel.last_error = Some(error);
                    cx.notify();
                }
            });
            if let Err(error) = told {
                log::debug!("panel closed before the engine answered: {error}");
            }
        }));
        cx.notify();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    #[cfg(test)]
    pub(crate) fn type_into(
        &mut self,
        field: usize,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(field) = self.fields.get(field) {
            field.set_text(text, window, cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn problem_of(&self, field: usize, cx: &App) -> Option<String> {
        self.fields
            .get(field)
            .and_then(|field| field.problem(self.attempted, cx))
    }
}

impl Render for FormModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.id;
        let busy = self.pending.is_some();
        let focus_handle = self.focus_handle(cx);
        div()
            .key_context("ContainerForm")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| this.submit(window, cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .occlude()
            .debug_selector(move || format!("{id}-dialog"))
            .w(rems(30.))
            .max_h(rems(40.))
            .child(
                Modal::new(id, None)
                    .header(
                        ModalHeader::new()
                            .child(Headline::new(self.title.clone()).size(HeadlineSize::Small)),
                    )
                    .section(
                        Section::new().child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .children(self.note.clone().map(|note| {
                                    Label::new(note).size(LabelSize::Small).color(Color::Muted)
                                }))
                                .children(self.fields.iter().enumerate().map(|(index, field)| {
                                    field_view(
                                        field,
                                        self.attempted,
                                        cx,
                                        format!("{id}-field-{index}"),
                                    )
                                }))
                                .children(self.error.clone().map(|error| {
                                    div()
                                        .debug_selector(move || format!("{id}-error"))
                                        .child(error_line(error))
                                })),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(Button::new((id, 0usize), "Cancel").on_click(
                                    cx.listener(|this, _: &ClickEvent, _window, cx| {
                                        this.cancel(cx)
                                    }),
                                ))
                                .child(
                                    div().debug_selector(move || format!("{id}-submit")).child(
                                        Button::new((id, 1usize), self.submit_label)
                                            .style(ButtonStyle::Filled)
                                            .disabled(busy)
                                            .loading(busy)
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

/// The reference an engine can be asked about for `image`: its name, or its id
/// when it has no name -- a dangling image's row name is a placeholder.
pub(crate) fn image_reference(image: &Resource) -> &str {
    let dangling = image
        .detail
        .iter()
        .any(|(label, value)| *label == "Dangling" && value == "yes");
    if dangling { &image.id } else { &image.name }
}

/// What the dialogs of this file do through the panel. Each opens a modal in the
/// workspace; with none to open it in, nothing happens, as for a removal.
impl ContainerPanel {
    fn open_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: impl FnOnce(
            WeakEntity<ContainerPanel>,
            &mut Window,
            &mut Context<FormModal>,
        ) -> FormModal
        + 'static,
    ) {
        let Some(workspace) = self.workspace.clone() else {
            log::warn!("no workspace to show a dialog in");
            return;
        };
        let panel = cx.entity().downgrade();
        workspace
            .update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| build(panel, window, cx));
            })
            .ok();
    }

    fn offers(&self, op: Op) -> bool {
        self.backend().is_some_and(|backend| backend.offers(op))
    }

    /// Whether the kind on screen has something to create or pull, and the op
    /// behind it.
    pub(crate) fn creation(&self) -> Option<(Op, &'static str)> {
        let (op, label) = match self.active_kind {
            ResourceKind::Image => (Op::ImagePull, "Pull Image\u{2026}"),
            ResourceKind::Volume => (Op::VolumeCreate, "Create Volume\u{2026}"),
            ResourceKind::Network => (Op::NetworkCreate, "Create Network\u{2026}"),
            _ => return None,
        };
        self.offers(op).then_some((op, label))
    }

    /// The header button and the empty-space menu entry: pull or create, by the
    /// kind on screen.
    pub(crate) fn start_creation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.creation().map(|(op, _)| op) {
            Some(Op::ImagePull) => self.open_pull_dialog(window, cx),
            Some(Op::VolumeCreate) => self.open_volume_dialog(window, cx),
            Some(Op::NetworkCreate) => self.open_network_dialog(window, cx),
            _ => {}
        }
    }

    pub(crate) fn open_pull_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        self.open_form(window, cx, move |panel, window, cx| {
            let fields = vec![Field::new(
                "Image",
                "nginx:latest",
                "",
                Rule::required(check_reference),
                window,
                cx,
            )];
            let submit_panel = panel.clone();
            FormModal::new(
                "container-pull",
                "Pull Image",
                "Pull",
                panel,
                fields,
                Box::new(move |values, window, cx| {
                    Task::ready(pull_in_terminal(
                        &backend,
                        &submit_panel,
                        &values,
                        window,
                        cx,
                    ))
                }),
                cx,
            )
            .noting("The pull runs in a terminal, so its progress is on screen.")
            .without_reload()
        });
    }

    pub(crate) fn open_tag_dialog(
        &mut self,
        image: Resource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        self.open_form(window, cx, move |panel, window, cx| {
            let fields = vec![Field::new(
                "New reference",
                "myrepo/app:1.0",
                "",
                Rule::required(check_reference),
                window,
                cx,
            )];
            let source = image.id.clone();
            FormModal::new(
                "container-tag",
                format!("Tag {}", image.name),
                "Tag",
                panel,
                fields,
                Box::new(move |values, _window, cx| {
                    let backend = backend.clone();
                    let source = source.clone();
                    let target = values.first().cloned().unwrap_or_default();
                    cx.background_spawn(async move { backend.tag(&source, &target).await })
                }),
                cx,
            )
        });
    }

    pub(crate) fn open_volume_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        self.open_form(window, cx, move |panel, window, cx| {
            let fields = vec![
                Field::new("Name", "pgdata", "", Rule::required(check_name), window, cx),
                Field::new(
                    "Driver (optional)",
                    "local",
                    "",
                    Rule::optional(check_name),
                    window,
                    cx,
                ),
            ];
            FormModal::new(
                "container-volume",
                "Create Volume",
                "Create",
                panel,
                fields,
                Box::new(move |values, _window, cx| {
                    let backend = backend.clone();
                    let name = values.first().cloned().unwrap_or_default();
                    let driver = values.get(1).filter(|text| !text.is_empty()).cloned();
                    cx.background_spawn(async move {
                        backend.create_volume(&name, driver.as_deref(), &[]).await
                    })
                }),
                cx,
            )
        });
    }

    pub(crate) fn open_network_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        self.open_form(window, cx, move |panel, window, cx| {
            let fields = vec![
                Field::new(
                    "Name",
                    "backend",
                    "",
                    Rule::required(check_name),
                    window,
                    cx,
                ),
                Field::new(
                    "Driver (optional)",
                    "bridge",
                    "",
                    Rule::optional(check_name),
                    window,
                    cx,
                ),
                Field::new(
                    "Subnet (optional)",
                    "10.5.0.0/16",
                    "",
                    Rule::optional(check_subnet),
                    window,
                    cx,
                ),
            ];
            FormModal::new(
                "container-network",
                "Create Network",
                "Create",
                panel,
                fields,
                Box::new(move |values, _window, cx| {
                    let backend = backend.clone();
                    let name = values.first().cloned().unwrap_or_default();
                    let driver = values.get(1).filter(|text| !text.is_empty()).cloned();
                    let subnet = values.get(2).filter(|text| !text.is_empty()).cloned();
                    cx.background_spawn(async move {
                        backend
                            .create_network(&name, driver.as_deref(), subnet.as_deref())
                            .await
                    })
                }),
                cx,
            )
        });
    }
}

/// Opens the engine's own `pull` in a terminal. A reference the backend refuses
/// comes back as the dialog's error, and nothing is opened.
fn pull_in_terminal(
    backend: &Arc<dyn ContainerBackend>,
    panel: &WeakEntity<ContainerPanel>,
    values: &[String],
    window: &mut Window,
    cx: &mut App,
) -> Result<(), ContainerError> {
    let reference = values.first().cloned().unwrap_or_default();
    let Some((program, args)) = backend.pull_command(&reference) else {
        return Err(ContainerError::Invalid {
            detail: format!(
                "the engine will not pull {reference:?}: it was refused, or pulling is not offered"
            ),
        });
    };
    panel
        .update(cx, |panel, cx| {
            panel.open_command_terminal(
                TerminalIntent::Pull,
                ResourceKind::Image,
                program,
                args,
                reference,
                window,
                cx,
            )
        })
        .map_err(|error| ContainerError::Invalid {
            detail: error.to_string(),
        })
}
