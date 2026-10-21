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

use std::path::{Path, PathBuf};

use gpui::{
    DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, Window, prelude::*,
};
use project::git_store::RepositoryId;
use settings::Settings as _;
use ui::{
    Chip, KeyBinding, Modal, ModalFooter, ModalHeader, Section, SwitchField, ToggleButtonGroup,
    ToggleButtonWithIcon, ToggleState, Tooltip, prelude::*,
};
use ui_input::{ErasedEditorEvent, InputField};
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

    fn index(self) -> usize {
        match self {
            NameMode::Name => 0,
            NameMode::Branch => 1,
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
                // The Name tab offers no base to pick, so HEAD it is -- which
                // is what this tab has always done.
                base: None,
            })
        }
        // The typed text filters the list here; it does not name anything. A
        // branch has to be picked, or there is nothing to check out.
        NameMode::Branch => selected_branch.map(|name| NewWorktreeBranchTarget::ExistingBranch {
            name: name.to_string(),
        }),
    }
}

/// The worktree's own name, as `CreateWorktree::worktree_name` takes it.
///
/// On the Name tab the worktree is named after the new branch, as it always
/// was. On the Branch tab the name is optional: left empty, the service picks
/// one; typed, it is checked with the same rule the service applies, so an
/// unusable name disables Create instead of failing after the click.
pub(crate) fn chosen_worktree_name(
    target: &NewWorktreeBranchTarget,
    typed: &str,
) -> Result<Option<String>, &'static str> {
    match target {
        NewWorktreeBranchTarget::NewBranch { name, .. } => Ok(Some(name.clone())),
        _ => {
            let name = typed.trim();
            if name.is_empty() {
                return Ok(None);
            }
            crate::worktree_names::validate_worktree_name(name)?;
            Ok(Some(name.to_string()))
        }
    }
}

/// Where the form starts: the absolute directory `git.worktree_directory`
/// currently points at.
///
/// The setting is relative and confined to the repository or its parent; what
/// the form holds is the absolute directory that resolves to, because that is
/// what a folder picker replaces it with. A setting the resolver rejects falls
/// back to the repository itself rather than leaving the form empty -- the
/// reader can see where it would go and pick elsewhere.
pub(crate) fn default_location(repo_path: &Path, setting: &str) -> PathBuf {
    match project::git_store::worktrees_directory_for_repo(repo_path, setting) {
        Ok(directory) => directory,
        Err(error) => {
            log::warn!("git.worktree_directory is not usable ({error}); offering the repository");
            repo_path.to_path_buf()
        }
    }
}

pub(crate) struct CreateWorktreeModal {
    focus_handle: FocusHandle,
    workspace: gpui::WeakEntity<Workspace>,
    project_name: SharedString,
    /// Where the checkout will be made. Read from the project, not chosen.
    run_on: SharedString,
    run_on_path: SharedString,
    /// The directory the worktree goes in. Starts where
    /// `git.worktree_directory` points, so leaving it alone is the behaviour
    /// every other entry point has.
    location: PathBuf,
    mode: NameMode,
    name_input: Entity<InputField>,
    /// The worktree's name on the Branch tab, where the first field filters
    /// branches. Empty means let the service pick one.
    worktree_name_input: Entity<InputField>,
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

/// The name field, so that the form opens ready to be typed into: it is the
/// one thing in it that takes text.
impl Focusable for CreateWorktreeModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name_input.focus_handle(cx)
    }
}

