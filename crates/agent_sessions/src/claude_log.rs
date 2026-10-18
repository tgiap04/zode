//! Reading Claude Code's JSONL transcripts.
//!
//! The format is undocumented and belongs to another program, so every field
//! here degrades on its own: a missing `ai-title` falls back to the first user
//! message, a missing `cwd` leaves the session ungrouped, an unparsable line is
//! skipped. A format change must cost a column, never the panel.
//!
//! Everything in this module takes `&str` rather than a path. That keeps the
//! parse decisions testable without a filesystem, and keeps them in one place.

use crate::{Speaker, SubagentEvent, TurnMark};
use serde::{
    Deserialize, Deserializer,
    de::{SeqAccess, Visitor},
};
use serde_json::{Value, value::RawValue};
use std::{fmt, path::PathBuf, sync::Arc};

/// What the tail of a transcript can tell us. The tail, not the head, because
/// `ai-title` is appended repeatedly as the title is refined and the last one
/// wins — measured at 74 occurrences in a single session.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct TailFacts {
    /// The name the user typed for this session themselves. Written as its own
    /// line the moment they rename it, and it outranks anything generated.
    pub custom_title: Option<String>,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub preview_speaker: Option<Speaker>,
    pub cwd: Option<PathBuf>,
    pub branch: Option<String>,
    pub model: Option<String>,
}

/// What only the head can tell us: the first thing the user said, which is the
/// title when no `ai-title` was ever written.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct HeadFacts {
    pub first_user_message: Option<String>,
    pub cwd: Option<PathBuf>,
}

pub(crate) fn parse_tail(tail: &str) -> TailFacts {
    let mut facts = TailFacts::default();
    for line in tail.lines() {
        let Some(entry) = parse_line(line) else {
            continue;
        };
        match entry.kind {
            // Later lines overwrite earlier ones on purpose: within the tail the
            // last of each of these is the current truth.
            EntryKind::AiTitle(title) => facts.title = Some(title),
            EntryKind::CustomTitle(title) => facts.custom_title = Some(title),
            EntryKind::Message { speaker, text } => {
                if let Some(text) = text {
                    facts.preview = Some(text);
                    facts.preview_speaker = Some(speaker);
                }
                if let Some(model) = entry.model {
                    facts.model = Some(model);
                }
            }
            EntryKind::Other => {}
        }
        if let Some(cwd) = entry.cwd {
            facts.cwd = Some(cwd);
        }
        if let Some(branch) = entry.branch {
            facts.branch = Some(branch);
        }
    }
    facts
}

pub(crate) fn parse_head(head: &str) -> HeadFacts {
    let mut facts = HeadFacts::default();
    for line in head.lines() {
        let Some(entry) = parse_line(line) else {
            continue;
        };
        if facts.cwd.is_none() {
            facts.cwd = entry.cwd;
        }
        if facts.first_user_message.is_none()
            && let EntryKind::Message {
                speaker: Speaker::User,
                text: Some(text),
            } = entry.kind
        {
            facts.first_user_message = Some(text);
        }
        if facts.first_user_message.is_some() && facts.cwd.is_some() {
            break;
        }
    }
    facts
}

/// Whether a line is a conversation message, for counting.
///
/// Deliberately a substring test on the type field rather than a full parse: an
/// assistant line can be hundreds of kilobytes, and deserializing every one of
/// them to learn a single word costs the entire budget. The test is anchored on
/// the quoted key, so the same text appearing inside message content does not
/// count.
pub(crate) fn line_is_message(line: &str) -> bool {
    line.contains(r#""type":"user""#) || line.contains(r#""type":"assistant""#)
}

/// What one pass over a stretch of transcript found.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct ChunkScan {
    pub tool_use_ids: Vec<String>,
    pub turn_marks: Vec<TurnMark>,
    pub subagent_events: Vec<SubagentEvent>,
}

impl ChunkScan {
    /// Appends what a later stretch found, keeping file order.
    pub(crate) fn extend(&mut self, later: ChunkScan) {
        self.tool_use_ids.extend(later.tool_use_ids);
        self.turn_marks.extend(later.turn_marks);
        self.subagent_events.extend(later.subagent_events);
    }
}

/// A field read on its own terms: a value of the wrong type is `None` instead
/// of a line that fails to parse.
///
/// The transcript format belongs to another program, and the reading above it
/// degrades one field at a time. Declaring only the fields a scan needs keeps
/// the rest -- a base64 image, a file a tool returned -- from being allocated
/// at all, and this keeps a field the CLI reshapes from costing the line.
struct Lenient<T>(Option<T>);

impl<T> Default for Lenient<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw: &RawValue = Deserialize::deserialize(deserializer)?;
        Ok(Self(serde_json::from_str(raw.get()).ok()))
    }
}

/// [`Lenient`] for a field that is a JSON object: one written as a list is
/// `None` as well.
///
/// A derived struct reads its fields from a JSON array too, in declaration
/// order, so `["x"]` would otherwise come out as a struct whose first field is
/// `"x"`. The transcript never writes an object that way, and the reading this
/// replaced found nothing in a list.
struct Object<T>(Option<T>);

impl<T> Default for Object<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw: &RawValue = Deserialize::deserialize(deserializer)?;
        Ok(Self(parse_object(raw.get())))
    }
}

/// `text` as a `T`, when it is a JSON object that reads as one.
fn parse_object<'a, T: Deserialize<'a>>(text: &'a str) -> Option<T> {
    if !text.trim_start().starts_with('{') {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// `message.content`: a bare string, or a list of blocks.
enum Content {
    Text(String),
    Blocks(Vec<Object<Block>>),
}

impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ContentVisitor;

        impl<'de> Visitor<'de> for ContentVisitor {
            type Value = Content;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string or a list of blocks")
            }

            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Content, E> {
                Ok(Content::Text(text.to_owned()))
            }

            fn visit_string<E: serde::de::Error>(self, text: String) -> Result<Content, E> {
                Ok(Content::Text(text))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Content, A::Error> {
                let mut blocks = Vec::new();
                while let Some(block) = seq.next_element::<Object<Block>>()? {
                    blocks.push(block);
                }
                Ok(Content::Blocks(blocks))
            }
        }

        deserializer.deserialize_any(ContentVisitor)
    }
}

#[derive(Deserialize, Default)]
struct Block {
    #[serde(rename = "type", default)]
    kind: Lenient<String>,
    #[serde(default)]
    tool_use_id: Lenient<String>,
    #[serde(default)]
    id: Lenient<String>,
    #[serde(default)]
    text: Lenient<String>,
}

#[derive(Deserialize, Default)]
struct Message {
    #[serde(default)]
    content: Lenient<Content>,
    #[serde(default)]
    stop_reason: Lenient<String>,
    #[serde(default)]
    id: Lenient<String>,
    #[serde(default)]
    model: Lenient<String>,
}

