pub mod platforms;
mod system_window_tabs;

use gpui::{
    Action, AnyElement, App, Context, Decorations, Entity, Hsla, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, StatefulInteractiveElement, Styled, Tiling, WeakEntity,
    Window, WindowButtonLayout, WindowControlArea, div, px,
};
use project::DisableAiSettings;
use settings::Settings;
use smallvec::SmallVec;
use std::mem;
use ui::{
    prelude::*,
    utils::{TRAFFIC_LIGHT_PADDING, platform_title_bar_height},
};
use workspace::{MultiWorkspace, SidebarRenderState};

use crate::{
    platforms::{platform_linux, platform_windows},
    system_window_tabs::SystemWindowTabs,
};

pub use system_window_tabs::{
    DraggedWindowTab, MergeAllWindows, MoveTabToNewWindow, ShowNextWindowTab, ShowPreviousWindowTab,
};

pub struct PlatformTitleBar {
    id: ElementId,
    platform_style: PlatformStyle,
    children: SmallVec<[AnyElement; 2]>,
    should_move: bool,
    system_window_tabs: Entity<SystemWindowTabs>,
    button_layout: Option<WindowButtonLayout>,
    multi_workspace: Option<WeakEntity<MultiWorkspace>>,
}

impl PlatformTitleBar {
    pub fn new(id: impl Into<ElementId>, cx: &mut Context<Self>) -> Self {
        let platform_style = PlatformStyle::platform();
        let system_window_tabs = cx.new(|_cx| SystemWindowTabs::new());

        Self {
            id: id.into(),
            platform_style,
            children: SmallVec::new(),
            should_move: false,
            system_window_tabs,
            button_layout: None,
            multi_workspace: None,
        }
    }

    pub fn with_multi_workspace(mut self, multi_workspace: WeakEntity<MultiWorkspace>) -> Self {
        self.multi_workspace = Some(multi_workspace);
        self
    }

    pub fn set_multi_workspace(&mut self, multi_workspace: WeakEntity<MultiWorkspace>) {
        self.multi_workspace = Some(multi_workspace);
    }

    pub fn title_bar_color(&self, window: &mut Window, cx: &mut Context<Self>) -> Hsla {
        if cfg!(any(target_os = "linux", target_os = "freebsd")) {
            if window.is_window_active() && !self.should_move {
                cx.theme().colors().title_bar_background
            } else {
                cx.theme().colors().title_bar_inactive_background
            }
        } else {
            cx.theme().colors().title_bar_background
        }
    }

    pub fn set_children<T>(&mut self, children: T)
    where
        T: IntoIterator<Item = AnyElement>,
    {
        self.children = children.into_iter().collect();
    }

    pub fn set_button_layout(&mut self, button_layout: Option<WindowButtonLayout>) {
        self.button_layout = button_layout;
    }

    fn effective_button_layout(
        &self,
        decorations: &Decorations,
        cx: &App,
    ) -> Option<WindowButtonLayout> {
        if self.platform_style == PlatformStyle::Linux
            && matches!(decorations, Decorations::Client { .. })
        {
            self.button_layout.or_else(|| cx.button_layout())
        } else {
            None
        }
    }

    pub fn init(cx: &mut App) {
        SystemWindowTabs::init(cx);
    }

    fn sidebar_render_state(&self, cx: &App) -> SidebarRenderState {
        self.multi_workspace
            .as_ref()
            .and_then(|mw| mw.upgrade())
            .map(|mw| mw.read(cx).sidebar_render_state(cx))
            .unwrap_or_default()
    }

    pub fn is_multi_workspace_enabled(cx: &App) -> bool {
        !DisableAiSettings::get_global(cx).disable_ai
    }
}

