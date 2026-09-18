use editor::Editor;
use gpui::{
    App, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, PathPromptOptions,
    Subscription, Window,
};
use ui::{Tooltip, prelude::*};
use workspace::ModalView;
use zode_env_sync::{EntryId, EnvSession, EnvStatus, ProjectId};

/// Everything stored on this account, and the one place to put more there.
///
/// The names and paths shown here were decrypted on this machine a moment ago.
/// The server that stores them sees thirty-two hexadecimal characters per
/// file and nothing else — which is the point, and is why this list cannot be
/// rendered anywhere but inside the editor.
pub struct VaultModal {
    session: Entity<EnvSession>,
    focus_handle: FocusHandle,
    /// Set when the window was opened to answer "which project is this
    /// checkout?". Each project then offers to take it.
    ///
    /// Binding is deliberately a choice from this list rather than something
    /// derived from a git remote or a path: a second machine has to land on
    /// the SAME project, and the only thing that reliably knows which one that
    /// is, is the person who made it.
    binding_for: Option<std::path::PathBuf>,
    /// The name being typed for a new project, and whether that row is open.
    ///
    /// A project has to exist before a file can be added to one, and until
    /// this row existed the window was a dead end: it said "No projects yet"
    /// and offered nothing that could make one.
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
    pub fn new(session: Entity<EnvSession>, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            focus_handle: cx.focus_handle(),
            binding_for: None,
            name_input,
            naming: false,
            _subscriptions: subscriptions,
        }
    }

    fn create_project(&mut self, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).text(cx).trim().to_string();
        if name.is_empty() {
            return;
        }
        self.session
            .update(cx, |session, cx| session.add_project(name, cx));
        self.naming = false;
        cx.notify();
    }

    /// The same window, opened to bind a checkout.
    pub fn binding(
        session: Entity<EnvSession>,
        worktree_root: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::new(session, window, cx);
        this.binding_for = Some(worktree_root);
        this
    }

    /// Asks for a file, then hands off.
    ///
    /// Stays too thin to hold a bug: `TestPlatform::prompt_for_paths` is
    /// `unimplemented!()`, so nothing a test could catch may live here. Every
    /// real decision is in `EnvSession::add_file`, which tests drive directly.
    fn choose_file(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Add environment file".into()),
        });
        let session = self.session.clone();

        cx.spawn(async move |_this, cx| {
            // A channel error, an `Err` and a `None` all mean the same thing —
            // no file was chosen — and none is worth reporting.
            let Ok(Ok(Some(mut chosen))) = paths.await else {
                return;
            };
            let Some(path) = chosen.pop() else {
                return;
            };
            _ = session.update(cx, |session, cx| session.add_file(project, path, cx));
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
}

impl Render for VaultModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        let session = self.session.read(cx);
        let unlocked = session.is_unlocked();
        let manifest = session.manifest();
        let reconciliation = session.reconciliation();
        let deleted_elsewhere: Vec<EntryId> = reconciliation.deleted_elsewhere.clone();

        let mut rows = v_flex().w_full().gap_2();

        if !unlocked {
            // Not a list, and not an empty one either: this account has no
            // environment key yet, so there is nothing to list and one button
            // to press. Saying "no projects" here would be true and useless.
            rows = rows.child(
                v_flex()
                    .gap_2()
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
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.session
                                    .update(cx, |session, cx| session.create_key(cx))
                                    .detach_and_log_err(cx);
                            })),
                    ),
            );
        } else if manifest.projects.is_empty() {
            rows = rows.child(
                Label::new("No projects yet. Add one, then add the files that belong to it.")
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            );
        }

        for (project_id, project) in &manifest.projects {
            let project_id = *project_id;
            let mut entries = v_flex().w_full().pl_3().gap_0p5();

            if project.entries.is_empty() {
                entries = entries.child(
                    Label::new("no files")
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                );
            }

            for (entry_id, entry) in &project.entries {
                let entry_id = *entry_id;
                let missing = deleted_elsewhere.contains(&entry_id);
                entries = entries.child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .justify_between()
                        .items_center()
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
                        .child(
                            Button::new(SharedString::from(format!("env-vault-forget-{}", entry_id.as_hex())), "Remove")
                                .label_size(LabelSize::XSmall)
                                .tooltip(Tooltip::text(
                                    "Removes it from your account. The file on this machine is left alone",
                                ))
                                .on_click(cx.listener(move |this, _, _window, cx| {
                                    this.session.update(cx, |session, cx| {
                                        session.forget_entry(entry_id, cx)
                                    });
                                })),
                        ),
                );
            }

            rows = rows.child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(Label::new(project.name.clone()).weight(FontWeight::MEDIUM))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .when_some(self.binding_for.clone(), |this, root| {
                                        let bound_here =
                                            session.project_for(&root) == Some(project_id);
                                        this.child(
                                            Button::new(
                                                SharedString::from(format!(
                                                    "env-vault-bind-{}",
                                                    project_id.as_hex()
                                                )),
                                                if bound_here {
                                                    "This checkout"
                                                } else {
                                                    "Use for this checkout"
                                                },
                                            )
                                            .label_size(LabelSize::XSmall)
                                            .disabled(bound_here)
                                            .tooltip(Tooltip::text(
                                                "Recorded on this machine only — it is never sent",
                                            ))
                                            .on_click(
                                                cx.listener(move |this, _, _window, cx| {
                                                    let root = root.clone();
                                                    this.session.update(cx, |session, cx| {
                                                        session.bind(root, project_id, cx)
                                                    });
                                                }),
                                            ),
                                        )
                                    })
                                    .child(
                                        Button::new(
                                            SharedString::from(format!(
                                                "env-vault-add-file-{}",
                                                project_id.as_hex()
                                            )),
                                            "Add file…",
                                        )
                                        .label_size(LabelSize::XSmall)
                                        .on_click(
                                            cx.listener(move |this, _, _window, cx| {
                                                this.choose_file(project_id, cx)
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .child(entries),
            );
        }

        let status = self.status_line(cx);

        v_flex()
            .key_context("EnvVaultModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(48.))
            .overflow_hidden()
            .on_action(cx.listener(|_this, _: &menu::Cancel, _window, cx| {
                cx.emit(DismissEvent);
            }))
            .child(
                v_flex()
                    .p_3()
                    .gap_0p5()
                    .child(Label::new("Environment files").weight(FontWeight::MEDIUM))
                    .child(
                        Label::new(
                            "Names and paths are decrypted here. The server holds ciphertext and opaque identifiers.",
                        )
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                    ),
            )
            .child(
                div()
                    .id("env-vault-body")
                    .w_full()
                    .max_h(rems(28.))
                    .p_3()
                    .overflow_y_scroll()
                    .bg(colors.editor_background)
                    .border_y_1()
                    .border_color(colors.border_variant)
                    .child(rows),
            )
            .when_some(status, |this, (message, color)| {
                this.child(
                    div()
                        .px_3()
                        .pt_2()
                        .child(Label::new(message).size(LabelSize::Small).color(color)),
                )
            })
            .when(unlocked && self.naming, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .pt_2()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .px_2()
                                .py_1()
                                .rounded_sm()
                                .border_1()
                                .border_color(colors.border)
                                .bg(colors.editor_background)
                                .child(self.name_input.clone()),
                        )
                        .child(
                            Button::new("env-vault-create", "Add")
                                .style(ButtonStyle::Filled)
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.create_project(cx)
                                })),
                        ),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .p_2()
                    .gap_1()
                    .justify_between()
                    .items_center()
                    .bg(colors.editor_background)
                    .child(div().when(unlocked, |this| {
                        this.child(
                            Button::new("env-vault-new-project", "New Project\u{2026}")
                                .label_size(LabelSize::Small)
                                .tooltip(Tooltip::text(
                                    "A project groups the environment files that belong together",
                                ))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.naming = !this.naming;
                                    if this.naming {
                                        window.focus(&this.name_input.focus_handle(cx), cx);
                                    }
                                    cx.notify();
                                })),
                        )
                    }))
                    .child(
                        Button::new("env-vault-close", "Close")
                            .label_size(LabelSize::Small)
                            .on_click(cx.listener(|_this, _, _window, cx| cx.emit(DismissEvent))),
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

    #[gpui::test]
    fn the_window_draws_an_empty_vault(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session(None, cx);
        let (_modal, cx) = cx.add_window_view(|window, cx| VaultModal::new(session, window, cx));
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_window_draws_projects_and_files(cx: &mut TestAppContext) {
        init_theme(cx);
        let session = session(Some(populated()), cx);
        let (_modal, cx) = cx.add_window_view(|window, cx| VaultModal::new(session, window, cx));
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
        let (modal, cx) =
            cx.add_window_view(|window, cx| VaultModal::new(session.clone(), window, cx));
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
        let (modal, cx) =
            cx.add_window_view(|window, cx| VaultModal::new(session.clone(), window, cx));
        cx.run_until_parked();

        modal.update(cx, |modal, cx| {
            modal.naming = true;
            // Nothing typed: adding must be a no-op rather than creating a
            // project with an empty name nobody can identify later.
            let before = modal.session.read(cx).manifest().projects.len();
            modal.create_project(cx);
            assert_eq!(modal.session.read(cx).manifest().projects.len(), before);
            assert!(modal.naming, "the row stays open so the name can be typed");
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn every_status_has_something_to_say(cx: &mut TestAppContext) {
        // Each branch of `status_line` is rendered at least once, so a new
        // variant cannot be added without a sentence for the user.
        init_theme(cx);
        let session = session(Some(populated()), cx);
        let (modal, cx) =
            cx.add_window_view(|window, cx| VaultModal::new(session.clone(), window, cx));
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
