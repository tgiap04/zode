//! The window's see-through settings, applied to a theme: how opaque its
//! surfaces are, and what the window lets show behind them.
//!
//! Applied after the theme's own overrides, because these are settings about
//! the window rather than about any one theme: they hold whichever theme is
//! picked.

use gpui::WindowBackgroundAppearance;
use theme::Theme;

/// What the user asked of the window, read from settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowEffects {
    /// 0 is see-through, 1 is solid.
    pub opacity: f32,
    /// Whether what is behind the window shows through, blurred.
    pub vibrancy: bool,
    /// Whether a background image is set. Its path is the backdrop's business;
    /// the theme only needs to know that something is behind the surfaces.
    pub has_background_image: bool,
}

impl WindowEffects {
    /// Nothing asked: the theme is left exactly as it was, its own
    /// `background.appearance` included.
    pub fn is_default(&self) -> bool {
        self.opacity >= 1.0 && !self.vibrancy && !self.has_background_image
    }
}

/// Picks what the window shows behind its contents. The theme's colours are
/// left alone: the window fades as a whole when it is composited (see
/// `gpui::Window::set_content_opacity`), because fading each surface could
/// never be even -- surfaces are stacked, and the editor, sitting on more
/// layers than the title bar, stayed solid long after the bars had gone.
pub fn apply_window_effects(theme: &mut Theme, effects: WindowEffects) {
    if effects.is_default() {
        return;
    }
    theme.styles.window_background_appearance = if effects.has_background_image {
        // The image is drawn by the window itself, behind everything, so the
        // desktop is never seen and the window can stay opaque.
        WindowBackgroundAppearance::Opaque
    } else if effects.vibrancy {
        WindowBackgroundAppearance::Vibrancy
    } else if effects.opacity >= 1.0 {
        theme.styles.window_background_appearance
    } else {
        WindowBackgroundAppearance::Transparent
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        let registry = theme::ThemeRegistry::new(Box::new(()));
        (*registry
            .get(theme::DEFAULT_DARK_THEME)
            .expect("the built-in dark theme is always registered"))
        .clone()
    }

    fn appearance_for(
        opacity: f32,
        vibrancy: bool,
        has_background_image: bool,
    ) -> WindowBackgroundAppearance {
        let mut theme = theme();
        apply_window_effects(
            &mut theme,
            WindowEffects {
                opacity,
                vibrancy,
                has_background_image,
            },
        );
        theme.styles.window_background_appearance
    }

    #[test]
    fn untouched_settings_leave_the_theme_alone() {
        assert_eq!(
            appearance_for(1.0, false, false),
            theme().styles.window_background_appearance
        );
    }

    /// The window fades as a whole; every surface keeps its colour so the
    /// fade is the same everywhere.
    #[test]
    fn surfaces_keep_their_colour() {
        let before = theme();
        let mut after = theme();
        apply_window_effects(
            &mut after,
            WindowEffects {
                opacity: 0.4,
                vibrancy: true,
                has_background_image: false,
            },
        );
        assert_eq!(
            after.styles.colors.editor_background,
            before.styles.colors.editor_background
        );
        assert_eq!(
            after.styles.colors.title_bar_background,
            before.styles.colors.title_bar_background
        );
    }

    #[test]
    fn a_see_through_window_shows_the_desktop_plain_or_through_vibrancy() {
        assert_eq!(
            appearance_for(0.8, false, false),
            WindowBackgroundAppearance::Transparent
        );
        assert_eq!(
            appearance_for(0.4, true, false),
            WindowBackgroundAppearance::Vibrancy
        );
    }

    #[test]
    fn an_image_keeps_the_window_opaque_with_or_without_vibrancy() {
        assert_eq!(
            appearance_for(0.4, true, true),
            WindowBackgroundAppearance::Opaque
        );
        assert_eq!(
            appearance_for(0.4, false, true),
            WindowBackgroundAppearance::Opaque
        );
    }
}
