//! The Inspect tab: everything the engine knows about one resource.
//!
//! Two views of one answer -- sections of label/value rows, and the engine's own
//! JSON. Secrets are masked in both by default. A row is revealed one at a time
//! with its eye, and the raw JSON shows the real values only after an explicit
//! "Show secrets". Nothing revealed is remembered: a refresh, leaving the tab or
//! closing the row puts every mask back.

use std::collections::HashSet;

use container::{ContainerError, InspectRow, InspectSection, Inspection, Resource};
use editor::Editor;
use gpui::{AnyElement, ClipboardItem, Entity, Task, Window};
use ui::{Button, ButtonStyle, prelude::*};

use crate::container_panel::ContainerPanel;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum InspectView {
    #[default]
    Sections,
    RawJson,
}

#[derive(Default)]
pub(crate) struct InspectState {
    /// `None` until the first answer arrives.
    pub(crate) inspection: Option<Result<Inspection, String>>,
    /// The containers using this resource. `None` when the kind has no such
    /// list or it has not arrived; its failure never hides the inspection.
    pub(crate) used_by: Option<Result<Vec<Resource>, String>>,
    /// `(section, row)` pairs shown unmasked.
    pub(crate) revealed: HashSet<(usize, usize)>,
    pub(crate) collapsed: HashSet<usize>,
    pub(crate) view: InspectView,
    /// Whether the raw JSON shows the real secret values.
    pub(crate) show_secrets: bool,
    /// The read-only editor holding the raw JSON, made on first use.
    pub(crate) raw_editor: Option<Entity<Editor>>,
    /// The request in flight. Held, so closing the row cancels it.
    pub(crate) load: Option<Task<()>>,
    /// Giving the raw JSON its highlighting once the language has loaded.
    pub(crate) language: Option<Task<()>>,
}

impl InspectState {
    /// Whether a load was ever asked for, so choosing the tab again does not ask
    /// twice.
    pub(crate) fn has_started(&self) -> bool {
        self.load.is_some() || self.inspection.is_some()
    }

    fn inspection(&self) -> Option<&Inspection> {
        self.inspection
            .as_ref()
            .and_then(|result| result.as_ref().ok())
    }

    /// The raw JSON as it may be shown right now.
    pub(crate) fn raw_text(&self) -> String {
        match self.inspection() {
            Some(inspection) if self.show_secrets => inspection.raw_json_revealed.clone(),
            Some(inspection) => inspection.raw_json.clone(),
            None => String::new(),
        }
    }

    /// Puts every mask back.
    fn conceal(&mut self) {
        self.revealed.clear();
        self.show_secrets = false;
    }
}

/// What a row shows: the real value only once its own eye was pressed.
pub(crate) fn shown_value(row: &InspectRow, revealed: bool) -> &str {
    if revealed { row.revealed() } else { &row.value }
}

