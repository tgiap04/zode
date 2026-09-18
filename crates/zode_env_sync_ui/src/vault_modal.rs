use std::path::{Path, PathBuf};

use editor::Editor;
use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, PathPromptOptions,
    Subscription, WeakEntity, Window,
};
use ui::{
    CommonAnimationExt, ListItem, ListSeparator, Modal, ModalFooter, ModalHeader, Section, Tooltip,
    prelude::*,
};
use workspace::{ModalView, Workspace};
use zode_env_sync::{EntryId, EnvSession, EnvStatus, ProjectId};

use crate::{ActiveEnvFile, WirePanel};

/// Why the window is open, and therefore what a project row does when clicked.
///
/// One window rather than three: the list of projects, the empty state, the
/// name field and the status line are identical in all three cases, and the
/// only thing that genuinely differs is what picking a project *means*.
enum Purpose {
    /// Browsing everything on the account.
    Browse,
    /// Answering "which project is this checkout?".
    Bind { worktree_root: PathBuf },
    /// Opened from an environment file that is not in the catalogue yet: pick
    /// the project it belongs to, and send it.
    Send { file: ActiveEnvFile },
}

/// Everything stored on this account, and the one place to put more there.
///
/// The names and paths shown here were decrypted on this machine a moment ago.
/// The server that stores them sees thirty-two hexadecimal characters per
/// file and nothing else — which is the point, and is why this list cannot be
/// rendered anywhere but inside the editor.
pub struct VaultModal {
    session: Entity<EnvSession>,
    /// Needed only to hand off to [`WirePanel`] after a file is added, since
    /// swapping one modal for another goes through the workspace.
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    purpose: Purpose,
    /// The name being typed for a new project, and whether that row is open.
    name_input: Entity<Editor>,
    naming: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DismissEvent> for VaultModal {}
impl ModalView for VaultModal {}

impl Focusable for VaultModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl VaultModal {
    pub fn new(
        session: Entity<EnvSession>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_purpose(session, workspace, Purpose::Browse, window, cx)
    }

    /// The same window, opened to bind a checkout.
    ///
    /// Binding is deliberately a choice from this list rather than something
    /// derived from a git remote or a path: a second machine has to land on
    /// the SAME project, and the only thing that reliably knows which one that
    /// is, is the person who made it.
    pub fn binding(
        session: Entity<EnvSession>,
        workspace: WeakEntity<Workspace>,
        worktree_root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_purpose(
            session,
            workspace,
            Purpose::Bind { worktree_root },
            window,
            cx,
        )
    }

    /// The same window, opened from an environment file that has no entry yet.
    pub fn sending(
        session: Entity<EnvSession>,
        workspace: WeakEntity<Workspace>,
        file: ActiveEnvFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_purpose(session, workspace, Purpose::Send { file }, window, cx)
    }

    fn with_purpose(
        session: Entity<EnvSession>,
        workspace: WeakEntity<Workspace>,
        purpose: Purpose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_input = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("acme-api", window, cx);
            editor
        });

        let mut subscriptions = vec![cx.observe(&session, |_this, _session, cx| cx.notify())];
        subscriptions.push(cx.subscribe(&name_input, |_this, _editor, event, cx| {
            if matches!(event, editor::EditorEvent::BufferEdited) {
                cx.notify();
            }
        }));

        // Unlock first, THEN load. Both were kicked off together before, and
        // because unlocking is asynchronous the load always ran without a key
        // and set "enter your recovery key" for a moment — a message that was
        // wrong by the time anyone read it.
        let unlock = session.update(cx, |session, cx| session.unlock(cx));
        cx.spawn({
            let session = session.clone();
            async move |_this, cx| {
                unlock.await;
                _ = session.update(cx, |session, cx| {
                    if session.is_unlocked() {
                        session.load_vault(cx);
                    }
                });
            }
        })
        .detach();

