//! Single-line inputs: inside a workspace window but not a workspace item (a
//! modal's input), and in a window that has no workspace at all (the
//! Settings window).

use std::sync::{Arc, Mutex};

use editor::{Editor, MultiBufferOffset, SelectionEffects, actions::Undo};
use gpui::{
    AppContext as _, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    Render, TestAppContext, VisualTestContext,
};
use translate::TranslateService;
use ui::prelude::*;
use workspace::{AppState, ModalView};

use crate::{
    TranslateSelection, TranslationPopover, read_only_sources_tests::Shared,
    translate_ui_tests::GatedProvider,
};

const KEYSTROKE: &str = "alt-!";
const TEXT: &str = "Xin chào bạn";

struct InputModal {
    input: Entity<Editor>,
}

impl EventEmitter<DismissEvent> for InputModal {}
impl ModalView for InputModal {}

impl Focusable for InputModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for InputModal {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(400.)).child(self.input.clone())
    }
}

/// Every popover created, so the test finds it whichever host shows it.
fn track_popovers(cx: &mut App) -> Arc<Mutex<Vec<Entity<TranslationPopover>>>> {
    let created = Arc::new(Mutex::new(Vec::new()));
    cx.observe_new({
        let created = created.clone();
        move |_: &mut TranslationPopover, _, cx| {
            if let Ok(mut created) = created.lock() {
                created.push(cx.entity());
            }
        }
    })
    .detach();
    created
}

fn single_line_input(window: &mut Window, cx: &mut App) -> Entity<Editor> {
    cx.new(|cx| {
        let mut input = Editor::single_line(window, cx);
        input.set_text(TEXT, window, cx);
        input
    })
}

fn select_first_word(input: &Entity<Editor>, cx: &mut VisualTestContext) {
    select_prefix(input, "Xin".len(), cx);
}

fn select_prefix(input: &Entity<Editor>, length: usize, cx: &mut VisualTestContext) {
    input.update_in(cx, |input, window, cx| {
        window.focus(&input.focus_handle(cx), cx);
        input.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
            selections.select_ranges([MultiBufferOffset(0)..MultiBufferOffset(length)])
        });
    });
    cx.run_until_parked();
}

/// Presses the key, answers the request and clicks Replace; returns the
/// input's text afterwards.
fn translate_and_replace(
    input: &Entity<Editor>,
    provider: &GatedProvider,
    popovers: &Mutex<Vec<Entity<TranslationPopover>>>,
    cx: &mut VisualTestContext,
) -> String {
    translate_and_replace_as(input, "Xin", "Hello", provider, popovers, cx)
}

fn translate_and_replace_as(
    input: &Entity<Editor>,
    selected: &str,
    answer: &str,
    provider: &GatedProvider,
    popovers: &Mutex<Vec<Entity<TranslationPopover>>>,
    cx: &mut VisualTestContext,
) -> String {
    cx.simulate_keystrokes(KEYSTROKE);
    cx.run_until_parked();
    let popover = popovers
        .lock()
        .ok()
        .and_then(|popovers| popovers.last().cloned())
        .expect("a translation popover is shown");
    assert_eq!(provider.call_count(), 1);
    assert_eq!(provider.request(0).text, selected);
    provider.answer(0, Ok(answer.into()));
    cx.run_until_parked();
    assert_eq!(
        popover.read_with(cx, |popover, cx| popover.replace_availability(cx)),
        Some(Ok(())),
        "an input can be written back"
    );
    popover.update_in(cx, |popover, window, cx| popover.replace(window, cx));
    cx.run_until_parked();
    input.read_with(cx, |input, cx| input.text(cx))
}

#[gpui::test]
async fn a_modal_input_in_a_workspace_window_translates_and_replaces(cx: &mut TestAppContext) {
    let mut shared = Shared::new_with_contexts(&["Editor"], cx).await;
    let popovers = shared.cx.update(|_, cx| track_popovers(cx));
    let workspace = shared.workspace.clone();
    let input = workspace.update_in(&mut shared.cx, |workspace, window, cx| {
        let input = single_line_input(window, cx);
        workspace.toggle_modal(window, cx, {
            let input = input.clone();
            move |_, _| InputModal { input }
        });
        input
    });
    select_first_word(&input, &mut shared.cx);

    let text = translate_and_replace(&input, &shared.provider, &popovers, &mut shared.cx);

    assert_eq!(text, "Hello chào bạn");
    assert!(shared.popover().is_none(), "Replace closes the popover");
}

