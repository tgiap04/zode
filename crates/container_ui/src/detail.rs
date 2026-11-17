//! One container, opened: its logs following live, and the buttons that change
//! what it is doing.
//!
//! The logs are a real Zode terminal, not a text buffer this crate fills. That
//! is the same call `terminal.rs` made and for the same reason -- colour,
//! scrollback, selection, search and Ctrl-C are the terminal's, and a
//! reimplementation would be four of those missing. The difference is only where
//! it is put: `terminal.rs` opens a tab beside the code, this puts one inside
//! the container view where the row was clicked.
//!
//! Replaces the list rather than splitting beside it: a log line is long, the
//! table is wide, and a pane holding both gives each half of a width neither can
//! spare. A back button returns.

use container::resource::row_key;
use container::{Resource, ResourceKind};
use gpui::{Entity, WeakEntity, Window};
use terminal_view::TerminalView;
use ui::{Tab, prelude::*};
use workspace::Workspace;

use crate::container_panel::ContainerPanel;
use crate::detail_history::HistoryState;
use crate::detail_inspect::InspectState;
use crate::detail_stats::StatsState;
use crate::detail_tabs::DetailTab;

/// The container being looked at, and the terminal following its logs.
pub(crate) struct Detail {
    /// Which tab is showing.
    pub(crate) tab: DetailTab,
    /// What the Inspect tab has loaded and what is revealed. Owned by the open
    /// row, so closing it cancels the request and forgets every reveal.
    pub(crate) inspect: InspectState,
    /// What the Stats tab has sampled, and the polling loop itself.
    pub(crate) stats: StatsState,
    /// The layers of an image, once asked for.
    pub(crate) history: HistoryState,
    /// The row as it was when it was opened, refreshed from each list that
    /// arrives.
    ///
    /// Held by value rather than by id: the header shows the name and state, and
    /// looking both up in `state` on every frame would put a linear scan in the
    /// render path for a row that is already in hand.
    pub(crate) resource: Resource,
    /// `None` until the terminal is built, and after a failure to build one.
    pub(crate) logs: Option<Entity<TerminalView>>,
    /// Why there is no terminal, when there is none.
    ///
    /// Shown rather than logged. A blank panel where logs were asked for is the
    /// failure mode this field exists to prevent.
    pub(crate) trouble: Option<String>,
    /// The git checkout a compose container was built from, once read.
    ///
    /// `None` for anything without a compose working directory, and for one
    /// whose directory is not a checkout: neither gets a Switch Branch button.
    pub(crate) checkout: Option<crate::compose_checkout::Checkout>,
}

impl ContainerPanel {
    /// Opens one resource on its first tab.
    pub(crate) fn open_detail(
        &mut self,
        resource: Resource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_detail_on(resource, None, window, cx);
    }

    /// Opens one resource on `tab` when it has it, else on its first.
    pub(crate) fn open_detail_on(
        &mut self,
        resource: Resource,
        tab: Option<DetailTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Re-opening the row already open would tear down a terminal only to
        // build the same one, losing whatever scrollback it had collected.
        if self
            .detail
            .as_ref()
            .is_some_and(|detail| row_key(&detail.resource) == row_key(&resource))
        {
            if let Some(tab) = tab {
                self.select_detail_tab(tab, window, cx);
            }
            return;
        }

        let tabs = self.detail_tabs();
        let Some(first) = tab
            .filter(|tab| tabs.contains(tab))
            .or_else(|| tabs.first().copied())
        else {
            return;
        };

        // A terminal still being built for the row that was open belongs to it.
        self.logs_build = None;
        self.detail = Some(Detail {
            tab: first,
            inspect: InspectState::default(),
            stats: StatsState::default(),
            history: HistoryState::default(),
            resource,
            logs: None,
            trouble: None,
            checkout: None,
        });
        self.probe_checkout(cx);
        self.enter_detail_tab(first, window, cx);
        cx.notify();
    }

