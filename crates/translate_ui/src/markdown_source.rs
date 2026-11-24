use gpui::{App, Entity, Focusable as _, Window};
use markdown::Markdown;
use workspace::PopoverLayer;

use crate::{SelectionSource, TranslateSelection, TranslationPopover, mouse_position_within};

/// Shows the popover for the markdown's selection. Returns `false` when
/// nothing is selected.
pub(crate) fn translate_selection(
    markdown: &Entity<Markdown>,
    action: &TranslateSelection,
    popover_layer: &Entity<PopoverLayer>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let Some(text) = markdown
        .read(cx)
        .selected_rendered_text()
        .filter(|text| !text.trim().is_empty())
    else {
        return false;
    };
    let position = match markdown.read(cx).last_visible_bounds() {
        Some(bounds) => mouse_position_within(bounds, window),
        None => window.mouse_position(),
    };
    let source = SelectionSource {
        text: text.clone(),
        position,
        replace_target: None,
        too_long: None,
        program_copy: false,
    };
    let popover = TranslationPopover::deploy_on(
        source,
        action.from.clone(),
        action.to.clone(),
        popover_layer,
        window,
        cx,
    );
    let markdown_focus = markdown.focus_handle(cx);
    // Markdown emits no event when its selection changes, only a notify.
    popover.update(cx, |popover, cx| {
        popover.follow_focus_of(&markdown_focus, window, cx);
        popover.dismiss_on_release(markdown, cx);
        popover.dismiss_when(
            markdown,
            move |markdown: &Markdown, _| {
                markdown.selected_rendered_text().as_deref() != Some(text.as_str())
            },
            cx,
        );
    });
    true
}
