//! The agent, as a tab in the editor's panes.
//!
//! An agent here is a **terminal session and nothing else**. There is no chat
//! view, no message editor, no model or mode selector, no diff review pane — that
//! whole surface, and the Agent Client Protocol stack underneath it, is gone. The
//! agent's own CLI already has an interface; this crate's job is to give it a tab,
//! a name, and a way back to it.

mod actions;
mod agent_view;
mod missing_binary;
mod session_history;

pub use actions::*;
pub use agent_view::{AgentView, agent_icon};
pub use session_history::AgentHistoryPanel;

use gpui::App;
use project::AgentId;
use workspace::Workspace;
use zed_actions::agent::{NewAgent, OpenAgent, ToggleAgent};

pub fn init(cx: &mut App) {
    // The agent is an item of the editor's panes, so the workspace's own
    // item-restore machinery is what brings a tab back — the same path an editor,
    // a terminal or an image view comes home by.
    workspace::register_serializable_item::<AgentView>(cx);

    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        // Shows or hides the history, taking the tool column turn by turn with
        // whatever else lives there.
        //
        // Written out rather than handed to `toggle_panel_focus`, which routes
        // through `Dock::show_panel` and therefore *stacks*: the history and the
        // project tree would split the column's height between them, and two
        // vertical lists in half a column each serves neither. `activate_panel`
        // is the call that makes one panel the only visible one.
        workspace.register_action(
            |workspace, _: &zed_actions::agent::ToggleHistory, window, cx| {
                let dock = workspace.right_dock().clone();
                let Some(index) = dock.read(cx).panel_index_for_type::<AgentHistoryPanel>() else {
                    return;
                };
                // Already up means: the dock is open and the panel it is drawing
                // is this one.
                let showing = {
                    let dock = dock.read(cx);
                    dock.is_open()
                        && dock
                            .visible_panel()
                            .and_then(|panel| panel.to_any().downcast::<AgentHistoryPanel>().ok())
                            .is_some()
                };
                dock.update(cx, |dock, cx| {
                    if showing {
                        // Put away, not closed: the dock keeps the panel, so its
                        // search text and expanded rows survive the round trip.
                        dock.set_open(false, window, cx);
                    } else {
                        dock.activate_panel(index, window, cx);
                        dock.set_open(true, window, cx);
                    }
                });
            },
        );
        workspace.register_action(|workspace, action: &OpenAgent, window, cx| {
            AgentView::open(workspace, action.agent.as_str(), action.mode, window, cx);
        });
        workspace.register_action(|workspace, action: &NewAgent, window, cx| {
            AgentView::open_new(workspace, action.agent.as_str(), action.mode, window, cx);
        });
        // What the rail button does, and the reason it is not `OpenAgent`: the
        // button is a toggle, and a lit toggle that does nothing when pressed
        // is the whole complaint.
        //
        // Put away rather than closed -- stepping back to the tab that was being
        // read leaves the agent where it stands, so this loses no thread and no
        // scroll position.
        workspace.register_action(|workspace, action: &ToggleAgent, window, cx| {
            if AgentView::put_away(workspace, &AgentId::new(action.agent.clone()), window, cx) {
                return;
            }
            AgentView::open(workspace, action.agent.as_str(), None, window, cx);
        });
    })
    .detach();
}
