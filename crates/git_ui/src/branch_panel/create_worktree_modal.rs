//! The "Create worktree" form.
//!
//! Every field here is wired to something. A form is a row of promises, and a
//! control that looks live but does nothing is worse than an absent one -- so
//! the reference design's "Smart" tab (parse a Jira or GitLab URL) and "GitHub"
//! tab (list issues) are deliberately not here: this editor has no integration
//! behind either, and drawing them would be drawing a lie.
//!
//! "Run on" is shown and not chosen: `handle_create_worktree` reads the
//! connection from the project itself, so the field reports where the checkout
//! will land rather than offering a choice the code below it cannot honour.

use gpui::{
    DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, Window, prelude::*,
};
use project::git_store::RepositoryId;
use ui::{Chip, Divider, KeyBinding, Tooltip, prelude::*};
use workspace::{ModalView, Workspace};
use zed_actions::NewWorktreeBranchTarget;

use crate::branch_panel::panel::BranchPanel;

/// Which way the new checkout is named.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameMode {
    /// Type a name; the worktree gets a branch of that name.
    Name,
    /// Pick a branch that already exists; the worktree checks it out.
    Branch,
}

impl NameMode {
    const ALL: [NameMode; 2] = [NameMode::Name, NameMode::Branch];

    fn label(self) -> &'static str {
        match self {
            NameMode::Name => "Name",
            NameMode::Branch => "Branch",
        }
    }

    fn icon(self) -> IconName {
        match self {
            NameMode::Name => IconName::FontSize,
            NameMode::Branch => IconName::GitBranch,
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            NameMode::Name => "Name the branch this worktree will work on",
            NameMode::Branch => "Filter existing branches",
        }
    }
}

/// The form's one decision, as a pure function of what the reader has filled
/// in.
///
/// Split out of the modal so it can be asserted against without a window: this
/// is what the Create button is enabled by *and* what it does, and those two
/// drifting apart is a button that lies.
pub(crate) fn chosen_target(
    mode: NameMode,
    typed: &str,
    selected_branch: Option<&str>,
) -> Option<NewWorktreeBranchTarget> {
    match mode {
        NameMode::Name => {
            let name = typed.trim();
            (!name.is_empty()).then(|| NewWorktreeBranchTarget::NewBranch {
                name: name.to_string(),
            })
        }
        // The typed text filters the list here; it does not name anything. A
        // branch has to be picked, or there is nothing to check out.
        NameMode::Branch => selected_branch.map(|name| NewWorktreeBranchTarget::ExistingBranch {
            name: name.to_string(),
        }),
    }
}

pub(crate) struct CreateWorktreeModal {
    focus_handle: FocusHandle,
    workspace: gpui::WeakEntity<Workspace>,
    project_name: SharedString,
    /// Where the checkout will be made. Read from the project, not chosen.
    run_on: SharedString,
    run_on_path: SharedString,
    mode: NameMode,
    name_editor: Entity<editor::Editor>,
    /// Local branch names, for the Branch tab.
    branches: Vec<SharedString>,
    selected_branch: Option<SharedString>,
    /// The agent to start in the new checkout once it is open. `None` means
    /// create the worktree and start nothing.
    agent: Option<&'static str>,
    /// Keep the form open after a create, for someone laying out several
    /// features at once.
    create_more: bool,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EventEmitter<DismissEvent> for CreateWorktreeModal {}
impl ModalView for CreateWorktreeModal {}

impl Focusable for CreateWorktreeModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl CreateWorktreeModal {
    pub(crate) fn new(
        workspace: gpui::WeakEntity<Workspace>,
        project_name: SharedString,
        run_on: SharedString,
        run_on_path: SharedString,
        branches: Vec<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_editor = cx.new(|cx| {
            let mut editor = editor::Editor::single_line(window, cx);
            editor.set_placeholder_text(NameMode::Name.placeholder(), window, cx);
            editor
        });
        let subscriptions =
            vec![
                cx.subscribe(&name_editor, |_, _, _: &editor::EditorEvent, cx| {
                    cx.notify();
                }),
            ];

        Self {
            focus_handle: cx.focus_handle(),
            workspace,
            project_name,
            run_on,
            run_on_path,
            mode: NameMode::Name,
            name_editor,
            branches,
            selected_branch: None,
            agent: None,
            create_more: false,
            _subscriptions: subscriptions,
        }
    }

