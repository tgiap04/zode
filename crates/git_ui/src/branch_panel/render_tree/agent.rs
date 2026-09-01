//! One agent under a branch.
//!
//! A running CLI keeps full contrast and a live dot; a finished session is
//! muted. That distinction is the point of the row -- someone with work on four
//! branches is looking for which of them is still moving.

use gpui::{AnyElement, ClickEvent};
use project::git_store::RepositoryId;
use ui::{Indicator, Tooltip, prelude::*};

use crate::branch_panel::panel::BranchPanel;
use crate::branch_panel::tree::AgentEntry;

impl BranchPanel {
    pub(super) fn agent_row(
        &self,
        ix: usize,
        indent: Pixels,
        _id: RepositoryId,
        entry: &AgentEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let running = entry.is_running();
        let label = entry.label().clone();
        let tooltip = label.clone();
        let entry = entry.clone();

        h_flex()
            .id(("agent", ix))
            .w_full()
            .pl(indent + px(20.))
            .pr_2()
            .py_0p5()
            .gap_1p5()
            .child(Indicator::dot().color(if running {
                Color::Success
            } else {
                Color::Muted
            }))
            .child(
                div().flex_1().min_w_0().child(
                    Label::new(label)
                        .size(LabelSize::XSmall)
                        .truncate()
                        .color(if running {
                            Color::Default
                        } else {
                            Color::Muted
                        }),
                ),
            )
            .tooltip(move |_, cx| Tooltip::simple(tooltip.clone(), cx))
            .on_click(cx.listener(move |panel, _: &ClickEvent, window, cx| {
                panel.open_agent(&entry, window, cx);
            }))
            .into_any_element()
    }

    /// Jumps to a running agent's tab, or brings a finished session back.
    ///
    /// Resuming goes through `agent_ui::resume_session`, the same call the
    /// history panel makes: the rules about what may be resumed -- the agent
    /// must support it, the working directory must still exist -- belong to the
    /// operation, not to whichever surface asked for it.
    pub(crate) fn open_agent(
        &mut self,
        entry: &AgentEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };

        match entry {
            AgentEntry::Running { view, .. } => {
                let Some(view) = view.upgrade() else {
                    return;
                };
                workspace.update(cx, |workspace, cx| {
                    workspace.activate_item(&view, true, true, window, cx);
                });
            }
            AgentEntry::Past { id, .. } => {
                let Some(store) = self.session_store.clone() else {
                    return;
                };
                let index = store.read(cx).index().clone();
                let Some(session) = index.find(id) else {
                    return;
                };
                agent_ui::resume_session(
                    &workspace,
                    session,
                    agent_sessions::Fork::Continue,
                    window,
                    cx,
                );
            }
        }
    }
}
