use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use editor::{
    Editor, MultiBufferOffset, SelectionEffects,
    actions::{Cancel, Undo},
};
use futures::{channel::oneshot, future::BoxFuture};
use gpui::{
    Context, Entity, Focusable as _, KeyBinding, TestAppContext, UpdateGlobal as _,
    VisualContext as _, VisualTestContext, Window,
};
use http_client::HttpClient;
use project::Project;
use settings::SettingsStore;
use translate::{TranslateError, TranslateRequest, TranslateService, TranslationProvider};
use workspace::{AppState, MultiWorkspace, Workspace};
use zode_actions::OpenSettingsAt;

use crate::{PopoverState, ReplaceBlocked, TranslateSelection, TranslationPopover};

/// A provider whose answers the test hands out one call at a time, so the
/// popover can be looked at while a request is still running.
#[derive(Default)]
pub(crate) struct GatedProvider {
    calls: Mutex<Vec<PendingCall>>,
}

struct PendingCall {
    request: TranslateRequest,
    reply: Option<oneshot::Sender<Result<String, TranslateError>>>,
}

impl GatedProvider {
    pub(crate) fn call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn with_call<R>(&self, index: usize, read: impl FnOnce(&mut PendingCall) -> R) -> R {
        let mut calls = self.calls.lock().unwrap_or_else(PoisonError::into_inner);
        let call_count = calls.len();
        let call = calls.get_mut(index).unwrap_or_else(|| {
            panic!(
                "expected provider call #{index}, but the provider was called {call_count} times"
            )
        });
        read(call)
    }

    pub(crate) fn request(&self, index: usize) -> TranslateRequest {
        self.with_call(index, |call| call.request.clone())
    }

    pub(crate) fn is_cancelled(&self, index: usize) -> bool {
        self.with_call(index, |call| {
            call.reply.as_ref().is_some_and(|reply| reply.is_canceled())
        })
    }

    pub(crate) fn answer(&self, index: usize, result: Result<String, TranslateError>) {
        let reply = self
            .with_call(index, |call| call.reply.take())
            .expect("each call is answered once");
        reply.send(result).expect("the request is still waiting");
    }
}

impl TranslationProvider for GatedProvider {
    fn name(&self) -> &'static str {
        "Gated"
    }

    fn translate(
        &self,
        request: &TranslateRequest,
        _http_client: Arc<dyn HttpClient>,
    ) -> BoxFuture<'static, Result<String, TranslateError>> {
        let (reply, answer) = oneshot::channel();
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(PendingCall {
                request: request.clone(),
                reply: Some(reply),
            });
        Box::pin(async move {
            answer.await.unwrap_or_else(|_| {
                Err(TranslateError::InvalidConfiguration(
                    "the test dropped the reply".into(),
                ))
            })
        })
    }
}

pub(crate) struct Harness {
    pub(crate) provider: Arc<GatedProvider>,
    pub(crate) workspace: Entity<Workspace>,
    pub(crate) editor: Entity<Editor>,
    pub(crate) cx: VisualTestContext,
}

impl Harness {
    pub(crate) async fn new(text: &str, cx: &mut TestAppContext) -> Self {
        let text = text.to_string();
        Self::with_editor(cx, move |_, window, cx| {
            let mut editor = Editor::multi_line(window, cx);
            editor.set_text(text, window, cx);
            editor
        })
        .await
    }

