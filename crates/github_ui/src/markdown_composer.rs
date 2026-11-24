//! A markdown field as GitHub draws one: Write and Preview tabs, and a
//! toolbar that formats the selection.

use std::sync::Arc;

use editor::{Editor, MultiBufferOffset};
use github::{GithubBackend, RepositoryId};
use gpui::{AppContext as _, Context, Entity, Focusable as _, Window};

use crate::github_markdown::GithubMarkdown;
use crate::github_view_session::SessionStamp;
use crate::markdown_format::{MarkdownFormat, format_edit};

/// Where a preview resolves `#12` and opens its links.
#[derive(Clone)]
pub struct PreviewSource {
    pub repository: RepositoryId,
    pub backend: Arc<dyn GithubBackend>,
    pub session: SessionStamp,
}

pub struct MarkdownComposer {
    pub(crate) editor: Entity<Editor>,
    pub(crate) source: PreviewSource,
    /// Set while the Preview tab is shown, made from the text at the time.
    pub(crate) preview: Option<Entity<GithubMarkdown>>,
}

impl MarkdownComposer {
    pub fn new(editor: Entity<Editor>, source: PreviewSource) -> Self {
        Self {
            editor,
            source,
            preview: None,
        }
    }

    /// Back on the Write tab, as after a comment is sent.
    pub fn back_to_write(&mut self, cx: &mut Context<Self>) {
        self.preview = None;
        cx.notify();
    }

    pub fn is_previewing(&self) -> bool {
        self.preview.is_some()
    }

    pub fn show_preview(&mut self, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).text(cx);
        let source = self.source.clone();
        self.preview = Some(cx.new(|cx| {
            GithubMarkdown::new(
                &text,
                &source.repository,
                source.backend,
                source.session,
                cx,
            )
        }));
        cx.notify();
    }

    pub fn show_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview = None;
        window.focus(&self.editor.focus_handle(cx), cx);
        cx.notify();
    }

    /// Formats the newest selection, back on the Write tab when the
    /// preview was showing, as one step to undo.
    pub fn apply(&mut self, format: MarkdownFormat, window: &mut Window, cx: &mut Context<Self>) {
        self.preview = None;
        self.editor.update(cx, |editor, cx| {
            let text = editor.text(cx);
            let selection = editor
                .selections
                .newest::<MultiBufferOffset>(&editor.display_snapshot(cx));
            let Some(edit) = format_edit(&text, selection.start.0..selection.end.0, format) else {
                return;
            };
            editor.transact(window, cx, |editor, window, cx| {
                let replaced =
                    MultiBufferOffset(edit.replaced.start)..MultiBufferOffset(edit.replaced.end);
                editor.edit([(replaced, edit.text)], cx);
                let selected =
                    MultiBufferOffset(edit.selection.start)..MultiBufferOffset(edit.selection.end);
                editor.change_selections(Default::default(), window, cx, |selections| {
                    selections.select_ranges([selected])
                });
            });
        });
        window.focus(&self.editor.focus_handle(cx), cx);
        cx.notify();
    }
}
