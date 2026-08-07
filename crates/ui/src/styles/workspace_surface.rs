use gpui::{App, Styled};
use theme::ActiveTheme;

/// The workspace's panel surfaces — the docks (left, right and the terminal at
/// the bottom), the center pane group, and the project sidebar — read as
/// separate panels: spaced apart, rounded, and outlined.
///
/// The title bar and the status bar are deliberately NOT surfaces. They span the
/// window edge to edge, the way VS Code treats them: only the sidebar, panel and
/// editor take part in its floating-panel layout.
///
/// The outline is not decoration. A seam only shows whatever sits *behind* the
/// surfaces, and a theme is free to give that background the same value as the
/// editor — VS Code's own 2026 themes do, both being `#121314` — which leaves
/// the gap invisible however wide it is. VS Code pairs its floating-panel
/// margin with a 1px `surface-border` for exactly this reason.
pub trait WorkspaceSurface: Styled + Sized {
    /// Spaces, rounds and outlines a top-level workspace surface.
    fn workspace_surface(self, cx: &App) -> Self {
        self.m(theme::WORKSPACE_SURFACE_MARGIN)
            .rounded(theme::WORKSPACE_SURFACE_ROUNDING)
            .border_1()
            .border_color(cx.theme().colors().border)
            .overflow_hidden()
    }
}

impl<E: Styled> WorkspaceSurface for E {}