impl CreateWorktreeModal {
    pub(crate) fn new(
        workspace: gpui::WeakEntity<Workspace>,
        project_name: SharedString,
        run_on: SharedString,
        run_on_path: SharedString,
        location: PathBuf,
        branches: Vec<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_input = cx.new(|cx| InputField::new(window, cx, NameMode::Name.placeholder()));
        let worktree_name_input = cx.new(|cx| {
            InputField::new(window, cx, "Name the worktree (optional)")
                .label("Worktree name")
                .label_size(LabelSize::Small)
        });
        // Whether Create is enabled, the collision warning, the branch matches
        // and the name check all read what is typed, so each keystroke redraws
        // the form.
        let subscriptions = [&name_input, &worktree_name_input]
            .into_iter()
            .map(|input| {
                let this = cx.entity().downgrade();
                input.read(cx).editor().clone().subscribe(
                    Box::new(move |event, _window, cx| {
                        if event == ErasedEditorEvent::BufferEdited {
                            this.update(cx, |_, cx| cx.notify()).ok();
                        }
                    }),
                    window,
                    cx,
                )
            })
            .collect();

        Self {
            focus_handle: cx.focus_handle(),
            workspace,
            project_name,
            run_on,
            run_on_path,
            location,
            mode: NameMode::Name,
            name_input,
            worktree_name_input,
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
            &self.name_input.read(cx).text(cx),
            self.selected_branch.as_ref().map(|name| name.as_ref()),
        )
    }

