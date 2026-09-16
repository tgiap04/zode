//! Reading Claude Code's JSONL transcripts.
//!
//! The format is undocumented and belongs to another program, so every field
//! here degrades on its own: a missing `ai-title` falls back to the first user
//! message, a missing `cwd` leaves the session ungrouped, an unparsable line is
//! skipped. A format change must cost a column, never the panel.
//!
//! Everything in this module takes `&str` rather than a path. That keeps the
//! parse decisions testable without a filesystem, and keeps them in one place.

use crate::Speaker;
use serde_json::Value;
use std::path::PathBuf;

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

/// The tool calls this stretch of transcript reports a result for.
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
/// Same substring guard as [`line_is_message`], for the same reason -- most
/// lines here are assistant messages that can run to hundreds of kilobytes, and
/// parsing all of them to find the few that carry a result costs the budget.
/// A line that said `tool_result` and could not be understood is **skipped and
/// never revisited** — the caller advances its cursor past every complete line,
/// parsed or not. That is a deliberate choice between two bad options: retrying
/// it would re-read the same unreadable bytes on every pass forever, growing the
/// read without ever making progress. The cost of skipping is that the subagent
/// that line was ending stays marked as running until its session's CLI exits.
///
/// So it is logged. A spinner that will not stop is a bug someone will report,
/// and this is the line in the log that explains it.
pub(crate) fn completed_tool_uses(chunk: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for line in chunk.lines() {
        if !line.contains(r#""tool_result""#) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            log::warn!(
                "a transcript line naming a tool result is not valid JSON; the \
                 subagent it ends will read as still running"
            );
            continue;
        };
        let blocks = value
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array);
        // Not every line mentioning the word carries a result block -- a user
        // message can say "tool_result" in prose -- so an absent one is normal
        // and silent. It is a *result block this code cannot read* that is
        // worth saying something about, and that is the case below.
        let Some(blocks) = blocks else {
            continue;
        };
        for block in blocks {
            if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            match block.get("tool_use_id").and_then(Value::as_str) {
                Some(id) => ids.push(id.to_owned()),
                None => log::warn!(
                    "a tool result carries no readable `tool_use_id`; the \
                     subagent it ends will read as still running"
                ),
            }
        }
    }
    ids
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
            completed_tool_uses(SUBAGENT_BRACKET),
            vec!["toolu_finished"]
        );
    }

    #[test]
    fn a_transcript_with_no_results_ends_nothing() {
        assert!(completed_tool_uses(TAIL).is_empty());
        assert!(completed_tool_uses("").is_empty());
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
            completed_tool_uses(mixed),
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
        assert!(completed_tool_uses(decoys).is_empty());
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
}
