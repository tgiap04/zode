//! Putting a container on a network, and taking one off.
//!
//! One dialog for both: a list of containers to choose from, which is the only
//! difference. Connect offers the containers not yet on the network, with an
//! optional alias; Disconnect offers those that are, and its button is the
//! confirmation -- it names the container and the network, and nothing is lost
//! by it that a Connect does not give back.

use std::sync::Arc;

use container::{ContainerBackend, ContainerError, Resource, ResourceKind};
use gpui::{
    App, ClickEvent, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, Task, WeakEntity,
    Window,
};
use ui::{KeyBinding, Modal, ModalFooter, ModalHeader, Section, prelude::*};

use crate::container_panel::ContainerPanel;
use crate::resource_form::{Field, Rule, check_name, error_line, field_view};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Membership {
    Connect,
    Disconnect,
}

impl Membership {
    fn verb(self) -> &'static str {
        match self {
            Membership::Connect => "Connect",
            Membership::Disconnect => "Disconnect",
        }
    }
}

pub(crate) struct NetworkMembership {
    mode: Membership,
    network: Resource,
    backend: Arc<dyn ContainerBackend>,
    panel: WeakEntity<ContainerPanel>,
    /// `None` until the engine has said which containers there are.
    choices: Option<Result<Vec<Resource>, String>>,
    chosen: Option<usize>,
    /// Only for Connect.
    alias: Option<Field>,
    attempted: bool,
    error: Option<String>,
    load: Option<Task<()>>,
    pending: Option<Task<()>>,
    focus_handle: FocusHandle,
}

impl EventEmitter<DismissEvent> for NetworkMembership {}

/// Closing the dialog while the engine is still connecting or disconnecting
/// leaves the call to finish: see the matching impl on `FormModal`.
impl Drop for NetworkMembership {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.detach();
        }
    }
}

impl workspace::ModalView for NetworkMembership {}

impl Focusable for NetworkMembership {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl NetworkMembership {
    pub(crate) fn new(
        mode: Membership,
        network: Resource,
        backend: Arc<dyn ContainerBackend>,
        panel: WeakEntity<ContainerPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let alias = (mode == Membership::Connect).then(|| {
            Field::new(
                "Alias (optional)",
                "db",
                "",
                Rule::optional(check_name),
                window,
                cx,
            )
        });
        let mut modal = Self {
            mode,
            network,
            backend,
            panel,
            choices: None,
            chosen: None,
            alias,
            attempted: false,
            error: None,
            load: None,
            pending: None,
            focus_handle: cx.focus_handle(),
        };
        modal.load_choices(cx);
        modal
    }

    /// Asks the engine which containers there are and which are on the network.
    fn load_choices(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let network = self.network.id.clone();
        let mode = self.mode;
        self.load = Some(cx.spawn(async move |this, cx| {
            let outcome = containers_for(&*backend, &network, mode).await;
            let applied = this.update(cx, |this, cx| {
                this.load = None;
                this.choices = Some(outcome.map_err(|error| error.to_string()));
                cx.notify();
            });
            if let Err(error) = applied {
                log::debug!("dialog closed before the containers arrived: {error}");
            }
        }));
    }

    pub(crate) fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        let count = match &self.choices {
            Some(Ok(choices)) => choices.len(),
            _ => 0,
        };
        if index < count {
            self.chosen = Some(index);
            self.error = None;
            cx.notify();
        }
    }

    fn chosen_container(&self) -> Option<&Resource> {
        match (&self.choices, self.chosen) {
            (Some(Ok(choices)), Some(index)) => choices.get(index),
            _ => None,
        }
    }

    fn alias_problem(&self, cx: &App) -> Option<String> {
        self.alias
            .as_ref()
            .and_then(|field| field.problem(self.attempted, cx))
    }

