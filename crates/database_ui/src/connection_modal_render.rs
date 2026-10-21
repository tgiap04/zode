//! Drawing the add-connection dialog.
//!
//! Split from its state so neither file has to be read to change the other:
//! which fields this form shows is decided by whichever driver was chosen, and
//! none of that decision is here.

use crate::connection_modal::{ConnectionModal, Step, TestState};
use crate::driver_registry::{self, CatalogueEntry};
use gpui::{ClickEvent, Entity, Focusable as _, Render, Window};
use ui::{
    CommonAnimationExt as _, KeyBinding, ListItem, ListItemSpacing, Modal, ModalFooter,
    ModalHeader, ProgressBar, Section, Tooltip, prelude::*,
};
use ui_input::InputField;

/// Wide enough for a description to sit on one line beside its name, which is
/// the whole reason the descriptions are there.
const DIALOG_WIDTH: Rems = rems(40.);
const DIALOG_MAX_HEIGHT: Rems = rems(40.);
/// About seven rows. Past that the list scrolls rather than the dialog growing
/// until it outruns a laptop screen.
const LIST_HEIGHT: Rems = rems(22.5);

impl Render for ConnectionModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("DatabaseConnectionModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(DIALOG_WIDTH)
            .max_h(DIALOG_MAX_HEIGHT)
            .on_action(cx.listener(|this, _: &menu::Cancel, _window, cx| this.cancel(cx)))
            .on_action(
                cx.listener(|this, _: &menu::Confirm, window, cx| match this.step {
                    Step::PickEngine => this.advance(window, cx),
                    _ => this.save(true, window, cx),
                }),
            )
            .on_action(
                cx.listener(|this, _: &menu::SelectNext, _window, cx| this.select_next(true, cx)),
            )
            .on_action(cx.listener(|this, _: &menu::SelectPrevious, _window, cx| {
                this.select_next(false, cx)
            }))
            .child(match &self.step {
                Step::PickEngine => self.render_picker(cx).into_any_element(),
                _ => self.render_form(cx).into_any_element(),
            })
    }
}

/// A failure in the words the driver or the form used, with the glyph that says
/// it is one.
pub(crate) fn inline_error(message: impl Into<SharedString>) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .items_center()
        .child(
            Icon::new(IconName::XCircle)
                .size(IconSize::XSmall)
                .color(Color::Error),
        )
        .child(
            div().min_w_0().child(
                Label::new(message)
                    .size(LabelSize::Small)
                    .color(Color::Error),
            ),
        )
}

impl ConnectionModal {
    /// What Enter will do, shown on the button that does it.
    ///
    /// Looked up from wherever focus really is: in the first step that is the
    /// search field, in the second the name field, and a binding resolved from
    /// the dialog's own handle would find none of them.
    fn confirm_hint(&self, cx: &App) -> KeyBinding {
        KeyBinding::for_action_in(&menu::Confirm, &self.focus_handle(cx), cx)
            .size(rems_from_px(12.))
    }

    // ---- First screen: choose an engine -------------------------------------

    fn render_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.matches(cx);
        let mut groups: Vec<(SharedString, Vec<(usize, CatalogueEntry)>)> = Vec::new();
        for (index, engine) in rows {
            let entry = (index, engine.clone());
            match groups.last_mut() {
                Some((name, members)) if *name == engine.group => members.push(entry),
                _ => groups.push((engine.group.clone(), vec![entry])),
            }
        }

