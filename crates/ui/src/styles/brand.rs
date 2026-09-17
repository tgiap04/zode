//! Zode's own chrome colours, held apart from the theme on purpose.
//!
//! [`crate::Color`]'s own documentation warns that a colour detached from the
//! theme loses its semantic meaning across themes, and it is right. This module
//! takes that trade knowingly, and only for chrome: the state a control is in
//! (active, on, checked, primary) is a fact about the app, not about the file
//! being edited, so it should read the same whichever theme the user picked.
//! Everything the theme legitimately owns -- syntax, editor surfaces, links,
//! hints, `Color::Accent` -- stays with the theme and must not be routed here.
//!
//! The values come from the `web/frontend` palette. Its icon token is
//! `--primary-strong`, not the raw `--primary`, and that split is load-bearing:
//! `#F68001` measures 2.62 against white, under even the 3:1 floor WCAG 1.4.11
//! sets for a control's boundary. The frontend uses it on white anyway because
//! it is a landing page with sparse, large buttons; an IDE packs dozens of 16px
//! controls into a 28px toolbar. `f68001_is_below_the_floor_on_white` below
//! records that measurement next to the constant it rules out.

use gpui::{App, Hsla};
use theme::{ActiveTheme, Appearance};

/// Packed rgb rather than [`Hsla`], because `Hsla` has no const constructor --
/// the same reason `agent_ui::agent_roster`'s mark table stores `u32`.
pub(crate) const BRAND_ACCENT_LIGHT: u32 = 0xAB4501;
pub(crate) const BRAND_ACCENT_DARK: u32 = 0xFFA65E;

pub(crate) const BRAND_SOLID_LIGHT: u32 = 0xAB4501;
pub(crate) const BRAND_SOLID_DARK: u32 = 0xF68001;

/// The pressed states move the fill *away from the canvas* -- darker on light,
/// lighter on dark -- rather than always darker.
///
/// `Theme::darken` only ever reduces lightness, which is correct for a tint sitting
/// near the surface but wrong for a saturated fill: the dark label on the dark
/// theme's fill measures 3.65:1 once that fill is darkened two steps, under the
/// 4.5:1 floor. Moving away from the canvas instead raises contrast as the button
/// is pressed, in both appearances.
pub(crate) const BRAND_SOLID_HOVER_LIGHT: u32 = 0x8D3901;
pub(crate) const BRAND_SOLID_HOVER_DARK: u32 = 0xFE8F18;

pub(crate) const BRAND_SOLID_ACTIVE_LIGHT: u32 = 0x6E2C01;
pub(crate) const BRAND_SOLID_ACTIVE_DARK: u32 = 0xFE9E36;

pub(crate) const BRAND_ON_SOLID_LIGHT: u32 = 0xFFFFFF;
pub(crate) const BRAND_ON_SOLID_DARK: u32 = 0x341A07;

/// The single light/dark branch in the brand layer.
///
/// Kept as one function so a later surface cannot quietly grow a second one --
/// a second `if is_light` anywhere downstream means the split landed in the
/// wrong place.
fn pick(appearance: Appearance, light: u32, dark: u32) -> Hsla {
    let packed = if appearance == Appearance::Light {
        light
    } else {
        dark
    };
    gpui::rgb(packed).into()
}

/// The brand accent: an icon that is active, a switch that is on, a checkbox
/// that is checked.
pub fn brand_accent(cx: &App) -> Hsla {
    pick(
        cx.theme().appearance(),
        BRAND_ACCENT_LIGHT,
        BRAND_ACCENT_DARK,
    )
}

/// The fill behind a solid brand button.
pub fn brand_solid(cx: &App) -> Hsla {
    pick(cx.theme().appearance(), BRAND_SOLID_LIGHT, BRAND_SOLID_DARK)
}

/// [`brand_solid`] with the pointer over it.
pub fn brand_solid_hovered(cx: &App) -> Hsla {
    pick(
        cx.theme().appearance(),
        BRAND_SOLID_HOVER_LIGHT,
        BRAND_SOLID_HOVER_DARK,
    )
}

