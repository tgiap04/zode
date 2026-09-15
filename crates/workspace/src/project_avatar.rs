//! The one element every surface draws a project's avatar with.
//!
//! It lives in `workspace` rather than `sidebar`, even though every surface
//! that will use it besides `DraggedProject` (the rail square and the panel
//! row, both `sidebar`) is over there: `DraggedProject` is a `workspace` type,
//! and `workspace` cannot depend on `sidebar`. Three surfaces, one dependency
//! direction, one element.
//!
//! A missing or corrupted logo file falls back to initials through
//! `StyledImage::with_fallback`, never through a check of whether the file
//! exists. `Path::exists()` reads the real disk regardless of the `Fs` a test
//! injects, so it would report a `FakeFs`-only file as absent and a file
//! deleted from behind a real `Fs`'s back as present -- wrong in both
//! directions. `with_fallback` asks the same question the renderer is about
//! to ask anyway: can this actually be decoded.

use std::path::Path;
use std::sync::Arc;

use gpui::{Hsla, StyledImage, img};
use ui::prelude::*;

/// The debug selector on the element drawn when a logo is present, whether or
/// not it decodes.
pub const LOGO_DEBUG_SELECTOR: &str = "PROJECT_AVATAR_LOGO";
/// The debug selector on the element drawn for the initials fallback.
pub const INITIALS_DEBUG_SELECTOR: &str = "PROJECT_AVATAR_INITIALS";

/// Background, then a logo or initials -- the one thing every project avatar
/// draws. Size, border, and decoration belong to the surface, not here.
#[derive(IntoElement)]
pub struct ProjectAvatar {
    initials: SharedString,
    colour: Option<Hsla>,
    logo: Option<Arc<Path>>,
    size: Pixels,
    background: Hsla,
    muted_initials: bool,
}

impl ProjectAvatar {
    pub fn new(
        initials: impl Into<SharedString>,
        colour: Option<Hsla>,
        logo: Option<Arc<Path>>,
    ) -> Self {
        Self {
            initials: initials.into(),
            colour,
            logo,
            size: px(32.),
            background: gpui::transparent_black(),
            muted_initials: false,
        }
    }

    /// Draws the initials muted rather than at full strength.
    ///
    /// Only reaches the initials that fall back to a themed colour: initials
    /// over a colour someone picked stay computed for contrast, which is the
    /// whole reason that branch exists. The rail asks for this on a project
    /// that is not the active one, so a column of squares reads as one active
    /// and the rest at rest.
    pub fn muted_initials(mut self, muted: bool) -> Self {
        self.muted_initials = muted;
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn background(mut self, background: Hsla) -> Self {
        self.background = background;
        self
    }
}

impl RenderOnce for ProjectAvatar {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let background = self.colour.unwrap_or(self.background);
        let initials = self.initials;
        let label_colour = self.colour;
        let muted_initials = self.muted_initials;

        let initials_element = move || {
            div()
                .debug_selector(|| INITIALS_DEBUG_SELECTOR.to_string())
                .child(Label::new(initials.clone()).size(LabelSize::Small).color(
                    match label_colour {
                        Some(colour) => {
                            Color::Custom(crate::project_appearance::label_colour_for(colour))
                        }
                        None if muted_initials => Color::Muted,
                        None => Color::Default,
                    },
                ))
                .into_any_element()
        };

        let content = match self.logo {
            Some(logo) => img(logo)
                .size_full()
                .rounded_md()
                .debug_selector(|| LOGO_DEBUG_SELECTOR.to_string())
                .with_fallback(initials_element)
                .into_any_element(),
            None => initials_element(),
        };

        div()
            .size(self.size)
            .rounded_md()
            .bg(background)
            .flex()
            .items_center()
            .justify_center()
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Render, TestAppContext};

    fn init_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });
    }

    struct Harness {
        initials: SharedString,
        colour: Option<Hsla>,
        logo: Option<Arc<Path>>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            ProjectAvatar::new(self.initials.clone(), self.colour, self.logo.clone())
                .size(px(32.))
                .background(gpui::black())
        }
    }

    #[gpui::test]
    fn an_avatar_without_a_logo_still_draws_its_initials(cx: &mut TestAppContext) {
        init_test(cx);
        let (_view, cx) = cx.add_window_view(|_, _| Harness {
            initials: "AB".into(),
            colour: Some(gpui::blue()),
            logo: None,
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds(INITIALS_DEBUG_SELECTOR).is_some());
        assert!(cx.debug_bounds(LOGO_DEBUG_SELECTOR).is_none());
    }

    #[gpui::test]
    fn an_avatar_with_a_logo_draws_the_image_element(cx: &mut TestAppContext) {
        init_test(cx);
        let logo: Arc<Path> = Arc::from(Path::new("/nonexistent/logo.png"));
        let (_view, cx) = cx.add_window_view(|_, _| Harness {
            initials: "AB".into(),
            colour: Some(gpui::blue()),
            logo: Some(logo),
        });
        cx.run_until_parked();

        // `ImgResourceLoader` reads the real disk (`img.rs:613`, `fs::read`), so
        // this path never decodes in a test. The element itself must still be
        // there -- pixels are not asserted, never can be here.
        assert!(cx.debug_bounds(LOGO_DEBUG_SELECTOR).is_some());
    }
}
