//! Creating a branch from the panel.
//!
//! An inline field rather than a modal: naming a branch is a two-second act and
//! a dialog for it costs more attention than the task is worth. Enter commits,
//! Escape abandons, and the field disappears either way.

use editor::Editor;
use gpui::Focusable as _;
use gpui::Window;
use project::git_store::RepositoryId;
use ui::prelude::*;

use git::repository::Branch;

use crate::branch_panel::panel::BranchPanel;
use crate::branch_service;

/// What the inline field will do with the name typed into it.
#[derive(Clone)]
pub(crate) enum NameFieldIntent {
    Create,
    /// Rename this branch. Carries the branch rather than just its name so the
    /// field cannot act on a branch that has since disappeared from the tree.
    Rename(Branch),
}

impl BranchPanel {
    /// Opens the field under the given repository, seeded empty and focused.
    pub(crate) fn begin_new_branch(
        &mut self,
        id: RepositoryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("New branch name", window, cx);
            editor
        });
        let focus = editor.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.new_branch = Some((id, editor, NameFieldIntent::Create));
        cx.notify();
    }

    /// Opens the same field seeded with the current name, to rename in place.
    pub(crate) fn begin_rename_branch(
        &mut self,
        id: RepositoryId,
        branch: Branch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = branch.name().to_string();
        let editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_text(current, window, cx);
            editor.select_all(&Default::default(), window, cx);
            editor
        });
        let focus = editor.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.new_branch = Some((id, editor, NameFieldIntent::Rename(branch)));
        cx.notify();
    }

    pub(crate) fn cancel_new_branch(&mut self, cx: &mut Context<Self>) {
        self.new_branch = None;
        cx.notify();
    }

    /// Creates the branch off the repository's current HEAD.
    ///
    /// A blank name is a cancel, not an error: the user pressed Enter on an
    /// empty field, which reads as "never mind".
    pub(crate) fn confirm_new_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, editor, intent)) = self.new_branch.take() else {
            return;
        };
        cx.notify();

        let name = editor.read(cx).text(cx).trim().to_string();
        if name.is_empty() {
            return;
        }
        let Some(repo) = self.repository(id, cx) else {
            return;
        };

        match intent {
            NameFieldIntent::Create => {
                let task = cx.spawn(async move |_, cx| {
                    branch_service::create_branch(repo.clone(), name.clone(), None, cx).await?;
                    branch_service::checkout(repo, name, cx).await
                });
                self.report_failure(task, "git switch -c", window, cx);
            }
            NameFieldIntent::Rename(branch) => {
                let old = branch.name().to_string();
                // Renaming to the same thing is a no-op, not an error worth a toast.
                if old == name {
                    return;
                }
                let task = cx.spawn(async move |_, cx| {
                    branch_service::rename_branch(repo, old, name, cx).await
                });
                self.report_failure(task, "git branch -m", window, cx);
            }
        }
    }

    pub(crate) fn render_new_branch_field(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let (_, editor, intent) = self.new_branch.as_ref()?;
        let icon = match intent {
            NameFieldIntent::Create => IconName::GitBranchPlus,
            NameFieldIntent::Rename(_) => IconName::Pencil,
        };
        Some(
            h_flex()
                .w_full()
                .px_2()
                .py_1()
                .gap_1()
                .border_b_1()
                .border_color(cx.theme().colors().border_variant)
                .child(Icon::new(icon).size(IconSize::XSmall).color(Color::Muted))
                .child(editor.clone())
                .into_any_element(),
        )
    }
}
