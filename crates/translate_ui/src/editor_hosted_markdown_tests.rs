//! `Markdown` drawn inside an `Editor`, as diagnostic blocks and hover
//! popovers are: both the `Editor` and the `Markdown` bindings match, and the
//! editor's handler runs first.

use std::sync::Arc;

use editor::{
    Editor, MultiBufferOffset, SelectionEffects,
    display_map::{BlockPlacement, BlockProperties, BlockStyle},
};
use gpui::{
    AppContext as _, Entity, Focusable as _, IntoElement as _, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, ParentElement as _, Styled as _, TestAppContext,
    VisualContext as _, div, point, px,
};
use markdown::{Markdown, MarkdownElement, MarkdownStyle};

use crate::read_only_sources_tests::Shared;

const EDITOR_TEXT: &str = "Chào buổi sáng\nhai";

struct Harness {
    shared: Shared,
    editor: Entity<Editor>,
    markdown: Entity<Markdown>,
}

impl Harness {
    async fn new(cx: &mut TestAppContext) -> Self {
        let mut shared = Shared::new_with_contexts(&["Editor", "Markdown"], cx).await;
        let markdown = shared.cx.new(|cx| {
            Markdown::new(
                "**Xin** chào [bạn](https://example.com)".into(),
                None,
                None,
                cx,
            )
        });
        let editor = shared.cx.new_window_entity(|window, cx| {
            let mut editor = Editor::multi_line(window, cx);
            editor.set_text(EDITOR_TEXT, window, cx);
            let anchor = editor
                .buffer()
                .read(cx)
                .snapshot(cx)
                .anchor_after(MultiBufferOffset(0));
            let block_markdown = markdown.clone();
            editor.insert_blocks(
                [BlockProperties {
                    placement: BlockPlacement::Below(anchor),
                    height: Some(2),
                    style: BlockStyle::Flex,
                    render: Arc::new(move |block_cx| {
                        div()
                            .w(block_cx.max_width)
                            .child(MarkdownElement::new(
                                block_markdown.clone(),
                                MarkdownStyle::default(),
                            ))
                            .into_any_element()
                    }),
                    priority: 0,
                }],
                None,
                cx,
            );
            editor
        });
        let workspace = shared.workspace.clone();
        workspace.update_in(&mut shared.cx, |workspace, window, cx| {
            workspace.add_item_to_active_pane(Box::new(editor.clone()), None, true, window, cx);
            editor.update(cx, |editor, cx| window.focus(&editor.focus_handle(cx), cx));
        });
        shared.cx.run_until_parked();
        Self {
            shared,
            editor,
            markdown,
        }
    }

    fn select_editor_first_line(&mut self) {
        self.editor
            .update_in(&mut self.shared.cx, |editor, window, cx| {
                editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
                    selections
                        .select_ranges([MultiBufferOffset(0)..MultiBufferOffset("Chào".len())])
                });
            });
        self.shared.cx.run_until_parked();
    }

    /// Triple-clicks the markdown, which selects its line and focuses it.
    fn select_markdown_line(&mut self) {
        let bounds = self
            .markdown
            .read_with(&mut self.shared.cx, |markdown, _| {
                markdown.last_visible_bounds()
            })
            .expect("the editor painted the markdown block");
        let position = bounds.origin + point(px(4.), px(4.));
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
    }

    fn focus_editor(&mut self) {
        self.editor
            .update_in(&mut self.shared.cx, |editor, window, cx| {
                window.focus(&editor.focus_handle(cx), cx)
            });
        self.shared.cx.run_until_parked();
    }

    fn markdown_is_focused(&mut self) -> bool {
        self.markdown
            .update_in(&mut self.shared.cx, |markdown, window, cx| {
                markdown.focus_handle(cx).is_focused(window)
            })
    }

    fn editor_selection(&mut self) -> String {
        self.editor.update(&mut self.shared.cx, |editor, cx| {
            let snapshot = editor.display_snapshot(cx);
            let selection = editor.selections.newest_anchor().clone();
            snapshot
                .buffer_snapshot()
                .text_for_range(selection.start..selection.end)
                .collect()
        })
    }
}

#[gpui::test]
async fn markdown_inside_an_editor_with_no_selection_is_translated(cx: &mut TestAppContext) {
    let mut harness = Harness::new(cx).await;
    harness.select_markdown_line();
    assert!(harness.markdown_is_focused());
    assert_eq!(harness.editor_selection(), "");

    harness.shared.press();

    harness.shared.expect_popover();
    assert_eq!(harness.shared.provider.call_count(), 1);
    assert_eq!(harness.shared.provider.request(0).text, "Xin chào bạn");
}

#[gpui::test]
async fn focused_markdown_wins_over_the_selection_of_its_host_editor(cx: &mut TestAppContext) {
    let mut harness = Harness::new(cx).await;
    harness.select_editor_first_line();
    harness.select_markdown_line();
    assert!(harness.markdown_is_focused());

    harness.shared.press();

    harness.shared.expect_popover();
    assert_eq!(harness.shared.provider.request(0).text, "Xin chào bạn");
}

#[gpui::test]
async fn a_focused_editor_with_a_selection_translates_its_own_text(cx: &mut TestAppContext) {
    let mut harness = Harness::new(cx).await;
    harness.select_markdown_line();
    harness.focus_editor();
    harness.select_editor_first_line();
    assert_eq!(harness.editor_selection(), "Chào");

    harness.shared.press();

    harness.shared.expect_popover();
    assert_eq!(harness.shared.provider.call_count(), 1);
    assert_eq!(harness.shared.provider.request(0).text, "Chào");
}
