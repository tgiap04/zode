use gpui::{Context, IntoElement, Render, Window};
use ui::{Callout, ContextMenu, DropdownMenu, IconPosition, prelude::*};

use crate::projects::move_item::WRITE_SCOPE_GUIDANCE;
use crate::projects::project_layout::shows_board;
use crate::projects::projects_section::{
    PROJECT_ITEM_CAP, ProjectsLoad, ProjectsSection, ProjectsSectionEvent,
};
use crate::untrusted_text::reveal_hidden_characters;

pub const MISSING_SCOPE_GUIDANCE: &str = "Projects need the `read:project` scope, which this token lacks. With the GitHub CLI, run `gh auth refresh -s project`, then check again; or sign in with GitHub, which asks for it.";

impl Render for ProjectsSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.missing_scope() {
            return self.render_scope_guidance(
                "github-projects-guidance",
                "Projects need another permission",
                MISSING_SCOPE_GUIDANCE,
                cx,
            );
        }
        let failed = match self.load_state() {
            ProjectsLoad::Failed(error) => Some(error.to_string()),
            _ => None,
        };
        let filter = self.view().and_then(|view| view.filter.clone());
        let capped = self.has_more_items() && *self.load_state() == ProjectsLoad::Idle;
        let board = shows_board(self.view());
        let body = if self.details().is_none() {
            self.render_placeholder(cx)
        } else if board {
            self.render_board(cx)
        } else {
            self.render_table(cx)
        };
        let panel = board.then(|| self.render_item_panel(window, cx)).flatten();
        // A board draws a card's menu by the card; a table has no card.
        let row_menu = (!board)
            .then(|| {
                let item_id = self.card_menu_item()?.to_string();
                self.render_card_menu(&item_id)
            })
            .flatten();
        let add_item = self.render_add_item(window, cx);
        let removal = self.render_removal_confirmation(cx);
        v_flex()
            .size_full()
            .gap_2()
            .child(self.render_toolbar(window, cx))
            .when_some(filter, |column, filter| {
                column.child(
                    Label::new(format!(
                        "View filter: {} (shown for reference; all items are listed)",
                        reveal_hidden_characters(&filter)
                    ))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                )
            })
            .when(self.write_scope_missing(), |column| {
                column.child(self.render_scope_guidance(
                    "github-projects-write-guidance",
                    "Changing this project needs another permission",
                    WRITE_SCOPE_GUIDANCE,
                    cx,
                ))
            })
            .when_some(
                self.move_message().map(str::to_string),
                |column, message| {
                    column.child(
                        div()
                            .debug_selector(|| "github-project-move-message".to_string())
                            .child(
                                Callout::new()
                                    .severity(Severity::Warning)
                                    .description(reveal_hidden_characters(&message).into_owned()),
                            ),
                    )
                },
            )
            .when_some(
                self.field_message().map(str::to_string),
                |column, message| {
                    column.child(
                        div()
                            .debug_selector(|| "github-project-field-message".to_string())
                            .child(
                                Callout::new()
                                    .severity(Severity::Info)
                                    .description(reveal_hidden_characters(&message).into_owned()),
                            ),
                    )
                },
            )
            .children(add_item)
            .children(removal)
            .when_some(failed, |column, error| {
                column.child(
                    Callout::new()
                        .severity(Severity::Warning)
                        .description(error),
                )
            })
            .when(capped, |column| {
                column.child(
                    h_flex()
                        .debug_selector(|| "github-project-cap-notice".to_string())
                        .gap_2()
                        .child(
                            Label::new(format!(
                                "Showing the first {} items; the project has more.",
                                self.items().len()
                            ))
                            .size(LabelSize::Small)
                            .color(Color::Warning),
                        )
                        .child(
                            Button::new(
                                "github-project-load-more",
                                format!("Load {PROJECT_ITEM_CAP} more"),
                            )
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, _window, cx| this.load_more(cx))),
                        ),
                )
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .gap_2()
                    .child(div().flex_1().min_w_0().h_full().child(body))
                    .children(panel),
            )
            .children(row_menu)
            .children(self.render_field_menu())
            .into_any_element()
    }
}