    /// The worktree name the Create button would send, or why it cannot.
    fn worktree_name(&self, cx: &App) -> Option<Result<Option<String>, &'static str>> {
        let target = self.target(cx)?;
        Some(chosen_worktree_name(
            &target,
            &self.worktree_name_input.read(cx).text(cx),
        ))
    }

    /// Opens the system's folder chooser and takes what it returns.
    ///
    /// Cancelling leaves the current location alone -- the form always holds a
    /// real directory, so there is no empty state to fall into.
    fn choose_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            show_hidden: false,
            prompt: Some("Choose".into()),
        });

        cx.spawn_in(window, async move |modal, cx| {
            if let Ok(Ok(Some(mut paths))) = chosen.await
                && let Some(directory) = paths.pop()
            {
                modal
                    .update(cx, |modal, cx| {
                        modal.location = directory;
                        cx.notify();
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn set_mode(&mut self, mode: NameMode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.selected_branch = None;
        let editor = self.name_input.read(cx).editor().clone();
        editor.set_placeholder_text(mode.placeholder(), window, cx);
        editor.clear(window, cx);
        cx.notify();
    }

    /// The existing branch the typed name would collide with, if any.
    ///
    /// Only in `Name` mode, where the typed text becomes a *new* branch: `git
    /// worktree add -b <name>` refuses a name a branch already holds, and the
    /// refusal arrives from git talking about branches in a dialog the reader
    /// opened to make a worktree. The branch list is already loaded for the
    /// other tab, so the collision is knowable before anything is submitted.
    ///
    /// Compared exactly rather than case-insensitively: git treats refs as
    /// case-sensitive, and a warning that fires on a name that would in fact
    /// succeed teaches people to ignore it.
    fn colliding_branch(&self, cx: &App) -> Option<SharedString> {
        branch_colliding_with(
            self.mode,
            &self.name_input.read(cx).text(cx),
            &self.branches,
        )
    }

    /// Branches matching what has been typed, for the Branch tab.
    fn matching_branches(&self, cx: &App) -> Vec<SharedString> {
        let query = self.name_input.read(cx).text(cx).trim().to_lowercase();
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
        let Ok(worktree_name) =
            chosen_worktree_name(&branch_target, &self.worktree_name_input.read(cx).text(cx))
        else {
            return;
        };
        let location = self.location.clone();

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
                        location: Some(location),
                    },
                    window,
                    None,
                    cx,
                );
            })
            .ok();

        if self.create_more {
            let editor = self.name_input.read(cx).editor().clone();
            editor.clear(window, cx);
            let worktree_name_editor = self.worktree_name_input.read(cx).editor().clone();
            worktree_name_editor.clear(window, cx);
            self.selected_branch = None;
            // The click that turned "Create more" on left focus on the modal's
            // root, where typed text goes nowhere.
            window.focus(&self.name_input.focus_handle(cx), cx);
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_create = matches!(self.worktree_name(cx), Some(Ok(_)));
        let focus_handle = self.focus_handle.clone();

        div()
            .key_context("CreateWorktreeModal")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::cancel))
            .on_action(
                cx.listener(|modal, _: &menu::Confirm, window, cx| modal.confirm(window, cx)),
            )
            .elevation_3(cx)
            .occlude()
            .debug_selector(|| "create-worktree-modal".into())
            .w(rems(40.))
            .max_h(rems(44.))
            .child(
                Modal::new("create-worktree", None)
                    .header(
                        ModalHeader::new()
                            .show_dismiss_button(true)
                            .child(Headline::new("Create Worktree").size(HeadlineSize::Small)),
                    )
                    .section(
                        Section::new().child(
                            v_flex()
                                .w_full()
                                .gap_3()
                                .child(self.render_readonly_field(
                                    "Project",
                                    &self.project_name,
                                    None,
                                    cx,
                                ))
                                .child(self.render_readonly_field(
                                    "Run on",
                                    &self.run_on,
                                    Some(self.run_on_path.clone()),
                                    cx,
                                ))
                                .child(self.render_location_section(cx))
                                .child(self.render_name_section(cx))
                                .child(self.render_agent_section(cx)),
                        ),
                    )
                    .footer(
                        ModalFooter::new()
                            .start_slot(
                                div()
                                    .id("create-more")
                                    .debug_selector(|| "create-more".into())
                                    .tooltip(Tooltip::text("Keep this open after creating"))
                                    .child(SwitchField::new(
                                        "create-more-switch",
                                        Some("Create more"),
                                        None,
                                        self.create_more,
                                        cx.listener(|modal, state: &ToggleState, _, cx| {
                                            modal.create_more = *state == ToggleState::Selected;
                                            cx.notify();
                                        }),
                                    )),
                            )
                            .end_slot(
                                h_flex()
                                    .debug_selector(|| "create-worktree-actions".into())
                                    .gap_1()
                                    .child(
                                        Button::new("cancel", "Cancel").on_click(
                                            cx.listener(|_, _, _, cx| cx.emit(DismissEvent)),
                                        ),
                                    )
                                    .child(
                                        Button::new("create", "Create Worktree")
                                            .style(ButtonStyle::Filled)
                                            .disabled(!can_create)
                                            .key_binding(
                                                KeyBinding::for_action_in(
                                                    &menu::Confirm,
                                                    &focus_handle,
                                                    cx,
                                                )
                                                .size(rems_from_px(12.)),
                                            )
                                            .on_click(cx.listener(|modal, _, window, cx| {
                                                modal.confirm(window, cx)
                                            })),
                                    ),
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
                    .min_h_8()
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

    /// The Location field: where it goes, and a button to change it.
    ///
    /// Shown in full rather than as a name: two folders called `worktrees` are
    /// a mistake waiting to happen, and the whole path is what tells them
    /// apart.
    fn render_location_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_1()
            .child(
                Label::new("Location")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .w_full()
                    .min_h_8()
                    .px_2()
                    .py_1p5()
                    .gap_1p5()
                    .justify_between()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().colors().border_variant)
                    .bg(cx.theme().colors().editor_background)
                    .child(
                        div().flex_1().min_w_0().child(
                            Label::new(self.location.display().to_string())
                                .size(LabelSize::Small)
                                .truncate(),
                        ),
                    )
                    .child(
                        Button::new("choose-location", "Choose\u{2026}")
                            .style(ButtonStyle::Subtle)
                            .on_click(cx.listener(|modal, _, window, cx| {
                                modal.choose_location(window, cx)
                            })),
                    ),
            )
            .child(
                Label::new(format!(
                    "The checkout lands in a folder named after the branch, inside {}",
                    self.location.display()
                ))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
            )
    }

    fn render_name_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(
                div().debug_selector(|| "name-mode".into()).child(
                    ToggleButtonGroup::single_row(
                        "name-mode",
                        NameMode::ALL.map(|candidate| {
                            ToggleButtonWithIcon::new(
                                candidate.label(),
                                candidate.icon(),
                                cx.listener(move |modal, _, window, cx| {
                                    modal.set_mode(candidate, window, cx)
                                }),
                            )
                        }),
                    )
                    .selected_index(mode.index())
                    .label_size(LabelSize::Default),
                ),
            )
            .child(self.name_input.clone())
            .when_some(matches, |this, matches| {
                this.child(self.render_branch_matches(matches, cx))
            })
            .when_some(self.colliding_branch(cx), |this, branch| {
                this.child(self.render_branch_collision(branch, cx))
            })
            .when(mode == NameMode::Branch, |this| {
                let problem = match self.worktree_name(cx) {
                    Some(Err(reason)) => Some(reason),
                    _ => None,
                };
                this.child(
                    v_flex()
                        .gap_1()
                        .pt_1()
                        .debug_selector(|| "worktree-name".into())
                        .child(self.worktree_name_input.clone())
                        .when_some(problem, |this, reason| {
                            this.child(
                                Label::new(reason)
                                    .size(LabelSize::Small)
                                    .color(Color::Error),
                            )
                        }),
                )
            })
    }

    /// Says the typed name is taken, and offers the tab that does what the
    /// reader was reaching for.
    ///
    /// Naming an existing branch in this tab is far more often "make me a
    /// worktree for that branch" than "make me a second branch of that name",
    /// and the second is not something git will do at all. So this points at
    /// the Branch tab rather than only refusing.
    fn render_branch_collision(
        &self,
        branch: SharedString,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .id("worktree-name-collides-with-branch")
            .w_full()
            .gap_1p5()
            .child(
                Icon::new(IconName::Warning)
                    .size(IconSize::XSmall)
                    .color(Color::Warning),
            )
            .child(
                Label::new(format!(
                    "A branch named {branch} already exists. Use 'Create From' to make a worktree for it."
                ))
                .size(LabelSize::Small)
                .color(Color::Muted),
            )
            .cursor_pointer()
            .on_click(cx.listener(|modal, _, window, cx| {
                modal.set_mode(NameMode::Branch, window, cx)
            }))
    }

    fn render_branch_matches(
        &self,
        matches: Vec<SharedString>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .id("branch-matches")
            .w_full()
            .max_h(rems(10.5))
            .overflow_y_scroll()
            .children(matches.into_iter().map(|name| {
                let selected = self.selected_branch.as_ref() == Some(&name);
                let chosen = name.clone();
                h_flex()
                    .id(SharedString::from(format!("branch-{name}")))
                    .w_full()
                    .gap_1p5()
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
                    .child(Label::new(name).size(LabelSize::Small))
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
            .child(
                h_flex()
                    .w_full()
                    .gap_1p5()
                    .flex_wrap()
                    .children(agent_ui::agent_marks().map(|(id, icon, label)| {
                        let selected = self.agent == Some(id);
                        div()
                            .id(SharedString::from(label))
                            .child(
                                Chip::new(label)
                                    .icon(icon)
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
                    })),
            )
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
        let repo_path = repo.path.to_path_buf();
        let run_on_path: SharedString = repo.path.display().to_string().into();
        // The setting is the starting point, not a floor: the field is free to
        // be changed, and what it is changed to applies to this creation only.
        let location = default_location(
            &repo_path,
            &project::project_settings::ProjectSettings::get_global(cx)
                .git
                .worktree_directory,
        );
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
                    location,
                    branches,
                    window,
                    cx,
                )
            });
        });
    }
}