    /// Like [`Self::new`], with the editor built by `build_editor` and added
    /// to the workspace as its active, focused item.
    pub(crate) async fn with_editor(
        cx: &mut TestAppContext,
        build_editor: impl FnOnce(&Entity<Project>, &mut Window, &mut Context<Editor>) -> Editor,
    ) -> Self {
        let provider = Arc::new(GatedProvider::default());
        let app_state = cx.update(|cx| {
            let app_state = AppState::test(cx);
            editor::init(cx);
            workspace::init(app_state.clone(), cx);
            translate::init(cx);
            crate::init(cx);
            // Bound as in the default keymaps, so Escape would collapse the
            // selection if the popover let it through.
            cx.bind_keys([KeyBinding::new("escape", Cancel, Some("Editor"))]);
            cx.set_global(TranslateService::with_provider(provider.clone()));
            app_state
        });
        let project = Project::test(app_state.fs.clone(), [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
        let workspace =
            multi_workspace.read_with(cx, |multi_workspace, _| multi_workspace.workspace().clone());
        let editor = cx.new_window_entity(|window, cx| build_editor(&project, window, cx));
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.add_item_to_active_pane(Box::new(editor.clone()), None, true, window, cx);
            editor.update(cx, |editor, cx| window.focus(&editor.focus_handle(cx), cx));
        });
        cx.run_until_parked();
        Self {
            provider,
            workspace,
            editor,
            cx: cx.clone(),
        }
    }

    pub(crate) fn select(&mut self, range: std::ops::Range<usize>) {
        self.editor.update_in(&mut self.cx, |editor, window, cx| {
            editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                selections
                    .select_ranges([MultiBufferOffset(range.start)..MultiBufferOffset(range.end)])
            });
        });
        self.cx.run_until_parked();
    }

    pub(crate) fn translate(&mut self, from: &str, to: &str) {
        self.cx.dispatch_action(TranslateSelection {
            from: from.into(),
            to: to.into(),
        });
        self.cx.run_until_parked();
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

    pub(crate) fn state(&mut self) -> PopoverState {
        let popover = self.expect_popover();
        popover.read_with(&mut self.cx, |popover, _| popover.state().clone())
    }

    pub(crate) fn answer(&mut self, index: usize, result: Result<String, TranslateError>) {
        self.provider.answer(index, result);
        self.cx.run_until_parked();
    }

    pub(crate) fn replace_availability(&mut self) -> Option<Result<(), ReplaceBlocked>> {
        let popover = self.expect_popover();
        popover.read_with(&mut self.cx, |popover, cx| popover.replace_availability(cx))
    }

    pub(crate) fn click_replace(&mut self) {
        let popover = self.expect_popover();
        popover.update_in(&mut self.cx, |popover, window, cx| {
            popover.replace(window, cx)
        });
        self.cx.run_until_parked();
    }

    pub(crate) fn text(&mut self) -> String {
        self.editor
            .read_with(&mut self.cx, |editor, cx| editor.text(cx))
    }
}

#[gpui::test]
async fn the_popover_shows_loading_then_the_translation(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");

    assert_eq!(harness.state(), PopoverState::Loading);
    assert_eq!(harness.provider.call_count(), 1);
    assert_eq!(
        harness.provider.request(0),
        TranslateRequest {
            text: "Xin chào".into(),
            from: "vi".into(),
            to: "en".into(),
        }
    );

    harness.answer(0, Ok("Hello".into()));
    assert_eq!(harness.state(), PopoverState::Done("Hello".into()));
}

#[gpui::test]
async fn without_a_selection_nothing_is_shown_or_sent(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào   bạn", cx).await;
    harness.select(3..3);
    harness.translate("vi", "en");
    assert!(harness.popover().is_none());

    harness.select(9..12);
    harness.translate("vi", "en");
    assert!(harness.popover().is_none());
    assert_eq!(harness.provider.call_count(), 0);
}

#[gpui::test]
async fn a_selection_over_the_limit_warns_without_calling_the_provider(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.cx.update(|_, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.translate.get_or_insert_default().max_characters = Some(5);
            });
        });
    });
    harness.select(0..9);
    harness.translate("vi", "en");

    assert_eq!(
        harness.state(),
        PopoverState::TooLong {
            length: 8,
            limit: 5
        }
    );
    assert_eq!(harness.provider.call_count(), 0);
}

