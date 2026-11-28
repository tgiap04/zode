use std::time::{SystemTime, UNIX_EPOCH};

use github::{GithubError, RateLimit, RateLimits, TokenSource, revoke_page, token_source_label};
use gpui::{Context, IntoElement, Render, Window};
use ui::{Callout, CopyButton, Tab, TabBar, TabPosition, prelude::*};

use crate::device_flow::is_github_page;
use crate::github_view::{AuthState, DeviceFlowStep, GithubSection, GithubView, Session};

const OAUTH_RESTRICTIONS_HELP: &str = "https://docs.github.com/en/organizations/managing-oauth-access-to-your-organizations-data/about-oauth-app-access-restrictions";

/// What to do about a token GitHub refused, by where it came from.
pub fn revoked_advice(source: TokenSource) -> String {
    match source {
        TokenSource::Keychain => "GitHub no longer accepts the token Zode signed in with. It may have been revoked, or replaced by a newer sign-in on another machine.".to_string(),
        TokenSource::GhCli => "GitHub no longer accepts the GitHub CLI's token. Run `gh auth login`, then check again.".to_string(),
        // `gh auth login` cannot help: `gh` hands back the variable first.
        TokenSource::GhEnvironment(variable) => format!(
            "GitHub no longer accepts the token in the {} variable. Update the GH_TOKEN/GITHUB_TOKEN variable (or unset it to use the account `gh` is signed in with), restart Zode, then check again.",
            variable.name()
        ),
    }
}

