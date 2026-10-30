//! The chrome's handle on Zode's brand palette.
//!
//! The palette, and the reasoning behind each value, live in `theme`'s
//! [`BrandColors`]: the theme step that paints links, focus rings, spinners and the
//! local cursor needs the same values and cannot depend on this crate.
//!
//! [`crate::Color`]'s own documentation warns that a colour detached from the theme
//! loses its semantic meaning across themes. These functions take that trade
//! knowingly, and only for a control's own state (active, on, checked, primary):
//! that is a fact about the app, so it reads the same whichever theme -- and
//! whichever `theme_overrides` -- the user picked. Everything else that is orange,
//! `Color::Accent` included, gets there through the theme's tokens instead, so a
//! user can still override it per theme.

use gpui::{App, Hsla};
use theme::{ActiveTheme, BrandColors};

fn brand(cx: &App) -> BrandColors {
    BrandColors::for_appearance(cx.theme().appearance())
}

/// The brand accent: an icon that is active, a switch that is on, a checkbox
/// that is checked.
pub fn brand_accent(cx: &App) -> Hsla {
    brand(cx).accent
}

/// The fill behind a solid brand button.
pub fn brand_solid(cx: &App) -> Hsla {
    brand(cx).solid
}

/// [`brand_solid`] with the pointer over it.
pub fn brand_solid_hovered(cx: &App) -> Hsla {
    brand(cx).solid_hovered
}

/// [`brand_solid`] while it is being pressed.
pub fn brand_solid_active(cx: &App) -> Hsla {
    brand(cx).solid_active
}

/// The label and icon colour to use *on top of* [`brand_solid`].
///
/// Not a general foreground -- it is only legible against the solid fill, and
/// is wrong anywhere else.
pub fn brand_on_solid(cx: &App) -> Hsla {
    brand(cx).on_solid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::calculate_contrast_ratio;
    use crate::{ButtonStyle, Color, TintColor};
    use theme::Appearance;

    /// The two backgrounds the palette was measured against: the shipped
    /// `Light 2026` and `Dark 2026` canvases.
    const LIGHT_CANVAS: u32 = 0xFFFFFF;
    const DARK_CANVAS: u32 = 0x121314;

    fn on_canvas(foreground: Hsla, canvas: u32) -> f32 {
        calculate_contrast_ratio(foreground, gpui::rgb(canvas).into())
    }

    /// Reads the palette rather than colour literals, so editing a value in
    /// `theme`'s brand module is what turns this red.
    #[test]
    fn brand_palette_meets_its_contrast_floors() {
        let light = BrandColors::for_appearance(Appearance::Light);
        let dark = BrandColors::for_appearance(Appearance::Dark);

        assert!(
            on_canvas(light.accent, LIGHT_CANVAS) >= 4.5,
            "accent on the light canvas fell under 4.5:1"
        );
        assert!(
            on_canvas(dark.accent, DARK_CANVAS) >= 4.5,
            "accent on the dark canvas fell under 4.5:1"
        );

        // Every state of the solid button, not just its resting fill: darkening
        // a saturated fill is what silently took the dark theme's label under
        // the floor, so each state is measured rather than assumed to follow.
        for (state, light_fill, dark_fill) in [
            ("enabled", light.solid, dark.solid),
            ("hovered", light.solid_hovered, dark.solid_hovered),
            ("active", light.solid_active, dark.solid_active),
        ] {
            assert!(
                on_canvas(light_fill, LIGHT_CANVAS) >= 3.0,
                "the light solid fill lost its boundary against the canvas when {state}"
            );
            assert!(
                on_canvas(dark_fill, DARK_CANVAS) >= 3.0,
                "the dark solid fill lost its boundary against the canvas when {state}"
            );
            assert!(
                calculate_contrast_ratio(light.on_solid, light_fill) >= 4.5,
                "the light solid button's label fell under 4.5:1 on its own fill when {state}"
            );
            assert!(
                calculate_contrast_ratio(dark.on_solid, dark_fill) >= 4.5,
                "the dark solid button's label fell under 4.5:1 on its own fill when {state}"
            );
        }
    }

    /// This guards nothing by itself. It records, next to the palette someone will
    /// one day "correct" to match the frontend, why the light solid is not `#F68001`.
    #[test]
    fn f68001_is_below_the_floor_on_white() {
        let measured = on_canvas(gpui::rgb(0xF68001).into(), LIGHT_CANVAS);
        assert!(
            measured < 3.0,
            "#F68001 now measures {measured:.2} on white; if this ever passes 3.0 the \
             reason the light solid differs from the frontend has gone away"
        );
    }

    /// Catches the branch being dropped -- a palette that ignored its appearance
    /// would still compile and still return a brand colour.
    #[test]
    fn brand_answers_differently_per_appearance() {
        let light = BrandColors::for_appearance(Appearance::Light);
        let dark = BrandColors::for_appearance(Appearance::Dark);
        for (role, light_value, dark_value) in [
            ("accent", light.accent, dark.accent),
            ("solid", light.solid, dark.solid),
            ("solid_hovered", light.solid_hovered, dark.solid_hovered),
            ("solid_active", light.solid_active, dark.solid_active),
            ("on_solid", light.on_solid, dark.on_solid),
        ] {
            assert_ne!(
                light_value, dark_value,
                "the brand's {role} answered the same in both appearances"
            );
        }
    }

    /// A selected `Tinted(Accent)` button paints its tint from `brand_accent`; its
    /// icon and label come through this conversion, so they must land on the same
    /// source rather than on `Color::Accent`, which a user's `text.accent` override
    /// can move.
    #[test]
    fn tinted_accent_draws_its_glyph_in_the_brand() {
        assert_eq!(
            Color::from(ButtonStyle::Tinted(TintColor::Accent)),
            Color::Selected
        );
    }
}
