//! Mid-port note: a handful of items here are unreachable at the moment.
//!
//! Their only call sites upstream were in `agent_panel` — dragging files onto the
//! agent, inserting the editor's selection, re-authenticating — and that panel is
//! not ported. Each is a real thing a user can do to an agent, so they are kept
//! rather than deleted, waiting to be re-homed on `AgentView`.
#![allow(dead_code)]

use crate::SendImmediately;
use crate::{
    ChatWithFollow,
    completion_provider::{
        PromptCompletionProvider, PromptCompletionProviderDelegate, PromptContextAction,
        PromptContextType, SlashCommandCompletion,
    },
    mention_set::{Mention, MentionImage, MentionSet, insert_crease_for_mention},
};
use acp_thread::MentionUri;
use agent_client_protocol::schema as acp;
use anyhow::{Result, anyhow};
use editor::{
    Addon, AnchorRangeExt, ContextMenuOptions, Editor, EditorElement, EditorEvent, EditorMode,
    EditorStyle, Inlay, MultiBuffer, MultiBufferOffset, MultiBufferSnapshot, ToOffset,
    actions::Paste, code_context_menus::CodeContextMenu, scroll::Autoscroll,
};
use futures::{FutureExt as _, future::join_all};
use gpui::{
    AppContext, ClipboardEntry, Context, Entity, EventEmitter, FocusHandle, Focusable, ImageFormat,
    KeyContext, SharedString, Subscription, Task, TextStyle, WeakEntity,
};
use language::{Buffer, language_settings::InlayHintKind};
use parking_lot::RwLock;
use project::AgentId;
use project::{
    CompletionIntent, InlayHint, InlayHintLabel, InlayId, Project, ProjectPath, Worktree,
};
use prompt_store::PromptStore;
use rope::Point;
use settings::Settings;
use std::{fmt::Write, ops::Range, rc::Rc, sync::Arc};
use theme_settings::ThemeSettings;
use ui::{ContextMenu, Disclosure, ElevationIndex, prelude::*};
use util::paths::PathStyle;
use util::{ResultExt, debug_panic};
use workspace::{CollaboratorId, Workspace};
use zed_actions::agent::{Chat, PasteRaw};

#[derive(Default)]
pub struct SessionCapabilities {
    prompt_capabilities: acp::PromptCapabilities,
    available_commands: Vec<acp::AvailableCommand>,
}

impl SessionCapabilities {
    pub fn new(
        prompt_capabilities: acp::PromptCapabilities,
        available_commands: Vec<acp::AvailableCommand>,
    ) -> Self {
        Self {
            prompt_capabilities,
            available_commands,
        }
    }

    pub fn supports_images(&self) -> bool {
        self.prompt_capabilities.image
    }

    pub fn supports_embedded_context(&self) -> bool {
        self.prompt_capabilities.embedded_context
    }

    pub fn available_commands(&self) -> &[acp::AvailableCommand] {
        &self.available_commands
    }

    fn supported_modes(&self, has_thread_store: bool) -> Vec<PromptContextType> {
        let mut supported = vec![PromptContextType::File, PromptContextType::Symbol];
        if self.prompt_capabilities.embedded_context {
            if has_thread_store {
                supported.push(PromptContextType::Thread);
            }
            supported.extend(&[
                PromptContextType::Diagnostics,
                PromptContextType::Fetch,
                PromptContextType::Rules,
                PromptContextType::BranchDiff,
            ]);
        }
        supported
    }

    pub fn completion_commands(&self) -> Vec<crate::completion_provider::AvailableCommand> {
        self.available_commands
            .iter()
            .map(|cmd| crate::completion_provider::AvailableCommand {
                name: cmd.name.clone().into(),
                description: cmd.description.clone().into(),
                requires_argument: cmd.input.is_some(),
            })
            .collect()
    }

    pub fn set_prompt_capabilities(&mut self, prompt_capabilities: acp::PromptCapabilities) {
        self.prompt_capabilities = prompt_capabilities;
    }

    pub fn set_available_commands(&mut self, available_commands: Vec<acp::AvailableCommand>) {
        self.available_commands = available_commands;
    }
}

pub type SharedSessionCapabilities = Arc<RwLock<SessionCapabilities>>;

struct MessageEditorCompletionDelegate {
    session_capabilities: SharedSessionCapabilities,
    has_thread_store: bool,
    message_editor: WeakEntity<MessageEditor>,
}

impl PromptCompletionProviderDelegate for MessageEditorCompletionDelegate {
    fn supports_images(&self, _cx: &App) -> bool {
        self.session_capabilities.read().supports_images()
    }

    fn supported_modes(&self, _cx: &App) -> Vec<PromptContextType> {
        self.session_capabilities
            .read()
            .supported_modes(self.has_thread_store)
    }

    fn available_commands(&self, _cx: &App) -> Vec<crate::completion_provider::AvailableCommand> {
        self.session_capabilities.read().completion_commands()
    }

    fn confirm_command(&self, cx: &mut App) {
        let _ = self.message_editor.update(cx, |this, cx| this.send(cx));
    }
}

pub struct MessageEditor {
    mention_set: Entity<MentionSet>,
    editor: Entity<Editor>,
    workspace: WeakEntity<Workspace>,
    session_capabilities: SharedSessionCapabilities,
    agent_id: AgentId,
    _subscriptions: Vec<Subscription>,
    _parse_slash_command_task: Task<()>,
}

#[derive(Clone, Debug)]
pub enum MessageEditorEvent {
    Send,
    SendImmediately,
    Cancel,
    Focus,
    LostFocus,
    InputAttempted {
        text: Arc<str>,
        cursor_offset: usize,
    },
}

impl EventEmitter<MessageEditorEvent> for MessageEditor {}

const COMMAND_HINT_INLAY_ID: InlayId = InlayId::Hint(0);

enum MentionInsertPosition {
    AtCursor,
    EndOfBuffer,
}