    /// Starts following the open row's logs, once.
    ///
    /// Not again when a terminal is already there, still being built, or has
    /// failed to build: switching back to the tab must not replace a terminal
    /// that has scrollback, nor retry a failure every time it is looked at.
    pub(crate) fn start_logs_if_needed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.active_kind;
        let Some(detail) = self.detail.as_ref() else {
            return;
        };
        if detail.logs.is_some() || detail.trouble.is_some() || self.logs_build.is_some() {
            return;
        }
        let resource = detail.resource.clone();
        let command = self
            .backend()
            .and_then(|backend| backend.logs_command(kind, &resource.id));

        let Some((program, args)) = command else {
            self.report_no_logs("this engine has no logs command for it", cx);
            return;
        };
        // A floating window has no workspace, so it has no project to build a
        // terminal from. Says so rather than showing an empty box.
        let Some(workspace) = self.workspace.clone() else {
            self.report_no_logs("logs need a workspace -- open the containers in a tab", cx);
            return;
        };

        self.start_logs(workspace, program, args, &resource, kind, window, cx);
    }

    fn report_no_logs(&mut self, reason: &str, cx: &mut Context<Self>) {
        if let Some(detail) = self.detail.as_mut() {
            detail.trouble = Some(reason.to_string());
        }
        cx.notify();
    }

    /// Builds the terminal that runs the engine's own `logs -f`.
    ///
    /// The command is the backend's, never assembled here: `docker logs -f x`
    /// and `kubectl logs -f -n ns x` are one intention in two vocabularies, and
    /// a view that knew either would end up knowing both.
    fn start_logs(
        &mut self,
        workspace: WeakEntity<Workspace>,
        program: String,
        args: Vec<String>,
        resource: &Resource,
        kind: ResourceKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spawn = crate::terminal::logs_task(&program, args, &resource.name, kind);
        let opened_for = row_key(resource);

        let project = match workspace.read_with(cx, |workspace, _| workspace.project().clone()) {
            Ok(project) => project,
            Err(error) => {
                self.report_no_logs(&format!("the workspace has gone: {error}"), cx);
                return;
            }
        };

        let built = project.update(cx, |project, cx| project.create_terminal_task(spawn, cx));

        // Held in a field, not detached: dropping the panel must stop the
        // terminal being built for it, and `logs -f` is a child process.
        self.logs_build = Some(cx.spawn_in(window, async move |this, cx| {
            let built = built.await;
            let outcome = this.update_in(cx, |this, window, cx| {
                // The row may have been closed or another opened while the
                // terminal was being built. Anything that arrives for a row
                // nobody is looking at is dropped rather than shown under the
                // wrong name.
                if this
                    .detail
                    .as_ref()
                    .is_none_or(|detail| row_key(&detail.resource) != opened_for)
                {
                    return;
                }
                match built {
                    Ok(terminal) => {
                        let view = cx.new(|cx| {
                            TerminalView::new(
                                terminal,
                                workspace.clone(),
                                None,
                                project.downgrade(),
                                window,
                                cx,
                            )
                        });
                        if let Some(detail) = this.detail.as_mut() {
                            detail.logs = Some(view);
                        }
                    }
                    Err(error) => this.report_no_logs(&format!("{error}"), cx),
                }
                cx.notify();
            });
            if let Err(error) = outcome {
                log::error!("could not attach a container log terminal: {error}");
            }
        }));
    }

    /// Goes back to the list.
    ///
    /// Drops the terminal, which stops `logs -f`. Keeping it alive behind the
    /// list would leave a child process per row anybody had ever clicked.
    pub(crate) fn close_detail(&mut self, cx: &mut Context<Self>) {
        self.detail = None;
        self.logs_build = None;
        self.checkout_probe = None;
        cx.notify();
    }

    /// Brings the open row up to date with a list that has just arrived.
    ///
    /// The header shows a state, and a header still saying "Running" after the
    /// Stop button worked is the specific lie this exists to prevent. A row that
    /// has left the list entirely closes the detail -- there is nothing left to
    /// show and its logs are following something that is gone.
    pub(crate) fn refresh_detail(&mut self, resources: &[Resource]) {
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        let open = row_key(&detail.resource);
        match resources.iter().find(|resource| row_key(resource) == open) {
            Some(fresh) => detail.resource = fresh.clone(),
            None => {
                self.detail = None;
                self.logs_build = None;
            }
        }
    }
}