        Self {
            session,
            workspace,
            focus_handle: cx.focus_handle(),
            purpose,
            name_input,
            naming: false,
            _subscriptions: subscriptions,
        }
    }

    /// Acts on the project the user just picked, according to why the window
    /// is open.
    fn choose(&mut self, project: ProjectId, window: &mut Window, cx: &mut Context<Self>) {
        match &self.purpose {
            Purpose::Browse => {}
            Purpose::Bind { worktree_root } => {
                let root = worktree_root.clone();
                self.session
                    .update(cx, |session, cx| session.bind(root, project, cx));
                cx.emit(DismissEvent);
            }
            Purpose::Send { file } => {
                let file = file.clone();
                self.send(project, file, window, cx);
            }
        }
    }

    /// Records the open file under a project and shows what would be sent.
    fn send(
        &mut self,
        project: ProjectId,
        file: ActiveEnvFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entry = self.session.update(cx, |session, cx| {
            // Binding here is not a side effect smuggled in — it is the answer
            // the user just gave. "This file, in this checkout, belongs to
            // that project" is exactly what a binding records, and having it
            // recorded is what lets the catalogue store `services/api/.env`
            // instead of a bare `.env`, so a pull on the next machine lands in
            // the right directory. It is local and never sent.
            if session.project_for(&file.worktree_root).is_none() {
                session.bind(file.worktree_root.clone(), project, cx);
            }
            session.add_file(project, file.absolute.clone(), cx)
        });
        // `add_file` has already put the reason on the status line.
        let Some(entry) = entry else {
            cx.notify();
            return;
        };
        self.show_push(entry, file, window, cx);
    }

    /// Builds the push, then hands the bytes to the window that displays them.
    ///
    /// Built, then shown, then sent — in that order, always. There is no path
    /// from this window to the network that skips the display.
    fn show_push(
        &mut self,
        entry: EntryId,
        file: ActiveEnvFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.update(cx, |session, cx| {
            session.prepare(entry, file.absolute.clone(), cx)
        });
        let Some(wire) = self.session.read(cx).prepared().cloned() else {
            // Nothing to send, or it failed to build. The status line says
            // which, so the window stays open to show it.
            cx.notify();
            return;
        };

        // Untestable by design rather than by omission: `WeakEntity` is
        // `new_invalid()` under test, so the handoff is skipped there. What it
        // hands over — the prepared bytes — is asserted in `zode_env_sync`.
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let session = self.session.clone();
        let name = file.display_name();
        cx.emit(DismissEvent);
        workspace.update(cx, |_workspace, cx| {
            // Deferred because `toggle_modal` would otherwise close the window
            // it just opened: this one is still being torn down.
            cx.defer_in(window, move |workspace, window, cx| {
                workspace.toggle_modal(window, cx, |_window, cx| {
                    WirePanel::new(session, wire, name, cx)
                });
            });
        });
    }

    fn create_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).text(cx).trim().to_string();
        if name.is_empty() {
            return;
        }
        let created = self
            .session
            .update(cx, |session, cx| session.add_project(name, cx));
        self.naming = false;
        self.name_input
            .update(cx, |editor, cx| editor.set_text("", window, cx));
        cx.notify();

        // A project made while a file is waiting exists *for* that file, and a
        // checkout being bound was going to be bound to something. Making the
        // user then pick it out of a one-row list would be a step that asks
        // nothing.
        if let Some(project) = created {
            self.choose(project, window, cx);
        }
    }

    /// Asks for a file, then hands off.
    ///
    /// Stays too thin to hold a bug: `TestPlatform::prompt_for_paths` is
    /// `unimplemented!()`, so nothing a test could catch may live here. Every
    /// real decision is in `EnvSession::add_file`.
    fn choose_file(&mut self, project: ProjectId, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            // Without this the picker cannot see a single file it exists to
            // choose: every name it is looking for starts with a dot, and
            // macOS hides those by default.
            show_hidden: true,
            prompt: Some("Add environment file".into()),
        });
        let session = self.session.clone();

        cx.spawn_in(window, async move |this, cx| {
            // A channel error, an `Err` and a `None` all mean the same thing —
            // no file was chosen — and none is worth reporting.
            let Ok(Ok(Some(mut chosen))) = paths.await else {
                return;
            };
            let Some(path) = chosen.pop() else {
                return;
            };
            let Some(entry) = session.update(cx, |session, cx| {
                session.add_file(project, path.clone(), cx)
            }) else {
                return;
            };
            _ = this.update_in(cx, |this, window, cx| {
                // A file inside a checkout already bound to this project was
                // stored relative to that root. One from anywhere else was
                // stored under its bare name, so its parent is the root that
                // makes the label match what was actually recorded.
                let worktree_root = session
                    .read(cx)
                    .bindings()
                    .worktrees_for(project)
                    .into_iter()
                    .find(|root| path.starts_with(root))
                    .or_else(|| path.parent().map(Path::to_path_buf))
                    .unwrap_or_else(|| path.clone());
                let file = ActiveEnvFile {
                    worktree_root,
                    absolute: path.clone(),
                };
                this.show_push(entry, file, window, cx);
            });
        })
        .detach();
    }

    fn status_line(&self, cx: &App) -> Option<(SharedString, Color)> {
        match self.session.read(cx).status() {
            EnvStatus::Idle => None,
            EnvStatus::Working => Some(("working…".into(), Color::Muted)),
            EnvStatus::Done(message) => Some((message.clone(), Color::Muted)),
            EnvStatus::NeedsRecoveryKey => Some((
                "Enter your recovery key first — Account → Enter Recovery Key…".into(),
                Color::Warning,
            )),
            EnvStatus::KeyMismatch => Some((
                "These files were encrypted with a different key, probably rotated elsewhere."
                    .into(),
                Color::Error,
            )),
            EnvStatus::Rollback { seen, got } => Some((
                format!(
                    "The server offered version {got} of a file this machine already has at {seen}. Nothing was written."
                )
                .into(),
                Color::Error,
            )),
            EnvStatus::Failed(message) => Some((message.clone(), Color::Error)),
        }
    }

    /// The headline and the sentence under it, which say why the window opened.
    fn heading(&self) -> (SharedString, SharedString) {
        match &self.purpose {
            Purpose::Browse => (
                "Environment files".into(),
                "Names and paths are decrypted here. The server holds ciphertext and opaque identifiers.".into(),
            ),
            Purpose::Bind { .. } => (
                "Which project is this checkout?".into(),
                "Recorded on this machine only — the answer is never sent.".into(),
            ),
            Purpose::Send { file } => (
                format!("Send {}", file.display_name()).into(),
                format!(
                    "Pick the project {} belongs to. You will see the exact bytes before anything leaves.",
                    file.relative_label()
                )
                .into(),
            ),
        }
    }
}

