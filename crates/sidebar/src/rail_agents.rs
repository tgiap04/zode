use crate::Sidebar;
use crate::rail::{RAIL_ICON_GAP, RAIL_ICON_SIZE};
use agent_ui::AgentView;
use gpui::{AnyElement, App, Context, Window};
use project::AgentId;
use ui::{Tooltip, prelude::*};
use zed_actions::agent::{AgentViewMode, OpenAgent, ToggleAgent};

/// The agents the rail draws a button for.
///
/// Hard-coded rather than read from `AgentServerStore`, unlike the panel buttons
/// above: an agent's glyph is its vendor's mark, so a new entry is a deliberate
/// choice of icon, not something to be derived. Keep in step with
/// `agent_ui::agent_icon`, which draws the same glyph on the tab.
const RAIL_AGENTS: &[(&str, IconName, &str)] = &[
    (
        project::CLAUDE_CODE_AGENT_ID,
        IconName::AiClaude,
        "Claude Code",
    ),
    (project::CODEX_AGENT_ID, IconName::AiOpenAi, "Codex"),
];

impl Sidebar {
    /// Whether a tab for this agent is open anywhere in the editor's panes.
    ///
    /// Read-only and independent of `rail_dock`: the agent is an item of the
    /// centre panes rather than a dock panel, so there is no dock state to
    /// consult — only whether such a tab exists.
    fn agent_is_open(&self, agent: &AgentId, cx: &App) -> bool {
        let Some(multi_workspace) = self.multi_workspace.upgrade() else {
            return false;
        };
        let workspace = multi_workspace.read(cx).workspace().clone();
        workspace
            .read(cx)
            .items_of_type::<AgentView>(cx)
            .any(|view| view.read(cx).is_agent(agent))
    }

    /// Buttons that open an agent beside the editor.
    ///
    /// These stand for a pane item rather than a dock panel, so unlike
    /// `render_rail_panels` there is nothing to enumerate — the wiring the panel
    /// buttons get for free from `Panel::icon` is written out here instead.
    pub(crate) fn render_rail_agents(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let buttons = RAIL_AGENTS.iter().map(|(agent, icon, label)| {
            // One click reopens the agent the way it was last used — the mode is a
            // choice someone already made, and asking them to make it again every
            // time is the same as not having asked. The terminal stays one
            // right-click away for the times the answer is "not that, this once".
            // A toggle, not an open: pressing a lit button has to put the
            // column away, and `OpenAgent` has no such branch. The mode is
            // still remembered -- the toggle's open path reopens the way it was
            // last used, which is the choice someone already made.
            let remembered = ToggleAgent {
                agent: (*agent).to_string(),
            };
            let terminal = OpenAgent {
                agent: (*agent).to_string(),
                mode: Some(AgentViewMode::Terminal),
            };

            // Lit whether or not its tab is the active one, and whichever pane it
            // sits in: two agents can be open at once, so there is no single "the
            // active one" to point at.
            // `AgentId::new(*agent)` rather than `.to_string()`: these ids are
            // `&'static str`, so a `SharedString` borrows them outright, while
            // going through `String` allocated once per button per frame.
            let is_active = self.agent_is_open(&AgentId::new(*agent), cx);

            IconButton::new(*agent, *icon)
                .icon_size(RAIL_ICON_SIZE)
                .toggle_state(is_active)
                // A right-click is the only way to open straight into the terminal,
                // and a gesture with nothing naming it is a gesture nobody finds.
                .tooltip({
                    let remembered = remembered.clone();
                    move |_window, cx| {
                        Tooltip::with_meta(
                            *label,
                            Some(&remembered),
                            "Right-click to open its terminal",
                            cx,
                        )
                    }
                })
                // Dispatch rather than opening the view directly: this body runs
                // inside `Sidebar::update`, and opening a pane reaches back into
                // the workspace. Same reasoning as `render_rail_footer`.
                .on_click(move |_, window, cx| {
                    window.dispatch_action(Box::new(remembered.clone()), cx)
                })
                .on_right_click(move |_, window, cx| {
                    let terminal = terminal.clone();
                    window.dispatch_action(Box::new(terminal), cx);
                })
        });

        v_flex()
            .flex_shrink_0()
            .py(RAIL_ICON_GAP)
            .gap(RAIL_ICON_GAP)
            .items_center()
            .border_t_1()
            .border_color(cx.theme().colors().border)
            .children(buttons)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use agent_ui::AgentView;
    use fs::FakeFs;
    use gpui::{AppContext as _, TestAppContext};
    use project::Project;
    use zed_actions::agent::AgentViewMode;

    use workspace::MultiWorkspace;

    /// Reading the workspace from inside `Sidebar::render` is the re-entrancy trap
    /// that already crashed the rail's panel toggle once, and it only shows up on a
    /// real draw — building the element by hand misses it. The agent block dispatches
    /// into the workspace the same way, so it needs the same proof.
    #[gpui::test]
    async fn rail_draws_with_agent_buttons(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });

        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// The button reads `is_active` through `Sidebar::agent_is_open` on every
    /// draw, so this exercises the one path the plain draw above never touches: a
    /// real agent tab standing in one of the workspace's panes at the moment the
    /// rail paints.
    #[gpui::test]
    async fn rail_draws_with_an_agent_already_open(cx: &mut TestAppContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());

        workspace.update_in(cx, |workspace, window, cx| {
            AgentView::open(
                workspace,
                project::CLAUDE_CODE_AGENT_ID,
                Some(AgentViewMode::Terminal),
                window,
                cx,
            );
        });
        cx.run_until_parked();

        multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });

        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    /// Every rail button must name an agent the store actually knows about, or the
    /// click resolves to nothing and the user gets a button that does nothing.
    #[test]
    fn every_rail_agent_is_a_registered_builtin() {
        for (id, _, _) in super::RAIL_AGENTS {
            assert!(
                project::builtin_agent(id).is_some(),
                "rail draws `{id}`, which no built-in agent claims"
            );
        }
    }
}
