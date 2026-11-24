use gpui::{Action, Focusable as _, FontWeight, IntoElement, Render, Window};
use ui::{ButtonLike, Tooltip, prelude::*};
use zode_actions::github as actions;

use crate::markdown_composer::MarkdownComposer;
use crate::markdown_format::MarkdownFormat;

/// What a toolbar button shows: an icon, or a letter drawn the way the
/// format looks.
enum Face {
    Icon(IconName),
    Letter(&'static str, Option<FontWeight>, bool),
}

/// The shortcut a format has, shown in its tooltip.
fn shortcut(format: MarkdownFormat) -> Option<Box<dyn Action>> {
    match format {
        MarkdownFormat::Bold => Some(Box::new(actions::FormatBold)),
        MarkdownFormat::Italic => Some(Box::new(actions::FormatItalic)),
        MarkdownFormat::Code => Some(Box::new(actions::FormatCode)),
        _ => None,
    }
}

const TOOLBAR: [(MarkdownFormat, &str, Face, &str); 9] = [
    (
        MarkdownFormat::Heading,
        "heading",
        Face::Letter("H", Some(FontWeight::BOLD), false),
        "Heading",
    ),
    (
        MarkdownFormat::Bold,
        "bold",
        Face::Letter("B", Some(FontWeight::BOLD), false),
        "Bold",
    ),
    (
        MarkdownFormat::Italic,
        "italic",
        Face::Letter("I", None, true),
        "Italic",
    ),
    (
        MarkdownFormat::Quote,
        "quote",
        Face::Icon(IconName::Quote),
        "Quote",
    ),
    (
        MarkdownFormat::Code,
        "code",
        Face::Icon(IconName::Code),
        "Code",
    ),
    (
        MarkdownFormat::Link,
        "link",
        Face::Icon(IconName::Link),
        "Link",
    ),
    (
        MarkdownFormat::BulletList,
        "bullets",
        Face::Icon(IconName::ListTree),
        "Bulleted list",
    ),
    (
        MarkdownFormat::NumberedList,
        "numbers",
        Face::Letter("1.", None, false),
        "Numbered list",
    ),
    (
        MarkdownFormat::TaskList,
        "tasks",
        Face::Icon(IconName::ListTodo),
        "Task list",
    ),
];

impl MarkdownComposer {
    /// Write when `preview` is false, Preview when it is true.
    fn render_tab(&self, preview: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, text) = if preview {
            ("markdown-preview", "Preview")
        } else {
            ("markdown-write", "Write")
        };
        Button::new(id, text)
            .label_size(LabelSize::Small)
            .style(ButtonStyle::Subtle)
            .toggle_state(self.is_previewing() == preview)
            .on_click(cx.listener(move |this, _, window, cx| {
                if preview {
                    this.show_preview(cx);
                } else {
                    this.show_editor(window, cx);
                }
            }))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.editor.focus_handle(cx);
        h_flex()
            .gap_0p5()
            .children(TOOLBAR.into_iter().map(|(format, id, face, tooltip)| {
                let face = match face {
                    Face::Icon(icon) => Icon::new(icon)
                        .size(IconSize::Small)
                        .color(Color::Muted)
                        .into_any_element(),
                    Face::Letter(text, weight, italic) => Label::new(text)
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .when_some(weight, |label, weight| label.weight(weight))
                        .when(italic, |label| label.italic())
                        .into_any_element(),
                };
                ButtonLike::new(SharedString::from(format!("markdown-toolbar-{id}")))
                    .size(ButtonSize::Compact)
                    .child(
                        div()
                            .w(rems_from_px(18.))
                            .flex()
                            .justify_center()
                            .child(face),
                    )
                    .tooltip({
                        let focus = focus.clone();
                        move |_window, cx| match shortcut(format) {
                            Some(action) => {
                                Tooltip::for_action_in(tooltip, action.as_ref(), &focus, cx)
                            }
                            None => Tooltip::simple(tooltip, cx),
                        }
                    })
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.apply(format, window, cx)),
                    )
            }))
    }
}

impl Render for MarkdownComposer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let previewing = self.is_previewing();
        let border = if !previewing && self.editor.focus_handle(cx).is_focused(window) {
            colors.border_focused
        } else {
            colors.border
        };
        let body = match &self.preview {
            Some(preview) if preview.read(cx).source(cx).trim().is_empty() => {
                Label::new("Nothing to preview")
                    .size(LabelSize::Small)
                    .color(Color::Muted)
                    .into_any_element()
            }
            Some(preview) => preview.clone().into_any_element(),
            None => self.editor.clone().into_any_element(),
        };
        v_flex()
            .key_context("GithubMarkdownComposer")
            .on_action(cx.listener(|this, _: &actions::FormatBold, window, cx| {
                this.apply(MarkdownFormat::Bold, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::FormatItalic, window, cx| {
                this.apply(MarkdownFormat::Italic, window, cx)
            }))
            .on_action(cx.listener(|this, _: &actions::FormatCode, window, cx| {
                this.apply(MarkdownFormat::Code, window, cx)
            }))
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(colors.editor_background)
            .overflow_hidden()
            .child(
                h_flex()
                    .px_1()
                    .py_0p5()
                    .gap_2()
                    .justify_between()
                    .border_b_1()
                    .border_color(colors.border_variant)
                    .bg(colors.panel_background)
                    .child(
                        h_flex()
                            .gap_0p5()
                            .child(self.render_tab(false, cx))
                            .child(self.render_tab(true, cx)),
                    )
                    .when(!previewing, |row| row.child(self.render_toolbar(cx))),
            )
            .child(
                div()
                    .px_2()
                    .py_1p5()
                    .when(previewing, |body| body.min_h(rems(6.)))
                    .child(body),
            )
    }
}