/// [`brand_solid`] while it is being pressed.
pub fn brand_solid_active(cx: &App) -> Hsla {
    pick(
        cx.theme().appearance(),
        BRAND_SOLID_ACTIVE_LIGHT,
        BRAND_SOLID_ACTIVE_DARK,
    )
}

/// The label and icon colour to use *on top of* [`brand_solid`].
///
/// Not a general foreground -- it is only legible against the solid fill, and
/// is wrong anywhere else.
pub fn brand_on_solid(cx: &App) -> Hsla {
    pick(
        cx.theme().appearance(),
        BRAND_ON_SOLID_LIGHT,
        BRAND_ON_SOLID_DARK,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::calculate_contrast_ratio;

    /// The two backgrounds the palette was measured against: the shipped
    /// `Light 2026` and `Dark 2026` canvases.
    const LIGHT_CANVAS: u32 = 0xFFFFFF;
    const DARK_CANVAS: u32 = 0x121314;

    fn ratio(fg: u32, bg: u32) -> f32 {
        calculate_contrast_ratio(gpui::rgb(fg).into(), gpui::rgb(bg).into())
    }

    /// Reads the constants rather than colour literals, so editing a constant
    /// is what turns this red.
    #[test]
    fn brand_palette_meets_its_contrast_floors() {
        assert!(
            ratio(BRAND_ACCENT_LIGHT, LIGHT_CANVAS) >= 4.5,
            "accent on the light canvas fell under 4.5:1"
        );
        assert!(
            ratio(BRAND_ACCENT_DARK, DARK_CANVAS) >= 4.5,
            "accent on the dark canvas fell under 4.5:1"
        );

        // Every state of the solid button, not just its resting fill: darkening
        // a saturated fill is what silently took the dark theme's label under
        // the floor, so each state is measured rather than assumed to follow.
        for (state, light, dark) in [
            ("enabled", BRAND_SOLID_LIGHT, BRAND_SOLID_DARK),
            ("hovered", BRAND_SOLID_HOVER_LIGHT, BRAND_SOLID_HOVER_DARK),
            ("active", BRAND_SOLID_ACTIVE_LIGHT, BRAND_SOLID_ACTIVE_DARK),
        ] {
            assert!(
                ratio(light, LIGHT_CANVAS) >= 3.0,
                "the light solid fill lost its boundary against the canvas when {state}"
            );
            assert!(
                ratio(dark, DARK_CANVAS) >= 3.0,
                "the dark solid fill lost its boundary against the canvas when {state}"
            );
            assert!(
                ratio(BRAND_ON_SOLID_LIGHT, light) >= 4.5,
                "the light solid button's label fell under 4.5:1 on its own fill when {state}"
            );
            assert!(
                ratio(BRAND_ON_SOLID_DARK, dark) >= 4.5,
                "the dark solid button's label fell under 4.5:1 on its own fill when {state}"
            );
        }
    }

    /// This guards nothing by itself. It records, next to the constant someone
    /// will one day "correct" to match the frontend, why `BRAND_SOLID_LIGHT` is
    /// not `#F68001`.
    #[test]
    fn f68001_is_below_the_floor_on_white() {
        let measured = ratio(0xF68001, LIGHT_CANVAS);
        assert!(
            measured < 3.0,
            "#F68001 now measures {measured:.2} on white; if this ever passes 3.0 the \
             reason BRAND_SOLID_LIGHT differs from the frontend has gone away"
        );
    }

    /// Catches the branch being dropped -- a `pick` that ignored its appearance
    /// would still compile and still return a brand colour.
    #[test]
    fn brand_answers_differently_per_appearance() {
        for (light, dark) in [
            (BRAND_ACCENT_LIGHT, BRAND_ACCENT_DARK),
            (BRAND_SOLID_LIGHT, BRAND_SOLID_DARK),
            (BRAND_SOLID_HOVER_LIGHT, BRAND_SOLID_HOVER_DARK),
            (BRAND_SOLID_ACTIVE_LIGHT, BRAND_SOLID_ACTIVE_DARK),
            (BRAND_ON_SOLID_LIGHT, BRAND_ON_SOLID_DARK),
        ] {
            assert_ne!(
                pick(Appearance::Light, light, dark),
                pick(Appearance::Dark, light, dark),
                "a brand role answered the same in both appearances"
            );
        }
    }
}