/// The fields of a transcript line a scan reads, and nothing else.
#[derive(Deserialize)]
struct Line<'a> {
    #[serde(rename = "type", default)]
    kind: Lenient<String>,
    #[serde(default)]
    message: Object<Message>,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: Lenient<bool>,
    #[serde(rename = "isMeta", default)]
    is_meta: Lenient<bool>,
    #[serde(rename = "isCompactSummary", default)]
    is_compact_summary: Lenient<bool>,
    #[serde(rename = "isApiErrorMessage", default)]
    is_api_error_message: Lenient<bool>,
    #[serde(default)]
    subtype: Lenient<String>,
    #[serde(rename = "pendingBackgroundAgentCount", default)]
    pending_background_agents: Lenient<u64>,
    #[serde(default)]
    operation: Lenient<String>,
    /// A queued operation carries its text here, outside `message`.
    #[serde(rename = "content", default)]
    queued: Lenient<String>,
    /// Kept as raw text: it is a string, an object or a list depending on the
    /// tool, can be as large as a file, and only a tool-result line ever needs
    /// to look inside it.
    #[serde(rename = "toolUseResult", borrow, default)]
    tool_use_result: Option<&'a RawValue>,
}

/// The tool calls this stretch of transcript reports a result for, and the
/// landmarks of the main conversation chain, in the order the file wrote them.
///
/// How a subagent's ending is known. The sidecar records the `toolUseId` the
/// parent used to spawn it; the parent's own transcript later carries a
/// `tool_result` block under that same id. Both were read off a finished
/// session before this was written -- the `tool_use` that starts a subagent and
/// the `tool_result` that ends it are two lines of the parent's log.
///
/// The alternative was the subagent's own file, and it was measured and
/// rejected: within a single run the gap between writes reached 165 seconds, so
/// no staleness threshold can tell a thinking subagent from a finished one.
///
/// The turn marks come from the same lines in the same pass, so the file is
/// read once and one cursor serves both. The substring guard is only a cheap
/// pre-filter -- most lines here are assistant messages that can run to hundreds
/// of kilobytes, and parsing all of them costs the budget -- and the parsed line
/// decides what a line is. A thinking block and a text block are separate lines
/// that each carry `end_turn`, so one turn can yield several `EndTurn` marks.
///
/// A candidate line that could not be understood is **skipped and never
/// revisited** — the caller advances its cursor past every complete line,
/// parsed or not. That is a deliberate choice between two bad options: retrying
/// it would re-read the same unreadable bytes on every pass forever, growing the
/// read without ever making progress. The cost of skipping is that the subagent
/// that line was ending stays marked as running until its session's CLI exits,
/// or that a turn's ending goes unseen.
///
/// So it is logged. A spinner that will not stop is a bug someone will report,
/// and this is the line in the log that explains it.
pub(crate) fn scan_chunk(chunk: &str) -> ChunkScan {
    let mut scan = ChunkScan::default();
    for line in chunk.lines() {
        if !is_scan_candidate(line) {
            continue;
        }
        let Some(parsed) = parse_object::<Line>(line.trim()) else {
            log::warn!(
                "a transcript line that may end a tool call or a turn is not a JSON \
                 object that can be read; whatever it ends will read as still running"
            );
            continue;
        };
        let message = parsed.message.0;
        let content = message
            .as_ref()
            .and_then(|message| message.content.0.as_ref());
        let is_sidechain = parsed.is_sidechain.0.unwrap_or(false);
        let kind = parsed.kind.0.as_deref();

        // Not every line mentioning the word carries a result block -- a user
        // message can say "tool_result" in prose -- so an absent one is normal
        // and silent. It is a *result block this code cannot read* that is
        // worth saying something about, and that is the case below.
        let mut has_result_block = false;
        for block in blocks_of(content) {
            if block.kind.0.as_deref() != Some("tool_result") {
                continue;
            }
            has_result_block = true;
            match block.tool_use_id.0.as_deref() {
                Some(id) => scan.tool_use_ids.push(id.to_owned()),
                None => log::warn!(
                    "a tool result carries no readable `tool_use_id`; the \
                     subagent it ends will read as still running"
                ),
            }
        }
        // Before the sidechain skip, like the tool_use_ids above: a subagent
        // resuming a sibling is still a run that started, and the tracker looks
        // for it by sidecar id whichever chain reported it.
        if has_result_block && let Some(task_id) = parsed.tool_use_result.and_then(resumed_agent_id)
        {
            scan.subagent_events
                .push(SubagentEvent::Resumed(sidecar_id(&task_id)));
        }
        if kind == Some("queue-operation") {
            for task_id in
                notified_task_ids(parsed.operation.0.as_deref(), parsed.queued.0.as_deref())
            {
                scan.subagent_events
                    .push(SubagentEvent::Stopped(sidecar_id(task_id)));
            }
            continue;
        }
        if is_sidechain {
            continue;
        }

        match kind {
            Some("assistant") => {
                // On resume the CLI writes a stand-in reply ("No response
                // requested.") under a fresh message id; no model produced it,
                // so it is not the end of anything. A synthetic API-error line
                // is different: it is how a failed request ends the turn.
                let is_synthetic = message
                    .as_ref()
                    .and_then(|message| message.model.0.as_deref())
                    == Some("<synthetic>");
                let is_api_error = parsed.is_api_error_message.0.unwrap_or(false);
                if is_synthetic && !is_api_error {
                    continue;
                }
                // Anything that stops the model other than asking for a tool
                // ends the turn as far as the user is concerned: `max_tokens`
                // and `stop_sequence` leave the agent waiting just as
                // `end_turn` does. `null` is a partial write, not a stop.
                match message
                    .as_ref()
                    .and_then(|message| message.stop_reason.0.as_deref())
                {
                    Some("tool_use") => {
                        scan.turn_marks.push(TurnMark::Working);
                        for block in blocks_of(content) {
                            if block.kind.0.as_deref() != Some("tool_use") {
                                continue;
                            }
                            if let Some(id) = block.id.0.as_deref() {
                                scan.turn_marks.push(TurnMark::ToolCall(Arc::from(id)));
                            }
                        }
                    }
                    Some(_) => scan.turn_marks.push(TurnMark::EndTurn {
                        message_id: message
                            .as_ref()
                            .and_then(|message| message.id.0.as_deref())
                            .map(Arc::from),
                    }),
                    None => {}
                }
            }
            Some("system") => {
                if parsed.subtype.0.as_deref() == Some("turn_duration") {
                    scan.turn_marks.push(TurnMark::TurnDuration {
                        background_pending: parsed
                            .pending_background_agents
                            .0
                            .is_some_and(|count| count > 0),
                    });
                }
            }
            Some("user") => {
                let flagged = |flag: &Lenient<bool>| flag.0.unwrap_or(false);
                // A compaction summary is the CLI restating history, not the
                // user asking.
                let is_meta = flagged(&parsed.is_meta) || flagged(&parsed.is_compact_summary);
                if let Some(mark) = user_mark(is_meta, content, has_result_block) {
                    scan.turn_marks.push(mark);
                }
            }
            _ => {}
        }
    }
    scan
}