/// `REST 4980/5000 · GraphQL 4990/5000, resets in 12 min` for whichever
/// buckets GitHub has reported; the reset shown is the soonest.
pub fn rate_limit_summary(rate_limits: &RateLimits, now_unix: u64) -> Option<String> {
    let buckets = [
        ("REST", &rate_limits.rest),
        ("GraphQL", &rate_limits.graphql),
    ]
    .into_iter()
    .filter_map(|(name, bucket)| bucket.as_ref().map(|bucket| (name, bucket)))
    .collect::<Vec<(&str, &RateLimit)>>();
    let soonest = buckets.iter().map(|(_, bucket)| bucket.reset_at).min()?;
    let counts = buckets
        .iter()
        .map(|(name, bucket)| format!("{name} {}/{}", bucket.remaining, bucket.limit))
        .collect::<Vec<_>>()
        .join(" · ");
    let minutes = soonest.saturating_sub(now_unix).div_ceil(60);
    Some(format!("{counts}, resets in {minutes} min"))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

impl GithubView {
    fn render_contents(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .key_context("GithubView")
            .track_focus(&self.focus_handle)
            .size_full()
            .gap_2()
            .p_3()
            .bg(cx.theme().colors().editor_background)
            .child(self.render_header(cx))
            .when(!self.access.allows_local_writes(), |column| {
                column.child(
                    Callout::new()
                        .severity(Severity::Info)
                        .title("Shared project")
                        .description(
                            "This project belongs to a collaborator, so nothing here changes its files.",
                        ),
                )
            })
            .when_some(self.notice.clone(), |column, notice| {
                column.child(Label::new(notice).size(LabelSize::Small).color(Color::Warning))
            })
            .child(self.render_account(cx))
            .when(matches!(self.auth, AuthState::SignedIn(_)), |column| {
                column
                    .child(self.render_repository_picker(cx))
                    .when_some(self.repository_error.clone(), |column, error| {
                        column.child(self.render_error(&error, cx))
                    })
                    .child(self.render_sections(cx))
                    .child(self.render_section_body())
            })
    }
}

impl GithubView {
    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let summary = rate_limit_summary(&self.rate_limits, now_unix());
        h_flex()
            .gap_2()
            .child(Icon::new(IconName::Github))
            .child(Headline::new("GitHub").size(HeadlineSize::Small))
            .child(div().flex_1())
            .when_some(summary, |row, summary| {
                row.child(
                    Label::new(summary)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            // Offered while a check is still waiting, too: a check that never
            // ends would otherwise leave nothing on screen to try again with.
            // Not while signing in, which has its own Cancel.
            .when(!matches!(self.auth, AuthState::SigningIn(_)), |row| {
                row.child(
                    div().debug_selector(|| "github-reload".into()).child(
                        IconButton::new("github-reload", IconName::RotateCw)
                            .loading(matches!(self.auth, AuthState::Checking))
                            .tooltip(ui::Tooltip::text("Reload"))
                            .on_click(cx.listener(|this, _, _window, cx| this.reload(cx))),
                    ),
                )
            })
    }

    fn render_account(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.auth {
            AuthState::Checking => Label::new("Checking GitHub…")
                .color(Color::Muted)
                .into_any_element(),
            AuthState::SignedIn(session) => self.render_session(session, cx),
            AuthState::SignedOut { notice } => self.render_signed_out(notice.clone(), cx),
            AuthState::SigningIn(step) => self.render_device_flow(step, cx),
            AuthState::Revoked { source } => self.render_revoked(*source, cx),
            AuthState::Failed(error) => v_flex()
                .gap_1()
                .child(self.render_error(error, cx))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("github-check-again", "Check again")
                                .on_click(cx.listener(|this, _, _window, cx| this.connect(cx))),
                        )
                        // Checking again reads the same entry; only deleting it
                        // gets past it.
                        .when(self.offers_disconnect(), |row| {
                            row.child(
                                Button::new("github-forget-keychain-token", "Disconnect").on_click(
                                    cx.listener(|this, _, _window, cx| this.disconnect(cx)),
                                ),
                            )
                        }),
                )
                .into_any_element(),
        }
    }

    fn render_session(&self, session: &Session, cx: &mut Context<Self>) -> AnyElement {
        let from_zode = session.token_source == TokenSource::Keychain;
        let hint = if from_zode {
            None
        } else {
            Some(
                "Zode uses the account the GitHub CLI has active. With several accounts, change it with `gh auth switch`, then reload.",
            )
        };
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(Label::new(format!("@{}", session.login)))
                    .child(
                        Label::new(format!("via {}", token_source_label(session.token_source)))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .when(from_zode, |row| {
                        row.child(
                            Button::new("github-disconnect", "Disconnect")
                                .label_size(LabelSize::Small)
                                .on_click(cx.listener(|this, _, _window, cx| this.disconnect(cx))),
                        )
                    }),
            )
            .when_some(hint, |column, hint| {
                column.child(Label::new(hint).size(LabelSize::Small).color(Color::Muted))
            })
            .into_any_element()
    }

    fn render_signed_out(
        &self,
        notice: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let options = self.sign_in_options(cx);
        v_flex()
            .gap_2()
            .when_some(notice, |column, notice| {
                column.child(Label::new(notice).color(Color::Warning))
            })
            .when(options.device_flow, |column| {
                column.child(
                    Button::new("github-sign-in", "Sign in with GitHub")
                        .style(ButtonStyle::Filled)
                        .on_click(cx.listener(|this, _, _window, cx| this.sign_in_with_github(cx))),
                )
            })
            .child(Label::new(
                "Or use the GitHub CLI: run `gh auth login` in a terminal, then check again.",
            ))
            .child(
                Button::new("github-check-again", "Check again")
                    .on_click(cx.listener(|this, _, _window, cx| this.connect(cx))),
            )
            .into_any_element()
    }

    fn render_device_flow(&self, step: &DeviceFlowStep, cx: &mut Context<Self>) -> AnyElement {
        let cancel = Button::new("github-cancel-sign-in", "Cancel")
            .on_click(cx.listener(|this, _, _window, cx| this.cancel_sign_in(cx)));
        match step {
            DeviceFlowStep::RequestingCode => h_flex()
                .gap_2()
                .child(Label::new("Asking GitHub for a code…").color(Color::Muted))
                .child(cancel)
                .into_any_element(),
            DeviceFlowStep::WaitingForUser {
                user_code,
                verification_uri,
            } => {
                let uri = verification_uri.clone();
                v_flex()
                    .gap_2()
                    .child(Label::new(format!(
                        "Enter this code at {verification_uri}, then come back:"
                    )))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Headline::new(user_code.clone()).size(HeadlineSize::Medium))
                            .child(CopyButton::new("github-copy-code", user_code.clone())),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("github-open-verification", "Open GitHub")
                                    .style(ButtonStyle::Filled)
                                    .on_click(move |_, _window, cx| cx.open_url(&uri)),
                            )
                            .child(cancel),
                    )
                    .child(
                        Label::new("Waiting for you to approve on GitHub…")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_revoked(&self, source: TokenSource, cx: &mut Context<Self>) -> AnyElement {
        let can_sign_in = self.sign_in_options(cx).device_flow;
        let from_zode = source == TokenSource::Keychain;
        v_flex()
            .gap_2()
            .child(Label::new(revoked_advice(source)))
            .child(
                h_flex()
                    .gap_2()
                    .when(can_sign_in, |row| {
                        row.child(
                            Button::new("github-sign-in-again", "Sign in again")
                                .style(ButtonStyle::Filled)
                                .on_click(
                                    cx.listener(|this, _, _window, cx| {
                                        this.sign_in_with_github(cx)
                                    }),
                                ),
                        )
                    })
                    .when(from_zode, |row| {
                        row.child(
                            Button::new("github-use-cli", "Use the GitHub CLI instead").on_click(
                                cx.listener(|this, _, _window, cx| this.use_github_cli(cx)),
                            ),
                        )
                    })
                    .when(!from_zode, |row| {
                        row.child(
                            Button::new("github-check-again", "Check again")
                                .on_click(cx.listener(|this, _, _window, cx| this.connect(cx))),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(crate) fn render_error(&self, error: &GithubError, cx: &mut Context<Self>) -> AnyElement {
        let link = match error {
            GithubError::SsoRequired { authorize_url } if is_github_page(authorize_url) => {
                Some(("Authorize on GitHub", authorize_url.clone()))
            }
            GithubError::OrgRestricted { .. } => {
                let page = revoke_page(cx)
                    .filter(|_| self.signed_in_through_zode())
                    .unwrap_or_else(|| OAUTH_RESTRICTIONS_HELP.to_string());
                Some(("Request access", page))
            }
            _ => None,
        };
        let callout = Callout::new()
            .severity(Severity::Warning)
            .description(error.to_string());
        match link {
            Some((label, url)) => callout
                .actions_slot(
                    Button::new("github-error-link", label)
                        .on_click(move |_, _window, cx| cx.open_url(&url)),
                )
                .into_any_element(),
            None => callout.into_any_element(),
        }
    }

    fn signed_in_through_zode(&self) -> bool {
        matches!(&self.auth, AuthState::SignedIn(session) if session.token_source == TokenSource::Keychain)
    }

    fn render_sections(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let last = GithubSection::ALL.len() - 1;
        let selected = GithubSection::ALL
            .iter()
            .position(|section| *section == self.section);
        TabBar::new("github-sections").children(GithubSection::ALL.into_iter().enumerate().map(
            |(index, section)| {
                let position = if index == 0 {
                    TabPosition::First
                } else if index == last {
                    TabPosition::Last
                } else {
                    TabPosition::Middle(
                        selected
                            .map_or(std::cmp::Ordering::Greater, |selected| index.cmp(&selected)),
                    )
                };
                let active = section == self.section;
                Tab::new(("github-section", index))
                    .position(position)
                    .toggle_state(active)
                    .child(
                        Label::new(section.label())
                            .size(LabelSize::Small)
                            .color(if active { Color::OnBrand } else { Color::Muted }),
                    )
                    .on_click(
                        cx.listener(move |this, _, _window, cx| this.select_section(section, cx)),
                    )
            },
        ))
    }

    fn render_section_body(&self) -> AnyElement {
        if self.section == GithubSection::Issues
            && let Some(list) = &self.issue_list
        {
            return div()
                .flex_1()
                .min_h_0()
                .child(list.clone())
                .into_any_element();
        }
        if self.section == GithubSection::PullRequests
            && let Some(list) = &self.pull_request_list
        {
            return div()
                .flex_1()
                .min_h_0()
                .child(list.clone())
                .into_any_element();
        }
        if self.section == GithubSection::Projects
            && let Some(section) = &self.projects_section
        {
            return div()
                .flex_1()
                .min_h_0()
                .child(section.clone())
                .into_any_element();
        }
        let text = match &self.active_repository {
            Some(repository) => format!(
                "{} of {repository} are not shown yet.",
                self.section.label()
            ),
            None => "Pick a repository above.".to_string(),
        };
        div()
            .flex_1()
            .child(Label::new(text).color(Color::Muted))
            .into_any_element()
    }
}

impl Render for GithubView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // In the terminal's font, as every tool panel is, so a column of
        // names and statuses reads the same as the shell beside it.
        terminal_view::terminal_font_scope(cx)
            .size_full()
            .child(self.render_contents(window, cx))
    }
}