impl Render for VaultModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (headline, description) = self.heading();

        // Read out of the purpose before the session borrow, so the row
        // closures below do not have to hold on to `self`.
        let bind_root = match &self.purpose {
            Purpose::Bind { worktree_root } => Some(worktree_root.clone()),
            _ => None,
        };
        let picking = !matches!(self.purpose, Purpose::Browse);

        let session = self.session.read(cx);
        let unlocked = session.is_unlocked();
        let working = matches!(session.status(), EnvStatus::Working);
        let manifest = session.manifest();
        let deleted_elsewhere: Vec<EntryId> = session.reconciliation().deleted_elsewhere.clone();

        let mut rows = v_flex().w_full().gap_1();

        if !unlocked {
            // Not a list, and not an empty one either: this account has no
            // environment key yet, so there is nothing to list and one button
            // to press. Saying "no projects" here would be true and useless.
            rows = rows.child(
                v_flex()
                    .gap_2()
                    .p_2()
                    .child(
                        Label::new(
                            "Environment sync is not set up on this account yet. Setting it up creates a key, wrapped under the recovery key you already have — there is no second phrase to write down.",
                        )
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    )
                    .child(
                        Button::new("env-vault-set-up", "Set Up Environment Sync")
                            .style(ButtonStyle::Filled)
                            .start_icon(Icon::new(IconName::LockOutlined).size(IconSize::Small))
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.session
                                    .update(cx, |session, cx| session.create_key(cx))
                                    .detach_and_log_err(cx);
                            })),
                    ),
            );
        } else if manifest.projects.is_empty() {
            // An empty state that only describes itself is a dead end, so the
            // action that resolves it sits inside it.
            rows = rows.child(
                v_flex()
                    .gap_2()
                    .p_2()
                    .items_start()
                    .child(
                        Label::new(if picking {
                            "No projects yet. Name one and this file goes straight into it."
                        } else {
                            "No projects yet. Make one, then add the files that belong to it."
                        })
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    )
                    .when(!self.naming, |this| {
                        this.child(
                            Button::new("env-vault-first-project", "New Project\u{2026}")
                                .style(ButtonStyle::Filled)
                                .start_icon(Icon::new(IconName::Plus).size(IconSize::Small))
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.naming = true;
                                    window.focus(&this.name_input.focus_handle(cx), cx);
                                    cx.notify();
                                })),
                        )
                    }),
            );
        }

        for (project_id, project) in &manifest.projects {
            let project_id = *project_id;
            let file_count = project.entries.len();
            let bound_here = bind_root
                .as_ref()
                .is_some_and(|root| session.project_for(root) == Some(project_id));

            let mut header = ListItem::new(SharedString::from(format!(
                "env-vault-project-{}",
                project_id.as_hex()
            )))
            .rounded()
            .start_slot(
                Icon::new(IconName::Folder)
                    .size(IconSize::Small)
                    .color(Color::Muted),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Label::new(project.name.clone()))
                    .child(
                        Label::new(match file_count {
                            0 => "no files".to_string(),
                            1 => "1 file".to_string(),
                            many => format!("{many} files"),
                        })
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                    ),
            );

            if picking {
                // The whole row is the target rather than a small button at
                // its end: it is the only thing this window is asking for.
                header = header
                    .selectable(true)
                    .end_slot(if bound_here {
                        Icon::new(IconName::Check)
                            .size(IconSize::Small)
                            .color(Color::Accent)
                    } else {
                        Icon::new(IconName::ArrowUp)
                            .size(IconSize::Small)
                            .color(Color::Muted)
                    })
                    .tooltip(Tooltip::text(if bound_here {
                        "Already this checkout's project"
                    } else {
                        "Use this project"
                    }))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.choose(project_id, window, cx)),
                    );
            } else {
                header = header.end_slot(
                    Button::new(
                        SharedString::from(format!("env-vault-add-file-{}", project_id.as_hex())),
                        "Add file\u{2026}",
                    )
                    .label_size(LabelSize::XSmall)
                    .start_icon(Icon::new(IconName::Plus).size(IconSize::Indicator))
                    .tooltip(Tooltip::text(
                        "Pick a file on this machine, including dotfiles",
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.choose_file(project_id, window, cx)
                    })),
                );
            }

            let mut entries = v_flex().w_full().gap_0p5();
            for (entry_id, entry) in &project.entries {
                let entry_id = *entry_id;
                let missing = deleted_elsewhere.contains(&entry_id);
                entries = entries.child(
                    ListItem::new(SharedString::from(format!(
                        "env-vault-entry-{}",
                        entry_id.as_hex()
                    )))
                    .rounded()
                    .indent_level(1)
                    .start_slot(
                        Icon::new(if missing {
                            IconName::Warning
                        } else {
                            IconName::FileLock
                        })
                        .size(IconSize::Small)
                        .color(if missing {
                            Color::Warning
                        } else {
                            Color::Muted
                        }),
                    )
                    .child(
                        v_flex()
                            .child(Label::new(entry.path.clone()).size(LabelSize::Small))
                            .when(missing, |this| {
                                this.child(
                                    Label::new(
                                        "deleted on another machine — the local file is untouched",
                                    )
                                    .size(LabelSize::XSmall)
                                    .color(Color::Warning),
                                )
                            }),
                    )
                    .end_slot(
                        IconButton::new(
                            SharedString::from(format!("env-vault-forget-{}", entry_id.as_hex())),
                            IconName::Trash,
                        )
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text(
                            "Removes it from your account. The file on this machine is left alone",
                        ))
                        .on_click(cx.listener(
                            move |this, _, _window, cx| {
                                this.session
                                    .update(cx, |session, cx| session.forget_entry(entry_id, cx));
                            },
                        )),
                    ),
                );
            }

            rows = rows.child(
                v_flex()
                    .w_full()
                    .child(header)
                    .when(!picking, |this| this.child(entries)),
            );
        }

        let status = self.status_line(cx);

        div()
            .key_context("EnvVaultModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .occlude()
            .w(rems(42.))
            .max_h(rems(40.))
            .on_action(cx.listener(|_this, _: &menu::Cancel, _window, cx| {
                cx.emit(DismissEvent);
            }))
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                if this.naming {
                    this.create_project(window, cx);
                }
            }))
            .child(
                Modal::new("env-vault", None)
                    .header(
                        ModalHeader::new()
                            .icon(
                                Icon::new(IconName::FileLock)
                                    .size(IconSize::Small)
                                    .color(Color::Muted),
                            )
                            .headline(headline)
                            .description(description),
                    )
                    .section(
                        Section::new().padded(false).child(
                            v_flex()
                                .w_full()
                                .child(ListSeparator)
                                .child(
                                    div()
                                        .id("env-vault-body")
                                        .w_full()
                                        .max_h(rems(22.))
                                        .p_1()
                                        .overflow_y_scroll()
                                        .child(rows),
                                )
                                .when(unlocked && self.naming, |this| {
                                    this.child(ListSeparator).child(
                                        v_flex()
                                            .w_full()
                                            .p_2()
                                            .gap_1()
                                            // A visible label, not just a
                                            // placeholder: the placeholder is
                                            // an example name and vanishes the
                                            // moment anything is typed.
                                            .child(
                                                Label::new("Project name")
                                                    .size(LabelSize::XSmall)
                                                    .color(Color::Muted),
                                            )
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .px_2()
                                                            .py_1()
                                                            .rounded_sm()
                                                            .border_1()
                                                            .border_color(colors.border_focused)
                                                            .bg(colors.editor_background)
                                                            .child(self.name_input.clone()),
                                                    )
                                                    .child(
                                                        Button::new("env-vault-create", "Add")
                                                            .style(ButtonStyle::Filled)
                                                            .label_size(LabelSize::Small)
                                                            .disabled(
                                                                self.name_input
                                                                    .read(cx)
                                                                    .text(cx)
                                                                    .trim()
                                                                    .is_empty(),
                                                            )
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    this.create_project(window, cx)
                                                                },
                                                            )),
                                                    ),
                                            ),
                                    )
                                })
                                .when_some(status, |this, (message, color)| {
                                    this.child(ListSeparator).child(
                                        h_flex()
                                            .w_full()
                                            .p_2()
                                            .gap_1p5()
                                            .items_center()
                                            .when(working, |this| {
                                                this.child(
                                                    Icon::new(IconName::LoadCircle)
                                                        .size(IconSize::Small)
                                                        .color(Color::Muted)
                                                        .with_rotate_animation(3),
                                                )
                                            })
                                            .when(!working && color != Color::Muted, |this| {
                                                this.child(
                                                    Icon::new(if color == Color::Error {
                                                        IconName::XCircle
                                                    } else {
                                                        IconName::Warning
                                                    })
                                                    .size(IconSize::Small)
                                                    .color(color),
                                                )
                                            })
                                            .child(
                                                Label::new(message)
                                                    .size(LabelSize::Small)
                                                    .color(color),
                                            ),
                                    )
                                }),
                        ),
                    )
                    .footer(
                        ModalFooter::new()
                            .start_slot(
                                div().when(unlocked && !manifest.projects.is_empty(), |this| {
                                    this.child(
                                        Button::new("env-vault-new-project", "New Project\u{2026}")
                                            .label_size(LabelSize::Small)
                                            .start_icon(
                                                Icon::new(IconName::Plus).size(IconSize::Small),
                                            )
                                            .tooltip(Tooltip::text(
                                                "A project groups the environment files that belong together",
                                            ))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.naming = !this.naming;
                                                if this.naming {
                                                    window
                                                        .focus(&this.name_input.focus_handle(cx), cx);
                                                }
                                                cx.notify();
                                            })),
                                    )
                                }),
                            )
                            .end_slot(
                                Button::new("env-vault-close", "Close")
                                    .label_size(LabelSize::Small)
                                    .on_click(cx.listener(|_this, _, _window, cx| {
                                        cx.emit(DismissEvent)
                                    })),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::collections::BTreeMap;
    use zode_account::{Account, AccountStatus, AccountUser};
    use zode_env_sync::manifest::{Manifest, ManifestEntry, ManifestProject};

    fn init_theme(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let settings_store = settings::SettingsStore::test(cx);
            cx.set_global(settings_store);
            theme_settings::init(theme::LoadThemes::JustBase, cx);
        });
    }

    fn session(manifest: Option<Manifest>, cx: &mut TestAppContext) -> Entity<EnvSession> {
        let account = cx.update(|cx| {
            cx.new(|_| {
                Account::for_test(AccountStatus::SignedIn(AccountUser {
                    id: "1".into(),
                    email: "ada@example.com".into(),
                    name: None,
                    avatar_url: None,
                }))
            })
        });
        let session = cx.update(|cx| cx.new(|_| EnvSession::new(account)));
        if let Some(manifest) = manifest {
            session.update(cx, |session, cx| {
                session.set_manifest_for_test(manifest, cx)
            });
        }
        session
    }

    fn populated() -> Manifest {
        let mut manifest = Manifest::new();
        manifest.projects.insert(
            ProjectId::parse(&"11".repeat(16)).unwrap(),
            ManifestProject {
                name: "acme-api".into(),
                entries: BTreeMap::from([(
                    EntryId::parse(&"a1".repeat(16)).unwrap(),
                    ManifestEntry {
                        path: "services/api/.env.production".into(),
                        seq: 3,
                    },
                )]),
            },
        );
        manifest
    }

    fn a_file() -> ActiveEnvFile {
        ActiveEnvFile {
            worktree_root: PathBuf::from("/checkout"),
            absolute: PathBuf::from("/checkout/services/api/.env"),
        }
    }

    #[gpui::test]
    fn the_window_draws_an_empty_vault(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session(None, cx);
        let (_modal, cx) = cx.add_window_view(|window, cx| {
            VaultModal::new(session, WeakEntity::new_invalid(), window, cx)
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_window_draws_projects_and_files(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session(Some(populated()), cx);
        let (_modal, cx) = cx.add_window_view(|window, cx| {
            VaultModal::new(session, WeakEntity::new_invalid(), window, cx)
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn an_account_with_no_key_is_offered_set_up_rather_than_an_empty_list(cx: &mut TestAppContext) {
        // The dead end this window used to be: it said "No projects yet" to an
        // account that had no environment key, which is true and unactionable.
        init_theme(cx);
        let session = session(None, cx);
        let (modal, cx) = cx.add_window_view(|window, cx| {
            VaultModal::new(session, WeakEntity::new_invalid(), window, cx)
        });
        cx.run_until_parked();

        modal.update(cx, |modal, cx| {
            assert!(
                !modal.session.read(cx).is_unlocked(),
                "the fixture account has no env key, which is the case under test",
            );
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_new_project_row_draws_and_refuses_an_empty_name(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session(Some(populated()), cx);
        let (modal, cx) = cx.add_window_view(|window, cx| {
            VaultModal::new(session, WeakEntity::new_invalid(), window, cx)
        });
        cx.run_until_parked();

        modal.update_in(cx, |modal, window, cx| {
            modal.naming = true;
            // Nothing typed: adding must be a no-op rather than creating a
            // project with an empty name nobody can identify later.
            let before = modal.session.read(cx).manifest().projects.len();
            modal.create_project(window, cx);
            assert_eq!(modal.session.read(cx).manifest().projects.len(), before);
            assert!(modal.naming, "the row stays open so the name can be typed");
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn sending_draws_a_picker_rather_than_the_whole_vault(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session(Some(populated()), cx);
        let (modal, cx) = cx.add_window_view(|window, cx| {
            VaultModal::sending(session, WeakEntity::new_invalid(), a_file(), window, cx)
        });
        cx.run_until_parked();

        modal.update(cx, |modal, _cx| {
            let (headline, description) = modal.heading();
            assert!(
                headline.contains(".env"),
                "the headline must name the file being sent, got {headline:?}",
            );
            assert!(
                description.contains("services/api/.env"),
                "the sentence must show where in the checkout it came from, got {description:?}",
            );
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn naming_a_project_while_a_file_waits_sends_it_without_a_second_choice(
        cx: &mut TestAppContext,
    ) {
        // The complaint this answers: pushing a file used to mean binding the
        // checkout, opening the vault, making a project and then finding the
        // file picker. One name is now enough.
        init_theme(cx);
        let session = session(None, cx);
        let (modal, cx) = cx.add_window_view({
            let session = session.clone();
            |window, cx| {
                VaultModal::sending(session, WeakEntity::new_invalid(), a_file(), window, cx)
            }
        });
        cx.run_until_parked();

        modal.update_in(cx, |modal, window, cx| {
            modal.naming = true;
            modal
                .name_input
                .update(cx, |editor, cx| editor.set_text("acme-api", window, cx));
            modal.create_project(window, cx);
        });
        cx.run_until_parked();

        session.update(cx, |session, _cx| {
            let projects = &session.manifest().projects;
            assert_eq!(projects.len(), 1, "the named project must exist");
            let project = projects.values().next().expect("just asserted one");
            assert_eq!(project.name, "acme-api");
            assert_eq!(
                project.entries.len(),
                1,
                "the waiting file must have been filed under it without a second prompt",
            );
            let entry = project.entries.values().next().expect("just asserted one");
            assert_eq!(
                entry.path, "services/api/.env",
                "the path must be relative to the checkout, so a pull elsewhere lands right",
            );
        });

        // Picking the project also answered "which project is this checkout?",
        // because that is the same fact.
        session.update(cx, |session, _cx| {
            assert!(
                session.project_for(&PathBuf::from("/checkout")).is_some(),
                "the checkout must be bound by the same choice",
            );
        });
    }

    #[gpui::test]
    fn every_status_has_something_to_say(cx: &mut TestAppContext) {
        // Each branch of `status_line` is rendered at least once, so a new
        // variant cannot be added without a sentence for the user.
        init_theme(cx);
        let session = session(Some(populated()), cx);
        let (modal, cx) = cx.add_window_view({
            let session = session.clone();
            |window, cx| VaultModal::new(session, WeakEntity::new_invalid(), window, cx)
        });
        cx.run_until_parked();

        for status in [
            EnvStatus::Working,
            EnvStatus::Done("sent".into()),
            EnvStatus::NeedsRecoveryKey,
            EnvStatus::KeyMismatch,
            EnvStatus::Rollback { seen: 9, got: 4 },
            EnvStatus::Failed("the service is unreachable".into()),
        ] {
            session.update(cx, |session, cx| {
                session.set_status_for_test(status.clone(), cx)
            });
            cx.run_until_parked();
            modal.update(cx, |modal, cx| {
                let line = modal.status_line(cx);
                assert!(line.is_some(), "{status:?} renders nothing");
                assert!(
                    !line.unwrap().0.is_empty(),
                    "{status:?} renders an empty line"
                );
            });
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        }

        session.update(cx, |session, cx| {
            session.set_status_for_test(EnvStatus::Idle, cx)
        });
        cx.run_until_parked();
        modal.update(cx, |modal, cx| assert!(modal.status_line(cx).is_none()));
    }
}
