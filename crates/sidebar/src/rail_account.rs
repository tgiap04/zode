use crate::Sidebar;
use crate::rail::RAIL_ICON_SIZE;
use gpui::{AnyElement, Context};
use ui::{ContextMenu, PopoverMenu, Tooltip, prelude::*};
use zode_account::{Account, AccountStatus};

impl Sidebar {
    /// The account button, at the very bottom of the rail.
    ///
    /// Signing in is optional in Zode, so this is deliberately quiet: a plain
    /// person glyph when signed out, the user's avatar once there is one. It
    /// never nags and it never appears mid-work.
    pub(crate) fn render_rail_account(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(account) = Account::global(cx) else {
            // The global is installed by `zode_account::init`. A window built
            // without it — tests, mostly — simply has no account button.
            return div().into_any_element();
        };

        let status = account.read(cx).status().clone();
        let summary = zode_account_ui::status_summary(&status);

        match &status {
            AccountStatus::SignedOut | AccountStatus::WaitingForApproval { .. } => {
                let waiting = matches!(status, AccountStatus::WaitingForApproval { .. });

                IconButton::new("project-rail-account", IconName::Person)
                    .icon_size(RAIL_ICON_SIZE)
                    .toggle_state(waiting)
                    .tooltip(move |_window, cx| {
                        Tooltip::with_meta(
                            summary.clone(),
                            Some(&zed_actions::account::SignIn),
                            "Optional — the editor works signed out",
                            cx,
                        )
                    })
                    // Dispatch rather than reaching into the account directly:
                    // this body runs inside `Sidebar::update`, and opening the
                    // modal reaches back through the workspace. Same trap as
                    // `render_rail_container`, and the panel toggle that hit it
                    // before either of them existed.
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(zed_actions::account::SignIn), cx)
                    })
                    .into_any_element()
            }
            AccountStatus::SignedIn(_) | AccountStatus::Offline(_) => {
                let tooltip = summary;
                let offline = matches!(status, AccountStatus::Offline(_));

                // The same `IconButton` as every other rail item, deliberately.
                // The rail is a column of monochrome glyphs — Git, Database,
                // Docker — and a colour photograph dropped into it reads as a
                // foreign object. Who is signed in belongs in the tooltip and
                // the menu, which is where someone looks for it anyway.
                PopoverMenu::new("project-rail-account-menu")
                    .trigger(
                        IconButton::new("project-rail-account", IconName::Person)
                            .icon_size(RAIL_ICON_SIZE)
                            .icon_color(if offline {
                                Color::Muted
                            } else {
                                Color::Default
                            })
                            .tooltip(move |_window, cx| Tooltip::simple(tooltip.clone(), cx)),
                    )
                    .menu(move |window, cx| {
                        Some(ContextMenu::build(window, cx, move |menu, _window, _cx| {
                            menu.action(
                                "Account on the Web",
                                Box::new(zed_actions::account::OpenAccountOnWeb),
                            )
                            .separator()
                            .action("Sign Out", Box::new(zed_actions::account::SignOut))
                        }))
                    })
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Sidebar;
    use crate::sidebar_tests::init_test;
    use fs::FakeFs;
    use gpui::{AppContext as _, TestAppContext};
    use project::Project;
    use workspace::MultiWorkspace;
    use zode_account::{Account, AccountStatus, AccountUser};

    /// Draws the rail for real at every account state.
    ///
    /// Building the element by hand would miss the thing worth testing: reading
    /// the workspace from inside `Sidebar::render` is the re-entrancy trap that
    /// already crashed the rail's panel toggle once, and it only shows up on an
    /// actual draw. Same reasoning, same shape, as the container test.
    async fn draw_rail_with(status: Option<AccountStatus>, cx: &mut TestAppContext) {
        init_test(cx);

        if let Some(status) = status {
            cx.update(|cx| {
                let account = cx.new(|_| Account::for_test(status));
                Account::set_global(account, cx);
            });
        }

        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project, window, cx));

        multi_workspace.update_in(cx, |mw, window, cx| {
            let mw_entity = cx.entity();
            let sidebar = cx.new(|cx| Sidebar::new(mw_entity, window, cx));
            mw.register_sidebar(sidebar, cx);
        });
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    fn a_user() -> AccountUser {
        AccountUser {
            id: "1".into(),
            email: "jane@example.com".into(),
            name: Some("Jane".into()),
            avatar_url: None,
        }
    }

    /// No account global at all — the case every existing test in this crate
    /// runs in, and the one a window built without `zode_account::init` hits.
    #[gpui::test]
    async fn the_rail_draws_with_no_account_installed(cx: &mut TestAppContext) {
        draw_rail_with(None, cx).await;
    }

    #[gpui::test]
    async fn the_rail_draws_signed_out(cx: &mut TestAppContext) {
        draw_rail_with(Some(AccountStatus::SignedOut), cx).await;
    }

    #[gpui::test]
    async fn the_rail_draws_while_waiting_for_approval(cx: &mut TestAppContext) {
        draw_rail_with(
            Some(AccountStatus::WaitingForApproval {
                user_code: "A1B2-C3D4".into(),
                verification_uri: "https://zode.dev/activate".into(),
                verification_uri_complete: "https://zode.dev/activate?code=A1B2-C3D4".into(),
            }),
            cx,
        )
        .await;
    }

    #[gpui::test]
    async fn the_rail_draws_signed_in(cx: &mut TestAppContext) {
        draw_rail_with(Some(AccountStatus::SignedIn(a_user())), cx).await;
    }

    #[gpui::test]
    async fn the_rail_draws_offline(cx: &mut TestAppContext) {
        draw_rail_with(Some(AccountStatus::Offline(a_user())), cx).await;
    }
}
