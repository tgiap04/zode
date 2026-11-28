use gpui::{AnyView, ClickEvent, Hsla, MouseButton, hsla};

use crate::prelude::*;

/// One of the three coloured dots at the start of a macOS-style title bar.
///
/// The glyph is always drawn and only faded in while the pointer is over the
/// `group` the dots share, so hovering one shows all three -- as a macOS
/// window does -- and revealing it never changes a dot's size. A dot with no
/// handler is drawn in a neutral colour and does nothing, the way macOS greys
/// out a control a window does not offer.
///
/// The debug selector carries the id, which is what tests find a dot by.
pub fn window_control(
    id: &'static str,
    group: &'static str,
    color: Hsla,
    glyph: IconName,
    tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    cx: &App,
) -> impl IntoElement {
    let enabled = on_click.is_some();
    let fill = if enabled {
        color
    } else {
        cx.theme().colors().border
    };
    div().debug_selector(move || id.into()).child(
        div()
            .id(id)
            .size(px(12.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(fill)
            .when(enabled, |dot| {
                // The dots sit on a title bar that moves its window on a press,
                // and a press that drifts a pixel would move it instead of
                // clicking the dot.
                dot.cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            })
            .tooltip(tooltip)
            .when_some(on_click, |dot, on_click| dot.on_click(on_click))
            .child(
                div()
                    .opacity(0.)
                    .when(enabled, |glyph| {
                        glyph.group_hover(group, |style| style.opacity(1.))
                    })
                    .child(
                        Icon::new(glyph)
                            .size(IconSize::Custom(rems(0.5)))
                            .color(Color::Custom(hsla(0., 0., 0., 0.6))),
                    ),
            ),
    )
}
