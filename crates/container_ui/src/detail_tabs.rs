//! The tabs of an open row: Logs, Inspect, Stats, and an image's History.
//!
//! Which tabs a kind has is the backend's answer, never this crate's guess: a
//! tab for something the engine cannot say would open onto an error, and an
//! image has never run, so Logs is not offered for it while Inspect is.

use container::{ContainerBackend, Op, ResourceKind};
use gpui::{AnyElement, Window};
use ui::{Tab, TabBar, TabPosition, prelude::*};

use crate::container_panel::ContainerPanel;

/// One tab of the open row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum DetailTab {
    Logs,
    Inspect,
    Stats,
    History,
}

impl DetailTab {
    pub(crate) fn label(self) -> &'static str {
        match self {
            DetailTab::Logs => "Logs",
            DetailTab::Inspect => "Inspect",
            DetailTab::Stats => "Stats",
            DetailTab::History => "History",
        }
    }

    fn icon(self) -> IconName {
        match self {
            DetailTab::Logs => IconName::Terminal,
            DetailTab::Inspect => IconName::Info,
            DetailTab::Stats => IconName::Server,
            DetailTab::History => IconName::Clock,
        }
    }
}

/// The tabs `backend` offers for `kind`, in the order they are drawn.
///
/// Stats is asked of the kinds that run something. The backend's
/// `supports_stats` is one answer for the whole engine, so the kinds that have
/// nothing running (an image, a volume) are left out here rather than drawn as
/// a tab that could only ever say "no metrics".
pub(crate) fn tabs_for(backend: &dyn ContainerBackend, kind: ResourceKind) -> Vec<DetailTab> {
    let mut tabs = Vec::new();
    if backend.logs_command(kind, "probe").is_some() {
        tabs.push(DetailTab::Logs);
    }
    if backend.supports_inspect(kind) {
        tabs.push(DetailTab::Inspect);
    }
    if backend.supports_stats()
        && matches!(
            kind,
            ResourceKind::Container | ResourceKind::Pod | ResourceKind::ComposeProject
        )
    {
        tabs.push(DetailTab::Stats);
    }
    if kind == ResourceKind::Image && backend.offers(Op::ImageHistory) {
        tabs.push(DetailTab::History);
    }
    tabs
}

impl ContainerPanel {
    /// The tabs a row of the kind on screen opens onto.
    pub(crate) fn detail_tabs(&self) -> Vec<DetailTab> {
        self.backend()
            .map(|backend| tabs_for(backend.as_ref(), self.active_kind))
            .unwrap_or_default()
    }

    /// Whether a row of this kind opens onto anything: any tab at all.
    pub(crate) fn rows_open_a_detail(&self) -> bool {
        !self.detail_tabs().is_empty()
    }

    /// Shows `tab` of the open row, starting whatever it needs.
    ///
    /// Leaving Stats stops the polling; leaving Logs keeps the terminal, so
    /// coming back finds its scrollback instead of a fresh `logs -f`.
    pub(crate) fn select_detail_tab(
        &mut self,
        tab: DetailTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.detail_tabs().contains(&tab) {
            return;
        }
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        if detail.tab == tab {
            return;
        }
        detail.tab = tab;
        if tab != DetailTab::Stats {
            detail.stats.stop_polling();
        }
        self.enter_detail_tab(tab, window, cx);
        cx.notify();
    }

    /// Starts what `tab` shows if it is not already running or loaded.
    pub(crate) fn enter_detail_tab(
        &mut self,
        tab: DetailTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match tab {
            DetailTab::Logs => self.start_logs_if_needed(window, cx),
            DetailTab::Inspect => {
                let loaded = self
                    .detail
                    .as_ref()
                    .is_some_and(|detail| detail.inspect.has_started());
                if !loaded {
                    self.load_inspect(window, cx);
                }
            }
            DetailTab::Stats => self.ensure_stats_polling(cx),
            DetailTab::History => {
                let loaded = self
                    .detail
                    .as_ref()
                    .is_some_and(|detail| detail.history.has_started());
                if !loaded {
                    self.load_history(window, cx);
                }
            }
        }
    }

    /// The row of tabs, or `None` when there is only one -- a bar holding a
    /// single tab says nothing the header does not.
    pub(crate) fn render_detail_tabs(
        &self,
        active: DetailTab,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let tabs = self.detail_tabs();
        if tabs.len() < 2 {
            return None;
        }
        let last = tabs.len() - 1;
        let selected = tabs.iter().position(|tab| *tab == active);
        let bar = TabBar::new("container-detail-tabs").children(
            tabs.iter().copied().enumerate().map(|(index, tab)| {
                let is_active = tab == active;
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
                let colour = if is_active {
                    Color::OnBrand
                } else {
                    Color::Muted
                };
                Tab::new(("container-detail-tab", tab as usize))
                    .position(position)
                    .toggle_state(is_active)
                    .start_slot(Icon::new(tab.icon()).size(IconSize::XSmall).color(colour))
                    .child(Label::new(tab.label()).size(LabelSize::Small).color(colour))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_detail_tab(tab, window, cx)
                    }))
            }),
        );
        Some(bar.into_any_element())
    }
}