/// The existing branch a typed name would collide with, if any.
///
/// Free-standing so the rule can be tested without a modal, an editor and a
/// window, the same way `chosen_target` is.
fn branch_colliding_with(
    mode: NameMode,
    typed: &str,
    branches: &[SharedString],
) -> Option<SharedString> {
    if mode != NameMode::Name {
        return None;
    }
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }
    branches.iter().find(|name| name.as_ref() == typed).cloned()
}

#[cfg(test)]
mod tests {
    use super::{NameMode, branch_colliding_with, chosen_target, chosen_worktree_name};
    use gpui::SharedString;

    /// The case that sent a reader in circles: a branch exists, no worktree
    /// uses it, so the worktree list is empty of it -- and typing that name
    /// here is refused by git talking about branches.
    #[test]
    fn a_name_already_held_by_a_branch_is_reported_before_git_refuses_it() {
        let branches = [SharedString::from("test"), SharedString::from("develop")];
        assert_eq!(
            branch_colliding_with(NameMode::Name, "test", &branches),
            Some(SharedString::from("test"))
        );
        assert_eq!(
            branch_colliding_with(NameMode::Name, "  test  ", &branches),
            Some(SharedString::from("test")),
            "the name is trimmed before it is submitted, so it must be trimmed before it is checked"
        );
    }