/// A section as `label: value` lines, with the values the screen shows.
pub(crate) fn section_text(
    section: &InspectSection,
    index: usize,
    revealed: &HashSet<(usize, usize)>,
) -> String {
    section
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            format!(
                "{}: {}",
                row.label,
                shown_value(row, revealed.contains(&(index, row_index)))
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn copy(text: String, cx: &mut gpui::App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
}

impl ContainerPanel {
    /// Asks the engine for the open row's inspection (and its users).
    ///
    /// What is on screen stays while the new answer is awaited; the refresh
    /// button spins instead of the tab blanking.
    pub(crate) fn load_inspect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend().cloned() else {
            return;
        };
        let kind = self.active_kind;
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        let id = detail.resource.id.clone();
        let wants_users = backend.supports_usages(kind);
        detail.inspect.load = Some(cx.spawn_in(window, async move |this, cx| {
            let inspected = backend.inspect(kind, &id);
            let users = async {
                if wants_users {
                    Some(backend.usages(kind, &id).await)
                } else {
                    None
                }
            };
            let (inspected, users) = futures::future::join(inspected, users).await;
            let applied = this.update_in(cx, |this, window, cx| {
                this.apply_inspection(inspected, users, window, cx);
            });
            if let Err(error) = applied {
                log::debug!("panel closed before an inspection arrived: {error}");
            }
        }));
        cx.notify();
    }

    fn apply_inspection(
        &mut self,
        inspected: Result<Option<Inspection>, ContainerError>,
        users: Option<Result<Option<Vec<Resource>>, ContainerError>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        let state = &mut detail.inspect;
        state.load = None;
        state.conceal();
        state.inspection = Some(match inspected {
            Ok(Some(inspection)) => Ok(inspection),
            Ok(None) => Err("This engine has nothing to show for it.".to_string()),
            Err(error) => Err(error.to_string()),
        });
        state.used_by = match users {
            Some(Ok(Some(rows))) => Some(Ok(rows)),
            Some(Err(error)) => Some(Err(error.to_string())),
            Some(Ok(None)) | None => None,
        };
        self.sync_raw_editor(window, cx);
        cx.notify();
    }

    pub(crate) fn set_inspect_view(
        &mut self,
        view: InspectView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        detail.inspect.view = view;
        self.sync_raw_editor(window, cx);
        cx.notify();
    }

    pub(crate) fn toggle_show_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        detail.inspect.show_secrets = !detail.inspect.show_secrets;
        self.sync_raw_editor(window, cx);
        cx.notify();
    }

    pub(crate) fn toggle_row_reveal(&mut self, section: usize, row: usize, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        if !detail.inspect.revealed.remove(&(section, row)) {
            detail.inspect.revealed.insert((section, row));
        }
        cx.notify();
    }

    fn toggle_section(&mut self, section: usize, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        if !detail.inspect.collapsed.remove(&section) {
            detail.inspect.collapsed.insert(section);
        }
        cx.notify();
    }

    /// Makes the raw-JSON editor hold what [`InspectState::raw_text`] says, when
    /// the raw view is the one showing.
    ///
    /// Written only when the text differs: `set_text` moves the cursor and drops
    /// the selection, which a refresh that changed nothing must not do.
    fn sync_raw_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(detail) = self.detail.as_ref() else {
            return;
        };
        if detail.inspect.view != InspectView::RawJson || detail.inspect.inspection().is_none() {
            return;
        }
        let text = detail.inspect.raw_text();
        let editor = match detail.inspect.raw_editor.clone() {
            Some(editor) => editor,
            None => {
                let editor = cx.new(|cx| {
                    let mut editor = Editor::multi_line(window, cx);
                    editor.set_read_only(true);
                    editor.set_show_gutter(false, cx);
                    editor.set_soft_wrap();
                    editor
                });
                let language = self.json_highlighting(&editor, cx);
                if let Some(detail) = self.detail.as_mut() {
                    detail.inspect.raw_editor = Some(editor.clone());
                    detail.inspect.language = language;
                }
                editor
            }
        };
        if editor.read(cx).text(cx) != text {
            editor.update(cx, |editor, cx| editor.set_text(text, window, cx));
        }
    }

    /// Highlights `editor` as JSON once the language is loaded. Best effort: a
    /// workspace without the language leaves plain text, which is still right.
    fn json_highlighting(
        &self,
        editor: &Entity<Editor>,
        cx: &mut Context<Self>,
    ) -> Option<Task<()>> {
        let workspace = self.workspace.clone()?;
        let language = workspace
            .read_with(cx, |workspace, cx| {
                workspace
                    .project()
                    .read(cx)
                    .languages()
                    .language_for_name("JSON")
            })
            .ok()?;
        let buffer = editor.read(cx).buffer().read(cx).as_singleton()?;
        Some(cx.spawn(async move |_, cx| {
            let language = language.await.ok();
            buffer.update(cx, |buffer, cx| buffer.set_language(language, cx));
        }))
    }

    /// Opens a container that uses the open image, volume or network.
    pub(crate) fn open_user_of_detail(
        &mut self,
        resource: Resource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // `choose_kind` closes the open row and reloads for the new kind, and
        // refuses a kind the engine does not list -- so a user the engine
        // cannot show is not opened onto a wrong tab.
        self.choose_kind(resource.kind, cx);
        if self.active_kind == resource.kind {
            self.open_detail(resource, window, cx);
        }
    }

    pub(crate) fn render_inspect(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(detail) = self.detail.as_ref() else {
            return div().into_any_element();
        };
        let state = &detail.inspect;
        let loading = state.load.is_some();

        let body = match &state.inspection {
            None => crate::render::note(None, "Asking the engine\u{2026}"),
            Some(Err(message)) => failure(message, cx),
            Some(Ok(inspection)) => match state.view {
                InspectView::Sections => self.render_sections(inspection, state, cx),
                InspectView::RawJson => match state.raw_editor.clone() {
                    Some(editor) => div().size_full().p_1().child(editor).into_any_element(),
                    None => div().into_any_element(),
                },
            },
        };

        v_flex()
            .debug_selector(|| "container-inspect".into())
            .size_full()
            .child(self.render_inspect_toolbar(state, loading, cx))
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .into_any_element()
    }

    fn render_inspect_toolbar(
        &self,
        state: &InspectState,
        loading: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ready = state.inspection().is_some();
        let has_secrets = state.inspection().is_some_and(Inspection::has_secrets);
        let raw = state.view == InspectView::RawJson;
        let show_secrets = state.show_secrets;
        let raw_text = state.raw_text();

        h_flex()
            .flex_none()
            .px_2()
            .py_1()
            .gap_1()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("container-inspect-sections", "Sections")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::Small)
                            .toggle_state(!raw)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_inspect_view(InspectView::Sections, window, cx)
                            })),
                    )
                    .child(
                        Button::new("container-inspect-raw", "Raw JSON")
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::Small)
                            .toggle_state(raw)
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_inspect_view(InspectView::RawJson, window, cx)
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_0p5()
                    .when(raw && has_secrets, |element| {
                        element.child(
                            Button::new(
                                "container-inspect-show-secrets",
                                if show_secrets {
                                    "Hide secrets"
                                } else {
                                    "Show secrets"
                                },
                            )
                            .style(ButtonStyle::Subtle)
                            .label_size(LabelSize::Small)
                            .start_icon(
                                Icon::new(if show_secrets {
                                    IconName::EyeOff
                                } else {
                                    IconName::Eye
                                })
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| this.toggle_show_secrets(window, cx),
                            )),
                        )
                    })
                    .when(raw && ready, |element| {
                        element.child(
                            IconButton::new("container-inspect-copy-raw", IconName::Copy)
                                .icon_size(IconSize::Small)
                                .tooltip(|_window, cx| ui::Tooltip::simple("Copy JSON", cx))
                                .on_click(move |_, _window, cx| copy(raw_text.clone(), cx)),
                        )
                    })
                    .child(
                        IconButton::new("container-inspect-refresh", IconName::RotateCw)
                            .icon_size(IconSize::Small)
                            .loading(loading)
                            .tooltip(|_window, cx| ui::Tooltip::simple("Refresh", cx))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.load_inspect(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_sections(
        &self,
        inspection: &Inspection,
        state: &InspectState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .id("container-inspect-sections")
            .size_full()
            .overflow_y_scroll()
            .children(
                inspection
                    .sections
                    .iter()
                    .enumerate()
                    .map(|(index, section)| self.render_section(index, section, state, cx)),
            )
            .children(self.render_used_by(state, cx))
            .into_any_element()
    }

    fn render_section(
        &self,
        index: usize,
        section: &InspectSection,
        state: &InspectState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let collapsed = state.collapsed.contains(&index);
        let copied = section_text(section, index, &state.revealed);
        v_flex()
            .debug_selector(move || format!("container-inspect-section-{index}"))
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_1()
                    .justify_between()
                    .bg(cx.theme().colors().title_bar_background)
                    .child(
                        h_flex()
                            .id(("container-inspect-section-header", index))
                            .gap_1()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.toggle_section(index, cx)
                            }))
                            .child(
                                Icon::new(if collapsed {
                                    IconName::ChevronRight
                                } else {
                                    IconName::ChevronDown
                                })
                                .size(IconSize::XSmall)
                                .color(Color::Muted),
                            )
                            .child(Label::new(section.title.clone()).size(LabelSize::Small))
                            .child(
                                Label::new(section.rows.len().to_string())
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(
                        IconButton::new(("container-inspect-copy-section", index), IconName::Copy)
                            .icon_size(IconSize::XSmall)
                            .tooltip(|_window, cx| ui::Tooltip::simple("Copy section", cx))
                            .on_click(move |_, _window, cx| copy(copied.clone(), cx)),
                    ),
            )
            .when(!collapsed, |element| {
                element.children(section.rows.iter().enumerate().map(|(row_index, row)| {
                    self.render_inspect_row(index, row_index, row, state, cx)
                }))
            })
            .into_any_element()
    }

    fn render_inspect_row(
        &self,
        section: usize,
        row_index: usize,
        row: &InspectRow,
        state: &InspectState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let revealed = state.revealed.contains(&(section, row_index));
        let value = shown_value(row, revealed).to_string();
        let copied = value.clone();
        let slot = section * 10_000 + row_index;
        h_flex()
            .debug_selector(move || format!("container-inspect-row-{section}-{row_index}"))
            .px_2()
            .py_0p5()
            .gap_2()
            .items_start()
            .child(
                div().w(px(180.)).flex_none().child(
                    Label::new(row.label.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .truncate(),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Label::new(value).size(LabelSize::Small)),
            )
            .when(row.is_secret(), |element| {
                element.child(
                    IconButton::new(
                        ("container-inspect-reveal", slot),
                        if revealed {
                            IconName::EyeOff
                        } else {
                            IconName::Eye
                        },
                    )
                    .icon_size(IconSize::XSmall)
                    .tooltip(move |_window, cx| {
                        ui::Tooltip::simple(if revealed { "Hide" } else { "Reveal" }, cx)
                    })
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.toggle_row_reveal(section, row_index, cx)
                    })),
                )
            })
            .child(
                IconButton::new(("container-inspect-copy-row", slot), IconName::Copy)
                    .icon_size(IconSize::XSmall)
                    .tooltip(|_window, cx| ui::Tooltip::simple("Copy value", cx))
                    .on_click(move |_, _window, cx| copy(copied.clone(), cx)),
            )
            .into_any_element()
    }

    /// The containers using the open resource, each one opens on a click.
    fn render_used_by(&self, state: &InspectState, cx: &mut Context<Self>) -> Option<AnyElement> {
        let used_by = state.used_by.as_ref()?;
        let header = |count: Option<usize>| {
            h_flex()
                .px_2()
                .py_1()
                .gap_1()
                .bg(cx.theme().colors().title_bar_background)
                .child(Label::new("Used by").size(LabelSize::Small))
                .children(count.map(|count| {
                    Label::new(count.to_string())
                        .size(LabelSize::XSmall)
                        .color(Color::Muted)
                }))
        };
        let section = v_flex().debug_selector(|| "container-inspect-used-by".into());
        Some(match used_by {
            Err(message) => section
                .child(header(None))
                .child(
                    div().px_2().py_1().child(
                        Label::new(format!("Could not read who uses it: {message}"))
                            .size(LabelSize::Small)
                            .color(Color::Warning),
                    ),
                )
                .into_any_element(),
            Ok(rows) if rows.is_empty() => section
                .child(header(Some(0)))
                .child(
                    div().px_2().py_1().child(
                        Label::new("Nothing uses it.")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
                )
                .into_any_element(),
            Ok(rows) => section
                .child(header(Some(rows.len())))
                .children(rows.iter().enumerate().map(|(index, row)| {
                    let target = row.clone();
                    h_flex()
                        .id(("container-inspect-user", index))
                        .px_2()
                        .py_0p5()
                        .gap_1p5()
                        .cursor_pointer()
                        .hover(|style| style.bg(cx.theme().colors().ghost_element_hover))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_user_of_detail(target.clone(), window, cx)
                        }))
                        .child(crate::render::state_dot(row.state))
                        .child(Label::new(row.name.clone()).size(LabelSize::Small))
                        .children(row.detail.iter().find(|(label, _)| *label == "Image").map(
                            |(_, image)| {
                                Label::new(image.clone())
                                    .size(LabelSize::Small)
                                    .color(Color::Muted)
                            },
                        ))
                }))
                .into_any_element(),
        })
    }
}

