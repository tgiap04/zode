use std::sync::Arc;

use editor::Editor;
use gpui::{
    AppContext as _, Bounds, Context, Entity, Focusable as _, KeyBinding, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, UpdateGlobal as _,
    VisualTestContext, Window, point, px,
};
use markdown::Markdown;
use markdown_preview::markdown_preview_view::{MarkdownPreviewMode, MarkdownPreviewView};
use project::Project;
use settings::SettingsStore;
use terminal::{
    Terminal, TerminalBuilder,
    terminal_settings::{AlternateScroll, CursorShape},
};
use terminal_view::TerminalView;
use translate::TranslateService;
use util::paths::PathStyle;
use workspace::{AppState, MultiWorkspace, Workspace, item::ItemHandle};

use crate::{TranslateSelection, TranslationPopover, translate_ui_tests::GatedProvider};

const KEYSTROKE: &str = "alt-!";
/// What the terminal writes for `alt-!` when Option is Meta.
pub(crate) const KEYSTROKE_BYTES: &[u8] = b"\x1b!";

pub(crate) struct Shared {
    pub(crate) provider: Arc<GatedProvider>,
    pub(crate) project: Entity<Project>,
    pub(crate) workspace: Entity<Workspace>,
    pub(crate) cx: VisualTestContext,
}

impl Shared {
    async fn new(binding_context: &str, cx: &mut TestAppContext) -> Self {
        Self::new_with_contexts(&[binding_context], cx).await
    }

    pub(crate) async fn new_with_contexts(
        binding_contexts: &[&str],
        cx: &mut TestAppContext,
    ) -> Self {
        let provider = Arc::new(GatedProvider::default());
        let app_state = cx.update(|cx| {
            let app_state = AppState::test(cx);
            editor::init(cx);
            workspace::init(app_state.clone(), cx);
            translate::init(cx);
            crate::init(cx);
            cx.bind_keys(binding_contexts.iter().map(|binding_context| {
                KeyBinding::new(
                    KEYSTROKE,
                    TranslateSelection {
                        from: "vi".into(),
                        to: "en".into(),
                    },
                    Some(binding_context),
                )
            }));
            cx.set_global(TranslateService::with_provider(provider.clone()));
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings.terminal.get_or_insert_default().option_as_meta = Some(true);
                });
            });
            app_state
        });
        let project = Project::test(app_state.fs.clone(), [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
        let workspace =
            multi_workspace.read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone());
        Self {
            provider,
            project,
            workspace,
            cx: cx.clone(),
        }
    }

    pub(crate) fn popover(&mut self) -> Option<Entity<TranslationPopover>> {
        self.workspace.read_with(&mut self.cx, |workspace, cx| {
            workspace
                .popover_layer()
                .read(cx)
                .active_popover::<TranslationPopover>()
        })
    }

    pub(crate) fn expect_popover(&mut self) -> Entity<TranslationPopover> {
        self.popover().expect("a translation popover is shown")
    }

    pub(crate) fn press(&mut self) {
        self.cx.simulate_keystrokes(KEYSTROKE);
        self.cx.run_until_parked();
    }

    pub(crate) fn move_mouse(&mut self, position: Point<Pixels>) {
        self.cx
            .simulate_mouse_move(position, None, Modifiers::default());
        self.cx.run_until_parked();
    }

    pub(crate) fn assert_read_only_popover_for(&mut self, text: &str, bounds: Bounds<Pixels>) {
        let popover = self.expect_popover();
        let (position, replace) = popover.read_with(&mut self.cx, |popover, cx| {
            (popover.position(), popover.replace_availability(cx))
        });
        assert_eq!(self.provider.call_count(), 1);
        assert_eq!(self.provider.request(0).text, text);
        assert!(
            bounds.contains(&position),
            "{position:?} lies inside the source {bounds:?}"
        );
        self.provider.answer(0, Ok("Hello".into()));
        self.cx.run_until_parked();
        assert_eq!(
            popover.read_with(&mut self.cx, |popover, cx| popover.replace_availability(cx)),
            None,
            "read-only text offers no Replace"
        );
        assert_eq!(replace, None);
    }
}

/// Puts the terminal view into the workspace, as a tab of its own or inside
/// another tab that hosts it.
pub(crate) type TerminalHost =
    fn(Entity<TerminalView>, &mut Window, &mut Context<Workspace>) -> Box<dyn ItemHandle>;

pub(crate) struct TerminalHarness {
    pub(crate) shared: Shared,
    pub(crate) terminal: Entity<Terminal>,
}

