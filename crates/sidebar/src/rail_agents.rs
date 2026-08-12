use crate::Sidebar;
use crate::rail::{RAIL_ICON_GAP, RAIL_ICON_SIZE};
use gpui::{AnyElement, Context, Window};
use ui::{Tooltip, prelude::*};
use zed_actions::agent::{AgentViewMode, OpenAgent};

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
            // One click opens the conversation — the case people are in most of the
            // time. The terminal is a right-click away rather than a second click
            // every time.
            let chat = OpenAgent {
                agent: (*agent).to_string(),
                mode: AgentViewMode::Chat,
            };
            let terminal = OpenAgent {
                agent: (*agent).to_string(),
                mode: AgentViewMode::Terminal,
            };

            IconButton::new(*agent, *icon)
                .icon_size(RAIL_ICON_SIZE)
                .tooltip({
                    let chat = chat.clone();
                    move |_window, cx| Tooltip::for_action(*label, &chat, cx)
                })
                // Dispatch rather than opening the view directly: this body runs
                // inside `Sidebar::update`, and opening a pane reaches back into
                // the workspace. Same reasoning as `render_rail_footer`.
                .on_click(move |_, window, cx| {
                    window.dispatch_action(Box::new(chat.clone()), cx)
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
    use fs::FakeFs;
    use gpui::{AppContext as _, TestAppContext};
    use project::Project;
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