#[gpui::test]
async fn replace_writes_the_translation_and_one_undo_restores_it(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
    harness.answer(0, Ok("Hello".into()));
    assert_eq!(harness.replace_availability(), Some(Ok(())));

    harness.click_replace();
    assert_eq!(harness.text(), "Hello bạn");
    assert!(harness.popover().is_none(), "replacing closes the popover");

    harness.cx.dispatch_action(Undo);
    harness.cx.run_until_parked();
    assert_eq!(harness.text(), "Xin chào bạn");
}

#[gpui::test]
async fn text_edited_while_waiting_locks_replace(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");

    // Edited from outside this editor (another split, a collaborator, a
    // reload), which moves no local selection and so keeps the popover.
    let buffer = harness
        .editor
        .read_with(&mut harness.cx, |editor, _| editor.buffer().clone());
    harness.cx.update(|_, cx| {
        buffer.update(cx, |buffer, cx| {
            buffer.edit(
                [(MultiBufferOffset(0)..MultiBufferOffset(3), "Chào")],
                None,
                cx,
            )
        });
    });
    harness.cx.run_until_parked();
    harness.answer(0, Ok("Hello".into()));

    assert_eq!(
        harness.replace_availability(),
        Some(Err(ReplaceBlocked::TextChanged))
    );
    harness.click_replace();
    assert_eq!(harness.text(), "Chào chào bạn");
    assert!(
        harness.popover().is_some(),
        "a refused replace keeps the popover"
    );
}

#[gpui::test]
async fn a_read_only_editor_offers_no_replace(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness
        .editor
        .update(&mut harness.cx, |editor, _| editor.set_read_only(true));
    harness.select(0..9);
    harness.translate("vi", "en");
    harness.answer(0, Ok("Hello".into()));

    assert_eq!(harness.state(), PopoverState::Done("Hello".into()));
    assert_eq!(harness.replace_availability(), None);
    harness.click_replace();
    assert_eq!(harness.text(), "Xin chào bạn");
}

#[gpui::test]
async fn a_second_keypress_replaces_the_popover_and_cancels_the_first(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
    let first = harness.expect_popover().downgrade();

    harness.translate("en", "vi");
    assert_eq!(harness.provider.call_count(), 2);
    assert!(
        harness.provider.is_cancelled(0),
        "the first request is dropped"
    );
    assert!(first.upgrade().is_none(), "the first popover is released");

    let second = harness.expect_popover();
    let direction = second.read_with(&mut harness.cx, |popover, _| {
        let (from, to) = popover.direction();
        (from.to_string(), to.to_string())
    });
    assert_eq!(direction, ("en".to_string(), "vi".to_string()));
    harness.answer(1, Ok("Xin chào".into()));
    assert_eq!(harness.state(), PopoverState::Done("Xin chào".into()));
}

#[gpui::test]
async fn escape_closes_the_popover_and_keeps_the_selection(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
    assert!(harness.popover().is_some());

    harness.cx.simulate_keystrokes("escape");
    assert!(harness.popover().is_none());
    assert!(harness.provider.is_cancelled(0));
    let selected = harness.editor.update_in(&mut harness.cx, |editor, _, cx| {
        let snapshot = editor.display_snapshot(cx);
        editor
            .selections
            .newest::<MultiBufferOffset>(&snapshot)
            .range()
    });
    assert_eq!(selected, MultiBufferOffset(0)..MultiBufferOffset(9));
}

#[gpui::test]
async fn a_selection_change_closes_the_popover(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
    harness.answer(0, Ok("Hello".into()));
    assert!(harness.popover().is_some());

    harness.select(10..15);
    assert!(harness.popover().is_none());
}