    /// What the Create button will do, or `None` when it cannot do anything
    /// yet. The button reads this rather than deciding for itself, so what is
    /// disabled and what would happen can never disagree.
    fn target(&self, cx: &App) -> Option<NewWorktreeBranchTarget> {
        chosen_target(
            self.mode,
            &self.name_editor.read(cx).text(cx),
            self.selected_branch.as_ref().map(|name| name.as_ref()),
        )
    }

    fn set_mode(&mut self, mode: NameMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.selected_branch = None;
        self.name_editor.update(cx, |editor, cx| {
            editor.set_placeholder_text(mode.placeholder(), window, cx);
            editor.clear(window, cx);
        });
        cx.notify();
    }

    /// Branches matching what has been typed, for the Branch tab.
    fn matching_branches(&self, cx: &App) -> Vec<SharedString> {
        let query = self.name_editor.read(cx).text(cx).trim().to_lowercase();
        self.branches
            .iter()
            .filter(|name| query.is_empty() || name.to_lowercase().contains(&query))
            .take(8)
            .cloned()
            .collect()
    }

    pub(crate) fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(branch_target) = self.target(cx) else {
            return;
        };
        let worktree_name = match &branch_target {
            NewWorktreeBranchTarget::NewBranch { name } => Some(name.clone()),
            _ => None,
        };

        // The agent rides on the action rather than being started here.
        // `worktree_service` runs it from the new workspace's own init hook, so
        // it lands in the checkout that was just made -- starting it from this
        // side would run it in the one being left behind, which looks identical
        // from the tab and is not.
        self.workspace
            .update(cx, |workspace, cx| {
                crate::worktree_service::handle_create_worktree(
                    workspace,
                    &zed_actions::CreateWorktree {
                        worktree_name,
                        branch_target,
                        agent: self.agent.map(|agent| agent.to_string()),
                    },
                    window,
                    None,
                    cx,
                );
            })
            .ok();

        if self.create_more {
            self.name_editor
                .update(cx, |editor, cx| editor.clear(window, cx));
            self.selected_branch = None;
            cx.notify();
        } else {
            cx.emit(DismissEvent);
        }
    }

    pub(crate) fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

impl Render for CreateWorktreeModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_create = self.target(cx).is_some();
        let focus_handle = self.focus_handle.clone();

        v_flex()
            .key_context("CreateWorktreeModal")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::cancel))
            .on_action(
                cx.listener(|modal, _: &menu::Confirm, window, cx| modal.confirm(window, cx)),
            )
            .elevation_3(cx)
            .w(px(440.))
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(Label::new("Create worktree").size(LabelSize::Large))
                    .child(
                        IconButton::new("close", IconName::Close)
                            .icon_size(IconSize::Small)
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                    ),
            )
            .child(self.render_readonly_field("Project", &self.project_name, None, cx))
            .child(self.render_readonly_field(
                "Run on",
                &self.run_on,
                Some(self.run_on_path.clone()),
                cx,
            ))
            .child(self.render_name_section(window, cx))
            .child(self.render_agent_section(cx))
            .child(Divider::horizontal())
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(
                        h_flex()
                            .id("create-more")
                            .gap_1p5()
                            .child(
                                ui::Switch::new("create-more-switch", self.create_more.into())
                                    .on_click(cx.listener(|modal, state, _, cx| {
                                        modal.create_more = *state == ui::ToggleState::Selected;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Label::new("Create more")
                                    .size(LabelSize::Small)
                                    .color(Color::Muted),
                            )
                            .tooltip(|_, cx| Tooltip::simple("Keep this open after creating", cx)),
                    )
                    .child(
                        Button::new("create", "Create worktree")
                            .style(ButtonStyle::Filled)
                            .disabled(!can_create)
                            .key_binding(
                                KeyBinding::for_action_in(&menu::Confirm, &focus_handle, cx)
                                    .map(|binding| binding.size(rems_from_px(12.))),
                            )
                            .on_click(
                                cx.listener(|modal, _, window, cx| modal.confirm(window, cx)),
                            ),
                    ),
            )
    }
}

