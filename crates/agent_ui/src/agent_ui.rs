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

pub use actions::*;
pub use agent_view::{AgentView, agent_icon};

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
