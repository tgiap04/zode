use devices::Device;
use gpui::{AnyElement, App, Context};
use project::trusted_worktrees::TrustedWorktrees;
use ui::{Tooltip, prelude::*};

use crate::devices_panel::DevicesPanel;

const RUN_TOOLTIP: &str = "Run the app on this device in a terminal. Stopping the task stops \
                           the command line tool; the app may keep running on the device.";

impl DevicesPanel {
    /// Why Run is off for some folders. Only queries the trust store: asking
    /// it to check a folder could raise the trust prompt on its own.
    pub(crate) fn run_note(&self, cx: &App) -> Option<&'static str> {
        let project = self.project.upgrade()?;
        let project = project.read(cx);
        if !project.is_local() {
            return None;
        }
        let Some(store) = TrustedWorktrees::try_get_global(cx) else {
            return Some("Run app is off: folder trust cannot be checked.");
        };
        store
            .read(cx)
            .has_restricted_worktrees(&project.worktree_store(), cx)
            .then_some("Run app is off for folders you have not trusted. Trust them, then Reload.")
    }

    pub(crate) fn render_run_note(&self, cx: &App) -> Option<AnyElement> {
        let note = self.run_note(cx)?;
        Some(
            div()
                .id("devices-run-note")
                .debug_selector(|| "devices-run-note".to_string())
                .px_3()
                .pb_1()
                .child(Label::new(note).size(LabelSize::Small).color(Color::Muted))
                .into_any_element(),
        )
    }

    pub(crate) fn render_run_button(
        &self,
        device: &Device,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.offers_run(device, cx) {
            return None;
        }
        let key = device.key.clone();
        let element_id = format!("{id}-run");
        let starting = self.is_run_starting(&key) || self.operations.contains_key(&key);
        Some(
            div()
                .debug_selector({
                    let element_id = element_id.clone();
                    move || element_id
                })
                .child(
                    IconButton::new(SharedString::from(element_id), IconName::PlayFilled)
                        .icon_size(IconSize::Small)
                        .disabled(starting)
                        .tooltip(Tooltip::text(RUN_TOOLTIP))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_clicked(&key, window, cx)
                        })),
                )
                .into_any_element(),
        )
    }

    /// Asked on every click; the choice is not remembered.
    pub(crate) fn render_app_choice(
        &self,
        device: &Device,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let choice = self
            .app_choice
            .as_ref()
            .filter(|choice| choice.device == device.key)?;
        let mut list = v_flex()
            .id(SharedString::from(format!("{id}-apps")))
            .debug_selector({
                let id = id.to_string();
                move || format!("{id}-apps")
            })
            .w_full()
            .pl_6()
            .gap_0p5()
            .child(Label::new("Run which app?").size(LabelSize::Small));
        for (index, choice) in choice.choices.iter().enumerate() {
            let app = &choice.app;
            let label = match &choice.scheme {
                Some(scheme) => format!("{} · {scheme}", app.display_name),
                None => format!("{} · {}", app.display_name, app.framework.display_name()),
            };
            list = list.child(
                Button::new(SharedString::from(format!("{id}-app-{index}")), label)
                    .label_size(LabelSize::Small)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.choose_app(index, window, cx)),
                    ),
            );
        }
        Some(
            list.child(
                Button::new(SharedString::from(format!("{id}-apps-cancel")), "Cancel")
                    .label_size(LabelSize::Small)
                    .on_click(cx.listener(|this, _, _window, cx| this.cancel_app_choice(cx))),
            )
            .into_any_element(),
        )
    }
}