fn insert_mention_for_project_path(
    project_path: &ProjectPath,
    position: MentionInsertPosition,
    editor: &Entity<Editor>,
    mention_set: &Entity<MentionSet>,
    project: &Entity<Project>,
    workspace: &Entity<Workspace>,
    supports_images: bool,
    window: &mut Window,
    cx: &mut App,
) -> Option<Task<()>> {
    let (file_name, mention_uri) = {
        let project = project.read(cx);
        let path_style = project.path_style(cx);
        let entry = project.entry_for_path(project_path, cx)?;
        let worktree = project.worktree_for_id(project_path.worktree_id, cx)?;
        let abs_path = worktree.read(cx).absolutize(&project_path.path);
        let (file_name, _) = crate::completion_provider::extract_file_name_and_directory(
            &project_path.path,
            worktree.read(cx).root_name(),
            path_style,
        );
        let mention_uri = if entry.is_dir() {
            MentionUri::Directory { abs_path }
        } else {
            MentionUri::File { abs_path }
        };
        (file_name, mention_uri)
    };

    let mention_text = mention_uri.as_link().to_string();
    let content_len = mention_text.len();

    let text_anchor = match position {
        MentionInsertPosition::AtCursor => editor.update(cx, |editor, cx| {
            let buffer = editor.buffer().read(cx);
            let snapshot = buffer.snapshot(cx);
            let buffer_snapshot = snapshot.as_singleton()?;
            let text_anchor = snapshot
                .anchor_to_buffer_anchor(editor.selections.newest_anchor().start)?
                .0
                .bias_left(&buffer_snapshot);

            editor.insert(&mention_text, window, cx);
            editor.insert(" ", window, cx);

            Some(text_anchor)
        }),
        MentionInsertPosition::EndOfBuffer => {
            let multi_buffer = editor.read(cx).buffer().clone();
            let buffer = multi_buffer.read(cx).as_singleton()?;
            let anchor = buffer.update(cx, |buffer, _cx| buffer.anchor_before(buffer.len()));
            let new_text = format!("{mention_text} ");
            editor.update(cx, |editor, cx| {
                editor.edit(
                    [(
                        multi_buffer::Anchor::Max..multi_buffer::Anchor::Max,
                        new_text,
                    )],
                    cx,
                );
            });
            Some(anchor)
        }
    }?;

    Some(mention_set.update(cx, |mention_set, cx| {
        mention_set.confirm_mention_completion(
            file_name,
            text_anchor,
            content_len,
            mention_uri,
            supports_images,
            editor.clone(),
            workspace,
            window,
            cx,
        )
    }))
}

enum ResolvedPastedContextItem {
    Image(gpui::Image, gpui::SharedString),
    ProjectPath(ProjectPath),
}

async fn resolve_pasted_context_items(
    project: Entity<Project>,
    project_is_local: bool,
    supports_images: bool,
    entries: Vec<ClipboardEntry>,
    cx: &mut gpui::AsyncWindowContext,
) -> (Vec<ResolvedPastedContextItem>, Vec<Entity<Worktree>>) {
    let mut items = Vec::new();
    let mut added_worktrees = Vec::new();
    let default_image_name: SharedString = "Image".into();

    for entry in entries {
        match entry {
            ClipboardEntry::String(_) => {}
            ClipboardEntry::Image(image) => {
                if supports_images {
                    items.push(ResolvedPastedContextItem::Image(
                        image,
                        default_image_name.clone(),
                    ));
                }
            }
            ClipboardEntry::ExternalPaths(paths) => {
                for path in paths.paths().iter() {
                    if let Some((image, name)) = cx
                        .background_spawn({
                            let path = path.clone();
                            let default_image_name = default_image_name.clone();
                            async move {
                                crate::mention_set::load_external_image_from_path(
                                    &path,
                                    &default_image_name,
                                )
                            }
                        })
                        .await
                    {
                        if supports_images {
                            items.push(ResolvedPastedContextItem::Image(image, name));
                        }
                        continue;
                    }

                    if !project_is_local {
                        continue;
                    }

                    let path = path.clone();
                    let Ok(resolve_task) = cx.update({
                        let project = project.clone();
                        move |_, cx| Workspace::project_path_for_path(project, &path, false, cx)
                    }) else {
                        continue;
                    };

                    if let Some((worktree, project_path)) = resolve_task.await.log_err() {
                        added_worktrees.push(worktree);
                        items.push(ResolvedPastedContextItem::ProjectPath(project_path));
                    }
                }
            }
        }
    }

    (items, added_worktrees)
}

fn insert_project_path_as_context(
    project_path: ProjectPath,
    editor: Entity<Editor>,
    mention_set: Entity<MentionSet>,
    workspace: WeakEntity<Workspace>,
    supports_images: bool,
    cx: &mut gpui::AsyncWindowContext,
) -> Option<Task<()>> {
    let workspace = workspace.upgrade()?;

    cx.update(move |window, cx| {
        let project = workspace.read(cx).project().clone();
        insert_mention_for_project_path(
            &project_path,
            MentionInsertPosition::AtCursor,
            &editor,
            &mention_set,
            &project,
            &workspace,
            supports_images,
            window,
            cx,
        )
    })
    .ok()
    .flatten()
}

async fn insert_resolved_pasted_context_items(
    items: Vec<ResolvedPastedContextItem>,
    added_worktrees: Vec<Entity<Worktree>>,
    editor: Entity<Editor>,
    mention_set: Entity<MentionSet>,
    workspace: WeakEntity<Workspace>,
    supports_images: bool,
    cx: &mut gpui::AsyncWindowContext,
) {
    let mut path_mention_tasks = Vec::new();

    for item in items {
        match item {
            ResolvedPastedContextItem::Image(image, name) => {
                crate::mention_set::insert_images_as_context(
                    vec![(image, name)],
                    editor.clone(),
                    mention_set.clone(),
                    workspace.clone(),
                    cx,
                )
                .await;
            }
            ResolvedPastedContextItem::ProjectPath(project_path) => {
                if let Some(task) = insert_project_path_as_context(
                    project_path,
                    editor.clone(),
                    mention_set.clone(),
                    workspace.clone(),
                    supports_images,
                    cx,
                ) {
                    path_mention_tasks.push(task);
                }
            }
        }
    }

    join_all(path_mention_tasks).await;
    drop(added_worktrees);
}

impl MessageEditor {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        project: WeakEntity<Project>,
        prompt_store: Option<Entity<PromptStore>>,
        session_capabilities: SharedSessionCapabilities,
        agent_id: AgentId,
        placeholder: &str,
        mode: EditorMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let language_registry = project
            .upgrade()
            .map(|project| project.read(cx).languages().clone());

        let editor = cx.new(|cx| {
            let buffer = cx.new(|cx| {
                let buffer = Buffer::local("", cx);
                if let Some(language_registry) = language_registry.as_ref() {
                    buffer.set_language_registry(language_registry.clone());
                }
                buffer
            });
            let buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));

            let mut editor = Editor::new(mode, buffer, None, window, cx);
            editor.set_placeholder_text(placeholder, window, cx);
            editor.set_show_indent_guides(false, cx);
            editor.set_show_completions_on_input(Some(true));
            editor.set_soft_wrap();
            editor.disable_mouse_wheel_zoom();
            editor.set_use_modal_editing(true);
            editor.set_context_menu_options(ContextMenuOptions {
                min_entries_visible: 12,
                max_entries_visible: 12,
                placement: None,
            });
            editor.register_addon(MessageEditorAddon::new());