/// Renders the platform-appropriate left-side window controls (e.g. Ubuntu/GNOME close button).
///
/// Only relevant on Linux with client-side decorations when the window manager
/// places controls on the left.
pub fn render_left_window_controls(
    button_layout: Option<WindowButtonLayout>,
    close_action: Box<dyn Action>,
    window: &Window,
) -> Option<AnyElement> {
    if PlatformStyle::platform() != PlatformStyle::Linux {
        return None;
    }
    if !matches!(window.window_decorations(), Decorations::Client { .. }) {
        return None;
    }
    let button_layout = button_layout?;
    if button_layout.left[0].is_none() {
        return None;
    }
    Some(
        platform_linux::LinuxWindowControls::new(
            "left-window-controls",
            button_layout.left,
            close_action,
        )
        .into_any_element(),
    )
}

/// Renders the platform-appropriate right-side window controls (close, minimize, maximize).
///
/// Returns `None` on Mac or when the platform doesn't need custom controls
/// (e.g. Linux with server-side decorations).
pub fn render_right_window_controls(
    button_layout: Option<WindowButtonLayout>,
    close_action: Box<dyn Action>,
    window: &Window,
) -> Option<AnyElement> {
    let decorations = window.window_decorations();
    let height = platform_title_bar_height(window);

    match PlatformStyle::platform() {
        PlatformStyle::Linux => {
            if !matches!(decorations, Decorations::Client { .. }) {
                return None;
            }
            let button_layout = button_layout?;
            if button_layout.right[0].is_none() {
                return None;
            }
            Some(
                platform_linux::LinuxWindowControls::new(
                    "right-window-controls",
                    button_layout.right,
                    close_action,
                )
                .into_any_element(),
            )
        }
        PlatformStyle::Windows => {
            Some(platform_windows::WindowsWindowControls::new(height).into_any_element())
        }
        PlatformStyle::Mac => None,
    }
}

impl Render for PlatformTitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let supported_controls = window.window_controls();
        let decorations = window.window_decorations();
        let height = platform_title_bar_height(window);
        let titlebar_color = self.title_bar_color(window, cx);
        let close_action = Box::new(workspace::CloseWindow);
        let children = mem::take(&mut self.children);

        let button_layout = self.effective_button_layout(&decorations, cx);
        let sidebar = self.sidebar_render_state(cx);

        let title_bar = h_flex()
            .window_control_area(WindowControlArea::Drag)
            .w_full()
            .h(height)
            .map(|this| {
                this.on_mouse_down_out(cx.listener(move |this, _ev, _window, _cx| {
                    this.should_move = false;
                }))
                .on_mouse_up(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _ev, _window, _cx| {
                        this.should_move = false;
                    }),
                )
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(move |this, _ev, _window, _cx| {
                        this.should_move = true;
                    }),
                )
                .on_mouse_move(cx.listener(move |this, _ev, window, _| {
                    if this.should_move {
                        this.should_move = false;
                        window.start_window_move();
                    }
                }))
            })
            .map(|this| {
                // Note: On Windows the title bar behavior is handled by the platform implementation.
                this.id(self.id.clone())
                    .when(self.platform_style == PlatformStyle::Mac, |this| {
                        this.on_click(|event, window, _| {
                            if event.click_count() == 2 {
                                window.titlebar_double_click();
                            }
                        })
                    })
                    .when(self.platform_style == PlatformStyle::Linux, |this| {
                        this.on_click(|event, window, _| {
                            if event.click_count() == 2 {
                                window.zoom_window();
                            }
                        })
                    })
            })
            .map(
                |this| match leading_edge(self.platform_style, window.is_fullscreen(), &sidebar) {
                    LeadingEdge::Padding => this.pl_2(),
                    LeadingEdge::TrafficLightInset(inset) => this.pl(inset),
                    LeadingEdge::WindowControls => match render_left_window_controls(
                        button_layout,
                        close_action.as_ref().boxed_clone(),
                        window,
                    ) {
                        Some(controls) => this.child(controls),
                        None => this.pl_2(),
                    },
                },
            )
            .map(|el| match decorations {
                Decorations::Server => el,
                Decorations::Client { tiling, .. } => el
                    .when(!(tiling.top || tiling.right), |el| {
                        el.rounded_tr(theme::CLIENT_SIDE_DECORATION_ROUNDING)
                    })
                    .when(rounds_top_left(tiling, &sidebar), |el| {
                        el.rounded_tl(theme::CLIENT_SIDE_DECORATION_ROUNDING)
                    })
                    // this border is to avoid a transparent gap in the rounded corners
                    .mt(px(-1.))
                    .mb(px(-1.))
                    .border(px(1.))
                    .border_color(titlebar_color),
            })
            .bg(titlebar_color)
            .content_stretch()
            .child(
                div()
                    .id(self.id.clone())
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .overflow_x_hidden()
                    .w_full()
                    // The last child is the layout-control cluster, which would
                    // otherwise sit flush against the window edge. Absolute px,
                    // like the traffic-light inset above: the gap to a window edge
                    // should not shrink with the UI font.
                    .pr(px(8.))
                    .children(children),
            )
            .when(!window.is_fullscreen(), |title_bar| {
                // Unconditional: the sidebar stands on the left, so it can never
                // be under these.
                let title_bar = title_bar.children(render_right_window_controls(
                    button_layout,
                    close_action.as_ref().boxed_clone(),
                    window,
                ));

                if self.platform_style == PlatformStyle::Linux
                    && matches!(decorations, Decorations::Client { .. })
                {
                    title_bar.when(supported_controls.window_menu, |titlebar| {
                        titlebar.on_mouse_down(MouseButton::Right, move |ev, window, _| {
                            window.show_window_menu(ev.position)
                        })
                    })
                } else {
                    title_bar
                }
            });

        v_flex()
            .w_full()
            .child(title_bar)
            .child(self.system_window_tabs.clone().into_any_element())
    }
}

