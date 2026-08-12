mod agent_notification;
mod mention_crease;


/// Upstream picked this side from the agent panel's dock position. Agents live in
/// the centre here, beside the editor, so the aside goes on the side the pane's
/// own rail sits on — the same setting everything else in this feature reads.
pub fn documentation_aside_side(cx: &gpui::App) -> ui::DocumentationSide {
    use settings::Settings as _;
    use workspace::{SidebarSide, WorkspaceSettings};

    match WorkspaceSettings::get_global(cx).multi_project.sidebar_side {
        SidebarSide::Left => ui::DocumentationSide::Right,
        SidebarSide::Right => ui::DocumentationSide::Left,
    }
}