impl TerminalHarness {
    pub(crate) async fn new(binding_context: &str, cx: &mut TestAppContext) -> Self {
        Self::new_in(binding_context, |view, _, _| Box::new(view), cx).await
    }

    pub(crate) async fn new_in(
        binding_context: &str,
        host: TerminalHost,
        cx: &mut TestAppContext,
    ) -> Self {
        let mut shared = Shared::new(binding_context, cx).await;
        let workspace = shared.workspace.clone();
        let project = shared.project.downgrade();
        let terminal = workspace.update_in(&mut shared.cx, |workspace, window, cx| {
            let terminal = cx.new(|cx| {
                TerminalBuilder::new_display_only(
                    CursorShape::default(),
                    AlternateScroll::On,
                    None,
                    0,
                    cx.background_executor(),
                    PathStyle::local(),
                )
                .expect("a display-only terminal needs no process")
                .subscribe(cx)
            });
            let terminal_view = cx.new(|cx| {
                TerminalView::new(
                    terminal.clone(),
                    workspace.weak_handle(),
                    None,
                    project,
                    window,
                    cx,
                )
            });
            let item = host(terminal_view.clone(), window, cx);
            workspace.add_item_to_active_pane(item, None, true, window, cx);
            window.focus(&terminal_view.focus_handle(cx), cx);
            terminal.update(cx, |terminal, cx| {
                terminal.write_output(b"Xin chao ban", cx)
            });
            terminal
        });
        let mut harness = Self { shared, terminal };
        harness.sync();
        harness
    }

    /// Applies queued terminal events, as painting the terminal would.
    pub(crate) fn sync(&mut self) {
        self.terminal
            .update_in(&mut self.shared.cx, |terminal, window, cx| {
                terminal.sync(window, cx)
            });
        self.shared.cx.run_until_parked();
    }

    pub(crate) fn select_all(&mut self) -> String {
        self.terminal
            .update(&mut self.shared.cx, |terminal, _| terminal.select_all());
        self.sync();
        self.terminal
            .read_with(&mut self.shared.cx, |terminal, _| {
                terminal.last_content.selection_text.clone()
            })
            .expect("select all selects the output")
    }

    pub(crate) fn bounds(&mut self) -> Bounds<Pixels> {
        self.terminal.read_with(&mut self.shared.cx, |terminal, _| {
            terminal.last_content.terminal_bounds.bounds
        })
    }

    pub(crate) fn take_input(&mut self) -> Vec<Vec<u8>> {
        self.terminal
            .update(&mut self.shared.cx, |terminal, _| terminal.take_input_log())
    }
}

#[gpui::test]
async fn a_terminal_selection_opens_the_popover_without_replace(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new("Terminal", cx).await;
    let selected = harness.select_all();
    assert!(selected.contains("Xin chao ban"), "selected {selected:?}");
    let bounds = harness.bounds();
    let inside = bounds.center();
    harness.shared.move_mouse(inside);
    harness.take_input();

    harness.shared.press();

    harness
        .shared
        .assert_read_only_popover_for(&selected, bounds);
    assert_eq!(
        harness
            .shared
            .expect_popover()
            .read_with(&mut harness.shared.cx, |popover, _| popover.position()),
        inside,
        "the popover opens at the mouse"
    );
    assert!(
        harness.take_input().is_empty(),
        "the keystroke went to the translation, not the shell"
    );
}

#[gpui::test]
async fn the_popover_is_kept_inside_the_terminal(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new("Terminal", cx).await;
    harness.select_all();
    let bounds = harness.bounds();
    harness.shared.move_mouse(point(px(0.), px(0.)));
    assert!(!bounds.contains(&point(px(0.), px(0.))));

    harness.shared.press();

    let position = harness
        .shared
        .expect_popover()
        .read_with(&mut harness.shared.cx, |popover, _| popover.position());
    assert_eq!(position, bounds.origin);
}

#[gpui::test]
async fn without_a_terminal_selection_the_keystroke_reaches_the_terminal(cx: &mut TestAppContext) {
    for binding_context in ["Terminal && selection", "Terminal"] {
        let mut harness = TerminalHarness::new(binding_context, cx).await;
        harness.take_input();

        harness.shared.press();

        assert!(harness.shared.popover().is_none(), "with {binding_context}");
        assert_eq!(harness.shared.provider.call_count(), 0);
        assert_eq!(
            harness.take_input(),
            vec![KEYSTROKE_BYTES.to_vec()],
            "vim or tmux gets the key when bound in {binding_context}"
        );
    }
}