impl CreateWorktreeModal {
    /// A field that reports rather than asks.
    ///
    /// Drawn as a field because it belongs in the form's rhythm, but with no
    /// affordance to change it: `handle_create_worktree` reads both of these
    /// off the project, so a dropdown here would be a control with nothing
    /// behind it.
    fn render_readonly_field(
        &self,
        label: &'static str,
        value: &SharedString,
        detail: Option<SharedString>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_1()
            .child(Label::new(label).size(LabelSize::Small).color(Color::Muted))
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().colors().border_variant)
                    .bg(cx.theme().colors().editor_background)
                    .child(Label::new(value.clone()).size(LabelSize::Small))
                    .when_some(detail, |this, detail| {
                        this.child(
                            div().flex_1().min_w_0().child(
                                Label::new(detail)
                                    .size(LabelSize::XSmall)
                                    .color(Color::Muted)
                                    .truncate(),
                            ),
                        )
                    }),
            )
    }

    fn render_name_section(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mode = self.mode;
        let matches = (mode == NameMode::Branch).then(|| self.matching_branches(cx));

        v_flex()
            .w_full()
            .gap_1()
            .child(
                Label::new("Name or 'Create From'")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(h_flex().gap_3().children(NameMode::ALL.map(|candidate| {
                let selected = candidate == mode;
                h_flex()
                    .id(candidate.label())
                    .gap_1()
                    .pb_0p5()
                    .when(selected, |this| {
                        this.border_b_2().border_color(cx.theme().colors().text)
                    })
                    .child(
                        Icon::new(candidate.icon())
                            .size(IconSize::XSmall)
                            .color(if selected {
                                Color::Default
                            } else {
                                Color::Muted
                            }),
                    )
                    .child(Label::new(candidate.label()).size(LabelSize::Small).color(
                        if selected {
                            Color::Default
                        } else {
                            Color::Muted
                        },
                    ))
                    .on_click(cx.listener(move |modal, _, window, cx| {
                        modal.set_mode(candidate, window, cx)
                    }))
            })))
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().colors().border_focused)
                    .bg(cx.theme().colors().editor_background)
                    .child(self.name_editor.clone()),
            )
            .when_some(matches, |this, matches| {
                this.child(self.render_branch_matches(matches, cx))
            })
    }

    fn render_branch_matches(
        &self,
        matches: Vec<SharedString>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .id("branch-matches")
            .w_full()
            .max_h(px(160.))
            .overflow_y_scroll()
            .children(matches.into_iter().map(|name| {
                let selected = self.selected_branch.as_ref() == Some(&name);
                let chosen = name.clone();
                h_flex()
                    .id(SharedString::from(format!("branch-{name}")))
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .when(selected, |this| {
                        this.bg(cx.theme().colors().element_selected)
                    })
                    .hover(|style| style.bg(cx.theme().colors().element_hover))
                    .child(
                        Icon::new(IconName::GitBranch)
                            .size(IconSize::XSmall)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .pl_1p5()
                            .child(Label::new(name).size(LabelSize::Small)),
                    )
                    .on_click(cx.listener(move |modal, _, _, cx| {
                        modal.selected_branch = Some(chosen.clone());
                        cx.notify();
                    }))
            }))
    }

    fn render_agent_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_1()
            .child(
                Label::new("Agent")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(h_flex().w_full().gap_1p5().flex_wrap().children(
                super::agent_choices().iter().map(|(id, label)| {
                    let selected = self.agent == Some(*id);
                    let id = *id;
                    div()
                        .id(SharedString::from(*label))
                        .child(
                            Chip::new(*label)
                                .icon(agent_ui::agent_icon(id))
                                .icon_color(Color::Custom(agent_ui::agent_color(id)))
                                .label_color(if selected {
                                    Color::Default
                                } else {
                                    Color::Muted
                                })
                                .when(selected, |chip| {
                                    chip.border_color(cx.theme().colors().border_focused)
                                }),
                        )
                        // Choosing the same agent twice clears it: the
                        // form must be able to say "create the worktree
                        // and start nothing".
                        .on_click(cx.listener(move |modal, _, _, cx| {
                            modal.agent = (modal.agent != Some(id)).then_some(id);
                            cx.notify();
                        }))
                }),
            ))
    }
}

