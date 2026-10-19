use gpui::{App, Context, IntoElement, Render, SharedString, Window};
use release_channel::{AppCommitSha, AppVersion, ReleaseChannel};
use ui::{Button, Tooltip, prelude::*};
use workspace::{ItemHandle, StatusItemView};

/// The running version, at the right end of the status bar. Clicking it opens
/// the About window, which carries the full build string and a copy button.
pub struct AppVersionIndicator {
    label: SharedString,
    title: SharedString,
    commit: Option<SharedString>,
}

impl AppVersionIndicator {
    /// Everything is read once: the version cannot change while the process
    /// runs, so re-reading it every frame would only cost.
    pub fn new(cx: &App) -> Self {
        let version = AppVersion::display(cx);
        // `try_global`, not `global`: every workspace builds this item, and
        // `global` panics in any context that never ran `release_channel::init`.
        let channel = ReleaseChannel::try_global(cx).unwrap_or_default();
        let title = format!("{} {version}", channel.display_name()).into();
        let commit = AppCommitSha::try_global(cx)
            .map(|sha| sha.short())
            .filter(|sha| !sha.is_empty())
            .map(SharedString::from);
        Self {
            label: format!("v{version}").into(),
            title,
            commit,
        }
    }
}

impl Render for AppVersionIndicator {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title.clone();
        let commit = self.commit.clone();
        Button::new("app-version-indicator", self.label.clone())
            .label_size(LabelSize::Small)
            .color(Color::Muted)
            .tooltip(move |_window, cx| match &commit {
                Some(commit) => Tooltip::with_meta(
                    title.clone(),
                    Some(&zed_actions::About),
                    format!("Commit {commit}"),
                    cx,
                ),
                None => Tooltip::for_action(title.clone(), &zed_actions::About, cx),
            })
            .on_click(|_, window, cx| window.dispatch_action(Box::new(zed_actions::About), cx))
    }
}

impl StatusItemView for AppVersionIndicator {
    /// Nothing to follow: the version is the same whichever tab is in front.
    fn set_active_pane_item(
        &mut self,
        _active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn the_label_drops_build_metadata_and_the_tooltip_names_the_channel(cx: &mut TestAppContext) {
        cx.update(|cx| {
            release_channel::init_test(
                "0.1.5-beta.1+preview.7".parse().unwrap(),
                ReleaseChannel::Preview,
                cx,
            );
            AppCommitSha::set_global(AppCommitSha::new("abcdef0123456789".into()), cx);

            let indicator = AppVersionIndicator::new(cx);
            assert_eq!(indicator.label.as_ref(), "v0.1.5-beta.1");
            assert_eq!(indicator.title.as_ref(), "Zode Preview 0.1.5-beta.1");
            assert_eq!(indicator.commit, Some(SharedString::from("abcdef0")));
        });
    }

    /// The case `try_global` exists for: no release channel and no commit set,
    /// as in a bare test harness. It must build rather than panic.
    #[gpui::test]
    fn it_builds_with_no_release_globals(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let indicator = AppVersionIndicator::new(cx);
            assert_eq!(indicator.label.as_ref(), "v0.0.0");
            assert_eq!(indicator.commit, None);
        });
    }
}