/// What sits at the title bar's leading edge, before its content.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LeadingEdge {
    Padding,
    /// Room for the window controls macOS draws over the window's corner.
    TrafficLightInset(Pixels),
    /// Linux's client-side controls when the window manager puts them on the
    /// left; plain padding when it does not.
    WindowControls,
}

fn leading_edge(
    platform_style: PlatformStyle,
    is_fullscreen: bool,
    sidebar: &SidebarRenderState,
) -> LeadingEdge {
    // Beside the bar, the sidebar sits on the window edge even with the panel
    // closed, so macOS's window controls (drawn by the OS over that same
    // corner) land partly on it -- but the rail alone is narrower than the
    // strip they occupy, so reserve however much of it the sidebar does not
    // already cover. A bar above the sidebar owns the whole corner.
    let covered_by_sidebar = if sidebar.below_title_bar {
        px(0.)
    } else {
        sidebar.edge_width
    };
    let traffic_light_inset = px(TRAFFIC_LIGHT_PADDING) - covered_by_sidebar;
    if is_fullscreen {
        LeadingEdge::Padding
    } else if platform_style == PlatformStyle::Mac && traffic_light_inset > px(0.) {
        LeadingEdge::TrafficLightInset(traffic_light_inset)
    } else if sidebar.below_title_bar || !sidebar.open {
        LeadingEdge::WindowControls
    } else {
        LeadingEdge::Padding
    }
}

/// Matches the window's own top-left corner, which the backdrop squares off
/// whenever the sidebar sits against the window's left edge.
fn rounds_top_left(tiling: Tiling, sidebar: &SidebarRenderState) -> bool {
    // Beside the bar, either component of the sidebar reaching the left edge
    // is enough to square that corner off. Above the sidebar the bar owns the
    // corner, and the backdrop squares it only while the panel is open.
    // Nothing puts the sidebar on the right, so the top-right corner is
    // unconditional.
    !(tiling.top || tiling.left) && !sidebar.open && (sidebar.below_title_bar || !sidebar.rail)
}