/// The blocks of a list-shaped `content`, skipping any that could not be read.
fn blocks_of(content: Option<&Content>) -> impl Iterator<Item = &Block> {
    match content {
        Some(Content::Blocks(blocks)) => blocks.as_slice(),
        Some(Content::Text(_)) | None => &[],
    }
    .iter()
    .filter_map(|block| block.0.as_ref())
}

/// The task a result says it resumed, when `toolUseResult` is an object that
/// names one. Any other shape of that field is simply not a resume.
fn resumed_agent_id(result: &RawValue) -> Option<String> {
    #[derive(Deserialize)]
    struct Resumed {
        #[serde(rename = "resumedAgentId", default)]
        task_id: Lenient<String>,
    }
    parse_object::<Resumed>(result.get())?.task_id.0
}

/// The sidecar file stem Claude Code gives a subagent: `agent-` and its task id.
fn sidecar_id(task_id: &str) -> Arc<str> {
    Arc::from(format!("agent-{task_id}"))
}

/// The tasks a queued `<task-notification>` enqueue is about -- one per block,
/// since a single enqueue may batch several. Only an `enqueue` counts: the same
/// text reappears on `remove`, in attachments and in prose, and counting those
/// would end a resumed run the moment it restarted.
fn notified_task_ids<'a>(operation: Option<&str>, content: Option<&'a str>) -> Vec<&'a str> {
    if operation != Some("enqueue") {
        return Vec::new();
    }
    let Some(content) = content else {
        return Vec::new();
    };
    if !content.trim_start().starts_with("<task-notification>") {
        return Vec::new();
    }
    content
        .split("<task-notification>")
        .skip(1)
        .filter_map(|block| {
            let task_id = between(block, "<task-id>", "</task-id>").map(str::trim);
            if task_id.is_none_or(str::is_empty) {
                log::warn!(
                    "a task notification names no task; the subagent it ends will read as still running"
                );
                return None;
            }
            task_id
        })
        .collect()
}

const INTERRUPT_MARKER: &str = "[Request interrupted by user";

fn is_scan_candidate(line: &str) -> bool {
    [
        r#""tool_result""#,
        r#""type":"user""#,
        r#""stop_reason":""#,
        r#""subtype":"turn_duration""#,
        r#""operation":"enqueue""#,
    ]
    .iter()
    .any(|needle| line.contains(needle))
}

/// How many lines at the top of a transcript are searched for the command that
/// opened it. The command is among the first few, ahead of nothing but the
/// mode lines, an attachment and the caveat.
const CLEAR_SEARCH_LINES: usize = 16;

/// The names a command that clears the conversation is recorded under.
const CLEAR_COMMANDS: [&str; 3] = ["/clear", "/reset", "/new"];

/// Whether the top of a transcript records the conversation having been
/// cleared: a *user* line whose text starts with the command's own tag.
///
/// A substring search would take any session that read this source, a fixture,
/// or a prompt quoting the command for a continuation. What the CLI writes is a
/// user line that begins with the tag, within the first few lines of the file,
/// and nothing else qualifies. `head` may end in a partial line, which is
/// simply not read.
pub(crate) fn opens_with_clear(head: &str) -> bool {
    head.lines().take(CLEAR_SEARCH_LINES).any(records_a_clear)
}

fn records_a_clear(line: &str) -> bool {
    if !line.contains("<command-name>") {
        return false;
    }
    let Some(parsed) = parse_object::<Line>(line.trim()) else {
        return false;
    };
    if parsed.kind.0.as_deref() != Some("user") {
        return false;
    }
    let content = parsed
        .message
        .0
        .as_ref()
        .and_then(|message| message.content.0.as_ref());
    let texts: Vec<&str> = match content {
        Some(Content::Text(text)) => vec![text.as_str()],
        Some(Content::Blocks(_)) => blocks_of(content)
            .filter(|block| block.kind.0.as_deref() == Some("text"))
            .filter_map(|block| block.text.0.as_deref())
            .collect(),
        None => return false,
    };
    texts.iter().any(|text| {
        let text = text.trim_start();
        (text.starts_with("<command-name>") || text.starts_with("<command-message>"))
            && between(text, "<command-name>", "</command-name>")
                .map(str::trim)
                .is_some_and(|name| CLEAR_COMMANDS.contains(&name))
    })
}

/// An interrupt outranks a prompt: the CLI writes it as a user line, and it is
/// the user stopping the turn rather than starting one.
fn user_mark(is_meta: bool, content: Option<&Content>, has_result_block: bool) -> Option<TurnMark> {
    let texts: Vec<&str> = match content? {
        Content::Text(text) => vec![text.as_str()],
        Content::Blocks(_) => blocks_of(content)
            .filter(|block| block.kind.0.as_deref() == Some("text"))
            .filter_map(|block| block.text.0.as_deref())
            .collect(),
    };
    // At the start, not anywhere: the user quoting the marker in a prompt is
    // still a prompt.
    if texts
        .iter()
        .any(|text| text.trim_start().starts_with(INTERRUPT_MARKER))
    {
        return Some(TurnMark::Interrupt);
    }
    // Slash-command bookkeeping is written under the user's name but is not
    // the user starting a turn; the work the command causes announces itself.
    let spoken = texts.iter().any(|text| !is_bookkeeping(text));
    (!is_meta && !has_result_block && spoken).then_some(TurnMark::Prompt)
}

enum EntryKind {
    AiTitle(String),
    CustomTitle(String),
    Message {
        speaker: Speaker,
        text: Option<String>,
    },
    Other,
}

struct Entry {
    kind: EntryKind,
    cwd: Option<PathBuf>,
    branch: Option<String>,
    model: Option<String>,
}

