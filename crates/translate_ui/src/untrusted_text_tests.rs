//! Text the popover must not send, and text it must not write back as is:
//! masked inputs, huge or folded selections, selections across excerpts, and
//! invisible characters in a provider's answer.

use std::cell::Cell;
use std::rc::Rc;

use editor::{Editor, MultiBuffer, MultiBufferOffset};
use gpui::{Modifiers, TestAppContext, UpdateGlobal as _, point, px};
use settings::SettingsStore;
use text::Point;

use crate::{PopoverState, ReplaceBlocked, TranslateSelection, translate_ui_tests::Harness};

#[gpui::test]
async fn a_masked_input_is_never_translated(cx: &mut TestAppContext) {
    let mut harness = Harness::with_editor(cx, |_, window, cx| {
        let mut input = Editor::single_line(window, cx);
        input.set_text("hunter2 is my password", window, cx);
        input.set_masked(true, cx);
        input
    })
    .await;
    let reached_the_app = Rc::new(Cell::new(0));
    harness.cx.update(|_, cx| {
        let reached_the_app = reached_the_app.clone();
        cx.on_action(move |_: &TranslateSelection, _| {
            reached_the_app.set(reached_the_app.get() + 1)
        });
    });
    harness.select(0..7);

    harness.translate("vi", "en");

    assert!(harness.popover().is_none(), "a password is never shown");
    assert_eq!(harness.provider.call_count(), 0, "nor sent");
    assert_eq!(
        reached_the_app.get(),
        1,
        "the key is passed on to other handlers"
    );
}

#[gpui::test]
async fn a_huge_selection_is_refused_before_its_text_is_copied(cx: &mut TestAppContext) {
    let text = "Xin chào bạn\n".repeat(50_000);
    let mut harness = Harness::new(&text, cx).await;
    harness.cx.update(|_, cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.translate.get_or_insert_default().max_characters = Some(2000);
            });
        });
    });
    harness.select(0..text.len());

    harness.translate("vi", "en");

    assert_eq!(
        harness.state(),
        PopoverState::TooLong {
            length: text.chars().count(),
            limit: 2000,
        }
    );
    assert_eq!(harness.provider.call_count(), 0);
    let popover = harness.expect_popover();
    let held = popover.read_with(&mut harness.cx, |popover, _| popover.source_text().len());
    assert_eq!(held, 0, "the selection was never copied into the popover");
}

#[gpui::test]
async fn a_selection_over_a_fold_is_not_sent(cx: &mut TestAppContext) {
    let text = "Xin chào bạn\nAPI_KEY=hidden\nTạm biệt";
    let mut harness = Harness::new(text, cx).await;
    let fold_start = text.find("API_KEY").unwrap_or_default();
    let fold_end = fold_start + "API_KEY=hidden".len();
    harness
        .editor
        .update_in(&mut harness.cx, |editor, window, cx| {
            editor.fold_ranges(
                vec![MultiBufferOffset(fold_start)..MultiBufferOffset(fold_end)],
                false,
                window,
                cx,
            )
        });
    harness.select(0..text.len());

    harness.translate("vi", "en");

    assert!(harness.popover().is_none());
    assert_eq!(
        harness.provider.call_count(),
        0,
        "folded text is not on screen, so it is not sent"
    );
}

#[gpui::test]
async fn replace_is_refused_across_excerpts(cx: &mut TestAppContext) {
    let mut harness = Harness::with_editor(cx, |project, window, cx| {
        let multibuffer = MultiBuffer::build_multi(
            [
                ("Xin chào\nmột\n", vec![Point::new(0, 0)..Point::new(1, 0)]),
                ("bạn hiền\nhai\n", vec![Point::new(0, 0)..Point::new(1, 0)]),
            ],
            cx,
        );
        Editor::for_multibuffer(multibuffer, Some(project.clone()), window, cx)
    })
    .await;
    let visible_text = harness.text();
    assert!(visible_text.contains("Xin chào") && visible_text.contains("bạn hiền"));

    harness.select(0..visible_text.len());
    harness.translate("vi", "en");
    assert_eq!(
        harness.provider.request(0).text,
        visible_text,
        "the text on screen is what gets sent"
    );
    harness.answer(0, Ok("Hello\ndear friend\n".into()));
    assert_eq!(
        harness.replace_availability(),
        Some(Err(ReplaceBlocked::SpansExcerpts))
    );
    harness.click_replace();
    assert_eq!(harness.text(), visible_text, "no excerpt was touched");

    harness.select(0.."Xin".len());
    harness.translate("vi", "en");
    harness.answer(1, Ok("Hello".into()));
    assert_eq!(
        harness.replace_availability(),
        Some(Ok(())),
        "a selection inside one excerpt can still be replaced"
    );
}

#[gpui::test]
async fn hidden_characters_in_the_answer_are_removed_before_display_and_replace(
    cx: &mut TestAppContext,
) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");

    harness.answer(
        0,
        Ok("He\u{202E}llo\u{200B}\u{1b}[31m!\u{1b}[0m\u{2066}\tA\r\nB\u{0}\u{FEFF}".into()),
    );

    assert_eq!(harness.state(), PopoverState::Done("Hello!\tA\nB".into()));
    assert!(
        harness
            .cx
            .debug_bounds("translate-hidden-characters-removed")
            .is_some(),
        "the popover says that characters were removed"
    );
    harness.click_replace();
    assert_eq!(harness.text(), "Hello!\tA\nB bạn");
}

#[gpui::test]
async fn an_off_screen_selection_head_keeps_the_popover_inside_the_editor(cx: &mut TestAppContext) {
    let mut harness = Harness::new(&"Xin chào bạn\n".repeat(200), cx).await;
    harness.select(0..9);
    harness
        .editor
        .update_in(&mut harness.cx, |editor, window, cx| {
            editor.set_scroll_position(point(0., 100.), window, cx);
        });
    harness.cx.run_until_parked();
    let editor_bounds = harness
        .editor
        .read_with(&mut harness.cx, |editor, _| editor.last_bounds().copied())
        .expect("the editor was laid out");
    let outside = point(px(1.), px(1.));
    assert!(!editor_bounds.contains(&outside));
    harness
        .cx
        .simulate_mouse_move(outside, None, Modifiers::default());
    harness.cx.run_until_parked();

    harness.translate("vi", "en");

    let position = harness
        .expect_popover()
        .read_with(&mut harness.cx, |popover, _| popover.position());
    assert_eq!(position, editor_bounds.origin);
}