impl ProjectsSection {
    fn render_scope_guidance(
        &self,
        selector: &'static str,
        title: &'static str,
        guidance: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let can_sign_in = github::sign_in_with_github_available(cx);
        v_flex()
            .debug_selector(move || selector.to_string())
            .gap_2()
            .child(
                Callout::new()
                    .severity(Severity::Info)
                    .title(title)
                    .description(guidance),
            )
            .child(
                h_flex()
                    .gap_2()
                    .when(can_sign_in, |row| {
                        row.child(
                            Button::new("github-projects-sign-in", "Sign in with GitHub")
                                .style(ButtonStyle::Filled)
                                .on_click(cx.listener(|_, _, _window, cx| {
                                    cx.emit(ProjectsSectionEvent::SignInWithGithub)
                                })),
                        )
                    })
                    .child(
                        Button::new("github-projects-check-again", "Check again").on_click(
                            cx.listener(|_, _, _window, cx| {
                                cx.emit(ProjectsSectionEvent::CheckAgain)
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_toolbar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let selected = self.selected_project().map(|project| project.id.clone());
        let projects = self.projects().to_vec();
        let project_label = self
            .selected_project()
            .map(|project| reveal_hidden_characters(&project.title).into_owned())
            .unwrap_or_else(|| "No project".to_string());
        let project_menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for project in projects {
                let this = this.clone();
                let mut shown = reveal_hidden_characters(&project.title).into_owned();
                if project.closed {
                    shown.push_str(" (closed)");
                }
                let toggled = selected.as_deref() == Some(project.id.as_str());
                menu = menu.toggleable_entry(
                    shown,
                    toggled,
                    IconPosition::Start,
                    None,
                    move |_, cx| {
                        let id = project.id.clone();
                        this.update(cx, |this, cx| this.select_project(id, cx)).ok();
                    },
                );
            }
            menu
        });
        let current_view = self.view().map(|view| view.number);
        let views = self
            .details()
            .map(|details| details.views.clone())
            .unwrap_or_default();
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(DropdownMenu::new(
                "github-project-picker",
                project_label,
                project_menu,
            ))
            .child(
                div()
                    .debug_selector(|| "github-project-new".to_string())
                    .child(
                        Button::new("github-project-new", "New project")
                            .label_size(LabelSize::Small)
                            .start_icon(Icon::new(IconName::Plus).size(IconSize::Small))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_create_project(window, cx)
                            })),
                    ),
            )
            .children(views.into_iter().map(|view| {
                let number = view.number;
                Button::new(
                    ("github-project-view", number),
                    reveal_hidden_characters(&view.name).into_owned(),
                )
                .label_size(LabelSize::Small)
                .toggle_state(current_view == Some(number))
                .on_click(cx.listener(move |this, _, _window, cx| this.select_view(number, cx)))
            }))
            .child(div().flex_1())
            .when(self.selected_project().is_some(), |row| {
                row.child(
                    Button::new("github-project-add-item-open", "Add item")
                        .label_size(LabelSize::Small)
                        .start_icon(Icon::new(IconName::Plus).size(IconSize::Small))
                        .on_click(cx.listener(|this, _, _window, cx| this.open_add_item(cx))),
                )
            })
            .when(*self.load_state() == ProjectsLoad::Loading, |row| {
                row.child(
                    Label::new(format!("Loading… {} items", self.items().len()))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            .child(
                IconButton::new("github-projects-reload", IconName::RotateCw)
                    .tooltip(ui::Tooltip::text("Reload projects"))
                    .on_click(cx.listener(|this, _, _window, cx| this.reload(cx))),
            )
    }

    fn render_placeholder(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.load_state() {
            ProjectsLoad::Loading => Label::new("Reading projects…")
                .color(Color::Muted)
                .into_any_element(),
            _ if self.projects().is_empty() => v_flex()
                .debug_selector(|| "github-projects-empty".to_string())
                .items_start()
                .gap_2()
                .child(Label::new("This repository has no projects yet.").color(Color::Muted))
                .child(
                    Button::new("github-projects-create-first", "Create project")
                        .style(ButtonStyle::Filled)
                        .start_icon(Icon::new(IconName::Plus).size(IconSize::Small))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_create_project(window, cx)),
                        ),
                )
                .into_any_element(),
            _ => div().into_any_element(),
        }
    }
}