            editor.set_custom_context_menu(|editor, _point, window, cx| {
                let has_selection = editor.has_non_empty_selection(&editor.display_snapshot(cx));

                Some(ContextMenu::build(window, cx, |menu, _, _| {
                    menu.action("Cut", Box::new(editor::actions::Cut))
                        .action_disabled_when(
                            !has_selection,
                            "Copy",
                            Box::new(editor::actions::Copy),
                        )
                        .action("Paste", Box::new(editor::actions::Paste))
                        .action("Paste as Plain Text", Box::new(PasteRaw))
                }))
            });

            editor
        });
        let mention_set =
            cx.new(|_cx| MentionSet::new(project, prompt_store.clone()));
        let completion_provider = Rc::new(PromptCompletionProvider::new(
            MessageEditorCompletionDelegate {
                session_capabilities: session_capabilities.clone(),
                has_thread_store: false,
                message_editor: cx.weak_entity(),
            },
            editor.downgrade(),
            mention_set.clone(),
            prompt_store.clone(),
            workspace.clone(),
        ));
        editor.update(cx, |editor, _cx| {
            editor.set_completion_provider(Some(completion_provider.clone()))
        });

        cx.on_focus_in(&editor.focus_handle(cx), window, |_, _, cx| {
            cx.emit(MessageEditorEvent::Focus)
        })
        .detach();
        cx.on_focus_out(&editor.focus_handle(cx), window, |_, _, _, cx| {
            cx.emit(MessageEditorEvent::LostFocus)
        })
        .detach();

        let mut has_hint = false;
        let mut subscriptions = Vec::new();

        subscriptions.push(cx.subscribe_in(&editor, window, {
            move |this, editor, event, window, cx| {
                let input_attempted_text = match event {
                    EditorEvent::InputHandled { text, .. } => Some(text),
                    EditorEvent::InputIgnored { text } => Some(text),
                    _ => None,
                };
                if let Some(text) = input_attempted_text
                    && editor.read(cx).read_only(cx)
                    && !text.is_empty()
                {
                    let editor = editor.read(cx);
                    let cursor_anchor = editor.selections.newest_anchor().head();
                    let cursor_offset = cursor_anchor
                        .to_offset(&editor.buffer().read(cx).snapshot(cx))
                        .0;
                    cx.emit(MessageEditorEvent::InputAttempted {
                        text: text.clone(),
                        cursor_offset,
                    });
                }

                if let EditorEvent::Edited { .. } = event
                    && !editor.read(cx).read_only(cx)
                {
                    editor.update(cx, |editor, cx| {
                        let snapshot = editor.snapshot(window, cx);
                        this.mention_set
                            .update(cx, |mention_set, _cx| mention_set.remove_invalid(&snapshot));

                        let new_hints = this
                            .command_hint(snapshot.buffer())
                            .into_iter()
                            .collect::<Vec<_>>();
                        let has_new_hint = !new_hints.is_empty();
                        editor.splice_inlays(
                            if has_hint {
                                &[COMMAND_HINT_INLAY_ID]
                            } else {
                                &[]
                            },
                            new_hints,
                            cx,
                        );
                        has_hint = has_new_hint;
                    });
                    cx.notify();
                }
            }
        }));

        if let Some(language_registry) = language_registry {
            let editor = editor.clone();
            cx.spawn(async move |_, cx| {
                let markdown = language_registry.language_for_name("Markdown").await?;
                editor.update(cx, |editor, cx| {
                    if let Some(buffer) = editor.buffer().read(cx).as_singleton() {
                        buffer.update(cx, |buffer, cx| {
                            buffer.set_language(Some(markdown), cx);
                        });
                    }
                });
                anyhow::Ok(())
            })
            .detach_and_log_err(cx);
        }

        Self {
            editor,
            mention_set,
            workspace,
            session_capabilities,
            agent_id,
            _subscriptions: subscriptions,
            _parse_slash_command_task: Task::ready(()),
        }
    }

    pub fn set_session_capabilities(
        &mut self,
        session_capabilities: SharedSessionCapabilities,
        _cx: &mut Context<Self>,
    ) {
        self.session_capabilities = session_capabilities;
    }

    fn command_hint(&self, snapshot: &MultiBufferSnapshot) -> Option<Inlay> {
        let session_capabilities = self.session_capabilities.read();
        let available_commands = session_capabilities.available_commands();
        if available_commands.is_empty() {
            return None;
        }

        let parsed_command = SlashCommandCompletion::try_parse(&snapshot.text(), 0)?;
        if parsed_command.argument.is_some() {
            return None;
        }

        let command_name = parsed_command.command?;
        let available_command = available_commands
            .iter()
            .find(|command| command.name == command_name)?;

        let acp::AvailableCommandInput::Unstructured(acp::UnstructuredCommandInput {
            mut hint,
            ..
        }) = available_command.input.clone()?
        else {
            return None;
        };

        let mut hint_pos = MultiBufferOffset(parsed_command.source_range.end) + 1usize;
        if hint_pos > snapshot.len() {
            hint_pos = snapshot.len();
            hint.insert(0, ' ');
        }

        let hint_pos = snapshot.anchor_after(hint_pos);

        Some(Inlay::hint(
            COMMAND_HINT_INLAY_ID,
            hint_pos,
            &InlayHint {
                position: snapshot.anchor_to_buffer_anchor(hint_pos)?.0,
                label: InlayHintLabel::String(hint),
                kind: Some(InlayHintKind::Parameter),
                padding_left: false,
                padding_right: false,
                tooltip: None,
                resolve_state: project::ResolveState::Resolved,
            },
        ))
    }

    /// Inserting a previous thread's summary needed a thread store to read it out
    /// of; upstream even gated the whole body on having one. Threads are not
    /// persisted here, so there is nothing to summarise and the call is a no-op
    /// rather than a half-working path.
    pub fn insert_thread_summary(
        &mut self,
        _session_id: acp::SessionId,
        _title: Option<SharedString>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    #[cfg(test)]
    pub(crate) fn editor(&self) -> &Entity<Editor> {
        &self.editor
    }

    pub fn is_empty(&self, cx: &App) -> bool {
        self.editor.read(cx).is_empty(cx)
    }

    pub fn is_completions_menu_visible(&self, cx: &App) -> bool {
        self.editor
            .read(cx)
            .context_menu()
            .borrow()
            .as_ref()
            .is_some_and(|menu| matches!(menu, CodeContextMenu::Completions(_)) && menu.visible())
    }

    #[cfg(test)]
    pub fn mention_set(&self) -> &Entity<MentionSet> {
        &self.mention_set
    }

    fn validate_slash_commands(
        text: &str,
        available_commands: &[acp::AvailableCommand],
        agent_id: &AgentId,
    ) -> Result<()> {
        if let Some(parsed_command) = SlashCommandCompletion::try_parse(text, 0) {
            if let Some(command_name) = parsed_command.command {
                // Check if this command is in the list of available commands from the server
                let is_supported = available_commands
                    .iter()
                    .any(|cmd| cmd.name == command_name);

                if !is_supported {
                    return Err(anyhow!(
                        "The /{} command is not supported by {}.\n\nAvailable commands: {}",
                        command_name,
                        agent_id,
                        if available_commands.is_empty() {
                            "none".to_string()
                        } else {
                            available_commands
                                .iter()
                                .map(|cmd| format!("/{}", cmd.name))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn contents(
        &self,
        full_mention_content: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<(Vec<acp::ContentBlock>, Vec<Entity<Buffer>>)>> {
        let text = self.editor.read(cx).text(cx);
        let available_commands = self
            .session_capabilities
            .read()
            .available_commands()
            .to_vec();
        let agent_id = self.agent_id.clone();
        let build_task = self.build_content_blocks(full_mention_content, cx);

        cx.spawn(async move |_, _cx| {
            Self::validate_slash_commands(&text, &available_commands, &agent_id)?;
            build_task.await
        })
    }

    pub fn draft_contents(&self, cx: &mut Context<Self>) -> Task<Result<Vec<acp::ContentBlock>>> {
        let build_task = self.build_content_blocks(false, cx);
        cx.spawn(async move |_, _cx| {
            let (blocks, _tracked_buffers) = build_task.await?;
            Ok(blocks)
        })
    }

    fn build_content_blocks(
        &self,
        full_mention_content: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<(Vec<acp::ContentBlock>, Vec<Entity<Buffer>>)>> {
        let contents = self
            .mention_set
            .update(cx, |store, cx| store.contents(full_mention_content, cx));
        let editor = self.editor.clone();
        let supports_embedded_context =
            self.session_capabilities.read().supports_embedded_context();

        cx.spawn(async move |_, cx| {
            let contents = contents.await?;
            let mut all_tracked_buffers = Vec::new();

            let result = editor.update(cx, |editor, cx| {
                let text = editor.text(cx);
                let (mut ix, _) = text
                    .char_indices()
                    .find(|(_, c)| !c.is_whitespace())
                    .unwrap_or((0, '\0'));
                let mut chunks: Vec<acp::ContentBlock> = Vec::new();
                editor.display_map.update(cx, |map, cx| {
                    let snapshot = map.snapshot(cx);
                    for (crease_id, crease) in snapshot.crease_snapshot.creases() {
                        let Some((uri, mention)) = contents.get(&crease_id) else {
                            continue;
                        };

                        let crease_range = crease.range().to_offset(&snapshot.buffer_snapshot());
                        if crease_range.start.0 > ix {
                            let chunk = text[ix..crease_range.start.0].into();
                            chunks.push(chunk);
                        }
                        let chunk = match mention {
                            Mention::Text {
                                content,
                                tracked_buffers,
                            } => {
                                all_tracked_buffers.extend(tracked_buffers.iter().cloned());
                                if supports_embedded_context {
                                    acp::ContentBlock::Resource(acp::EmbeddedResource::new(
                                        acp::EmbeddedResourceResource::TextResourceContents(
                                            acp::TextResourceContents::new(
                                                content.clone(),
                                                uri.to_uri().to_string(),
                                            ),
                                        ),
                                    ))
                                } else {
                                    acp::ContentBlock::ResourceLink(acp::ResourceLink::new(
                                        uri.name(),
                                        uri.to_uri().to_string(),
                                    ))
                                }
                            }
                            Mention::Image(mention_image) => acp::ContentBlock::Image(
                                acp::ImageContent::new(
                                    mention_image.data.clone(),
                                    mention_image.format.mime_type(),
                                )
                                .uri(match uri {
                                    MentionUri::File { .. } => Some(uri.to_uri().to_string()),
                                    MentionUri::PastedImage { .. } => {
                                        Some(uri.to_uri().to_string())
                                    }
                                    other => {
                                        debug_panic!(
                                            "unexpected mention uri for image: {:?}",
                                            other
                                        );
                                        None
                                    }
                                }),
                            ),
                            Mention::Link => acp::ContentBlock::ResourceLink(
                                acp::ResourceLink::new(uri.name(), uri.to_uri().to_string()),
                            ),
                        };
                        chunks.push(chunk);
                        ix = crease_range.end.0;
                    }

                    if ix < text.len() {
                        let last_chunk = text[ix..].trim_end().to_owned();
                        if !last_chunk.is_empty() {
                            chunks.push(last_chunk.into());
                        }
                    }
                });
                anyhow::Ok((chunks, all_tracked_buffers))
            })?;
            Ok(result)
        })
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.clear(window, cx);
            editor.remove_creases(
                self.mention_set.update(cx, |mention_set, _cx| {
                    mention_set
                        .clear()
                        .map(|(crease_id, _)| crease_id)
                        .collect::<Vec<_>>()
                }),
                cx,
            )
        });
    }

    pub fn send(&mut self, cx: &mut Context<Self>) {
        if !self.is_empty(cx) {
            self.editor.update(cx, |editor, cx| {
                editor.clear_inlay_hints(cx);
            });
        }
        cx.emit(MessageEditorEvent::Send)
    }

    pub fn trigger_completion_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.insert_context_prefix("@", window, cx);
    }

    pub fn insert_context_type(
        &mut self,
        context_keyword: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prefix = format!("@{}", context_keyword);
        self.insert_context_prefix(&prefix, window, cx);
    }

    fn insert_context_prefix(&mut self, prefix: &str, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.clone();
        let prefix = prefix.to_string();

        cx.spawn_in(window, async move |_, cx| {
            editor
                .update_in(cx, |editor, window, cx| {
                    let menu_is_open =
                        editor.context_menu().borrow().as_ref().is_some_and(|menu| {
                            matches!(menu, CodeContextMenu::Completions(_)) && menu.visible()
                        });

                    let has_prefix = {
                        let snapshot = editor.display_snapshot(cx);
                        let cursor = editor.selections.newest::<text::Point>(&snapshot).head();
                        let offset = cursor.to_offset(&snapshot);
                        let buffer_snapshot = snapshot.buffer_snapshot();
                        let prefix_char_count = prefix.chars().count();
                        buffer_snapshot
                            .reversed_chars_at(offset)
                            .take(prefix_char_count)
                            .eq(prefix.chars().rev())
                    };

                    if menu_is_open && has_prefix {
                        return;
                    }

                    editor.insert(&prefix, window, cx);
                    editor.show_completions(&editor::actions::ShowCompletions, window, cx);
                })
                .log_err();
        })
        .detach();
    }

    fn chat(&mut self, _: &Chat, _: &mut Window, cx: &mut Context<Self>) {
        self.send(cx);
    }

    fn send_immediately(&mut self, _: &SendImmediately, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_empty(cx) {
            return;
        }

        self.editor.update(cx, |editor, cx| {
            editor.clear_inlay_hints(cx);
        });

        cx.emit(MessageEditorEvent::SendImmediately)
    }

    fn chat_with_follow(
        &mut self,
        _: &ChatWithFollow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace
            .update(cx, |this, cx| {
                this.follow(CollaboratorId::Agent, window, cx)
            })
            .log_err();

        self.send(cx);
    }

    fn cancel(&mut self, _: &editor::actions::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(MessageEditorEvent::Cancel)
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let editor_clipboard_selections = cx.read_from_clipboard().and_then(|item| {
            item.entries().iter().find_map(|entry| match entry {
                ClipboardEntry::String(text) => {
                    text.metadata_json::<Vec<editor::ClipboardSelection>>()
                }
                _ => None,
            })
        });

        // Insert creases for pasted clipboard selections that:
        // 1. Contain exactly one selection
        // 2. Have an associated file path
        // 3. Span multiple lines (not single-line selections)
        // 4. Belong to a file that exists in the current project
        let should_insert_creases = util::maybe!({
            let selections = editor_clipboard_selections.as_ref()?;
            if selections.len() > 1 {
                return Some(false);
            }
            let selection = selections.first()?;
            let file_path = selection.file_path.as_ref()?;
            let line_range = selection.line_range.as_ref()?;

            if line_range.start() == line_range.end() {
                return Some(false);
            }

            Some(
                workspace
                    .read(cx)
                    .project()
                    .read(cx)
                    .project_path_for_absolute_path(file_path, cx)
                    .is_some(),
            )
        })
        .unwrap_or(false);

        if should_insert_creases && let Some(selections) = editor_clipboard_selections {
            cx.stop_propagation();
            let snapshot = self.editor.read(cx).buffer().read(cx).snapshot(cx);
            let (insertion_target, _) = snapshot
                .anchor_to_buffer_anchor(self.editor.read(cx).selections.newest_anchor().start)
                .unwrap();

            let project = workspace.read(cx).project().clone();
            for selection in selections {
                if let (Some(file_path), Some(line_range)) =
                    (selection.file_path, selection.line_range)
                {
                    let crease_text =
                        acp_thread::selection_name(Some(file_path.as_ref()), &line_range);

                    let mention_uri = MentionUri::Selection {
                        abs_path: Some(file_path.clone()),
                        line_range: line_range.clone(),
                    };

                    let mention_text = mention_uri.as_link().to_string();
                    let (text_anchor, content_len) = self.editor.update(cx, |editor, cx| {
                        let buffer = editor.buffer().read(cx);
                        let snapshot = buffer.snapshot(cx);
                        let buffer_snapshot = snapshot.as_singleton().unwrap();
                        let text_anchor = insertion_target.bias_left(&buffer_snapshot);

                        editor.insert(&mention_text, window, cx);
                        editor.insert(" ", window, cx);

                        (text_anchor, mention_text.len())
                    });

                    let Some((crease_id, tx)) = insert_crease_for_mention(
                        text_anchor,
                        content_len,
                        crease_text.into(),
                        mention_uri.icon_path(cx),
                        mention_uri.tooltip_text(),
                        Some(mention_uri.clone()),
                        Some(self.workspace.clone()),
                        None,
                        self.editor.clone(),
                        window,
                        cx,
                    ) else {
                        continue;
                    };
                    drop(tx);

                    let mention_task = cx
                        .spawn({
                            let project = project.clone();
                            async move |_, cx| {
                                let project_path = project
                                    .update(cx, |project, cx| {
                                        project.project_path_for_absolute_path(&file_path, cx)
                                    })
                                    .ok_or_else(|| "project path not found".to_string())?;

                                let buffer = project
                                    .update(cx, |project, cx| project.open_buffer(project_path, cx))
                                    .await
                                    .map_err(|e| e.to_string())?;

                                Ok(buffer.update(cx, |buffer, cx| {
                                    let start =
                                        Point::new(*line_range.start(), 0).min(buffer.max_point());
                                    let end = Point::new(*line_range.end() + 1, 0)
                                        .min(buffer.max_point());
                                    let content = buffer.text_for_range(start..end).collect();
                                    Mention::Text {
                                        content,
                                        tracked_buffers: vec![cx.entity()],
                                    }
                                }))
                            }
                        })
                        .shared();

                    self.mention_set.update(cx, |mention_set, _cx| {
                        mention_set.insert_mention(crease_id, mention_uri.clone(), mention_task)
                    });
                }
            }
            return;
        }
        // Handle text paste with potential markdown mention links before
        // clipboard context entries so markdown text still pastes as text.
        if let Some(clipboard_text) = cx.read_from_clipboard().and_then(|item| {
            item.entries().iter().find_map(|entry| match entry {
                ClipboardEntry::String(text) => Some(text.text().to_string()),
                _ => None,
            })
        }) {
            if clipboard_text.contains("[@") {
                cx.stop_propagation();
                let selections_before = self.editor.update(cx, |editor, cx| {
                    let snapshot = editor.buffer().read(cx).snapshot(cx);
                    editor
                        .selections
                        .disjoint_anchors()
                        .iter()
                        .map(|selection| {
                            (
                                selection.start.bias_left(&snapshot),
                                selection.end.bias_right(&snapshot),
                            )
                        })
                        .collect::<Vec<_>>()
                });

                self.editor.update(cx, |editor, cx| {
                    editor.insert(&clipboard_text, window, cx);
                });

                let snapshot = self.editor.read(cx).buffer().read(cx).snapshot(cx);
                let path_style = workspace.read(cx).project().read(cx).path_style(cx);

                let mut all_mentions = Vec::new();
                for (start_anchor, end_anchor) in selections_before {
                    let start_offset = start_anchor.to_offset(&snapshot);
                    let end_offset = end_anchor.to_offset(&snapshot);

                    // Get the actual inserted text from the buffer (may differ due to auto-indent)
                    let inserted_text: String =
                        snapshot.text_for_range(start_offset..end_offset).collect();

                    let parsed_mentions = parse_mention_links(&inserted_text, path_style);
                    for (range, mention_uri) in parsed_mentions {
                        let mention_start_offset = MultiBufferOffset(start_offset.0 + range.start);
                        let anchor = snapshot.anchor_before(mention_start_offset);
                        let content_len = range.end - range.start;
                        all_mentions.push((anchor, content_len, mention_uri));
                    }
                }

                if !all_mentions.is_empty() {
                    let supports_images = self.session_capabilities.read().supports_images();
                    let http_client = workspace.read(cx).client().http_client();

                    for (anchor, content_len, mention_uri) in all_mentions {
                        let Some((crease_id, tx)) = insert_crease_for_mention(
                            snapshot.anchor_to_buffer_anchor(anchor).unwrap().0,
                            content_len,
                            mention_uri.name().into(),
                            mention_uri.icon_path(cx),
                            mention_uri.tooltip_text(),
                            Some(mention_uri.clone()),
                            Some(self.workspace.clone()),
                            None,
                            self.editor.clone(),
                            window,
                            cx,
                        ) else {
                            continue;
                        };

                        // Create the confirmation task based on the mention URI type.
                        // This properly loads file content, fetches URLs, etc.
                        let task = self.mention_set.update(cx, |mention_set, cx| {
                            mention_set.confirm_mention_for_uri(
                                mention_uri.clone(),
                                supports_images,
                                http_client.clone(),
                                cx,
                            )
                        });
                        let task = cx
                            .spawn(async move |_, _| task.await.map_err(|e| e.to_string()))
                            .shared();

                        self.mention_set.update(cx, |mention_set, _cx| {
                            mention_set.insert_mention(crease_id, mention_uri.clone(), task.clone())
                        });

                        // Drop the tx after inserting to signal the crease is ready
                        drop(tx);
                    }
                    return;
                }
            }
        }

        if self.handle_pasted_context(window, cx) {
            return;
        }

        // Fall through to default editor paste
        cx.propagate();
    }

    fn paste_raw(&mut self, _: &PasteRaw, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.clone();
        window.defer(cx, move |window, cx| {
            editor.update(cx, |editor, cx| editor.paste(&Paste, window, cx));
        });
    }

    fn handle_pasted_context(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(clipboard) = cx.read_from_clipboard() else {
            return false;
        };

        if matches!(
            clipboard.entries().first(),
            Some(ClipboardEntry::String(_)) | None
        ) {
            return false;
        }

        let Some(workspace) = self.workspace.upgrade() else {
            return false;
        };
        let project = workspace.read(cx).project().clone();
        let project_is_local = project.read(cx).is_local();
        let supports_images = self.session_capabilities.read().supports_images();
        if !project_is_local && !supports_images {
            return false;
        }
        let editor = self.editor.clone();
        let mention_set = self.mention_set.clone();
        let workspace = self.workspace.clone();
        let entries = clipboard.into_entries().collect::<Vec<_>>();

        cx.stop_propagation();

        window
            .spawn(cx, async move |mut cx| {
                let (items, added_worktrees) = resolve_pasted_context_items(
                    project,
                    project_is_local,
                    supports_images,
                    entries,
                    &mut cx,
                )
                .await;
                insert_resolved_pasted_context_items(
                    items,
                    added_worktrees,
                    editor,
                    mention_set,
                    workspace,
                    supports_images,
                    &mut cx,
                )
                .await;
                Ok::<(), anyhow::Error>(())
            })
            .detach_and_log_err(cx);

        true
    }

    // Reached only from the dock panel upstream. `AgentView` is where this needs
    // re-homing; until then nothing calls it.
    #[allow(dead_code)]
    pub fn insert_dragged_files(
        &mut self,
        paths: Vec<project::ProjectPath>,
        added_worktrees: Vec<Entity<Worktree>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let project = workspace.read(cx).project().clone();
        let supports_images = self.session_capabilities.read().supports_images();
        let mut tasks = Vec::new();
        for path in paths {
            if let Some(task) = insert_mention_for_project_path(
                &path,
                MentionInsertPosition::EndOfBuffer,
                &self.editor,
                &self.mention_set,
                &project,
                &workspace,
                supports_images,
                window,
                cx,
            ) {
                tasks.push(task);
            }
        }
        cx.spawn(async move |_, _| {
            join_all(tasks).await;
            drop(added_worktrees);
        })
        .detach();
    }

    /// Inserts code snippets as creases into the editor.
    /// Each tuple contains (code_text, crease_title).
    pub fn insert_code_creases(
        &mut self,
        creases: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            editor.insert("\n", window, cx);
        });
        for (text, crease_title) in creases {
            self.insert_crease_impl(text, crease_title, IconName::TextSnippet, true, window, cx);
        }
    }

    pub fn insert_branch_diff_crease(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };

        let project = workspace.read(cx).project().clone();

        let Some(repo) = project.read(cx).active_repository(cx) else {
            return;
        };

        let default_branch_receiver = repo.update(cx, |repo, _| repo.default_branch(false));
        let editor = self.editor.clone();
        let mention_set = self.mention_set.clone();
        let weak_workspace = self.workspace.clone();

        window
            .spawn(cx, async move |cx| {
                let base_ref: SharedString = default_branch_receiver
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .flatten()
                    .ok_or_else(|| anyhow!("Could not determine default branch"))?;

                cx.update(|window, cx| {
                    let mention_uri = MentionUri::GitDiff {
                        base_ref: base_ref.to_string(),
                    };
                    let mention_text = mention_uri.as_link().to_string();

                    let (text_anchor, content_len) = editor.update(cx, |editor, cx| {
                        let buffer = editor.buffer().read(cx);
                        let snapshot = buffer.snapshot(cx);
                        let buffer_snapshot = snapshot.as_singleton().unwrap();
                        let text_anchor = snapshot
                            .anchor_to_buffer_anchor(editor.selections.newest_anchor().start)
                            .unwrap()
                            .0
                            .bias_left(&buffer_snapshot);

                        editor.insert(&mention_text, window, cx);
                        editor.insert(" ", window, cx);

                        (text_anchor, mention_text.len())
                    });

                    let Some((crease_id, tx)) = insert_crease_for_mention(
                        text_anchor,
                        content_len,
                        mention_uri.name().into(),
                        mention_uri.icon_path(cx),
                        mention_uri.tooltip_text(),
                        Some(mention_uri.clone()),
                        Some(weak_workspace),
                        None,
                        editor,
                        window,
                        cx,
                    ) else {
                        return;
                    };
                    drop(tx);

                    let confirm_task = mention_set.update(cx, |mention_set, cx| {
                        mention_set.confirm_mention_for_git_diff(base_ref, cx)
                    });

                    let mention_task = cx
                        .spawn(async move |_cx| confirm_task.await.map_err(|e| e.to_string()))
                        .shared();

                    mention_set.update(cx, |mention_set, _| {
                        mention_set.insert_mention(crease_id, mention_uri, mention_task);
                    });
                })
            })
            .detach_and_log_err(cx);
    }

    fn insert_crease_impl(
        &mut self,
        text: String,
        title: String,
        icon: IconName,
        add_trailing_newline: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use editor::display_map::{Crease, FoldPlaceholder};
        use multi_buffer::MultiBufferRow;
        use rope::Point;

        self.editor.update(cx, |editor, cx| {
            let point = editor
                .selections
                .newest::<Point>(&editor.display_snapshot(cx))
                .head();
            let start_row = MultiBufferRow(point.row);

            editor.insert(&text, window, cx);

            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let anchor_before = snapshot.anchor_after(point);
            let anchor_after = editor
                .selections
                .newest_anchor()
                .head()
                .bias_left(&snapshot);

            if add_trailing_newline {
                editor.insert("\n", window, cx);
            }

            let fold_placeholder = FoldPlaceholder {
                render: Arc::new({
                    let title = title.clone();
                    move |_fold_id, _fold_range, _cx| {
                        Button::new("crease", title.clone())
                            .layer(ElevationIndex::ElevatedSurface)
                            .start_icon(Icon::new(icon))
                            .into_any_element()
                    }
                }),
                merge_adjacent: false,
                ..Default::default()
            };

            let crease = Crease::inline(
                anchor_before..anchor_after,
                fold_placeholder,
                |row, is_folded, fold, _window, _cx| {
                    Disclosure::new(("crease-toggle", row.0 as u64), !is_folded)
                        .toggle_state(is_folded)
                        .on_click(move |_e, window, cx| fold(!is_folded, window, cx))
                        .into_any_element()
                },
                |_, _, _, _| gpui::Empty.into_any(),
            );
            editor.insert_creases(vec![crease], cx);
            editor.fold_at(start_row, window, cx);
        });
    }

    // Reached only from the dock panel upstream. `AgentView` is where this needs
    // re-homing; until then nothing calls it.
    #[allow(dead_code)]
    pub fn insert_selections(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        let editor_buffer = editor.buffer().read(cx);
        let Some(buffer) = editor_buffer.as_singleton() else {
            return;
        };
        let cursor_anchor = editor.selections.newest_anchor().head();
        let cursor_offset = cursor_anchor.to_offset(&editor_buffer.snapshot(cx));
        let anchor = buffer.update(cx, |buffer, _cx| {
            buffer.anchor_before(cursor_offset.0.min(buffer.len()))
        });
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let Some(completion) =
            PromptCompletionProvider::<MessageEditorCompletionDelegate>::completion_for_action(
                PromptContextAction::AddSelections,
                anchor..anchor,
                self.editor.downgrade(),
                self.mention_set.downgrade(),
                &workspace,
                cx,
            )
        else {
            return;
        };

        self.editor.update(cx, |message_editor, cx| {
            message_editor.edit([(cursor_anchor..cursor_anchor, completion.new_text)], cx);
            message_editor.request_autoscroll(Autoscroll::fit(), cx);
        });
        if let Some(confirm) = completion.confirm {
            confirm(CompletionIntent::Complete, window, cx);
        }
    }

    pub fn add_images_from_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session_capabilities.read().supports_images() {
            return;
        }

        let editor = self.editor.clone();
        let mention_set = self.mention_set.clone();
        let workspace = self.workspace.clone();

        let paths_receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Select Images".into()),
        });

        window
            .spawn(cx, async move |cx| {
                let paths = match paths_receiver.await {
                    Ok(Ok(Some(paths))) => paths,
                    _ => return Ok::<(), anyhow::Error>(()),
                };

                let default_image_name: SharedString = "Image".into();
                let images = cx
                    .background_spawn(async move {
                        paths
                            .into_iter()
                            .filter_map(|path| {
                                crate::mention_set::load_external_image_from_path(
                                    &path,
                                    &default_image_name,
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .await;

                crate::mention_set::insert_images_as_context(
                    images,
                    editor,
                    mention_set,
                    workspace,
                    cx,
                )
                .await;
                Ok(())
            })
            .detach_and_log_err(cx);
    }

    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.editor.update(cx, |message_editor, cx| {
            message_editor.set_read_only(read_only);
            cx.notify()
        })
    }

    pub fn set_mode(&mut self, mode: EditorMode, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            if *editor.mode() != mode {
                editor.set_mode(mode);
                cx.notify()
            }
        });
    }

    pub fn set_message(
        &mut self,
        message: Vec<acp::ContentBlock>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear(window, cx);
        self.insert_message_blocks(message, false, window, cx);
    }

    pub fn append_message(
        &mut self,
        message: Vec<acp::ContentBlock>,
        separator: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if message.is_empty() {
            return;
        }

        if let Some(separator) = separator
            && !separator.is_empty()
            && !self.is_empty(cx)
        {
            self.editor.update(cx, |editor, cx| {
                editor.insert(separator, window, cx);
            });
        }

        self.insert_message_blocks(message, true, window, cx);
    }

    fn insert_message_blocks(
        &mut self,
        message: Vec<acp::ContentBlock>,
        append_to_existing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };

        let path_style = workspace.read(cx).project().read(cx).path_style(cx);
        let mut text = String::new();
        let mut mentions = Vec::new();

        for chunk in message {
            match chunk {
                acp::ContentBlock::Text(text_content) => {
                    text.push_str(&text_content.text);
                }
                acp::ContentBlock::Resource(acp::EmbeddedResource {
                    resource: acp::EmbeddedResourceResource::TextResourceContents(resource),
                    ..
                }) => {
                    let Some(mention_uri) = MentionUri::parse(&resource.uri, path_style).log_err()
                    else {
                        continue;
                    };
                    let start = text.len();
                    write!(&mut text, "{}", mention_uri.as_link()).ok();
                    let end = text.len();
                    mentions.push((
                        start..end,
                        mention_uri,
                        Mention::Text {
                            content: resource.text,
                            tracked_buffers: Vec::new(),
                        },
                    ));
                }
                acp::ContentBlock::ResourceLink(resource) => {
                    if let Some(mention_uri) =
                        MentionUri::parse(&resource.uri, path_style).log_err()
                    {
                        let start = text.len();
                        write!(&mut text, "{}", mention_uri.as_link()).ok();
                        let end = text.len();
                        mentions.push((start..end, mention_uri, Mention::Link));
                    }
                }
                acp::ContentBlock::Image(acp::ImageContent {
                    uri,
                    data,
                    mime_type,
                    ..
                }) => {
                    let mention_uri = if let Some(uri) = uri {
                        MentionUri::parse(&uri, path_style)
                    } else {
                        Ok(MentionUri::PastedImage {
                            name: "Image".to_string(),
                        })
                    };
                    let Some(mention_uri) = mention_uri.log_err() else {
                        continue;
                    };
                    let Some(format) = ImageFormat::from_mime_type(&mime_type) else {
                        log::error!("failed to parse MIME type for image: {mime_type:?}");
                        continue;
                    };
                    let start = text.len();
                    write!(&mut text, "{}", mention_uri.as_link()).ok();
                    let end = text.len();
                    mentions.push((
                        start..end,
                        mention_uri,
                        Mention::Image(MentionImage {
                            data: data.into(),
                            format,
                        }),
                    ));
                }
                _ => {}
            }
        }

        if text.is_empty() && mentions.is_empty() {
            return;
        }

        let insertion_start = if append_to_existing {
            self.editor.read(cx).text(cx).len()
        } else {
            0
        };

        let snapshot = if append_to_existing {
            self.editor.update(cx, |editor, cx| {
                editor.insert(&text, window, cx);
                editor.buffer().read(cx).snapshot(cx)
            })
        } else {
            self.editor.update(cx, |editor, cx| {
                editor.set_text(text, window, cx);
                editor.buffer().read(cx).snapshot(cx)
            })
        };

        for (range, mention_uri, mention) in mentions {
            let adjusted_start = insertion_start + range.start;
            let anchor = snapshot.anchor_before(MultiBufferOffset(adjusted_start));
            let Some((crease_id, tx)) = insert_crease_for_mention(
                snapshot.anchor_to_buffer_anchor(anchor).unwrap().0,
                range.end - range.start,
                mention_uri.name().into(),
                mention_uri.icon_path(cx),
                mention_uri.tooltip_text(),
                Some(mention_uri.clone()),
                Some(self.workspace.clone()),
                None,
                self.editor.clone(),
                window,
                cx,
            ) else {
                continue;
            };
            drop(tx);

            self.mention_set.update(cx, |mention_set, _cx| {
                mention_set.insert_mention(
                    crease_id,
                    mention_uri.clone(),
                    Task::ready(Ok(mention)).shared(),
                )
            });
        }

        cx.notify();
    }

    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).text(cx)
    }

    pub fn set_cursor_offset(
        &mut self,
        offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let offset = snapshot.clip_offset(MultiBufferOffset(offset), text::Bias::Left);
            editor.change_selections(Default::default(), window, cx, |selections| {
                selections.select_ranges([offset..offset]);
            });
        });
    }

    pub fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if text.is_empty() {
            return;
        }

        self.editor.update(cx, |editor, cx| {
            editor.insert(text, window, cx);
        });
    }

    pub fn set_placeholder_text(
        &mut self,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            editor.set_placeholder_text(placeholder, window, cx);
        });
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |editor, cx| {
            editor.set_text(text, window, cx);
        });
    }
}