#[gpui::test]
async fn a_terminal_selection_change_closes_the_popover(cx: &mut TestAppContext) {
    let mut harness = TerminalHarness::new("Terminal", cx).await;
    harness.select_all();
    harness.shared.press();
    assert!(harness.shared.popover().is_some());

    // Typing clears the terminal selection.
    harness
        .terminal
        .update(&mut harness.shared.cx, |terminal, _| {
            terminal.input(b"a".as_slice())
        });
    harness.sync();
    assert!(harness.shared.popover().is_none());
}

struct MarkdownHarness {
    shared: Shared,
    markdown: Entity<Markdown>,
}

impl MarkdownHarness {
    async fn new(text: &str, cx: &mut TestAppContext) -> Self {
        let mut shared = Shared::new("Markdown", cx).await;
        let created = Arc::new(std::sync::Mutex::new(Vec::new()));
        shared.cx.update(|_, cx| {
            let created = created.clone();
            cx.observe_new(move |_: &mut Markdown, _, cx| {
                if let Ok(mut created) = created.lock() {
                    created.push(cx.entity());
                }
            })
            .detach();
        });
        let workspace = shared.workspace.clone();
        let languages = shared
            .project
            .read_with(&mut shared.cx, |project, _| project.languages().clone());
        workspace.update_in(&mut shared.cx, |workspace, window, cx| {
            let editor = cx.new(|cx| {
                let mut editor = Editor::multi_line(window, cx);
                editor.set_text(text, window, cx);
                editor
            });
            let preview = MarkdownPreviewView::new(
                MarkdownPreviewMode::Default,
                editor,
                workspace.weak_handle(),
                languages,
                window,
                cx,
            );
            workspace.add_item_to_active_pane(Box::new(preview.clone()), None, true, window, cx);
            window.focus(&preview.focus_handle(cx), cx);
        });
        shared.cx.run_until_parked();
        let markdown = created
            .lock()
            .ok()
            .and_then(|created| created.first().cloned())
            .expect("the preview creates its markdown");
        Self { shared, markdown }
    }

    fn bounds(&mut self) -> Bounds<Pixels> {
        self.markdown
            .read_with(&mut self.shared.cx, |markdown, _| {
                markdown.last_visible_bounds()
            })
            .expect("the preview painted its markdown")
    }

    /// Triple-clicks the first line: the preview turns double-clicks into
    /// a jump to the source, but a triple-click selects the line.
    fn select_first_line(&mut self) -> String {
        let position = self.bounds().origin + point(px(4.), px(4.));
        for click_count in 1..=3 {
            self.shared.cx.simulate_event(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count,
                first_mouse: false,
            });
            self.shared.cx.simulate_event(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count,
            });
        }
        self.shared.cx.run_until_parked();
        self.markdown
            .read_with(&mut self.shared.cx, |markdown, _| {
                markdown.selected_rendered_text()
            })
            .expect("a triple-click selects the line")
    }
}

#[gpui::test]
async fn a_markdown_preview_selection_opens_the_popover(cx: &mut TestAppContext) {
    let mut harness = MarkdownHarness::new("Xin chào bạn", cx).await;
    let selected = harness.select_first_line();
    assert!(selected.contains("Xin chào bạn"), "selected {selected:?}");
    let bounds = harness.bounds();

    harness.shared.press();

    harness
        .shared
        .assert_read_only_popover_for(&selected, bounds);
}

#[gpui::test]
async fn a_cleared_markdown_selection_closes_the_popover(cx: &mut TestAppContext) {
    let mut harness = MarkdownHarness::new("Xin chào bạn", cx).await;
    harness.select_first_line();
    harness.shared.press();
    assert!(harness.shared.popover().is_some());

    harness
        .markdown
        .update(&mut harness.shared.cx, |markdown, cx| {
            markdown.reset("Tạm biệt".into(), cx)
        });
    harness.shared.cx.run_until_parked();
    assert!(harness.shared.popover().is_none());
}

#[gpui::test]
async fn markdown_sends_the_text_as_rendered_not_its_source(cx: &mut TestAppContext) {
    let mut harness = MarkdownHarness::new("**Xin** chào [bạn](https://example.com)", cx).await;
    harness.select_first_line();

    harness.shared.press();

    harness.shared.expect_popover();
    assert_eq!(harness.shared.provider.request(0).text, "Xin chào bạn");
}