        // Built eagerly into owned elements rather than inside `.children(..)`:
        // a closure there would have to hold `self` and `cx` while returning
        // something that borrows them, which is exactly what it cannot do.
        let mut list: Vec<AnyElement> = Vec::new();
        for (group, members) in groups.iter() {
            list.push(
                h_flex()
                    .h(rems(1.75))
                    .px(DynamicSpacing::Base06.rems(cx))
                    .child(
                        Label::new(group.clone())
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
            );
            for (index, engine) in members {
                list.push(self.render_engine(*index, engine, cx));
            }
        }

        let results = if groups.is_empty() {
            v_flex()
                .h(LIST_HEIGHT)
                .items_center()
                .justify_center()
                .child(
                    Label::new("No database matches that search")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element()
        } else {
            // `overflow_y_scroll` needs the element to carry an id: the
            // scroll offset is state, and state needs somewhere to live
            // across frames.
            v_flex()
                .id("database-engine-list")
                .debug_selector(|| "database-engine-list".into())
                .h(LIST_HEIGHT)
                .min_h_0()
                .overflow_y_scroll()
                .children(list)
                .into_any_element()
        };

        Modal::new("database-picker", None)
            .header(
                ModalHeader::new()
                    .child(Headline::new("Choose a Database").size(HeadlineSize::Small))
                    .description("Pick the type of database you want to connect to."),
            )
            .section(Section::new().child(self.search_editor.clone()))
            .section(Section::new().padded(false).child(results))
            .when_some(self.error.clone(), |modal, error| {
                modal.section(Section::new().child(inline_error(error)))
            })
            .footer(self.render_picker_footer(cx))
    }

    fn render_engine(
        &self,
        index: usize,
        engine: &CatalogueEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = index == self.selected;
        let installed = engine.installed;
        // Dimmed rather than hidden or unclickable: it can still be selected
        // and read, and the Continue button is where it says why not.
        let text = if installed {
            Color::Default
        } else {
            Color::Muted
        };

        ListItem::new(("database-engine", index))
            .spacing(ListItemSpacing::Dense)
            .toggle_state(selected)
            .start_slot(
                Icon::new(IconName::Database)
                    .size(IconSize::Small)
                    .color(if installed {
                        Color::Accent
                    } else {
                        Color::Muted
                    }),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .py_1()
                    .child(Label::new(engine.name.clone()).color(text).truncate())
                    .child(
                        Label::new(engine.description.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Muted)
                            .truncate(),
                    ),
            )
            .end_slot(
                h_flex()
                    .gap_0p5()
                    .when(engine.downloaded, |element| {
                        let engine = engine.clone();
                        element.child(
                            IconButton::new(
                                ("database-engine-remove-driver", index),
                                IconName::Trash,
                            )
                            .icon_size(IconSize::Small)
                            .icon_color(Color::Muted)
                            .tooltip(Tooltip::text(if engine.hand_placed {
                                "Remove the driver"
                            } else {
                                "Remove the downloaded driver"
                            }))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    // The row underneath selects on click; this one only
                                    // removes.
                                    cx.stop_propagation();
                                    this.remove_driver(engine.clone(), window, cx);
                                },
                            )),
                        )
                    })
                    .when(!installed, |element| {
                        // Zode bundles no drivers, so "not installed" is the ordinary
                        // state of an engine nobody has used yet -- and for the ones
                        // Zode publishes it is one button away from being untrue.
                        let text = if driver_registry::is_publishable(&engine.driver) {
                            "Not Downloaded"
                        } else {
                            "No Driver"
                        };
                        element.child(Label::new(text).size(LabelSize::XSmall).color(Color::Muted))
                    }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.select(index, cx);
                // A double click is the same decision made twice, so it may as
                // well be the one that moves on.
                if event.click_count() > 1 {
                    this.advance(window, cx);
                }
            }))
            .into_any_element()
    }

    fn render_picker_footer(&self, cx: &mut Context<Self>) -> ModalFooter {
        let selected = self.selected_engine();
        let installed = selected.is_some_and(|engine| engine.installed);
        // A driver Zode publishes is one it can go and get, so the button says
        // what pressing it will do rather than refusing to be pressed.
        let downloadable = selected.is_some_and(|engine| {
            !engine.installed && driver_registry::is_publishable(&engine.driver)
        });
        let can_continue = installed || downloadable;
        let (label, icon) = if downloadable {
            ("Download Driver", Some(IconName::Download))
        } else {
            ("Continue", None)
        };

        ModalFooter::new()
            .start_slot(
                Button::new("database-import-url", "Import from URL…")
                    .start_icon(Icon::new(IconName::Link).size(IconSize::Small))
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.import_from_url(window, cx);
                    })),
            )
            .end_slot(
                h_flex()
                    .gap_1()
                    .debug_selector(|| "database-modal-actions".into())
                    .child(
                        Button::new("database-picker-cancel", "Cancel")
                            .on_click(cx.listener(|this, _event, _window, cx| this.cancel(cx))),
                    )
                    .child(
                        Button::new("database-picker-continue", label)
                            .style(ButtonStyle::Filled)
                            .when_some(icon, |button, icon| {
                                button.start_icon(Icon::new(icon).size(IconSize::Small))
                            })
                            .disabled(!can_continue)
                            .key_binding(self.confirm_hint(cx))
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.advance(window, cx);
                            })),
                    ),
            )
    }

    // ---- Second screen: fill the driver's form ------------------------------

    fn render_form(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self
            .engine()
            .map(|engine| format!("New {} Connection", engine.name))
            .unwrap_or_else(|| "New Connection".to_string());
        let filling = matches!(self.step, Step::Filling { .. });

        Modal::new("database-form", None)
            .header(
                ModalHeader::new()
                    .child(
                        IconButton::new("database-form-back", IconName::ArrowLeft)
                            .icon_size(IconSize::Small)
                            .tooltip(|_window, cx| Tooltip::simple("Choose another database", cx))
                            .on_click(cx.listener(|this, _event, _window, cx| this.back(cx))),
                    )
                    .child(Headline::new(title).size(HeadlineSize::Small)),
            )
            .section(
                Section::new().child(
                    v_flex()
                        .id("database-form-body")
                        .debug_selector(|| "database-form-body".into())
                        .gap_3()
                        .max_h(LIST_HEIGHT)
                        .overflow_y_scroll()
                        .child(self.render_group(0, None, vec![("Name", self.name_editor.clone())]))
                        .children(self.render_driver_groups(cx))
                        .children(self.render_status(cx)),
                ),
            )
            .when_some(self.error.clone(), |modal, error| {
                modal.section(Section::new().child(inline_error(error)))
            })
            .footer(
                ModalFooter::new().end_slot(
                    h_flex()
                        .gap_1()
                        .debug_selector(|| "database-modal-actions".into())
                        .child(
                            Button::new("database-form-cancel", "Cancel")
                                .on_click(cx.listener(|this, _event, _window, cx| this.cancel(cx))),
                        )
                        .child(
                            Button::new("database-form-save", "Save")
                                .disabled(!filling)
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.save(false, window, cx);
                                })),
                        )
                        .child(
                            Button::new("database-form-save-connect", "Save & Connect")
                                .style(ButtonStyle::Filled)
                                .disabled(!filling)
                                .key_binding(self.confirm_hint(cx))
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.save(true, window, cx);
                                })),
                        ),
                ),
            )
    }

    /// The driver's own fields, under the headings the driver chose.
    ///
    /// Grouped in declaration order rather than sorted: the driver decided what
    /// order to ask in, and reordering its form here would be this crate having
    /// an opinion about a engine it is not allowed to know.
    fn render_driver_groups(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        match &self.step {
            Step::Asking { engine } => vec![
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(
                        Icon::new(IconName::LoadCircle)
                            .size(IconSize::XSmall)
                            .color(Color::Muted)
                            .with_keyed_rotate_animation("database-asking-driver", 2),
                    )
                    .child(
                        Label::new(format!("Asking {}…", engine.name))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
            ],
            Step::Downloading { engine, progress } => {
                let (status, fraction) = ConnectionModal::download_status(progress);
                vec![
                    v_flex()
                        .gap_2()
                        .child(
                            Label::new(format!("Getting the {} driver", engine.name))
                                .size(LabelSize::Small),
                        )
                        .children(Self::render_progress_bar(fraction, cx))
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .gap_2()
                                .child(
                                    Label::new(status)
                                        .size(LabelSize::XSmall)
                                        .color(Color::Muted),
                                )
                                .child(Button::new("database-download-cancel", "Cancel").on_click(
                                    cx.listener(|this, _event, _window, cx| {
                                        this.cancel_download(cx)
                                    }),
                                )),
                        )
                        .into_any_element(),
                ]
            }
            Step::Unreachable { engine, message } => {
                // A driver that failed to *download* and one that failed to
                // *start* are different failures with different remedies, and
                // only the first is worth offering to try again.
                let retryable =
                    !engine.installed && driver_registry::is_publishable(&engine.driver);
                let heading = if retryable {
                    "That driver could not be downloaded."
                } else {
                    "That driver did not answer."
                };
                vec![
                    v_flex()
                        .gap_1()
                        .child(inline_error(heading))
                        .child(
                            Label::new(message.clone())
                                .size(LabelSize::XSmall)
                                .color(Color::Muted),
                        )
                        .when(retryable, |element| {
                            element.child(
                                div().pt_1().child(
                                    Button::new("database-download-retry", "Try Again")
                                        .start_icon(
                                            Icon::new(IconName::RotateCw).size(IconSize::Small),
                                        )
                                        .on_click(cx.listener(|this, _event, window, cx| {
                                            this.retry_download(window, cx)
                                        })),
                                ),
                            )
                        })
                        .into_any_element(),
                ]
            }
            Step::Filling { form, inputs, .. } => {
                let mut groups: Vec<(Option<SharedString>, Vec<(&str, Entity<InputField>)>)> =
                    Vec::new();
                for (field, input) in form.fields.iter().zip(inputs) {
                    let heading = field.group.clone().map(SharedString::from);
                    let row = (field.label.as_str(), input.clone());
                    match groups.last_mut() {
                        Some((name, rows)) if *name == heading => rows.push(row),
                        _ => groups.push((heading, vec![row])),
                    }
                }
                groups
                    .into_iter()
                    .enumerate()
                    // One past the name group, which is drawn above these and
                    // has the first number.
                    .map(|(position, (heading, rows))| {
                        self.render_group(position + 1, heading, rows)
                            .into_any_element()
                    })
                    .collect()
            }
            Step::PickEngine => Vec::new(),
        }
    }

    /// Nothing, rather than a bar that invents a position.
    ///
    /// `ProgressBar` documents itself as not for indeterminate progress, and a
    /// bar sitting at zero while bytes are plainly arriving reads as stuck. The
    /// status line beside it still says how much has come.
    fn render_progress_bar(fraction: Option<f32>, cx: &App) -> Option<AnyElement> {
        let fraction = fraction?;
        Some(ProgressBar::new("database-download-progress", fraction, 1., cx).into_any_element())
    }

    /// One headed block of fields, each with its label above it.
    ///
    /// `group_index` and the field's place in it are the element ids: two fields
    /// can share a label or a placeholder, and an id shared between siblings
    /// would make them share state.
    fn render_group(
        &self,
        group_index: usize,
        heading: Option<SharedString>,
        rows: Vec<(&str, Entity<InputField>)>,
    ) -> impl IntoElement {
        v_flex()
            .id(("database-form-group", group_index))
            .gap_1p5()
            .when_some(heading, |element, heading| {
                element.child(Label::new(heading).size(LabelSize::Small))
            })
            .child(
                v_flex().gap_3().children(rows.into_iter().enumerate().map(
                    |(position, (label, input))| {
                        // The label is drawn here rather than by `InputField`,
                        // which would set it in the default colour.
                        v_flex()
                            .id(("database-form-field", position))
                            .gap_1()
                            .child(
                                Label::new(label.to_string())
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .child(input)
                    },
                )),
            )
    }

    /// Status, and the button that fills it in.
    fn render_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !matches!(self.step, Step::Filling { .. }) {
            return None;
        }
        let (message, colour, leading) = match &self.test {
            TestState::Untested => (SharedString::new_static("Not tested"), Color::Muted, None),
            TestState::Running => (
                SharedString::new_static("Connecting…"),
                Color::Muted,
                Some(
                    Icon::new(IconName::LoadCircle)
                        .size(IconSize::XSmall)
                        .color(Color::Muted)
                        .with_keyed_rotate_animation("database-testing-connection", 2)
                        .into_any_element(),
                ),
            ),
            TestState::Reached(driver) => (
                format!("Reached · {driver}").into(),
                Color::Success,
                Some(
                    Icon::new(IconName::Check)
                        .size(IconSize::XSmall)
                        .color(Color::Success)
                        .into_any_element(),
                ),
            ),
            TestState::Refused(error) => (
                error.clone(),
                Color::Error,
                Some(
                    Icon::new(IconName::XCircle)
                        .size(IconSize::XSmall)
                        .color(Color::Error)
                        .into_any_element(),
                ),
            ),
        };

        Some(
            h_flex()
                .w_full()
                .px_3()
                .py_1p5()
                .gap_3()
                .justify_between()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().colors().border)
                .child(
                    v_flex()
                        .min_w_0()
                        .gap_1()
                        .child(
                            Label::new("Status")
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                        .child(
                            h_flex().gap_1p5().items_center().children(leading).child(
                                div().min_w_0().child(
                                    Label::new(message).size(LabelSize::Small).color(colour),
                                ),
                            ),
                        ),
                )
                .child(
                    Button::new("database-test-connection", "Test Connection")
                        .start_icon(Icon::new(IconName::PlayFilled).size(IconSize::Small))
                        .disabled(matches!(self.test, TestState::Running))
                        .on_click(cx.listener(|this, _event, window, cx| {
                            this.test_connection(window, cx);
                        })),
                )
                .into_any_element(),
        )
    }
}