#[gpui::test]
async fn scrolling_the_editor_closes_the_popover(cx: &mut TestAppContext) {
    let mut harness = Harness::new(&"Xin chào bạn\n".repeat(200), cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
    assert!(harness.popover().is_some());

    harness
        .editor
        .update_in(&mut harness.cx, |editor, window, cx| {
            editor.set_scroll_position(gpui::point(0., 20.), window, cx);
        });
    harness.cx.run_until_parked();
    assert!(harness.popover().is_none());
}

#[gpui::test]
async fn a_missing_key_or_a_forbidden_key_offers_the_settings(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
    let missing_key = TranslateError::MissingKey { provider: "DeepL" };
    harness.answer(0, Err(missing_key.clone()));
    assert_eq!(harness.state(), PopoverState::NotConfigured(missing_key));

    // An API that is not enabled, or billing that is off, is fixed in the
    // provider's console and the key settings, not by trying again.
    harness.translate("vi", "en");
    let forbidden = TranslateError::Forbidden {
        provider: "Google",
        status: 403,
        message: "Cloud Translation API has not been used in this project.".into(),
    };
    harness.answer(1, Err(forbidden.clone()));
    assert_eq!(harness.state(), PopoverState::NotConfigured(forbidden));

    harness.translate("vi", "en");
    let redirected = TranslateError::Redirected {
        provider: "Google",
        status: 307,
    };
    harness.answer(2, Err(redirected.clone()));
    assert_eq!(harness.state(), PopoverState::Failed(redirected));
}

#[gpui::test]
async fn swap_asks_again_in_the_other_direction(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
    harness.answer(0, Ok("Hello".into()));

    let popover = harness.expect_popover();
    popover.update(&mut harness.cx, |popover, cx| popover.swap_direction(cx));
    harness.cx.run_until_parked();
    assert_eq!(harness.state(), PopoverState::Loading);
    assert_eq!(harness.provider.request(1).from, "en");
    assert_eq!(harness.provider.request(1).to, "vi");
}

#[gpui::test]
async fn the_popover_sits_under_the_selection_head(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn\nsecond line", cx).await;
    let editor_bounds = harness
        .editor
        .read_with(&mut harness.cx, |editor, _| editor.last_bounds().copied())
        .expect("the editor was laid out");
    let position_for = |harness: &mut Harness, range| {
        harness.select(range);
        harness.translate("vi", "en");
        let popover = harness.expect_popover();
        popover.read_with(&mut harness.cx, |popover, _| popover.position())
    };

    let short = position_for(&mut harness, 0..3);
    let long = position_for(&mut harness, 0..9);
    let second_line = position_for(&mut harness, 16..22);
    let mouse = harness.cx.update(|window, _| window.mouse_position());

    for position in [short, long, second_line] {
        assert!(
            editor_bounds.contains(&position),
            "{position:?} lies inside the editor {editor_bounds:?}"
        );
        assert_ne!(position, mouse, "the head was placed, not the mouse");
    }
    assert!(
        short.y > editor_bounds.origin.y,
        "the popover sits below the first line, at {short:?}"
    );
    assert!(long.x > short.x, "it follows the head along the line");
    assert_eq!(long.y, short.y);
    assert!(second_line.y > long.y, "it follows the head down a line");
}

#[gpui::test]
async fn open_settings_shows_the_whole_translate_page(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    let opened_paths = Rc::new(RefCell::new(Vec::new()));
    harness.cx.update(|_, cx| {
        let opened_paths = opened_paths.clone();
        cx.on_action(move |action: &OpenSettingsAt, _| {
            opened_paths.borrow_mut().push(action.path.clone());
        });
    });
    harness.select(0..9);
    harness.translate("vi", "en");
    harness.answer(0, Err(TranslateError::MissingKey { provider: "DeepL" }));

    let popover = harness.expect_popover();
    popover.update_in(&mut harness.cx, |popover, window, cx| {
        popover.open_settings(window, cx)
    });
    harness.cx.run_until_parked();

    // The provider field alone would hide the API key and Shortcuts entries
    // that a missing key sends the user to.
    assert_eq!(*opened_paths.borrow(), vec!["translate".to_string()]);
    assert!(harness.popover().is_none());
}