impl ParentElement for PlatformTitleBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sidebar(open: bool, rail: bool, below_title_bar: bool) -> SidebarRenderState {
        let rail_width = if rail { px(48.) } else { px(0.) };
        let panel_width = if open { px(300.) } else { px(0.) };
        SidebarRenderState {
            open,
            rail,
            edge_width: rail_width + panel_width,
            below_title_bar,
        }
    }

    #[test]
    fn leading_edge_beside_the_sidebar() {
        let rail_only = sidebar(false, true, false);
        assert_eq!(
            leading_edge(PlatformStyle::Mac, false, &rail_only),
            LeadingEdge::TrafficLightInset(px(TRAFFIC_LIGHT_PADDING) - rail_only.edge_width),
            "macOS reserves what the rail leaves of the traffic-light strip"
        );
        assert_eq!(
            leading_edge(PlatformStyle::Mac, false, &sidebar(false, false, false)),
            LeadingEdge::TrafficLightInset(px(TRAFFIC_LIGHT_PADDING)),
        );
        assert_eq!(
            leading_edge(PlatformStyle::Mac, false, &sidebar(true, true, false)),
            LeadingEdge::Padding,
            "the open panel already covers the traffic lights"
        );
        for platform_style in [PlatformStyle::Linux, PlatformStyle::Windows] {
            assert_eq!(
                leading_edge(platform_style, false, &rail_only),
                LeadingEdge::WindowControls,
            );
            assert_eq!(
                leading_edge(platform_style, false, &sidebar(true, true, false)),
                LeadingEdge::Padding,
            );
        }
    }

    #[test]
    fn leading_edge_above_the_sidebar() {
        for platform_style in [PlatformStyle::Linux, PlatformStyle::Windows] {
            for open in [false, true] {
                for rail in [false, true] {
                    let sidebar = sidebar(open, rail, true);
                    assert_eq!(
                        leading_edge(platform_style, false, &sidebar),
                        LeadingEdge::WindowControls,
                        "a bar above the sidebar owns the window's corner \
                         ({platform_style:?}, open {open}, rail {rail})"
                    );
                    assert_eq!(
                        leading_edge(platform_style, true, &sidebar),
                        LeadingEdge::Padding,
                        "fullscreen has no window controls ({platform_style:?})"
                    );
                }
            }
        }
        assert_eq!(
            leading_edge(PlatformStyle::Mac, false, &sidebar(true, true, true)),
            LeadingEdge::TrafficLightInset(px(TRAFFIC_LIGHT_PADDING)),
            "above the sidebar nothing covers the traffic lights for the bar"
        );
    }

    fn tiling(top: bool, left: bool) -> Tiling {
        Tiling {
            top,
            left,
            ..Tiling::default()
        }
    }

    #[test]
    fn rounds_top_left_beside_the_sidebar() {
        for (top, left) in [(false, false), (true, false), (false, true), (true, true)] {
            for open in [false, true] {
                for rail in [false, true] {
                    assert_eq!(
                        rounds_top_left(tiling(top, left), &sidebar(open, rail, false)),
                        !(top || left || open || rail),
                        "beside: top {top}, left {left}, open {open}, rail {rail}"
                    );
                }
            }
        }
    }

    #[test]
    fn rounds_top_left_above_the_sidebar() {
        // Above the sidebar the bar owns the window's top-left corner, so it
        // follows the backdrop, which squares that corner only while the panel
        // is open (`Tiling.left` in `MultiWorkspace::render`).
        for (top, left) in [(false, false), (true, false), (false, true), (true, true)] {
            for open in [false, true] {
                for rail in [false, true] {
                    assert_eq!(
                        rounds_top_left(tiling(top, left), &sidebar(open, rail, true)),
                        !(top || left || open),
                        "above: top {top}, left {left}, open {open}, rail {rail}"
                    );
                }
            }
        }
    }
}
