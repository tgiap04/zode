//! The behaviour half of `project_logo`: reads and writes the actual bytes a
//! project's logo is made of.
//!
//! Split out of `project_logo` once the pure helpers plus this behaviour
//! pushed that module past its 200-line bound. `project_logo` stays pure —
//! no `Fs`, no `cx` — and this module is where `MultiWorkspace` copies a
//! chosen file in, swaps the record, and deletes what it replaces.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use fs::{CopyOptions, RemoveOptions};
use gpui::{App, AppContext, Context, ImageSource, Task};
use project::ProjectGroupKey;
use util::ResultExt;

use crate::MultiWorkspace;
use crate::Toast;
use crate::notifications::NotificationId;
use crate::project_logo::{
    MAX_LOGO_BYTES, logo_extension, new_logo_file_name, project_avatars_dir,
};

/// Marker type for this module's toast notifications (`NotificationId::unique`
/// needs a type, not a value).
struct ProjectLogoToast;

impl MultiWorkspace {
    /// The copy Zode drew this project's avatar from most recently, if it has
    /// one — a read for the menu, so it can decide whether "Remove Logo" is
    /// worth offering at all.
    pub fn project_logo(&self, key: &ProjectGroupKey) -> Option<Arc<Path>> {
        self.group_state_by_key(key)?.logo.clone()
    }

    /// Marks a new logo write for this project and hands back its generation,
    /// or `None` if this window does not show the project.
    ///
    /// Both writes take one. A copy runs on the background executor and can
    /// land long after the user has asked for something else -- a second
    /// upload, or "Remove Logo". Without a generation to check on arrival,
    /// the later-finishing copy wins the field and the other file is left on
    /// disk with nothing naming it, and a copy that lands after a clear puts
    /// back a logo the user just took off.
    fn bump_logo_generation(&mut self, key: &ProjectGroupKey, cx: &App) -> Option<u64> {
        let group = self.presentation_state_for(key, cx)?;
        group.logo_generation = group.logo_generation.wrapping_add(1);
        Some(group.logo_generation)
    }

    /// Copies `source` into `project_avatars_dir()` and makes it this
    /// project's logo, replacing and deleting whatever it had before.
    ///
    /// Copy first, swap second, delete third: a failed copy leaves the
    /// project exactly as it was, and the previous file is only released
    /// once the new one is definitely in place. Every refusal or failure
    /// reaches the user as a toast rather than failing silently.
    pub fn set_project_logo(
        &mut self,
        key: &ProjectGroupKey,
        source: PathBuf,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let Some(extension) = logo_extension(&source) else {
            self.notify_project_logo_failure(
                format!("{} is not an image Zode can draw", source.display()),
                cx,
            );
            return Task::ready(());
        };

        let previous = self.project_logo(key);
        // Before the copy starts, not after: a project this window does not
        // show has nothing to set a logo on, and copying a file first would
        // leave it on disk with no record naming it.
        let Some(generation) = self.bump_logo_generation(key, cx) else {
            return Task::ready(());
        };
        let fs = self.workspace().read(cx).app_state().fs.clone();
        let key = key.clone();

        cx.spawn(async move |this, cx| {
            let result: Result<()> = async {
                let metadata = fs
                    .metadata(&source)
                    .await?
                    .context("the source file could not be found")?;
                if metadata.len > MAX_LOGO_BYTES {
                    anyhow::bail!(
                        "{} is too large for Zode to keep a copy of",
                        source.display()
                    );
                }

                fs.create_dir(&project_avatars_dir()).await?;
                let target: Arc<Path> =
                    Arc::from(project_avatars_dir().join(new_logo_file_name(&extension)));
                fs.copy_file(&source, &target, CopyOptions::default())
                    .await?;

                let landed = this.update(cx, |this, cx| {
                    let current = this
                        .group_state_by_key(&key)
                        .map(|group| group.logo_generation);
                    if current != Some(generation) {
                        // Something newer was asked for while this copy ran --
                        // another upload, a clear, or the project leaving the
                        // window. What was copied is already unwanted, so it
                        // goes now rather than sitting there with nothing
                        // naming it.
                        this.delete_logo_file(target.clone(), cx);
                        return;
                    }
                    if let Some(group) = this.presentation_state_for(&key, cx) {
                        group.logo = Some(target.clone());
                    }
                    this.serialize(cx);
                    cx.notify();
                    if let Some(previous) = previous {
                        this.delete_logo_file(previous, cx);
                    }
                });

                if landed.is_err() {
                    // The window closed while the copy was running. Nothing
                    // will ever name this file, and the entity that would
                    // normally delete it is already gone -- but `fs` was
                    // cloned before the spawn and outlives it, so the copy
                    // can still be cleaned up from here. Without this, an
                    // ordinary "close the window mid-upload" leaves a file
                    // behind that nothing ever collects.
                    fs.remove_file(
                        &target,
                        RemoveOptions {
                            recursive: false,
                            ignore_if_not_exists: true,
                        },
                    )
                    .await
                    .log_err();
                    return Ok(());
                }

                Ok(())
            }
            .await;

            if let Err(error) = result {
                this.update(cx, |this, cx| {
                    this.notify_project_logo_failure(error.to_string(), cx);
                })
                .log_err();
            }
        })
    }

    /// Unsets the project's logo and deletes the copy it named. A project
    /// with no logo is a no-op, not an error.
    pub fn clear_project_logo(&mut self, key: &ProjectGroupKey, cx: &mut Context<Self>) {
        let logo = match self.bump_logo_generation(key, cx) {
            // The generation is taken even when there is no file to delete:
            // clearing a project that has no logo *yet* still has to cancel
            // an upload that is mid-copy, or it lands and sets one.
            Some(_) => self.group_state_by_key_mut(key).and_then(|g| g.logo.take()),
            None => return,
        };
        let Some(logo) = logo else {
            return;
        };
        self.serialize(cx);
        cx.notify();
        self.delete_logo_file(logo, cx);
    }

    /// Drops the decoded copy from the image cache and removes the file on
    /// the background executor, without leaving a dropped, un-awaited `Task`.
    ///
    /// A file another project still names is left alone. Two records can name
    /// one file -- a record copied by hand is the way there -- and the
    /// restore path already refuses to drop a file for that reason. Deleting
    /// here would take the logo out from under a project that is still on the
    /// rail, which then falls back to initials with nothing to explain it.
    /// Every deletion in this feature goes through this one door, so the
    /// check belongs here rather than at each of the callers.
    ///
    /// `ignore_if_not_exists: true` because a file already gone is the
    /// outcome we wanted, not a failure worth telling anyone about.
    /// `detach_and_log_err` rather than discarding the result outright: a
    /// deletion that fails must be visible in the log, and is not worth
    /// interrupting the user over.
    pub(crate) fn delete_logo_file(&mut self, path: Arc<Path>, cx: &mut Context<Self>) {
        if self.any_group_names_logo(path.as_ref()) {
            return;
        }
        ImageSource::from(path.clone()).remove_asset(cx);
        let fs = self.workspace().read(cx).app_state().fs.clone();
        cx.background_spawn(async move {
            fs.remove_file(
                &path,
                RemoveOptions {
                    recursive: false,
                    ignore_if_not_exists: true,
                },
            )
            .await
        })
        .detach_and_log_err(cx);
    }

    fn notify_project_logo_failure(&self, message: String, cx: &mut Context<Self>) {
        self.workspace().clone().update(cx, |workspace, cx| {
            workspace.show_toast(
                Toast::new(NotificationId::unique::<ProjectLogoToast>(), message),
                cx,
            );
        });
    }
}
