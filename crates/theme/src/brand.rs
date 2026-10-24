//! Zode's brand palette, and the step that lays it over every theme.
//!
//! The brand is a fact about the app, not about the file being edited, so it reads
//! the same whichever theme is picked: active icons, focus rings, spinners and
//! progress, links and accent text, and the local cursor and selection all carry
//! it. Syntax, surfaces, status colours and collaborators stay with the theme.
//! [`apply_brand_colors`] runs before the user's theme overrides, so every token it
//! writes can still be taken back per theme.
//!
//! The values come from the `web/frontend` palette. Its icon token is
//! `--primary-strong`, not the raw `--primary`, and that split is load-bearing:
//! `#F68001` measures 2.62 against white, under even the 3:1 floor WCAG 1.4.11
//! sets for a control's boundary. The frontend uses it on white anyway because it
//! is a landing page with sparse, large buttons; an IDE packs dozens of 16px
//! controls into a 28px toolbar. The `ui` crate's
//! `f68001_is_below_the_floor_on_white` records that measurement next to the
//! palette's other contrast checks.

use gpui::Hsla;

use crate::{Appearance, PlayerColor, Theme};

// Packed rgb rather than `Hsla`, because `Hsla` has no const constructor.
const ACCENT_LIGHT: u32 = 0xAB4501;
const ACCENT_DARK: u32 = 0xFFA65E;

const SOLID_LIGHT: u32 = 0xAB4501;
const SOLID_DARK: u32 = 0xF68001;

// The pressed states move the fill away from the canvas -- darker on light,
// lighter on dark -- rather than always darker. Darkening a saturated fill took
// the dark theme's label to 3.65:1 two steps in, under the 4.5:1 floor; moving
// away from the canvas raises contrast as the button is pressed instead.
const SOLID_HOVER_LIGHT: u32 = 0x8D3901;
const SOLID_HOVER_DARK: u32 = 0xFE8F18;

const SOLID_ACTIVE_LIGHT: u32 = 0x6E2C01;
const SOLID_ACTIVE_DARK: u32 = 0xFE9E36;

const ON_SOLID_LIGHT: u32 = 0xFFFFFF;
const ON_SOLID_DARK: u32 = 0x341A07;

// A selection is laid under the text, so it has to stay translucent. Dark needs the
// heavier alpha: at 0x3d the brand orange over a #121314 canvas is about 1.65:1, too
// faint to see where the selection ends. Light keeps 0x3d, the alpha the bundled
// themes give almost every player's selection.
const SELECTION_ALPHA_LIGHT: f32 = 0x3d as f32 / 255.0;
const SELECTION_ALPHA_DARK: f32 = 0x66 as f32 / 255.0;
// The alpha the default 2026 themes give every player's background.
const LOCAL_BACKGROUND_ALPHA: f32 = 0x4d as f32 / 255.0;

/// Zode's brand colours, resolved for one appearance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrandColors {
    /// An indicator drawn on the canvas: an active icon, a link, a focus ring, a
    /// spinner, the local cursor.
    pub accent: Hsla,
    /// The fill behind a solid brand button.
    pub solid: Hsla,
    /// [`Self::solid`] with the pointer over it.
    pub solid_hovered: Hsla,
    /// [`Self::solid`] while it is being pressed.
    pub solid_active: Hsla,
    /// The label and icon colour on top of [`Self::solid`]; only legible there.
    pub on_solid: Hsla,
    /// The fill behind selected text, translucent so the text stays legible.
    pub selection: Hsla,
}

impl BrandColors {
    /// The brand for `appearance`.
    ///
    /// This is the brand layer's only light/dark branch, so a later surface cannot
    /// quietly grow a second one -- a second `if is_light` downstream means the
    /// split landed in the wrong place.
    pub fn for_appearance(appearance: Appearance) -> Self {
        let (accent, solid, solid_hovered, solid_active, on_solid, selection_alpha) =
            match appearance {
                Appearance::Light => (
                    ACCENT_LIGHT,
                    SOLID_LIGHT,
                    SOLID_HOVER_LIGHT,
                    SOLID_ACTIVE_LIGHT,
                    ON_SOLID_LIGHT,
                    SELECTION_ALPHA_LIGHT,
                ),
                Appearance::Dark => (
                    ACCENT_DARK,
                    SOLID_DARK,
                    SOLID_HOVER_DARK,
                    SOLID_ACTIVE_DARK,
                    ON_SOLID_DARK,
                    SELECTION_ALPHA_DARK,
                ),
            };
        Self {
            accent: gpui::rgb(accent).into(),
            solid: gpui::rgb(solid).into(),
            solid_hovered: gpui::rgb(solid_hovered).into(),
            solid_active: gpui::rgb(solid_active).into(),
            on_solid: gpui::rgb(on_solid).into(),
            selection: Hsla::from(gpui::rgb(accent)).alpha(selection_alpha),
        }
    }

    /// The local player's cursor, selection and background.
    pub fn local_player(&self) -> PlayerColor {
        PlayerColor {
            cursor: self.accent,
            background: self.accent.alpha(LOCAL_BACKGROUND_ALPHA),
            selection: self.selection,
        }
    }
}

/// Lays the brand over `theme`'s accent text, accent icons, focus border, text
/// selection and local player.
///
/// Only the first player moves: the others are collaborators, and keeping the
/// theme's colours for them is what tells them apart from the local cursor.
pub fn apply_brand_colors(theme: &mut Theme) {
    let brand = BrandColors::for_appearance(theme.appearance);
    let colors = &mut theme.styles.colors;
    colors.text_accent = brand.accent;
    colors.icon_accent = brand.accent;
    colors.border_focused = brand.accent;
    // Loading derived this from the theme's own selection, and no bundled theme sets
    // it, so without this the selected text outside the editor would stay blue.
    colors.element_selection_background = brand.selection;
    if let Some(local) = theme.styles.player.0.first_mut() {
        *local = brand.local_player();
    }
}
