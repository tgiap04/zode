//! When the popover goes away on its own, and that it is driven through what
//! it renders.

use editor::{Editor, MultiBufferOffset};
use gpui::{
    DismissEvent, EventEmitter, FocusHandle, Focusable, KeyBinding, Modifiers, TestAppContext,
    actions,
};
use ui::prelude::*;
use workspace::{
    ModalView, SaveIntent,
    pane::{ActivateNextItem, CloseActiveItem},
};

use crate::translate_ui_tests::Harness;

actions!(translate_ui_test_modal, [CloseTestModal]);

struct TestModal {
    focus_handle: FocusHandle,
}

impl EventEmitter<DismissEvent> for TestModal {}
impl ModalView for TestModal {}

impl Focusable for TestModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TestModal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("TestModal")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &CloseTestModal, _, cx| cx.emit(DismissEvent)))
            .w(px(200.))
            .h(px(100.))
    }
}

fn close_active_item() -> CloseActiveItem {
    CloseActiveItem {
        save_intent: Some(SaveIntent::Skip),
        close_pinned: false,
    }
}

impl Harness {
    /// Adds a second tab to the active pane, keeping the first one active and
    /// focused.
    fn add_background_editor(&mut self, text: &str) {
        let text = text.to_string();
        let first = self.editor.clone();
        self.workspace
            .update_in(&mut self.cx, |workspace, window, cx| {
                let second = cx.new(|cx| {
                    let mut editor = Editor::multi_line(window, cx);
                    editor.set_text(text, window, cx);
                    editor
                });
                workspace.add_item_to_active_pane(Box::new(second), None, false, window, cx);
                workspace.activate_item(&first, true, true, window, cx);
            });
    }
}

#[gpui::test]
async fn a_released_source_closes_the_popover(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
    let popover = harness.expect_popover();
    let source = harness.cx.new(|_| ());
    popover.update(&mut harness.cx, |popover, cx| {
        popover.dismiss_on_release(&source, cx)
    });

    drop(popover);
    drop(source);
    // A dropped handle is released when effects are next flushed.
    harness.cx.update(|_, _| {});
    harness.cx.run_until_parked();

    assert!(harness.popover().is_none());
    assert!(harness.provider.is_cancelled(0));
}

#[gpui::test]
async fn closing_the_source_tab_closes_the_popover(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.add_background_editor("Tạm biệt");
    harness.select(0..9);
    harness.translate("vi", "en");
    assert!(harness.popover().is_some());

    harness.cx.dispatch_action(close_active_item());
    harness.cx.run_until_parked();

    assert!(harness.popover().is_none());
}

#[gpui::test]
async fn switching_to_another_tab_closes_the_popover(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.add_background_editor("Tạm biệt");
    harness.select(0..9);
    harness.translate("vi", "en");
    assert!(harness.popover().is_some());

    harness.cx.dispatch_action(ActivateNextItem::default());
    harness.cx.run_until_parked();

    assert!(harness.popover().is_none());
}

#[gpui::test]
async fn one_escape_reaches_a_modal_opened_over_the_popover(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.cx.update(|_, cx| {
        cx.bind_keys([KeyBinding::new("escape", CloseTestModal, Some("TestModal"))]);
    });
    harness.select(0..9);
    harness.translate("vi", "en");
    assert!(harness.popover().is_some());
    harness
        .workspace
        .update_in(&mut harness.cx, |workspace, window, cx| {
            workspace.toggle_modal(window, cx, |_, cx| TestModal {
                focus_handle: cx.focus_handle(),
            });
        });
    harness.cx.run_until_parked();

    harness.cx.simulate_keystrokes("escape");
    harness.cx.run_until_parked();

    let modal_open = harness
        .workspace
        .read_with(&mut harness.cx, |workspace, cx| {
            workspace.active_modal::<TestModal>(cx).is_some()
        });
    assert!(!modal_open, "the first Escape closed the modal");
}

#[gpui::test]
async fn clicking_the_rendered_replace_button_writes_the_translation(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
    harness.answer(0, Ok("Hello".into()));

    let replace = harness
        .cx
        .debug_bounds("translate-replace")
        .expect("the Replace button is rendered");
    harness
        .cx
        .simulate_click(replace.center(), Modifiers::none());
    harness.cx.run_until_parked();

    assert_eq!(harness.text(), "Hello bạn");
    assert!(harness.popover().is_none());
}

#[gpui::test]
async fn a_locked_replace_shows_why(cx: &mut TestAppContext) {
    let mut harness = Harness::new("Xin chào bạn", cx).await;
    harness.select(0..9);
    harness.translate("vi", "en");
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
    harness.answer(0, Ok("Hello".into()));

    assert!(
        harness
            .cx
            .debug_bounds("translate-replace-blocked-reason")
            .is_some(),
        "the reason is shown next to the locked button"
    );
    let replace = harness
        .cx
        .debug_bounds("translate-replace")
        .expect("the locked Replace button is still rendered");
    harness
        .cx
        .simulate_click(replace.center(), Modifiers::none());
    harness.cx.run_until_parked();
    assert_eq!(harness.text(), "Chào chào bạn");
}