impl BranchPanel {
    /// Opens the create-worktree form for one repository.
    pub(crate) fn open_create_worktree_modal(
        &mut self,
        id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let Some(repo) = self.repos.iter().find(|repo| repo.id == id) else {
            return;
        };

        let project_name = repo.name.clone();
        let run_on_path: SharedString = repo.path.display().to_string().into();
        // Read from the project rather than offered as a choice, because
        // `handle_create_worktree` reads it from the project too.
        let run_on: SharedString = workspace
            .read(cx)
            .project()
            .read(cx)
            .remote_connection_options(cx)
            .map(|_| "Remote".into())
            .unwrap_or_else(|| "This machine".into());
        let branches: Vec<SharedString> = repo
            .branches
            .iter()
            .filter(|branch| !branch.is_remote())
            .map(|branch| SharedString::from(branch.name().to_string()))
            .collect();

        let weak_workspace = self.workspace.clone();
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, |window, cx| {
                CreateWorktreeModal::new(
                    weak_workspace,
                    project_name,
                    run_on,
                    run_on_path,
                    branches,
                    window,
                    cx,
                )
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{NameMode, chosen_target};
    use zed_actions::NewWorktreeBranchTarget;

    /// A worktree with a branch of its own is the point: a detached checkout
    /// has nowhere to commit, so parallel feature work cannot happen in one.
    #[test]
    fn a_typed_name_creates_a_branch() {
        assert_eq!(
            chosen_target(NameMode::Name, "feat.parser", None),
            Some(NewWorktreeBranchTarget::NewBranch {
                name: "feat.parser".into()
            })
        );
    }

    #[test]
    fn surrounding_space_is_not_part_of_a_branch_name() {
        assert_eq!(
            chosen_target(NameMode::Name, "  feat.parser  ", None),
            Some(NewWorktreeBranchTarget::NewBranch {
                name: "feat.parser".into()
            })
        );
    }

    /// The button is disabled by the same call that decides what it does, so
    /// an empty name must produce nothing rather than a branch called "".
    #[test]
    fn an_empty_name_can_do_nothing() {
        assert_eq!(chosen_target(NameMode::Name, "", None), None);
        assert_eq!(chosen_target(NameMode::Name, "   ", None), None);
    }

    #[test]
    fn a_picked_branch_is_checked_out() {
        assert_eq!(
            chosen_target(NameMode::Branch, "dev", Some("develop")),
            Some(NewWorktreeBranchTarget::ExistingBranch {
                name: "develop".into()
            })
        );
    }

    /// On the Branch tab the typed text filters the list -- it names nothing.
    /// Treating it as a name would create a branch called "dev" when the reader
    /// was searching for "develop".
    #[test]
    fn typing_on_the_branch_tab_does_not_name_a_branch() {
        assert_eq!(chosen_target(NameMode::Branch, "dev", None), None);
    }
}
