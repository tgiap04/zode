mod agent_panel;
mod agent_view;
mod missing_binary;

// Phase 04 in progress — the ACP slice is restored on disk but not yet in the
// module tree. Bringing a module in here is the last step of porting it, not the
// first: until its references to the native-agent stack are resolved it would
// only make the crate red. See the phase-04 file for the remaining work, module
// by module.
//
mod actions;
mod agent_connection_store;
mod agent_diff;
mod completion_provider;
mod config_options;
mod conversation_view;
mod diagnostics;
mod entry_view_state;
mod external_source_prompt;
mod markdown_style;
mod mention_image;
mod mention_set;
mod message_editor;
mod mode_selector;
mod model_selector;
mod model_selector_popover;
mod outline;
mod ui;

pub use actions::*;
pub use agent_diff::{AgentDiffPane, AgentDiffToolbar};
pub use agent_panel::AgentPanel;
pub use agent_view::{AgentView, agent_icon};
pub use model_selector::ModelSelector;

use gpui::App;
use workspace::Workspace;
use zed_actions::agent::{NewAgent, OpenAgent};

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|workspace, action: &OpenAgent, window, cx| {
            AgentView::open(workspace, action.agent.as_str(), action.mode, window, cx);
        });
        workspace.register_action(|workspace, action: &NewAgent, window, cx| {
            AgentView::open_new(workspace, action.agent.as_str(), action.mode, window, cx);
        });
    })
    .detach();
}