    /// Nothing is being created in the Branch tab, so nothing can collide --
    /// naming an existing branch there is the entire point of the tab.
    #[test]
    fn picking_an_existing_branch_is_never_a_collision() {
        let branches = [SharedString::from("test")];
        assert_eq!(
            branch_colliding_with(NameMode::Branch, "test", &branches),
            None
        );
    }

    /// git refs are case-sensitive, and a warning that fires on a name which
    /// would in fact succeed teaches people to ignore the warning.
    #[test]
    fn a_different_case_is_a_different_branch() {
        let branches = [SharedString::from("test")];
        assert_eq!(
            branch_colliding_with(NameMode::Name, "Test", &branches),
            None
        );
        assert_eq!(
            branch_colliding_with(NameMode::Name, "testing", &branches),
            None
        );
        assert_eq!(branch_colliding_with(NameMode::Name, "", &branches), None);
    }
    use zed_actions::NewWorktreeBranchTarget;

    /// A worktree with a branch of its own is the point: a detached checkout
    /// has nowhere to commit, so parallel feature work cannot happen in one.
    fn existing(name: &str) -> zed_actions::NewWorktreeBranchTarget {
        zed_actions::NewWorktreeBranchTarget::ExistingBranch {
            name: name.to_string(),
        }
    }

    #[test]
    fn a_worktree_made_from_a_branch_takes_the_name_typed_for_it() {
        assert_eq!(
            chosen_worktree_name(&existing("develop"), "  release-check  "),
            Ok(Some("release-check".to_string()))
        );
    }

    #[test]
    fn a_worktree_made_from_a_branch_with_no_name_typed_is_named_for_you() {
        assert_eq!(chosen_worktree_name(&existing("develop"), ""), Ok(None));
        assert_eq!(chosen_worktree_name(&existing("develop"), "   "), Ok(None));
    }

    #[test]
    fn a_worktree_name_that_would_leave_its_directory_is_refused_before_create() {
        for unusable in ["../outside", "/tmp/elsewhere", "a\\b"] {
            assert!(
                chosen_worktree_name(&existing("develop"), unusable).is_err(),
                "{unusable:?}"
            );
        }
    }

    #[test]
    fn a_new_branch_still_names_its_worktree_after_itself() {
        let target = chosen_target(NameMode::Name, "feat.parser", None).expect("a target");
        assert_eq!(
            chosen_worktree_name(&target, "ignored"),
            Ok(Some("feat.parser".to_string()))
        );
    }

    #[test]
    fn a_typed_name_creates_a_branch() {
        assert_eq!(
            chosen_target(NameMode::Name, "feat.parser", None),
            Some(NewWorktreeBranchTarget::NewBranch {
                name: "feat.parser".into(),
                base: None
            })
        );
    }

    /// The Name tab and the worktree picker are two ways into one feature and
    /// used to disagree: the form created a branch, the picker created a
    /// detached checkout from the same typed text. Whichever one someone
    /// reaches for, a name means a branch.
    #[test]
    fn a_typed_name_means_the_same_thing_as_it_does_in_the_picker() {
        let from_form = chosen_target(NameMode::Name, "feat.parser", None);
        let from_picker = Some(NewWorktreeBranchTarget::NewBranch {
            name: "feat.parser".into(),
            base: None,
        });

        assert_eq!(from_form, from_picker);
    }