impl Focusable for MessageEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl Render for MessageEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("MessageEditor")
            .on_action(cx.listener(Self::chat))
            .on_action(cx.listener(Self::send_immediately))
            .on_action(cx.listener(Self::chat_with_follow))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::paste_raw))
            .capture_action(cx.listener(Self::paste))
            .flex_1()
            .child({
                let settings = ThemeSettings::get_global(cx);

                let text_style = TextStyle {
                    color: cx.theme().colors().text,
                    font_family: settings.buffer_font.family.clone(),
                    font_fallbacks: settings.buffer_font.fallbacks.clone(),
                    font_features: settings.buffer_font.features.clone(),
                    font_size: settings.buffer_font_size(cx).into(),
                    font_weight: settings.buffer_font.weight,
                    line_height: relative(settings.buffer_line_height.value()),
                    ..Default::default()
                };

                EditorElement::new(
                    &self.editor,
                    EditorStyle {
                        background: cx.theme().colors().editor_background,
                        local_player: cx.theme().players().local(),
                        text: text_style,
                        syntax: cx.theme().syntax().clone(),
                        inlay_hints_style: editor::make_inlay_hints_style(cx),
                        ..Default::default()
                    },
                )
            })
    }
}

pub struct MessageEditorAddon {}

impl MessageEditorAddon {
    pub fn new() -> Self {
        Self {}
    }
}

