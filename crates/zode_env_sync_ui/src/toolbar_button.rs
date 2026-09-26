use editor::Editor;
use gpui::{Context, EventEmitter, Render, Window};
use ui::{ContextMenu, PopoverMenu, Tooltip, prelude::*};
use workspace::{ToolbarItemEvent, ToolbarItemLocation, ToolbarItemView, item::ItemHandle};

/// The Sync button that appears while an environment file is open, and only
/// then.
///
/// Whether a file counts is decided by `File::is_private`, which is driven by
/// the existing `private_files` setting — `**/.env*`, `**/*.pem`, `**/*.key`
/// and the rest, user-configurable. Asking the buffer rather than matching
/// globs here is what stops a second table drifting from the first.
pub struct EnvSyncToolbar {
    /// Set while the active item is an environment file. `None` hides the
    /// button entirely rather than showing one that does nothing.
    active: bool,
}

impl EnvSyncToolbar {
    pub fn new() -> Self {
        Self { active: false }
    }
}

impl Default for EnvSyncToolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitter<ToolbarItemEvent> for EnvSyncToolbar {}

/// Whether this pane item is a single environment file.
///
/// A multi-buffer has no one file to sync, so it is not offered — pushing
/// "the active file" from a search result would be a guess about which one.
fn is_env_file(item: &dyn ItemHandle, cx: &App) -> bool {
    let Some(editor) = item.downcast::<Editor>() else {
        return false;
    };
    let buffer = editor.read(cx).buffer().read(cx);
    let Some(buffer) = buffer.as_singleton() else {
        return false;
    };
    buffer.read(cx).file().is_some_and(|file| file.is_private())
}

impl ToolbarItemView for EnvSyncToolbar {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ToolbarItemLocation {
        self.active = active_pane_item.is_some_and(|item| is_env_file(item, cx));
        if self.active {
            ToolbarItemLocation::PrimaryRight
        } else {
            ToolbarItemLocation::Hidden
        }
    }
}

impl Render for EnvSyncToolbar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Rendered only where `set_active_pane_item` said PrimaryRight, but
        // guarded anyway: a stale frame between items must not draw a button
        // that acts on the wrong file.
        if !self.active {
            return div().into_any_element();
        }

        PopoverMenu::new("env-sync-toolbar-menu")
            .trigger(
                IconButton::new("env-sync-toolbar", IconName::ArrowCircle)
                    .icon_size(IconSize::Small)
                    .tooltip(|_window, cx| Tooltip::simple("Sync this environment file", cx)),
            )
            .menu(move |window, cx| {
                Some(ContextMenu::build(window, cx, move |menu, _window, _cx| {
                    // "Link This Checkout…" used to sit here as a step to do
                    // first. It is gone because sending now asks which project
                    // owns the file and records the answer, which is the same
                    // fact linking asked for. The action still exists for
                    // re-linking a checkout later.
                    menu.action(
                        "Send to Your Account\u{2026}",
                        Box::new(zed_actions::env_sync::PushEnvFile),
                    )
                    .action(
                        "Fetch From Your Account\u{2026}",
                        Box::new(zed_actions::env_sync::PullEnvFile),
                    )
                    .separator()
                    .action(
                        "Environment Files\u{2026}",
                        Box::new(zed_actions::env_sync::OpenEnvVault),
                    )
                }))
            })
            .into_any_element()
    }
}