    #[test]
    fn surrounding_space_is_not_part_of_a_branch_name() {
        assert_eq!(
            chosen_target(NameMode::Name, "  feat.parser  ", None),
            Some(NewWorktreeBranchTarget::NewBranch {
                name: "feat.parser".into(),
                base: None
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

/// Where the form starts before anyone touches the Choose button.
///
/// The picker itself belongs to the platform and is not asserted here. What is
/// asserted is the value it starts from, because that is the one the reader
/// most often keeps: the form has to open pointing at the same directory every
/// other entry point would have used.
#[cfg(test)]
mod location {
    use super::default_location;
    use std::path::Path;

    const REPO: &str = "/home/dev/zode";

    #[test]
    fn the_form_opens_where_the_setting_points() {
        assert_eq!(
            default_location(Path::new(REPO), "../worktrees"),
            Path::new("/home/dev/worktrees/zode").to_path_buf(),
            "the repo's own name is appended once the path leaves the repo, \
             so two projects sharing a worktrees folder do not collide"
        );
    }

    #[test]
    fn a_directory_inside_the_repository_resolves_too() {
        assert_eq!(
            default_location(Path::new(REPO), ".git/zode-worktrees"),
            Path::new("/home/dev/zode/.git/zode-worktrees").to_path_buf()
        );
    }

    /// A setting the resolver refuses must not leave the form with nothing to
    /// show. The reader sees the repository, and can pick elsewhere.
    #[test]
    fn an_unusable_setting_falls_back_to_the_repository() {
        for unusable in ["/tmp/worktrees", "", "..", "../../elsewhere"] {
            assert_eq!(
                default_location(Path::new(REPO), unusable),
                Path::new(REPO).to_path_buf(),
                "{unusable:?} is refused by the setting's own validator"
            );
        }
    }
}

/// The form as a person meets it: opened, clicked and confirmed.
///
/// There is no workspace behind these. `confirm` hands the action to the
/// workspace and carries on if it is gone, so what is asserted is everything
/// that happens around that hand-off.
#[cfg(test)]
mod form {
    use super::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::Cell;
    use std::rc::Rc;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let store = settings::SettingsStore::test(cx);
            cx.set_global(store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
            editor::init(cx);
        });
    }

    fn open_with(
        branches: Vec<SharedString>,
        cx: &mut TestAppContext,
    ) -> (Entity<CreateWorktreeModal>, &mut VisualTestContext) {
        init(cx);
        let (modal, cx) = cx.add_window_view(|window, cx| {
            CreateWorktreeModal::new(
                gpui::WeakEntity::new_invalid(),
                "zode".into(),
                "This machine".into(),
                "/repos/zode".into(),
                PathBuf::from("/repos/worktrees/zode"),
                branches,
                window,
                cx,
            )
        });
        // What the modal layer does the moment it shows a modal.
        modal.update_in(cx, |modal, window, cx| {
            let focus = modal.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(800.)));
        cx.run_until_parked();
        (modal, cx)
    }

    fn open(cx: &mut TestAppContext) -> (Entity<CreateWorktreeModal>, &mut VisualTestContext) {
        open_with(vec!["develop".into(), "main".into()], cx)
    }

    fn type_name(modal: &Entity<CreateWorktreeModal>, text: &str, cx: &mut VisualTestContext) {
        let editor = modal.read_with(cx, |modal, cx| modal.name_input.read(cx).editor().clone());
        cx.update(|window, cx| editor.set_text(text, window, cx));
        cx.run_until_parked();
    }

    fn typed(modal: &Entity<CreateWorktreeModal>, cx: &mut VisualTestContext) -> String {
        modal.read_with(cx, |modal, cx| modal.name_input.read(cx).text(cx))
    }

    /// Clicks `selector` a `fraction` of the way along it, so one wrapper can
    /// stand for either half of a segmented control.
    fn click_along(selector: &'static str, fraction: f32, cx: &mut VisualTestContext) {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} was not drawn, so it cannot be clicked"));
        let position = gpui::point(
            bounds.origin.x + bounds.size.width * fraction,
            bounds.center().y,
        );
        cx.simulate_click(position, Modifiers::default());
        cx.run_until_parked();
    }

    fn watch_dismissal(
        modal: &Entity<CreateWorktreeModal>,
        cx: &mut VisualTestContext,
    ) -> (Rc<Cell<bool>>, gpui::Subscription) {
        let dismissed = Rc::new(Cell::new(false));
        let subscription = cx.update(|_, cx| {
            let dismissed = dismissed.clone();
            cx.subscribe(modal, move |_, _: &DismissEvent, _| dismissed.set(true))
        });
        (dismissed, subscription)
    }

    /// The form's one text field is what the reader types into, so it has to
    /// be what the modal layer focuses.
    #[gpui::test]
    async fn the_form_opens_ready_to_be_typed_into(cx: &mut TestAppContext) {
        let (modal, cx) = open(cx);
        modal.update_in(cx, |modal, window, cx| {
            assert!(
                modal
                    .name_input
                    .read(cx)
                    .editor()
                    .focus_handle(cx)
                    .is_focused(window),
                "what the modal layer focuses is the name field itself"
            );
        });
    }

    #[gpui::test]
    async fn create_more_keeps_the_form_open_and_empties_the_name(cx: &mut TestAppContext) {
        let (modal, cx) = open(cx);
        let (dismissed, _subscription) = watch_dismissal(&modal, cx);

        type_name(&modal, "feat.parser", cx);
        click_along("create-more", 0.5, cx);
        assert!(
            modal.read_with(cx, |modal, _| modal.create_more),
            "a click on the switch turns it on"
        );

        modal.update_in(cx, |modal, window, cx| modal.confirm(window, cx));
        cx.run_until_parked();

        assert!(!dismissed.get(), "the form stays for the next worktree");
        assert_eq!(typed(&modal, cx), "", "and is ready for its name");
        modal.update_in(cx, |modal, window, cx| {
            assert!(
                modal
                    .name_input
                    .read(cx)
                    .editor()
                    .focus_handle(cx)
                    .is_focused(window),
                "clicking the switch moved focus off the name field, so typing the next name \
                 needs it handed back"
            );
        });
    }

    #[gpui::test]
    async fn without_create_more_a_create_closes_the_form(cx: &mut TestAppContext) {
        let (modal, cx) = open(cx);
        let (dismissed, _subscription) = watch_dismissal(&modal, cx);

        type_name(&modal, "feat.parser", cx);
        modal.update_in(cx, |modal, window, cx| modal.confirm(window, cx));
        cx.run_until_parked();

        assert!(dismissed.get());
    }

    #[gpui::test]
    async fn nothing_typed_means_nothing_to_confirm(cx: &mut TestAppContext) {
        let (modal, cx) = open(cx);
        let (dismissed, _subscription) = watch_dismissal(&modal, cx);

        modal.update_in(cx, |modal, window, cx| modal.confirm(window, cx));
        cx.run_until_parked();

        assert!(!dismissed.get(), "an empty name cannot create anything");
    }

    #[gpui::test]
    async fn choosing_branch_swaps_the_name_for_a_pick(cx: &mut TestAppContext) {
        let (modal, cx) = open(cx);
        type_name(&modal, "feat", cx);
        assert!(modal.read_with(cx, |modal, _| modal.mode == NameMode::Name));

        click_along("name-mode", 0.75, cx);

        assert!(
            modal.read_with(cx, |modal, _| modal.mode == NameMode::Branch),
            "the second segment is the Branch tab"
        );
        assert_eq!(
            typed(&modal, cx),
            "",
            "a name typed for a new branch is not a filter for existing ones"
        );

        click_along("name-mode", 0.25, cx);
        assert!(modal.read_with(cx, |modal, _| modal.mode == NameMode::Name));
    }

    /// The dialog is as tall as it is allowed to be at its fullest -- the
    /// Branch tab with a long list -- and everything in it is still inside.
    #[gpui::test]
    async fn the_form_fits_inside_its_maximum_height(cx: &mut TestAppContext) {
        let branches = (0..12)
            .map(|index| SharedString::from(format!("feature-{index}")))
            .collect();
        let (modal, cx) = open_with(branches, cx);
        modal.update_in(cx, |modal, window, cx| {
            modal.set_mode(NameMode::Branch, window, cx)
        });
        cx.run_until_parked();

        let shell = cx
            .debug_bounds("create-worktree-modal")
            .expect("the form was not drawn");
        let actions = cx
            .debug_bounds("create-worktree-actions")
            .expect("the footer buttons were not drawn");
        assert!(
            actions.bottom() <= shell.bottom(),
            "the footer ends at {:?} but the form ends at {:?}",
            actions.bottom(),
            shell.bottom()
        );
    }
}