impl Addon for MessageEditorAddon {
    fn to_any(&self) -> &dyn std::any::Any {
        self
    }

    fn to_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn extend_key_context(&self, key_context: &mut KeyContext, cx: &App) {
        let settings = agent_settings::AgentSettings::get_global(cx);
        if settings.use_modifier_to_send {
            key_context.add("use_modifier_to_send");
        }
    }
}

/// Parses markdown mention links in the format `[@name](uri)` from text.
/// Returns a vector of (range, MentionUri) pairs where range is the byte range in the text.
fn parse_mention_links(text: &str, path_style: PathStyle) -> Vec<(Range<usize>, MentionUri)> {
    let mut mentions = Vec::new();
    let mut search_start = 0;

    while let Some(link_start) = text[search_start..].find("[@") {
        let absolute_start = search_start + link_start;

        // Find the matching closing bracket for the name, handling nested brackets.
        // Start at the '[' character so find_matching_bracket can track depth correctly.
        let Some(name_end) = find_matching_bracket(&text[absolute_start..], '[', ']') else {
            search_start = absolute_start + 2;
            continue;
        };
        let name_end = absolute_start + name_end;

        // Check for opening parenthesis immediately after
        if text.get(name_end + 1..name_end + 2) != Some("(") {
            search_start = name_end + 1;
            continue;
        }

        // Find the matching closing parenthesis for the URI, handling nested parens
        let uri_start = name_end + 2;
        let Some(uri_end_relative) = find_matching_bracket(&text[name_end + 1..], '(', ')') else {
            search_start = uri_start;
            continue;
        };
        let uri_end = name_end + 1 + uri_end_relative;
        let link_end = uri_end + 1;

        let uri_str = &text[uri_start..uri_end];

        // Try to parse the URI as a MentionUri
        if let Ok(mention_uri) = MentionUri::parse(uri_str, path_style) {
            mentions.push((absolute_start..link_end, mention_uri));
        }

        search_start = link_end;
    }

    mentions
}

/// Finds the position of the matching closing bracket, handling nested brackets.
/// The input `text` should start with the opening bracket.
/// Returns the index of the matching closing bracket relative to `text`.
fn find_matching_bracket(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0;
    for (index, character) in text.char_indices() {
        if character == open {
            depth += 1;
        } else if character == close {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

// The tests upstream kept here reached through `conversation_view::tests` for a
// harness that built an `AgentPanel`, and through the native agent for a thread
// store. Both are gone, so this coverage has to be rebuilt against the
// centre-pane view rather than ported.