impl ContainerPanel {
    /// The open row: what it is and what can be done to it, over its tabs.
    pub(crate) fn render_detail(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(detail) = self.detail.as_ref() else {
            return div().into_any_element();
        };
        let resource = detail.resource.clone();
        let tab = detail.tab;

        let body = match tab {
            DetailTab::Logs => self.render_logs_body(),
            DetailTab::Inspect => self.render_inspect(window, cx),
            DetailTab::Stats => self.render_stats(cx),
            DetailTab::History => self.render_history(cx),
        };

        v_flex()
            .size_full()
            .debug_selector(|| "container-detail".into())
            .child(self.render_detail_header(&resource, cx))
            .children(self.render_checkout_row(cx))
            .children(self.render_detail_tabs(tab, cx))
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .into_any_element()
    }

    fn render_logs_body(&self) -> gpui::AnyElement {
        let Some(detail) = self.detail.as_ref() else {
            return div().into_any_element();
        };
        match (detail.logs.clone(), detail.trouble.clone()) {
            // The terminal owns everything below the header: its own
            // scrollback, selection and search, none of which this
            // crate reimplements.
            (Some(view), _) => div().size_full().p_1().child(view).into_any_element(),
            (None, Some(trouble)) => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_1()
                .p_4()
                .child(
                    Icon::new(IconName::Warning)
                        .size(IconSize::XLarge)
                        .color(Color::Muted),
                )
                .child(Label::new("No logs here").color(Color::Default))
                .child(
                    Label::new(trouble)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .into_any_element(),
            (None, None) => crate::render::note(None, "Attaching\u{2026}"),
        }
    }

    /// Back, what this is, and the same buttons the row had.
    ///
    /// The buttons come from `render_trailing`, the list's own: two sets would be
    /// two things to keep in step, and the first to drift would be the one used
    /// less.
    fn render_detail_header(
        &self,
        resource: &Resource,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let handle = cx.entity().downgrade();
        let subtitle = resource
            .detail
            .iter()
            .find(|(label, _)| *label == "Image")
            .map(|(_, value)| value.clone());

        h_flex()
            .debug_selector(|| "container-detail-header".into())
            .flex_none()
            .h(Tab::container_height(cx))
            .px_2()
            .gap_1p5()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().colors().border)
            .child(
                h_flex()
                    .gap_1p5()
                    .min_w_0()
                    .child(
                        IconButton::new("container-detail-back", IconName::ArrowLeft)
                            .icon_size(IconSize::Small)
                            .tooltip(|_window, cx| ui::Tooltip::simple("Back to the list", cx))
                            .on_click(cx.listener(|this, _, _window, cx| this.close_detail(cx))),
                    )
                    .child(crate::render::state_dot(resource.state))
                    .child(crate::render::truncated_text(
                        "container-detail-name",
                        resource.name.clone(),
                        Color::Default,
                    ))
                    .when_some(subtitle, |element, subtitle| {
                        element.child(
                            Label::new(subtitle)
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        )
                    }),
            )
            .child(if resource.kind == ResourceKind::ComposeProject {
                crate::compose_render::render_compose_trailing(
                    resource,
                    &self.compose_context(resource, None),
                    self.busy_for(&row_key(resource)),
                    false,
                    handle,
                )
            } else {
                crate::render::render_trailing(
                    resource,
                    self.available_actions(),
                    self.busy_for(&row_key(resource)),
                    self.terminal_available(crate::terminal::TerminalIntent::FollowLogs),
                    self.terminal_available(crate::terminal::TerminalIntent::Shell),
                    self.removable(),
                    handle,
                )
            })
            .into_any_element()
    }
}
