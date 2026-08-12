use std::ops::Range;

use acp_thread::{AcpThread, AgentThreadEntry};
use agent_client_protocol::schema as acp;
use collections::HashMap;
use editor::{Editor, EditorEvent, EditorMode, MinimapVisibility, SizingBehavior};
use gpui::{
    AnyEntity, App, AppContext as _, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    ScrollHandle, TextStyleRefinement, WeakEntity, Window,
};
use language::language_settings::SoftWrap;
use project::{AgentId, Project};
use prompt_store::PromptStore;
use rope::Point;
use settings::Settings as _;
use terminal_view::TerminalView;
use theme_settings::ThemeSettings;
use ui::{Context, TextSize};
use workspace::Workspace;

use crate::message_editor::{MessageEditor, MessageEditorEvent, SharedSessionCapabilities};

pub struct EntryViewState {
    workspace: WeakEntity<Workspace>,
    project: WeakEntity<Project>,
    prompt_store: Option<Entity<PromptStore>>,
    entries: Vec<Entry>,
    session_capabilities: SharedSessionCapabilities,
    agent_id: AgentId,
}

impl EntryViewState {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        project: WeakEntity<Project>,
        prompt_store: Option<Entity<PromptStore>>,
        session_capabilities: SharedSessionCapabilities,
        agent_id: AgentId,
    ) -> Self {
        Self {
            workspace,
            project,
            prompt_store,
            entries: Vec::new(),
            session_capabilities,
            agent_id,
        }
    }

    pub fn entry(&self, index: usize) -> Option<&Entry> {
        self.entries.get(index)
    }

    pub fn sync_entry(
        &mut self,
        index: usize,
        thread: &Entity<AcpThread>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread_entry) = thread.read(cx).entries().get(index) else {
            return;
        };

        match thread_entry {
            AgentThreadEntry::UserMessage(message) => {
                let can_rewind = thread.read(cx).supports_truncate(cx);
                let has_id = message.id.is_some();
                let is_subagent = thread.read(cx).parent_session_id().is_some();
                let chunks = message.chunks.clone();
                if let Some(Entry::UserMessage(editor)) = self.entries.get_mut(index) {
                    if !editor.focus_handle(cx).is_focused(window) {
                        // Only update if we are not editing.
                        // If we are, cancelling the edit will set the message to the newest content.
                        editor.update(cx, |editor, cx| {
                            editor.set_message(chunks, window, cx);
                        });
                    }
                } else {
                    let message_editor = cx.new(|cx| {
                        let mut editor = MessageEditor::new(
                            self.workspace.clone(),
                            self.project.clone(),
                            self.prompt_store.clone(),
                            self.session_capabilities.clone(),
                            self.agent_id.clone(),
                            "Edit message － @ to include context",
                            editor::EditorMode::AutoHeight {
                                min_lines: 1,
                                max_lines: None,
                            },
                            window,
                            cx,
                        );
                        if !can_rewind || !has_id || is_subagent {
                            editor.set_read_only(true, cx);
                        }
                        editor.set_message(chunks, window, cx);
                        editor
                    });
                    cx.subscribe(&message_editor, move |_, editor, event, cx| {
                        cx.emit(EntryViewEvent {
                            entry_index: index,
                            view_event: ViewEvent::MessageEditorEvent(editor, event.clone()),
                        })
                    })
                    .detach();
                    self.set_entry(index, Entry::UserMessage(message_editor));
                }
            }
            AgentThreadEntry::ToolCall(tool_call) => {
                let id = tool_call.id.clone();
                let terminals = tool_call.terminals().cloned().collect::<Vec<_>>();
                let diffs = tool_call.diffs().cloned().collect::<Vec<_>>();

                let views = if let Some(Entry::ToolCall(tool_call)) = self.entries.get_mut(index) {
                    &mut tool_call.content
                } else {
                    self.set_entry(
                        index,
                        Entry::ToolCall(ToolCallEntry {
                            content: HashMap::default(),
                            focus_handle: cx.focus_handle(),
                        }),
                    );
                    let Some(Entry::ToolCall(tool_call)) = self.entries.get_mut(index) else {
                        unreachable!()
                    };
                    &mut tool_call.content
                };

                let is_tool_call_completed =
                    matches!(tool_call.status, acp_thread::ToolCallStatus::Completed);

                for terminal in terminals {
                    match views.entry(terminal.entity_id()) {
                        collections::hash_map::Entry::Vacant(entry) => {
                            let element = create_terminal(
                                self.workspace.clone(),
                                self.project.clone(),
                                terminal.clone(),
                                window,
                                cx,
                            )
                            .into_any();
                            cx.emit(EntryViewEvent {
                                entry_index: index,
                                view_event: ViewEvent::NewTerminal(id.clone()),
                            });
                            entry.insert(element);
                        }
                        collections::hash_map::Entry::Occupied(_entry) => {
                            if is_tool_call_completed && terminal.read(cx).output().is_none() {
                                cx.emit(EntryViewEvent {
                                    entry_index: index,
                                    view_event: ViewEvent::TerminalMovedToBackground(id.clone()),
                                });
                            }
                        }
                    }
                }

                for diff in diffs {
                    views.entry(diff.entity_id()).or_insert_with(|| {
                        let editor = create_editor_diff(diff.clone(), window, cx);
                        cx.subscribe(&editor, {
                            let diff = diff.clone();
                            let entry_index = index;
                            move |_this, _editor, event: &EditorEvent, cx| {
                                if let EditorEvent::OpenExcerptsRequested {
                                    selections_by_buffer,
                                    split,
                                } = event
                                {
                                    let multibuffer = diff.read(cx).multibuffer();
                                    if let Some((buffer_id, (ranges, _))) =
                                        selections_by_buffer.iter().next()
                                    {
                                        if let Some(buffer) =
                                            multibuffer.read(cx).buffer(*buffer_id)
                                        {
                                            if let Some(range) = ranges.first() {
                                                let point =
                                                    buffer.read(cx).offset_to_point(range.start.0);
                                                if let Some(path) = diff.read(cx).file_path(cx) {
                                                    cx.emit(EntryViewEvent {
                                                        entry_index,
                                                        view_event: ViewEvent::OpenDiffLocation {
                                                            path,
                                                            position: point,
                                                            split: *split,
                                                        },
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        })
                        .detach();
                        cx.emit(EntryViewEvent {
                            entry_index: index,
                            view_event: ViewEvent::NewDiff(id.clone()),
                        });
                        editor.into_any()
                    });
                }
            }
            AgentThreadEntry::AssistantMessage(message) => {
                let entry = if let Some(Entry::AssistantMessage(entry)) =
                    self.entries.get_mut(index)
                {
                    entry
                } else {
                    self.set_entry(
                        index,
                        Entry::AssistantMessage(AssistantMessageEntry {
                            scroll_handles_by_chunk_index: HashMap::default(),
                            focus_handle: cx.focus_handle(),
                        }),
                    );
                    let Some(Entry::AssistantMessage(entry)) = self.entries.get_mut(index) else {
                        unreachable!()
                    };
                    entry
                };
                entry.sync(message);
            }
            AgentThreadEntry::CompletedPlan(_) => {
                if !matches!(self.entries.get(index), Some(Entry::CompletedPlan)) {
                    self.set_entry(index, Entry::CompletedPlan);
                }
            }
        };
    }

    fn set_entry(&mut self, index: usize, entry: Entry) {
        if index == self.entries.len() {
            self.entries.push(entry);
        } else {
            self.entries[index] = entry;
        }
    }

    pub fn remove(&mut self, range: Range<usize>) {
        self.entries.drain(range);
    }

    pub fn agent_ui_font_size_changed(&mut self, cx: &mut App) {
        for entry in self.entries.iter() {
            match entry {
                Entry::UserMessage { .. }
                | Entry::AssistantMessage { .. }
                | Entry::CompletedPlan => {}
                Entry::ToolCall(ToolCallEntry { content, .. }) => {
                    for view in content.values() {
                        if let Ok(diff_editor) = view.clone().downcast::<Editor>() {
                            diff_editor.update(cx, |diff_editor, cx| {
                                diff_editor.set_text_style_refinement(
                                    diff_editor_text_style_refinement(cx),
                                );
                                cx.notify();
                            })
                        }
                    }
                }
            }
        }
    }
}

impl EventEmitter<EntryViewEvent> for EntryViewState {}

pub struct EntryViewEvent {
    pub entry_index: usize,
    pub view_event: ViewEvent,
}

pub enum ViewEvent {
    NewDiff(acp::ToolCallId),
    NewTerminal(acp::ToolCallId),
    TerminalMovedToBackground(acp::ToolCallId),
    MessageEditorEvent(Entity<MessageEditor>, MessageEditorEvent),
    OpenDiffLocation {
        path: String,
        position: Point,
        split: bool,
    },
}

#[derive(Debug)]
pub struct AssistantMessageEntry {
    scroll_handles_by_chunk_index: HashMap<usize, ScrollHandle>,
    focus_handle: FocusHandle,
}

impl AssistantMessageEntry {
    pub fn scroll_handle_for_chunk(&self, ix: usize) -> Option<ScrollHandle> {
        self.scroll_handles_by_chunk_index.get(&ix).cloned()
    }

    pub fn sync(&mut self, message: &acp_thread::AssistantMessage) {
        if let Some(acp_thread::AssistantMessageChunk::Thought { .. }) = message.chunks.last() {
            let ix = message.chunks.len() - 1;
            let handle = self.scroll_handles_by_chunk_index.entry(ix).or_default();
            handle.scroll_to_bottom();
        }
    }
}

#[derive(Debug)]
pub struct ToolCallEntry {
    content: HashMap<EntityId, AnyEntity>,
    focus_handle: FocusHandle,
}

#[derive(Debug)]
pub enum Entry {
    UserMessage(Entity<MessageEditor>),
    AssistantMessage(AssistantMessageEntry),
    ToolCall(ToolCallEntry),
    CompletedPlan,
}

impl Entry {
    pub fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        match self {
            Self::UserMessage(editor) => Some(editor.read(cx).focus_handle(cx)),
            Self::AssistantMessage(message) => Some(message.focus_handle.clone()),
            Self::ToolCall(tool_call) => Some(tool_call.focus_handle.clone()),
            Self::CompletedPlan => None,
        }
    }

    pub fn message_editor(&self) -> Option<&Entity<MessageEditor>> {
        match self {
            Self::UserMessage(editor) => Some(editor),
            Self::AssistantMessage(_) | Self::ToolCall(_) | Self::CompletedPlan => None,
        }
    }

    pub fn editor_for_diff(&self, diff: &Entity<acp_thread::Diff>) -> Option<Entity<Editor>> {
        self.content_map()?
            .get(&diff.entity_id())
            .cloned()
            .map(|entity| entity.downcast::<Editor>().unwrap())
    }

    pub fn terminal(
        &self,
        terminal: &Entity<acp_thread::Terminal>,
    ) -> Option<Entity<TerminalView>> {
        self.content_map()?
            .get(&terminal.entity_id())
            .cloned()
            .map(|entity| entity.downcast::<TerminalView>().unwrap())
    }

    pub fn scroll_handle_for_assistant_message_chunk(
        &self,
        chunk_ix: usize,
    ) -> Option<ScrollHandle> {
        match self {
            Self::AssistantMessage(message) => message.scroll_handle_for_chunk(chunk_ix),
            Self::UserMessage(_) | Self::ToolCall(_) | Self::CompletedPlan => None,
        }
    }

    fn content_map(&self) -> Option<&HashMap<EntityId, AnyEntity>> {
        match self {
            Self::ToolCall(ToolCallEntry { content, .. }) => Some(content),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn has_content(&self) -> bool {
        match self {
            Self::ToolCall(ToolCallEntry { content, .. }) => !content.is_empty(),
            Self::UserMessage(_) | Self::AssistantMessage(_) | Self::CompletedPlan => false,
        }
    }
}

impl Focusable for ToolCallEntry {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Focusable for Entry {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::UserMessage(editor) => editor.read(cx).focus_handle(cx),
            Self::AssistantMessage(message) => message.focus_handle.clone(),
            Self::ToolCall(tool_call) => tool_call.focus_handle.clone(),
            Self::CompletedPlan => cx.focus_handle(),
        }
    }
}

fn create_terminal(
    workspace: WeakEntity<Workspace>,
    project: WeakEntity<Project>,
    terminal: Entity<acp_thread::Terminal>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<TerminalView> {
    cx.new(|cx| {
        let mut view = TerminalView::new(
            terminal.read(cx).inner().clone(),
            workspace,
            None,
            project,
            window,
            cx,
        );
        view.set_embedded_mode(Some(1000), cx);
        view
    })
}

fn create_editor_diff(
    diff: Entity<acp_thread::Diff>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Editor> {
    cx.new(|cx| {
        let mut editor = Editor::new(
            EditorMode::Full {
                scale_ui_elements_with_buffer_font_size: false,
                show_active_line_background: false,
                sizing_behavior: SizingBehavior::SizeByContent,
            },
            diff.read(cx).multibuffer().clone(),
            None,
            window,
            cx,
        );
        editor.set_show_gutter(false, cx);
        editor.disable_inline_diagnostics();
        editor.disable_expand_excerpt_buttons(cx);
        editor.set_show_vertical_scrollbar(false, cx);
        editor.set_minimap_visibility(MinimapVisibility::Disabled, window, cx);
        editor.set_soft_wrap_mode(SoftWrap::None, cx);
        editor.scroll_manager.set_forbid_vertical_scroll(true);
        editor.set_show_indent_guides(false, cx);
        editor.set_read_only(true);
        editor.set_delegate_open_excerpts(true);
        editor.set_show_bookmarks(false, cx);
        editor.set_show_breakpoints(false, cx);
        editor.set_show_code_actions(false, cx);
        editor.set_show_git_diff_gutter(false, cx);
        editor.set_expand_all_diff_hunks(cx);
        editor.set_text_style_refinement(diff_editor_text_style_refinement(cx));
        editor
    })
}

fn diff_editor_text_style_refinement(cx: &mut App) -> TextStyleRefinement {
    TextStyleRefinement {
        font_size: Some(
            TextSize::Small
                .rems(cx)
                .to_pixels(ThemeSettings::get_global(cx).ui_font_size(cx))
                .into(),
        ),
        ..Default::default()
    }
}

// The tests upstream kept here reached through `conversation_view::tests` for a
// harness that built an `AgentPanel`, and through the native agent for a thread
// store. Both are gone, so this coverage has to be rebuilt against the
// centre-pane view rather than ported.