fn failure(message: &str, cx: &mut Context<ContainerPanel>) -> AnyElement {
    v_flex()
        .debug_selector(|| "container-inspect-error".into())
        .size_full()
        .items_center()
        .justify_center()
        .gap_1()
        .p_4()
        .child(
            Icon::new(IconName::Warning)
                .size(IconSize::XLarge)
                .color(Color::Muted),
        )
        .child(Label::new("Could not inspect it"))
        .child(
            Label::new(message.to_string())
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
        .child(
            Button::new("container-inspect-retry", "Try again")
                .style(ButtonStyle::Subtle)
                .label_size(LabelSize::Small)
                .on_click(cx.listener(|this, _, window, cx| this.load_inspect(window, cx))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section() -> InspectSection {
        InspectSection {
            title: "Environment".into(),
            rows: vec![
                InspectRow::new("DB", "app"),
                InspectRow::secret("PASSWORD", "hunter2"),
            ],
        }
    }

    #[test]
    fn a_secret_row_shows_the_mask_until_its_own_eye_is_pressed() {
        let section = section();
        let secret = &section.rows[1];
        assert_ne!(shown_value(secret, false), "hunter2");
        assert_eq!(shown_value(secret, true), "hunter2");
        assert_eq!(shown_value(&section.rows[0], false), "app");
    }

    #[test]
    fn copying_a_section_copies_what_the_screen_shows() {
        let section = section();
        let masked = section_text(&section, 0, &HashSet::new());
        assert!(!masked.contains("hunter2"), "a masked row must copy masked");
        let revealed = section_text(&section, 0, &HashSet::from([(0, 1)]));
        assert!(revealed.contains("PASSWORD: hunter2"));
    }
}
