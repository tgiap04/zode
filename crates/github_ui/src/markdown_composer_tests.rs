use std::sync::Arc;

use editor::MultiBufferOffset;
use github::FakeGithubBackend;
use gpui::{
    AppContext as _, Entity, Focusable as _, KeyBinding, TestAppContext, VisualTestContext,
};

use crate::github_view_session::test_session;
use crate::issues::issues_test_support::{init, repository, workspace_window};
use crate::issues::markdown_editor;
use crate::markdown_composer::{MarkdownComposer, PreviewSource};
use crate::markdown_format::MarkdownFormat;

fn composer(text: &str, cx: &mut VisualTestContext) -> Entity<MarkdownComposer> {
    let source = PreviewSource {
        repository: repository(),
        backend: Arc::new(FakeGithubBackend::new()),
        session: test_session(),
    };
    cx.update(|window, cx| {
        let editor = markdown_editor(3, "", text, None, window, cx);
        cx.new(|_| MarkdownComposer::new(editor, source))
    })
}

fn text_and_selection(
    composer: &Entity<MarkdownComposer>,
    cx: &mut VisualTestContext,
) -> (String, std::ops::Range<usize>) {
    composer.update(cx, |composer, cx| {
        composer.editor.update(cx, |editor, cx| {
            let selection = editor
                .selections
                .newest::<MultiBufferOffset>(&editor.display_snapshot(cx));
            (editor.text(cx), selection.start.0..selection.end.0)
        })
    })
}

#[gpui::test]
async fn the_toolbar_formats_the_selection_and_one_undo_takes_it_back(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let composer = composer("fix the bug", cx);
    cx.update(|window, cx| {
        composer.update(cx, |composer, cx| {
            composer.editor.update(cx, |editor, cx| {
                editor.change_selections(Default::default(), window, cx, |selections| {
                    selections.select_ranges([MultiBufferOffset(4)..MultiBufferOffset(7)])
                });
            });
            composer.apply(MarkdownFormat::Bold, window, cx);
        })
    });
    assert_eq!(
        text_and_selection(&composer, cx),
        ("fix **the** bug".to_string(), 6..9)
    );

    cx.update(|window, cx| {
        composer.update(cx, |composer, cx| {
            composer.editor.update(cx, |editor, cx| {
                editor.undo(&Default::default(), window, cx)
            });
        })
    });
    assert_eq!(text_and_selection(&composer, cx).0, "fix the bug");
}

#[gpui::test]
async fn preview_shows_the_text_as_it_is_and_formatting_returns_to_write(cx: &mut TestAppContext) {
    init(cx);
    let (_workspace, cx) = workspace_window(cx).await;
    let composer = composer("# Steps", cx);
    composer.update(cx, |composer, cx| composer.show_preview(cx));
    let source = composer.update(cx, |composer, cx| {
        composer
            .preview
            .as_ref()
            .map(|preview| preview.read(cx).source(cx).to_string())
    });
    assert_eq!(source.as_deref(), Some("# Steps"));

    cx.update(|window, cx| {
        composer.update(cx, |composer, cx| {
            composer.apply(MarkdownFormat::Quote, window, cx)
        })
    });
    assert!(!composer.read_with(cx, |composer, _| composer.is_previewing()));
    assert_eq!(text_and_selection(&composer, cx).0, "> # Steps");
}

#[gpui::test]
async fn the_bold_shortcut_formats_the_selection_while_the_editor_has_focus(
    cx: &mut TestAppContext,
) {
    init(cx);
    cx.update(|cx| {
        cx.bind_keys([KeyBinding::new(
            "cmd-b",
            zode_actions::github::FormatBold,
            Some("GithubMarkdownComposer > Editor"),
        )])
    });
    let source = PreviewSource {
        repository: repository(),
        backend: Arc::new(FakeGithubBackend::new()),
        session: test_session(),
    };
    let (composer, cx) = cx.add_window_view(|window, cx| {
        let editor = markdown_editor(3, "", "fix the bug", None, window, cx);
        MarkdownComposer::new(editor, source)
    });
    cx.update(|window, cx| {
        composer.update(cx, |composer, cx| {
            composer.editor.update(cx, |editor, cx| {
                editor.change_selections(Default::default(), window, cx, |selections| {
                    selections.select_ranges([MultiBufferOffset(4)..MultiBufferOffset(7)])
                });
            });
            window.focus(&composer.editor.focus_handle(cx), cx);
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-b");
    assert_eq!(text_and_selection(&composer, cx).0, "fix **the** bug");
}