    pub(crate) fn submit(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        self.attempted = true;
        let Some(container) = self.chosen_container().cloned() else {
            self.error = Some("Choose a container.".to_string());
            cx.notify();
            return;
        };
        if self.alias_problem(cx).is_some() {
            cx.notify();
            return;
        }
        let alias = self
            .alias
            .as_ref()
            .map(|field| field.text(cx))
            .filter(|text| !text.is_empty());
        let backend = self.backend.clone();
        let network = self.network.id.clone();
        let mode = self.mode;
        self.error = None;
        let running = cx.background_spawn(async move {
            match mode {
                Membership::Connect => {
                    backend
                        .connect(&network, &container.id, alias.as_deref())
                        .await
                }
                Membership::Disconnect => backend.disconnect(&network, &container.id).await,
            }
        });
        let panel = self.panel.clone();
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
            // Told after the dialog has settled, and told even when the dialog
            // was closed meanwhile: the engine did what it was asked.
            let dialog_was_there = shown.is_ok();
            let told = panel.update(cx, |panel, cx| match outcome {
                Ok(()) => panel.reload(cx),
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
    pub(crate) fn choice_names(&self) -> Option<Vec<String>> {
        match &self.choices {
            Some(Ok(choices)) => Some(choices.iter().map(|row| row.name.clone()).collect()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    #[cfg(test)]
    pub(crate) fn load_failure(&self) -> Option<&str> {
        match &self.choices {
            Some(Err(message)) => Some(message),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn type_alias(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = &self.alias {
            field.set_text(text, window, cx);
        }
    }
}

async fn containers_for(
    backend: &dyn ContainerBackend,
    network: &str,
    mode: Membership,
) -> Result<Vec<Resource>, ContainerError> {
    let attached = backend.usages(ResourceKind::Network, network).await?;
    match mode {
        Membership::Disconnect => attached.ok_or_else(|| ContainerError::NotOffered {
            operation: "listing the containers on a network".to_string(),
        }),
        Membership::Connect => {
            let attached = attached.unwrap_or_default();
            Ok(backend
                .list(ResourceKind::Container)
                .await?
                .into_iter()
                .filter(|row| !attached.iter().any(|on| on.id == row.id))
                .collect())
        }
    }
}

impl Render for NetworkMembership {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.pending.is_some();
        let mode = self.mode;
        let focus_handle = self.focus_handle.clone();
        let list = match &self.choices {
            None => crate::render::note(None, "Asking the engine\u{2026}"),
            Some(Err(message)) => div()
                .debug_selector(|| "container-membership-failure".into())
                .child(error_line(message.clone()))
                .into_any_element(),
            Some(Ok(choices)) if choices.is_empty() => crate::render::note(
                None,
                match mode {
                    Membership::Connect => "Every container is already on this network.",
                    Membership::Disconnect => "No container is on this network.",
                },
            ),
            Some(Ok(choices)) => v_flex()
                .id("container-membership-choices")
                .max_h(rems(11.25))
                .overflow_y_scroll()
                .children(choices.iter().enumerate().map(|(index, row)| {
                    let chosen = self.chosen == Some(index);
                    h_flex()
                        .id(("container-membership-choice", index))
                        .debug_selector(move || format!("container-membership-choice:{index}"))
                        .px_2()
                        .py_0p5()
                        .gap_1p5()
                        .cursor_pointer()
                        .rounded_sm()
                        .when(chosen, |element| {
                            element.bg(cx.theme().colors().ghost_element_selected)
                        })
                        .hover(|style| style.bg(cx.theme().colors().ghost_element_hover))
                        .on_click(cx.listener(move |this, _, _window, cx| this.choose(index, cx)))
                        .child(crate::render::state_dot(row.state))
                        .child(Label::new(row.name.clone()).size(LabelSize::Small))
                        .into_any_element()
                }))
                .into_any_element(),
        };

        div()
            .key_context("ContainerForm")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &menu::Confirm, _window, cx| this.submit(cx)))
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .elevation_3(cx)
            .occlude()
            .debug_selector(|| "container-membership-dialog".into())
            .w(rems(28.))
            .max_h(rems(40.))
            .child(
                Modal::new("container-membership", None)
                    .header(
                        ModalHeader::new().child(
                            Headline::new(format!(
                                "{} a container {} {}",
                                mode.verb(),
                                match mode {
                                    Membership::Connect => "to",
                                    Membership::Disconnect => "from",
                                },
                                self.network.name
                            ))
                            .size(HeadlineSize::Small),
                        ),
                    )
                    .section(
                        Section::new().child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .child(list)
                                .children(self.alias.as_ref().map(|field| {
                                    field_view(
                                        field,
                                        self.attempted,
                                        cx,
                                        "container-membership-alias".to_string(),
                                    )
                                }))
                                .children(self.error.clone().map(|error| {
                                    div()
                                        .debug_selector(|| "container-membership-error".into())
                                        .child(error_line(error))
                                })),
                        ),
                    )
                    .footer(
                        ModalFooter::new().end_slot(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("container-membership-cancel", "Cancel").on_click(
                                        cx.listener(|this, _: &ClickEvent, _window, cx| {
                                            this.cancel(cx)
                                        }),
                                    ),
                                )
                                .child(
                                    div()
                                        .debug_selector(|| "container-membership-submit".into())
                                        .child(
                                            Button::new("container-membership-go", mode.verb())
                                                .style(ButtonStyle::Filled)
                                                .disabled(busy || self.chosen.is_none())
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
                                                    |this, _: &ClickEvent, _window, cx| {
                                                        this.submit(cx)
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
    pub(crate) fn open_membership(
        &mut self,
        mode: Membership,
        network: Resource,
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
                    NetworkMembership::new(mode, network, backend, panel, window, cx)
                });
            })
            .ok();
    }
}