fn parse_line(line: &str) -> Option<Entry> {
    let line = line.trim();
    if line.is_empty() || !line.starts_with('{') {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let kind = match value.get("type").and_then(Value::as_str)? {
        "ai-title" => {
            let title = value.get("aiTitle").and_then(Value::as_str)?;
            EntryKind::AiTitle(one_line(title))
        }
        "custom-title" => {
            let title = value.get("customTitle").and_then(Value::as_str)?;
            EntryKind::CustomTitle(one_line(title))
        }
        role @ ("user" | "assistant") => {
            let speaker = if role == "user" {
                Speaker::User
            } else {
                Speaker::Agent
            };
            let text = value
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(message_text);
            let text = match speaker {
                Speaker::User => text.and_then(|text| {
                    spoken_by_the_user(text, value.get("isMeta").and_then(Value::as_bool))
                }),
                Speaker::Agent => text,
            };
            EntryKind::Message { speaker, text }
        }
        _ => EntryKind::Other,
    };
    Some(Entry {
        kind,
        cwd: value.get("cwd").and_then(Value::as_str).map(PathBuf::from),
        branch: value
            .get("gitBranch")
            .and_then(Value::as_str)
            .filter(|branch| !branch.is_empty())
            .map(str::to_owned),
        model: value
            .get("message")
            .and_then(|message| message.get("model"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// `content` is either a bare string or an array of blocks. Only text blocks
/// carry anything a human wants to read; tool calls and their results are noise
/// in a two-line preview.
fn message_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(one_line(text)),
        Value::Array(blocks) => {
            let mut out = String::new();
            for block in blocks {
                if block.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(text) = block.get("text").and_then(Value::as_str)
                {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(text);
                }
            }
            (!out.trim().is_empty()).then(|| one_line(&out))
        }
        _ => None,
    }
}

/// Tags Claude Code opens a line of its own scaffolding with. Matched at the
/// start of the text rather than anywhere inside it, so prose that quotes one of
/// them is still prose.
const SCAFFOLDING: [&str; 3] = [
    "<local-command-caveat>",
    "<local-command-stdout>",
    "<task-notification>",
];

/// Every tag a line the CLI wrote on the user's behalf opens with, for deciding
/// whether a line starts a turn. Wider than [`SCAFFOLDING`] because the
/// invocation of a slash command is something the user did type, and a title
/// can be made of it, yet it is not a message to the agent.
fn is_bookkeeping(text: &str) -> bool {
    let text = text.trim_start();
    SCAFFOLDING.iter().any(|tag| text.starts_with(tag))
        || text.starts_with("<command-name>")
        || text.starts_with("<command-message>")
}

/// What a `"type":"user"` line says, or `None` when the person did not say it.
///
/// Several kinds of line are written under the user's name that the user never
/// typed: the caveat inserted before a local command is replayed, that command's
/// stdout, a notification that a subagent finished, and the body of a skill
/// expanded on their behalf. They are indistinguishable from speech to anything
/// that reads the role alone, and 25 of the 32 transcripts this was written
/// against opened on one of them -- which is how a list of sessions comes to be
/// titled `<local-command-caveat>Caveat: The messages below...`.
fn spoken_by_the_user(text: String, is_meta: Option<bool>) -> Option<String> {
    // `isMeta` is the CLI's own mark for a line it wrote in the user's place.
    // It covers the caveat and the expanded skill body; it does not cover the
    // command itself or its output, which are marked by nothing at all.
    if is_meta == Some(true) {
        return None;
    }
    if SCAFFOLDING.iter().any(|tag| text.starts_with(tag)) {
        return None;
    }
    if text.starts_with("<command-name>") || text.starts_with("<command-message>") {
        return invoked_command(&text);
    }
    Some(text)
}

/// `/debug-code why is this slow` out of the tags a slash command is recorded
/// as. `None` when the command carried no arguments: `/clear` and `/model` are
/// housekeeping, and a session named after one says nothing about itself.
///
/// A line with no `<command-args>` tag at all reads the same as one whose tag is
/// empty. Every command seen on disk writes the tag either way, so the two cases
/// are the same case today — but if the CLI ever stops writing it, every slash
/// command goes quiet here rather than loudly wrong, and that is the failure
/// worth knowing about in advance.
fn invoked_command(text: &str) -> Option<String> {
    let arguments = between(text, "<command-args>", "</command-args>")?.trim();
    if arguments.is_empty() {
        return None;
    }
    match between(text, "<command-name>", "</command-name>") {
        Some(name) => Some(format!("{} {arguments}", name.trim())),
        None => Some(arguments.to_owned()),
    }
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    text.get(start..end)
}

/// Collapse to a single line and drop control characters. The preview goes into
/// a two-line label, and a transcript can carry raw terminal output.
fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = false;
    for c in text.chars() {
        let c = if c.is_control() { ' ' } else { c };
        if c == ' ' {
            if !last_was_space && !out.is_empty() {
                out.push(' ');
            }
            last_was_space = true;
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const TAIL: &str = r#"
{"type":"mode","mode":"default","sessionId":"s1"}
{"type":"user","message":{"role":"user","content":"first thing"},"cwd":"/w/one","gitBranch":"main","sessionId":"s1"}
not json at all
{"type":"ai-title","aiTitle":"An early title","sessionId":"s1"}
{"type":"assistant","message":{"role":"assistant","model":"claude-opus-5","content":[{"type":"tool_use","name":"Read"},{"type":"text","text":"the last\nword"}]},"cwd":"/w/one","gitBranch":"feat/x","sessionId":"s1"}
{"type":"ai-title","aiTitle":"The final title","sessionId":"s1"}
"#;

    #[test]
    fn the_last_ai_title_wins_and_the_rest_comes_from_the_last_message() {
        let facts = parse_tail(TAIL);
        assert_eq!(facts.title.as_deref(), Some("The final title"));
        assert_eq!(facts.preview.as_deref(), Some("the last word"));
        assert_eq!(facts.preview_speaker, Some(Speaker::Agent));
        assert_eq!(facts.cwd, Some(PathBuf::from("/w/one")));
        // The branch of the *last* line that carried one, not the first.
        assert_eq!(facts.branch.as_deref(), Some("feat/x"));
        assert_eq!(facts.model.as_deref(), Some("claude-opus-5"));
    }

    /// The opening of a real session, byte for byte in shape: a caveat the CLI
    /// marked as its own, the slash command the person ran, and that command's
    /// stdout. Not one of the three is speech, and the first of them used to be
    /// the session's title.
    const A_SESSION_THAT_OPENS_ON_SCAFFOLDING: &str = r#"
{"type":"mode","mode":"default","sessionId":"s1"}
{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>Caveat: The messages below were generated by the user while running local commands."},"cwd":"/w/one"}
{"type":"user","message":{"role":"user","content":"<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args></command-args>"},"cwd":"/w/one"}
{"type":"user","message":{"role":"user","content":"<local-command-stdout>Set model to Opus 5</local-command-stdout>"},"cwd":"/w/one"}
"#;

    #[test]
    fn nothing_the_cli_wrote_under_the_users_name_counts_as_a_prompt() {
        let facts = parse_head(A_SESSION_THAT_OPENS_ON_SCAFFOLDING);
        assert_eq!(
            facts.first_user_message, None,
            "a caveat, a bare slash command and its stdout are all the CLI talking to itself"
        );
        assert_eq!(
            facts.cwd,
            Some(PathBuf::from("/w/one")),
            "the cwd is still read off the lines that were skipped"
        );
        assert_eq!(
            parse_tail(A_SESSION_THAT_OPENS_ON_SCAFFOLDING).preview,
            None,
            "what is not a title is not a preview either"
        );
    }

    #[test]
    fn a_slash_command_is_named_by_what_was_typed_after_it() {
        let head = parse_head(
            r#"{"type":"user","message":{"role":"user","content":"<command-message>debug-code</command-message>\n<command-name>/debug-code</command-name>\n<command-args>the titles are wrong</command-args>"}}"#,
        );
        assert_eq!(
            head.first_user_message.as_deref(),
            Some("/debug-code the titles are wrong"),
            "the arguments are the part the person typed, and the name says which command took them"
        );
    }

    #[test]
    fn a_task_notification_is_not_the_user_speaking() {
        let head = parse_head(
            r#"{"type":"user","message":{"role":"user","content":"<task-notification> <task-id>abc</task-id> agent finished"}}"#,
        );
        assert_eq!(head.first_user_message, None);
    }

    /// The guard is anchored at the start of the text on purpose: someone asking
    /// about one of these tags is asking a real question.
    #[test]
    fn prose_that_quotes_a_tag_is_still_prose() {
        let head = parse_head(
            r#"{"type":"user","message":{"role":"user","content":"why does <local-command-stdout> show up in my titles"}}"#,
        );
        assert_eq!(
            head.first_user_message.as_deref(),
            Some("why does <local-command-stdout> show up in my titles")
        );
    }

    #[test]
    fn the_title_the_user_typed_outranks_the_one_that_was_generated() {
        let facts = parse_tail(concat!(
            r#"{"type":"ai-title","aiTitle":"Generated later"}"#,
            "\n",
            r#"{"type":"custom-title","customTitle":"Renamed by hand"}"#,
            "\n",
            r#"{"type":"ai-title","aiTitle":"Generated later still"}"#,
        ));
        assert_eq!(facts.custom_title.as_deref(), Some("Renamed by hand"));
        assert_eq!(
            facts.title.as_deref(),
            Some("Generated later still"),
            "both are read; which one wins is the caller's decision"
        );
    }

    /// The two lines that bracket a subagent, copied from a finished session on
    /// disk: the `tool_use` that spawned it and the `tool_result` that ended it,
    /// under one `toolUseId`. Only the second is an ending, and reading the
    /// first as one would mark every subagent finished the moment it started.
    const SUBAGENT_BRACKET: &str = r#"
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_started","name":"Task"}]}}
{"type":"attachment","attachment":{}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_finished","is_error":false,"content":"done"}]}}
"#;

    #[test]
    fn only_a_result_ends_a_subagent_not_the_call_that_started_it() {
        assert_eq!(
            scan_chunk(SUBAGENT_BRACKET).tool_use_ids,
            vec!["toolu_finished"]
        );
    }

    #[test]
    fn a_transcript_with_no_results_ends_nothing() {
        assert!(scan_chunk(TAIL).tool_use_ids.is_empty());
        assert!(scan_chunk("").tool_use_ids.is_empty());
    }

    /// The chosen behaviour on a result nobody can read, pinned so it is a
    /// decision rather than an accident.
    ///
    /// The line is skipped, and the caller will advance its cursor past it and
    /// never look again. The alternative — refusing to advance — re-reads the
    /// same unreadable bytes on every pass forever without ever getting
    /// further, which is worse. The visible cost is that the subagent that line
    /// was ending stays marked running, which is why the skip is logged.
    #[test]
    fn a_result_that_cannot_be_read_is_skipped_and_does_not_stop_the_rest() {
        let mixed = concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_good"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result",}]}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":42}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_after"}]}}"#,
        );
        assert_eq!(
            scan_chunk(mixed).tool_use_ids,
            vec!["toolu_good", "toolu_after"],
            "one unreadable result must not cost the readable ones around it"
        );
    }

    /// The substring guard in front of the parse must not change the answer —
    /// it exists to skip hundred-kilobyte assistant lines, not to drop results.
    #[test]
    fn the_word_alone_is_not_a_result() {
        let decoys = concat!(
            r#"{"type":"user","message":{"role":"user","content":"the tool_result never arrived"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"tool_result"}]}}"#,
        );
        assert!(scan_chunk(decoys).tool_use_ids.is_empty());
    }

    fn marks(chunk: &str) -> Vec<TurnMark> {
        scan_chunk(chunk).turn_marks
    }

    fn tool_call(id: &str) -> TurnMark {
        TurnMark::ToolCall(Arc::from(id))
    }

    const PROMPT: &str = r#"{"type":"user","message":{"role":"user","content":"do the thing"}}"#;
    const WORKING: &str = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_1","name":"Bash"}]}}"#;
    const END_TURN: &str = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"done"}]}}"#;

    fn end_turn(id: Option<&str>) -> TurnMark {
        TurnMark::EndTurn {
            message_id: id.map(Arc::from),
        }
    }

    fn lines(parts: &[&str]) -> String {
        parts.join("\n")
    }

    #[test]
    fn an_end_turn_without_turn_duration_is_the_whole_ending() {
        let chunk = lines(&[
            PROMPT,
            WORKING,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1"}]}}"#,
            END_TURN,
            END_TURN,
        ]);
        assert_eq!(
            marks(&chunk),
            vec![
                TurnMark::Prompt,
                TurnMark::Working,
                tool_call("toolu_1"),
                end_turn(None),
                end_turn(None),
            ]
        );
    }

    #[test]
    fn a_stop_hook_summary_is_ignored_and_turn_duration_is_marked() {
        let chunk = lines(&[
            END_TURN,
            r#"{"type":"system","subtype":"stop_hook_summary","hookCount":1}"#,
            r#"{"type":"system","subtype":"turn_duration","durationMs":1200}"#,
        ]);
        assert_eq!(
            marks(&chunk),
            vec![
                end_turn(None),
                TurnMark::TurnDuration {
                    background_pending: false
                },
            ]
        );
    }

    #[test]
    fn turn_duration_reports_whether_background_work_was_pending() {
        let duration =
            |extra: &str| format!(r#"{{"type":"system","subtype":"turn_duration"{extra}}}"#);
        let pending = |chunk: String| match marks(&chunk).as_slice() {
            [TurnMark::TurnDuration { background_pending }] => *background_pending,
            other => panic!("expected one TurnDuration, got {other:?}"),
        };
        assert!(pending(duration(r#","pendingBackgroundAgentCount":2"#)));
        assert!(!pending(duration(r#","pendingBackgroundAgentCount":0"#)));
        assert!(!pending(duration("")));
    }

    #[test]
    fn an_interrupt_is_marked_in_either_content_form_and_is_not_a_prompt() {
        let string_form = r#"{"type":"user","message":{"role":"user","content":"[Request interrupted by user]"}}"#;
        let block_form = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user for tool use]"}]}}"#;
        let duration = r#"{"type":"system","subtype":"turn_duration"}"#;
        assert_eq!(marks(string_form), vec![TurnMark::Interrupt]);
        assert_eq!(marks(block_form), vec![TurnMark::Interrupt]);
        assert_eq!(
            marks(&lines(&[string_form, duration])),
            vec![
                TurnMark::Interrupt,
                TurnMark::TurnDuration {
                    background_pending: false
                },
            ]
        );
    }

    #[test]
    fn sidechain_lines_make_no_marks_but_their_results_are_still_reported() {
        let chunk = lines(&[
            r#"{"type":"assistant","isSidechain":true,"message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"x"}]}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"toolu_side"}]}}"#,
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"sub prompt"}}"#,
            r#"{"type":"user","isSidechain":true,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_side"}]}}"#,
        ]);
        let ChunkScan {
            tool_use_ids: ids,
            turn_marks: marks,
            ..
        } = scan_chunk(&chunk);
        assert!(marks.is_empty());
        assert_eq!(ids, vec!["toolu_side"]);
    }

    #[test]
    fn meta_and_tool_result_user_lines_are_not_prompts() {
        let meta = r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<command-name>/x</command-name>"}}"#;
        let result = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_r"}]}}"#;
        assert!(marks(meta).is_empty());
        let ChunkScan {
            tool_use_ids: ids,
            turn_marks: marks,
            ..
        } = scan_chunk(result);
        assert_eq!(ids, vec!["toolu_r"]);
        assert!(marks.is_empty());
    }

    #[test]
    fn an_end_turn_carries_the_id_of_the_reply_it_belongs_to() {
        let block = |id: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"id":"{id}","role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"x"}}]}}}}"#
            )
        };
        let chunk = lines(&[&block("msg_a"), &block("msg_a"), &block("msg_b")]);
        assert_eq!(
            marks(&chunk),
            vec![
                end_turn(Some("msg_a")),
                end_turn(Some("msg_a")),
                end_turn(Some("msg_b"))
            ]
        );
    }

    #[test]
    fn any_stop_other_than_a_tool_call_ends_the_turn() {
        let stop = |reason: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"{reason}","content":[{{"type":"text","text":"x"}}]}}}}"#
            )
        };
        assert_eq!(marks(&stop("max_tokens")), vec![end_turn(None)]);
        assert_eq!(marks(&stop("stop_sequence")), vec![end_turn(None)]);
        let partial = r#"{"type":"assistant","message":{"role":"assistant","stop_reason":null,"content":[{"type":"text","text":"x"}]}}"#;
        assert!(marks(partial).is_empty(), "null is a partial write");
    }

    #[test]
    fn the_resume_stand_in_reply_is_not_a_turn_but_an_api_error_is() {
        let resume = lines(&[
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
            r#"{"type":"assistant","message":{"id":"msg_new","model":"<synthetic>","role":"assistant","stop_reason":"stop_sequence","content":[{"type":"text","text":"No response requested."}]}}"#,
        ]);
        assert!(marks(&resume).is_empty());
        let api_error = r#"{"type":"assistant","isApiErrorMessage":true,"message":{"id":"msg_err","model":"<synthetic>","role":"assistant","stop_reason":"stop_sequence","content":[{"type":"text","text":"API Error"}]}}"#;
        assert_eq!(marks(api_error), vec![end_turn(Some("msg_err"))]);
    }

    #[test]
    fn a_compaction_summary_is_not_a_prompt() {
        let summary = r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"This session is being continued from a previous conversation."}}"#;
        assert!(marks(summary).is_empty());
    }

    #[test]
    fn a_prompt_that_only_quotes_the_interrupt_marker_is_a_prompt() {
        let quoting = r#"{"type":"user","message":{"role":"user","content":"why does it print [Request interrupted by user] here"}}"#;
        let padded = r#"{"type":"user","message":{"role":"user","content":"  [Request interrupted by user]"}}"#;
        assert_eq!(marks(quoting), vec![TurnMark::Prompt]);
        assert_eq!(marks(padded), vec![TurnMark::Interrupt]);
    }

    #[test]
    fn slash_command_bookkeeping_is_not_a_prompt() {
        let user = |text: &str| {
            serde_json::json!({
                "type": "user",
                "message": { "role": "user", "content": text }
            })
            .to_string()
        };
        for text in [
            "<command-name>/model</command-name>\n<command-args></command-args>",
            "<command-message>debug</command-message>\n<command-name>/debug</command-name>",
            "<local-command-stdout>Set model</local-command-stdout>",
            "<local-command-caveat>Caveat: ignore</local-command-caveat>",
        ] {
            assert!(marks(&user(text)).is_empty(), "{text}");
        }
        assert_eq!(marks(&user("run the tests")), vec![TurnMark::Prompt]);
    }

    #[test]
    fn a_continuation_after_a_stop_hook_reads_as_working_after_an_end() {
        let chunk = lines(&[END_TURN, WORKING]);
        assert_eq!(
            marks(&chunk),
            vec![end_turn(None), TurnMark::Working, tool_call("toolu_1")]
        );
    }

    #[test]
    fn a_prompt_that_mentions_tool_result_is_a_prompt_with_no_id() {
        let line =
            r#"{"type":"user","message":{"role":"user","content":"why is tool_result missing"}}"#;
        let ChunkScan {
            tool_use_ids: ids,
            turn_marks: marks,
            ..
        } = scan_chunk(line);
        assert!(ids.is_empty());
        assert_eq!(marks, vec![TurnMark::Prompt]);
    }

    #[test]
    fn a_text_block_prompt_is_a_prompt() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"hello"}]}}"#;
        assert_eq!(marks(line), vec![TurnMark::Prompt]);
    }

    #[test]
    fn an_invalid_candidate_line_is_skipped_and_later_lines_still_count() {
        let chunk = lines(&[
            r#"{"type":"assistant","stop_reason":"end_turn" oops"#,
            END_TURN,
        ]);
        assert_eq!(marks(&chunk), vec![end_turn(None)]);
    }

    /// The format belongs to another program, and the scan reads it a field at a
    /// time: a field of the wrong type costs the field, and never the line or
    /// the readable lines around it.
    #[test]
    fn a_field_of_an_unexpected_type_costs_the_field_and_not_the_line() {
        let chunk = lines(&[
            r#"{"type":"assistant","message":{"role":"assistant","stop_reason":5,"id":7,"content":"x"}}"#,
            r#"{"type":"user","isSidechain":null,"toolUseResult":"Error: boom","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_a","content":"Error"}]}}"#,
            r#"{"type":"user","toolUseResult":[1,2,3],"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_b"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":42}}"#,
            r#"{"type":"user","message":"hello"}"#,
            r#"{"type":"user","message":{"role":"user","content":[7,"text",{"type":"text","text":"a real prompt"}]}}"#,
            END_TURN,
        ]);

        let scan = scan_chunk(&chunk);

        assert_eq!(scan.tool_use_ids, vec!["toolu_a", "toolu_b"]);
        assert_eq!(
            scan.turn_marks,
            vec![TurnMark::Prompt, end_turn(None)],
            "a result is not a prompt, content of no known shape says nothing, a block \
             that is not an object is skipped, and the real prompt and reply still count"
        );
        assert!(scan.subagent_events.is_empty());
    }

    /// What dominates a transcript's size, and what the scan never reads: an
    /// image is a base64 string of several megabytes inside the line that
    /// carries the prompt.
    #[test]
    fn a_line_carrying_a_huge_image_is_still_read_for_its_prompt() {
        let image = "A".repeat(3 * 1024 * 1024);
        let line = format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{image}"}}}},{{"type":"text","text":"what is this"}}]}}}}"#
        );
        assert_eq!(marks(&line), vec![TurnMark::Prompt]);
    }

    /// A tool result can carry a file as large as it likes under `toolUseResult`,
    /// and only a `resumedAgentId` in it means anything.
    #[test]
    fn a_resume_is_read_out_of_an_object_that_also_carries_a_large_file() {
        let file = "x".repeat(1024 * 1024);
        let line = format!(
            r#"{{"type":"user","toolUseResult":{{"file":"{file}","resumedAgentId":"abc"}},"message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_r"}}]}}}}"#
        );
        let scan = scan_chunk(&line);
        assert_eq!(scan.tool_use_ids, vec!["toolu_r"]);
        assert_eq!(scan.subagent_events, vec![resumed("abc")]);
    }

    /// A derived struct will also take its fields from a JSON array, in order.
    /// The transcript never writes one that way, and the reading above it used
    /// to find nothing in an array, so an array stays nothing.
    #[test]
    fn a_list_where_an_object_belongs_is_not_read_as_one() {
        let chunk = lines(&[
            r#"{"type":"user","toolUseResult":["x"],"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_a"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[["tool_use","toolu_x"]]}}"#,
            r#"{"type":"user","message":["user","a prompt"]}"#,
            r#"["{\"type\":\"user\"}"]"#,
        ]);

        let scan = scan_chunk(&chunk);

        assert_eq!(scan.tool_use_ids, vec!["toolu_a"]);
        assert!(
            scan.subagent_events.is_empty(),
            "`[\"x\"]` is not a resume of agent-x: {:?}",
            scan.subagent_events
        );
        assert_eq!(
            scan.turn_marks,
            vec![TurnMark::Working],
            "a block that is a list names no tool call, and a message that is a list says nothing"
        );
    }

    const CLEAR_COMMAND: &str = "<command-name>/clear</command-name>\\n            <command-message>clear</command-message>\\n            <command-args></command-args>";

    fn user_line(content: &str) -> String {
        format!(
            r#"{{"type":"user","message":{{"role":"user","content":"{content}"}},"cwd":"/w/one"}}"#
        )
    }

    /// What a `/clear` leaves at the top of the file the CLI starts for it,
    /// read off real transcripts.
    #[test]
    fn a_transcript_that_opens_with_a_clear_command_is_recognised() {
        let head = lines(&[
            r#"{"type":"mode","mode":"default","sessionId":"x"}"#,
            r#"{"type":"file-history-snapshot","snapshot":{}}"#,
            &user_line(
                "<local-command-caveat>Caveat: generated while running local commands</local-command-caveat>",
            ),
            &user_line(CLEAR_COMMAND),
            r#"{"type":"system","subtype":"local_command","content":"<local-command-stdout></local-command-stdout>"}"#,
        ]);
        assert!(opens_with_clear(&head));
    }

    #[test]
    fn the_other_names_for_it_and_the_text_block_form_count_too() {
        for name in ["/reset", "/new"] {
            let head = user_line(&format!(
                "<command-name>{name}</command-name>\\n<command-message>x</command-message>"
            ));
            assert!(opens_with_clear(&head), "{name}");
        }
        let message_first = user_line(
            "<command-message>clear</command-message>\\n<command-name>/clear</command-name>",
        );
        assert!(opens_with_clear(&message_first));
        let blocks = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<command-name>/clear</command-name>"}]}}"#;
        assert!(opens_with_clear(blocks));
    }

    /// The tag is only a clear when a user line *starts* with it. A session
    /// that reads this very file, a fixture, or a prompt that quotes the
    /// command all contain the same text without being what it names.
    #[test]
    fn text_that_merely_contains_the_command_does_not_qualify() {
        let quoted = "<command-name>/clear</command-name>";
        for head in [
            // A tool result that read a file containing it.
            format!(
                r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_a","content":"const CLEAR: &str = \"{quoted}\";"}}]}}}}"#
            ),
            // The assistant saying it.
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"it records {quoted}"}}]}}}}"#
            ),
            // A prompt that quotes it part-way through.
            user_line(&format!("why does the transcript hold {quoted} first?")),
            // Recorded, but not as something the user ran.
            format!(r#"{{"type":"system","subtype":"local_command","content":"{quoted}"}}"#),
            // Another command entirely.
            user_line(
                "<command-name>/model</command-name>\\n<command-message>model</command-message>",
            ),
        ] {
            assert!(!opens_with_clear(&head), "{head}");
        }
    }

    #[test]
    fn only_the_top_of_the_file_is_searched() {
        let mut lines_before = vec![r#"{"type":"mode","mode":"default"}"#.to_string(); 16];
        lines_before.push(user_line(CLEAR_COMMAND));
        assert!(
            !opens_with_clear(&lines_before.join("\n")),
            "a clear deep in a conversation did not open it"
        );
    }

    #[test]
    fn a_head_cut_off_mid_line_still_reads_the_whole_lines_above_the_cut() {
        let head = format!(
            "{}\n{{\"type\":\"user\",\"message\":{{\"role\":\"us",
            user_line(CLEAR_COMMAND)
        );
        assert!(opens_with_clear(&head));
    }

    #[test]
    fn lines_that_are_neither_turn_nor_result_make_nothing() {
        let chunk = lines(&[
            r#"{"type":"attachment","attachment":{}}"#,
            r#"{"type":"mode","mode":"plan"}"#,
            r#"{"type":"system","subtype":"informational"}"#,
        ]);
        assert_eq!(scan_chunk(&chunk), ChunkScan::default());
    }

    #[test]
    fn a_line_that_is_not_json_is_skipped_rather_than_fatal() {
        // Proven by the test above passing over `not json at all`, and by a tail
        // that is nothing but rubbish still parsing to nothing.
        let facts = parse_tail("garbage\n{oops\n\n");
        assert_eq!(facts, TailFacts::default());
    }

    #[test]
    fn with_no_ai_title_the_head_supplies_the_first_user_message() {
        let tail =
            r#"{"type":"assistant","message":{"role":"assistant","content":"hi"},"cwd":"/w/two"}"#;
        assert_eq!(parse_tail(tail).title, None);

        let head = parse_head(TAIL);
        assert_eq!(head.first_user_message.as_deref(), Some("first thing"));
        assert_eq!(head.cwd, Some(PathBuf::from("/w/one")));
    }

    #[test]
    fn only_conversation_lines_count_as_messages() {
        assert!(line_is_message(r#"{"type":"user","message":{}}"#));
        assert!(line_is_message(r#"{"type":"assistant","message":{}}"#));
        assert!(!line_is_message(r#"{"type":"attachment"}"#));
        assert!(!line_is_message(r#"{"type":"ai-title","aiTitle":"x"}"#));
        // The trap: the same text inside message content must not count.
        assert!(!line_is_message(
            r#"{"type":"system","text":"the line \"type\":\"user\" appears here"}"#
        ));
    }

    /// Built with `json!` rather than written out, so the control characters are
    /// escaped the way Claude escapes them. A literal tab inside a JSON string is
    /// not JSON at all — `serde_json` rejects the line, which is the behaviour the
    /// skip-the-bad-line rule already covers.
    #[test]
    fn control_characters_in_a_preview_are_flattened() {
        let line = serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": "ab\tc\n\nd  e" }
        })
        .to_string();
        assert_eq!(parse_tail(&line).preview.as_deref(), Some("ab c d e"));
    }

    const LAUNCH_RESULT: &str = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_bg","type":"tool_result","content":[{"type":"text","text":"Async agent launched successfully."}]}]},"toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"a254ef155e0d05a00"}}"#;
    const RESUME_RESULT: &str = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_resume","type":"tool_result","content":[{"type":"text","text":"resumed"}]}]},"toolUseResult":{"success":true,"message":"Resuming agent a1ea775","resumedAgentId":"a254ef155e0d05a00"}}"#;

    fn notification(task_id: &str, status: &str) -> String {
        format!(
            r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-10-05T13:27:22.560Z","sessionId":"s","content":"<task-notification>\n<task-id>{task_id}</task-id>\n<tool-use-id>toolu_bg</tool-use-id>\n<output-file>/tmp/{task_id}.output</output-file>\n<status>{status}</status>\n<summary>Agent finished</summary>\n</task-notification>"}}"#
        )
    }

    fn stopped(task_id: &str) -> SubagentEvent {
        SubagentEvent::Stopped(Arc::from(format!("agent-{task_id}")))
    }

    fn resumed(task_id: &str) -> SubagentEvent {
        SubagentEvent::Resumed(Arc::from(format!("agent-{task_id}")))
    }

    #[test]
    fn a_background_launch_result_and_its_notification_both_register() {
        let chunk = lines(&[
            LAUNCH_RESULT,
            &notification("a254ef155e0d05a00", "completed"),
        ]);
        let scan = scan_chunk(&chunk);
        assert_eq!(scan.tool_use_ids, vec!["toolu_bg"]);
        assert_eq!(scan.subagent_events, vec![stopped("a254ef155e0d05a00")]);
    }

    #[test]
    fn a_resume_sits_between_two_stops_in_file_order() {
        let chunk = lines(&[
            &notification("a254ef155e0d05a00", "completed"),
            RESUME_RESULT,
            &notification("a254ef155e0d05a00", "completed"),
        ]);
        assert_eq!(
            scan_chunk(&chunk).subagent_events,
            vec![
                stopped("a254ef155e0d05a00"),
                resumed("a254ef155e0d05a00"),
                stopped("a254ef155e0d05a00")
            ]
        );
    }

    #[test]
    fn every_terminal_status_ends_the_run() {
        for status in ["completed", "failed", "killed", "stopped", "something-new"] {
            assert_eq!(
                scan_chunk(&notification("abc", status)).subagent_events,
                vec![stopped("abc")],
                "{status}"
            );
        }
    }

    #[test]
    fn copies_of_a_notification_outside_an_enqueue_line_are_ignored() {
        let text = r#"<task-notification>\n<task-id>abc</task-id>\n<status>completed</status>\n</task-notification>"#;
        let chunk = lines(&[
            &format!(r#"{{"type":"queue-operation","operation":"remove","content":"{text}"}}"#),
            &format!(
                r#"{{"type":"attachment","attachment":{{"type":"queued_command","prompt":"{text}"}}}}"#
            ),
            &format!(r#"{{"type":"user","message":{{"role":"user","content":"{text}"}}}}"#),
            &format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"{text}"}}]}}}}"#
            ),
        ]);
        assert!(scan_chunk(&chunk).subagent_events.is_empty());
    }

    #[test]
    fn a_foreground_result_makes_no_event_and_a_send_message_is_not_a_resume() {
        let send_message = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_sm","type":"tool_result","content":"queued"}]},"toolUseResult":{"success":true,"message":"Message queued"}}"#;
        let chunk = lines(&[SUBAGENT_BRACKET.trim(), send_message]);
        let scan = scan_chunk(&chunk);
        assert_eq!(scan.tool_use_ids, vec!["toolu_finished", "toolu_sm"]);
        assert!(scan.subagent_events.is_empty());
    }

    #[test]
    fn a_background_bash_notification_yields_an_id_no_subagent_has() {
        assert_eq!(
            scan_chunk(&notification("bq1w2e3r4", "completed")).subagent_events,
            vec![stopped("bq1w2e3r4")]
        );
    }

    #[test]
    fn a_notification_missing_its_task_id_is_skipped() {
        let line = r#"{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\n<status>completed</status>\n</task-notification>"}"#;
        assert!(scan_chunk(line).subagent_events.is_empty());
    }

    #[test]
    fn every_notification_block_in_one_enqueue_ends_its_own_task() {
        let line = r#"{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\n<task-id>aaa</task-id>\n<status>completed</status>\n</task-notification>\n<task-notification>\n<task-id>bbb</task-id>\n<status>failed</status>\n</task-notification>"}"#;
        assert_eq!(
            scan_chunk(line).subagent_events,
            vec![stopped("aaa"), stopped("bbb")]
        );
    }

    #[test]
    fn a_block_without_a_task_id_does_not_hide_the_blocks_around_it() {
        let line = r#"{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\n<task-id>aaa</task-id>\n</task-notification>\n<task-notification>\n<status>completed</status>\n</task-notification>\n<task-notification>\n<task-id>ccc</task-id>\n</task-notification>"}"#;
        assert_eq!(
            scan_chunk(line).subagent_events,
            vec![stopped("aaa"), stopped("ccc")]
        );
    }

    #[test]
    fn a_resume_reported_on_a_sidechain_line_still_counts() {
        let line = RESUME_RESULT.replacen(
            r#"{"type":"user","#,
            r#"{"type":"user","isSidechain":true,"#,
            1,
        );
        let scan = scan_chunk(&line);
        assert_eq!(scan.subagent_events, vec![resumed("a254ef155e0d05a00")]);
        assert!(scan.turn_marks.is_empty());
    }
}
