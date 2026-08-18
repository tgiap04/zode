//! Drawing the add-connection dialog.
//!
//! Split from its state so neither file has to be read to change the other:
//! which fields this form shows is decided by whichever driver was chosen, and
//! none of that decision is here.

use crate::connection_modal::{ConnectionModal, Step, TestState};
use crate::driver_registry::CatalogueEntry;
use editor::Editor;
use gpui::{Entity, Render, Window};
use ui::{Tooltip, prelude::*};

/// Wide enough for a description to sit on one line beside its name, which is
/// the whole reason the descriptions are there.
const DIALOG_WIDTH: Pixels = px(620.);
/// About seven rows. Past that the list scrolls rather than the dialog growing
/// until it outruns a laptop screen.
const LIST_HEIGHT: Pixels = px(340.);

impl Render for ConnectionModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("DatabaseConnectionModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(DIALOG_WIDTH)
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
                _ => self.render_form(window, cx).into_any_element(),
            })
    }
}

impl ConnectionModal {
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
                div()
                    .px_3()
                    .pt_2()
                    .pb_1()
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

        v_flex()
            .child(self.render_picker_header(cx))
            .child(if groups.is_empty() {
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
                    .h(LIST_HEIGHT)
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(list)
                    .into_any_element()
            })
            .child(self.render_picker_footer(cx))
    }

    fn render_picker_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .p_3()
            .gap_3()
            .items_start()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(
                v_flex()
                    .gap_0p5()
                    .child(Label::new("Choose a Database"))
                    .child(
                        Label::new("Pick the type of database you want to connect to.")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(
                h_flex()
                    .w(px(240.))
                    .px_2()
                    .py_1()
                    .gap_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .bg(cx.theme().colors().editor_background)
                    .child(
                        Icon::new(IconName::MagnifyingGlass)
                            .size(IconSize::Small)
                            .color(Color::Muted),
                    )
                    .child(div().flex_1().child(self.search_editor.clone())),
            )
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

        h_flex()
            .id(("database-engine", index))
            .w_full()
            .px_3()
            .py_1p5()
            .gap_3()
            .justify_between()
            .cursor_pointer()
            .when(selected, |element| {
                element.bg(cx.theme().colors().element_selected)
            })
            .when(!selected, |element| {
                element.hover(|style| style.bg(cx.theme().colors().element_hover))
            })
            .child(
                h_flex()
                    .gap_2p5()
                    .child(Icon::new(IconName::Database).size(IconSize::Medium).color(
                        if installed {
                            Color::Accent
                        } else {
                            Color::Muted
                        },
                    ))
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(Label::new(engine.name.clone()).color(text))
                            .child(
                                Label::new(engine.description.clone())
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            ),
                    ),
            )
            .when(!installed, |element| {
                element.child(
                    Label::new("Not Installed")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.select(index, cx);
                    // A double click is the same decision made twice, so it may as
                    // well be the one that moves on.
                    if event.click_count() > 1 {
                        this.advance(window, cx);
                    }
                }),
            )
            .into_any_element()
    }

    fn render_picker_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_continue = self
            .selected_engine()
            .is_some_and(|engine| engine.installed);

        v_flex()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .when_some(self.error.clone(), |element, error| {
                element.child(
                    div()
                        .px_3()
                        .pt_2()
                        .child(Label::new(error).size(LabelSize::Small).color(Color::Error)),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .p_3()
                    .gap_2()
                    .justify_between()
                    .child(
                        Button::new("database-import-url", "Import from URL…")
                            .start_icon(Icon::new(IconName::Link))
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.import_from_url(window, cx);
                            })),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("database-picker-cancel", "Cancel").on_click(
                                    cx.listener(|this, _event, _window, cx| this.cancel(cx)),
                                ),
                            )
                            .child(
                                Button::new("database-picker-continue", "Continue")
                                    .style(ButtonStyle::Filled)
                                    .disabled(!can_continue)
                                    .on_click(cx.listener(|this, _event, window, cx| {
                                        this.advance(window, cx);
                                    })),
                            ),
                    ),
            )
    }

    // ---- Second screen: fill the driver's form ------------------------------

    fn render_form(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self
            .engine()
            .map(|engine| format!("New {} Connection", engine.name))
            .unwrap_or_else(|| "New Connection".to_string());

        v_flex()
            .child(self.render_form_header(title, cx))
            .child(
                v_flex()
                    .id("database-form-body")
                    .p_3()
                    .gap_3()
                    .max_h(LIST_HEIGHT)
                    .overflow_y_scroll()
                    .child(self.render_group(None, vec![("Name", self.name_editor.clone())], cx))
                    .children(self.render_driver_groups(cx))
                    .children(self.render_status(cx)),
            )
            .when_some(self.error.clone(), |element, error| {
                element.child(
                    div()
                        .px_3()
                        .pb_2()
                        .child(Label::new(error).size(LabelSize::Small).color(Color::Error)),
                )
            })
    }

    fn render_form_header(&self, title: String, cx: &mut Context<Self>) -> impl IntoElement {
        let filling = matches!(self.step, Step::Filling { .. });

        h_flex()
            .w_full()
            .p_3()
            .gap_2()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(
                h_flex()
                    .gap_1p5()
                    .child(
                        IconButton::new("database-form-back", IconName::ArrowLeft)
                            .icon_size(IconSize::Small)
                            .tooltip(|_window, cx| Tooltip::simple("Choose another database", cx))
                            .on_click(cx.listener(|this, _event, _window, cx| this.back(cx))),
                    )
                    .child(Label::new(title)),
            )
            .child(
                h_flex()
                    .gap_2()
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
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.save(true, window, cx);
                            })),
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
                Label::new(format!("Asking {}…", engine.name))
                    .color(Color::Muted)
                    .into_any_element(),
            ],
            Step::Unreachable { message, .. } => vec![
                v_flex()
                    .gap_1()
                    .child(
                        Label::new("That driver did not answer.")
                            .size(LabelSize::Small)
                            .color(Color::Error),
                    )
                    .child(
                        Label::new(message.clone())
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    )
                    .into_any_element(),
            ],
            Step::Filling { form, inputs, .. } => {
                let mut groups: Vec<(Option<SharedString>, Vec<(&str, Entity<Editor>)>)> =
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
                    .map(|(heading, rows)| self.render_group(heading, rows, cx).into_any_element())
                    .collect()
            }
            Step::PickEngine => Vec::new(),
        }
    }

    /// One headed block of label-and-field rows.
    ///
    /// Rows share a single bordered surface with hairlines between them rather
    /// than each carrying its own box: at this density a box per field turns
    /// the form into a stack of outlines with the content lost inside them.
    fn render_group(
        &self,
        heading: Option<SharedString>,
        rows: Vec<(&str, Entity<Editor>)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let last = rows.len().saturating_sub(1);
        v_flex()
            .gap_1p5()
            .when_some(heading, |element, heading| {
                element.child(Label::new(heading).size(LabelSize::Small))
            })
            .child(
                v_flex()
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .bg(cx.theme().colors().editor_background)
                    .children(rows.into_iter().enumerate().map(|(index, (label, input))| {
                        h_flex()
                            .w_full()
                            .px_3()
                            .py_1p5()
                            .gap_3()
                            .when(index != last, |element| {
                                element
                                    .border_b_1()
                                    .border_color(cx.theme().colors().border_variant)
                            })
                            .child(
                                div()
                                    .w(px(140.))
                                    .flex_none()
                                    .child(Label::new(label.to_string()).color(Color::Muted)),
                            )
                            .child(div().flex_1().child(input))
                    })),
            )
    }

    /// Status, and the button that fills it in.
    fn render_status(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !matches!(self.step, Step::Filling { .. }) {
            return None;
        }
        let (message, colour) = match &self.test {
            TestState::Untested => (SharedString::new_static("Not tested"), Color::Muted),
            TestState::Running => (SharedString::new_static("Connecting…"), Color::Muted),
            TestState::Reached(driver) => (format!("Reached · {driver}").into(), Color::Success),
            TestState::Refused(error) => (error.clone(), Color::Error),
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
                        .gap_0p5()
                        .child(Label::new("Status").color(Color::Muted))
                        .child(Label::new(message).size(LabelSize::Small).color(colour)),
                )
                .child(
                    Button::new("database-test-connection", "Test Connection")
                        .start_icon(Icon::new(IconName::PlayOutlined))
                        .disabled(matches!(self.test, TestState::Running))
                        .on_click(cx.listener(|this, _event, window, cx| {
                            this.test_connection(window, cx);
                        })),
                )
                .into_any_element(),
        )
    }
}