/// A commit subject or a rename field holds one line: a translation with line
/// breaks must not put a `\n` into it.
#[gpui::test]
async fn a_single_line_input_never_receives_a_line_break(cx: &mut TestAppContext) {
    for (answer, expected) in [
        ("Hello\nWorld", "Hello World chào bạn"),
        ("Hello\r\n\r\nWorld", "Hello World chào bạn"),
        ("Hello", "Hello chào bạn"),
    ] {
        let mut shared = Shared::new_with_contexts(&["Editor"], cx).await;
        let popovers = shared.cx.update(|_, cx| track_popovers(cx));
        let workspace = shared.workspace.clone();
        let input = workspace.update_in(&mut shared.cx, |workspace, window, cx| {
            let input = single_line_input(window, cx);
            workspace.toggle_modal(window, cx, {
                let input = input.clone();
                move |_, _| InputModal { input }
            });
            input
        });
        select_first_word(&input, &mut shared.cx);

        let text = translate_and_replace_as(
            &input,
            "Xin",
            answer,
            &shared.provider,
            &popovers,
            &mut shared.cx,
        );

        assert_eq!(text, expected, "{answer:?}");
    }
}

/// The provider trims its answer, so the selection's own edges are put back.
#[gpui::test]
async fn replacing_a_selection_keeps_its_leading_and_trailing_spaces(cx: &mut TestAppContext) {
    let mut shared = Shared::new_with_contexts(&["Editor"], cx).await;
    let popovers = shared.cx.update(|_, cx| track_popovers(cx));
    let workspace = shared.workspace.clone();
    let input = workspace.update_in(&mut shared.cx, |workspace, window, cx| {
        let input = single_line_input(window, cx);
        workspace.toggle_modal(window, cx, {
            let input = input.clone();
            move |_, _| InputModal { input }
        });
        input
    });
    select_prefix(&input, "Xin ".len(), &mut shared.cx);

    let text = translate_and_replace_as(
        &input,
        "Xin ",
        "Hello",
        &shared.provider,
        &popovers,
        &mut shared.cx,
    );

    assert_eq!(text, "Hello chào bạn");
}

/// The app as the Settings window sees it: initialized, but the window that
/// gets opened holds no workspace.
fn set_up_without_a_workspace(
    cx: &mut TestAppContext,
) -> (
    Arc<GatedProvider>,
    Arc<Mutex<Vec<Entity<TranslationPopover>>>>,
) {
    let provider = Arc::new(GatedProvider::default());
    let popovers = cx.update(|cx| {
        let app_state = AppState::test(cx);
        editor::init(cx);
        workspace::init(app_state, cx);
        translate::init(cx);
        crate::init(cx);
        cx.bind_keys([KeyBinding::new(
            KEYSTROKE,
            TranslateSelection {
                from: "vi".into(),
                to: "en".into(),
            },
            Some("Editor"),
        )]);
        cx.set_global(TranslateService::with_provider(provider.clone()));
        track_popovers(cx)
    });
    (provider, popovers)
}

#[gpui::test]
async fn an_input_in_a_window_without_a_workspace_translates_and_replaces(cx: &mut TestAppContext) {
    let (provider, popovers) = set_up_without_a_workspace(cx);
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut input = Editor::single_line(window, cx);
        input.set_text(TEXT, window, cx);
        input
    });
    select_first_word(&input, cx);

    let text = translate_and_replace(&input, &provider, &popovers, cx);

    assert_eq!(text, "Hello chào bạn");
    assert!(
        !input.read_with(cx, |input, _| input.has_mouse_context_menu()),
        "Replace closes the popover"
    );
    input.update_in(cx, |input, window, cx| {
        window.focus(&input.focus_handle(cx), cx);
    });
    cx.dispatch_action(Undo);
    assert_eq!(input.read_with(cx, |input, cx| input.text(cx)), TEXT);
}

#[gpui::test]
async fn escape_closes_the_popover_of_an_input_without_a_workspace(cx: &mut TestAppContext) {
    set_up_without_a_workspace(cx);
    let (input, cx) = cx.add_window_view(|window, cx| Editor::single_line(window, cx));
    input.update_in(cx, |input, window, cx| input.set_text(TEXT, window, cx));
    select_first_word(&input, cx);
    cx.simulate_keystrokes(KEYSTROKE);
    cx.run_until_parked();
    assert!(input.read_with(cx, |input, _| input.has_mouse_context_menu()));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    assert!(!input.read_with(cx, |input, _| input.has_mouse_context_menu()));
    assert_eq!(input.read_with(cx, |input, cx| input.text(cx)), TEXT);
}
