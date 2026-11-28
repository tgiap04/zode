//! Sets a panel's text in the terminal's font.

use gpui::{App, Pixels, Styled as _, px};
use settings::Settings as _;
use terminal::terminal_settings::TerminalSettings;
use theme_settings::ThemeSettings;
use ui::utils::WithRemSize;

/// The size `Label`'s default size is written as: 14px against a 16px rem.
/// Dividing the terminal's size by it gives the rem at which a default label
/// comes out at exactly the terminal's size.
const DEFAULT_LABEL_REMS: f32 = 14. / 16.;

/// An element whose children are drawn in the terminal's font family and
/// weight, at the terminal's size.
///
/// The size is carried by the rem rather than set on the text: every `ui`
/// label sizes itself in rems and would override a plain text size, so the
/// rem is the one lever that reaches all of them -- and it scales the padding
/// and icons with the text, so the panel stays in proportion rather than
/// growing small text in large boxes. The family is inherited by labels, which
/// never set one of their own.
///
/// Falls back the way the terminal itself does: to the buffer font when the
/// terminal names no family, and to the buffer size when it names no size.
pub fn terminal_font_scope(cx: &App) -> WithRemSize {
    let terminal = TerminalSettings::get_global(cx);
    let theme = ThemeSettings::get_global(cx);
    let family = terminal.font_family.as_ref().map_or_else(
        || theme.buffer_font.family.clone(),
        |family| family.0.clone().into(),
    );
    let size = terminal.font_size.map_or_else(
        || theme.buffer_font_size(cx),
        |size| theme_settings::adjusted_font_size(size, cx),
    );
    let weight = terminal.font_weight.unwrap_or_default();
    WithRemSize::new(rem_for_label_size(size))
        .font_family(family)
        .font_weight(weight)
}

/// The rem at which a default-size label is drawn at `size`.
fn rem_for_label_size(size: Pixels) -> Pixels {
    px(f32::from(size) / DEFAULT_LABEL_REMS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A default label comes out at the terminal's size, and the smaller
    /// label sizes keep their proportion to it.
    #[test]
    fn a_default_label_lands_on_the_terminal_size() {
        let rem = rem_for_label_size(px(11.));
        let default_label = f32::from(rem) * DEFAULT_LABEL_REMS;
        assert!((default_label - 11.).abs() < 0.001, "{default_label}");
        let small_label = f32::from(rem) * (12. / 16.);
        assert!(small_label < 11. && small_label > 9., "{small_label}");
    }
}
