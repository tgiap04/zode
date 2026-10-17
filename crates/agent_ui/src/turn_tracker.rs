//! Folding a transcript's turn landmarks into "a turn started / ended".
//!
//! The pty cannot say a turn is over: its write rate is the same for a model
//! that is thinking, a tool that is silent for forty seconds and a user who is
//! typing. The transcript can, because the CLI writes a line when a turn ends.
//! What it writes is messier than one marker, though, so this is a small state
//! machine rather than a flag — it exists to turn several overlapping facts
//! into exactly one [`TurnEvent::Ended`] per turn.
//!
//! Pure on purpose: no gpui, no clock, no files. Everything it needs arrives as
//! arguments, which is what lets every rule below be pinned by a test.

use agent_sessions::TurnMark;
use collections::HashSet;
use std::sync::Arc;

/// A change in a tab's turn, reported once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnEvent {
    /// The agent began working on something.
    Started,
    /// The agent finished the turn and is waiting for the user.
    Ended,
    /// The user cut the turn short. Always accompanies the end of the turn,
    /// and means "do not announce it".
    Interrupted,
    /// A tool call is waiting on the user's permission.
    ApprovalNeeded,
    /// That wait is over, whichever way it was answered.
    ApprovalCleared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Phase {
    /// Nothing has been seen yet.
    #[default]
    Idle,
    Running,
    /// The reply is written but the store has not yet said the turn is closed.
    /// Only reachable once a transcript has shown it closes turns explicitly.
    Answered,
    Ended,
    /// The turn closed while work it started was still running in the
    /// background. Nobody has been told it ended: when that work reports back
    /// the agent starts a follow-up turn, and that one announces itself.
    Held,
}

/// How many passes a reply may wait for the line that closes its turn before
/// the turn is ended without it.
///
/// A pass is one transcript scan, about a second apart. The closing line
/// normally lands in the same pass as the reply or the next one; waiting for it
/// is only to give a blocking stop hook time to send the agent back to work.
/// Without a bound, a hook that hangs, or a CLI that stopped writing the line
/// after the transcript had shown it once, would leave the turn open for good.
const ANSWERED_PATIENCE_PASSES: u32 = 8;

/// How long every background agent of a held turn may go without writing
/// before the turn is taken to have been abandoned and is ended.
///
/// A held turn ends on its own when a later turn closes with nothing pending;
/// this is only the way out for an agent that hung or died, which never lets
/// that happen. Ten minutes because an agent that is alive is not silent for
/// long -- writes within one run were measured up to 165 seconds apart -- and
/// being late costs one delayed notification where being early announces a
/// turn that is still going on.
const BACKGROUND_QUIET_LIMIT: std::time::Duration = std::time::Duration::from_secs(600);

/// How many consecutive passes a held turn may have nothing to judge its
/// background agents by before it is ended anyway, about thirty minutes.
///
/// With no subagent file to read there is no silence to measure, so
/// [`BACKGROUND_QUIET_LIMIT`] can never be reached and the turn would stay open
/// for as long as the tab does. Far longer than any realistic wait, because
/// being late costs one delayed notification and being early announces a turn
/// that is still going on.
const HELD_UNJUDGED_PASSES: u32 = 1800;

/// What one pass over the transcript hands the tracker.
pub struct TurnPass<'a> {
    pub marks: &'a [TurnMark],
    /// The tool calls the same pass saw a result for. They are applied after
    /// the marks so a call and its result inside one chunk resolve to "not
    /// pending", whichever order the file wrote them.
    pub finished: &'a [Arc<str>],
    /// False for the pass that reads the past: a resumed session is full of
    /// turns that ended long ago and none of them are news. State still
    /// advances, so the turn that is in progress when the baseline stops is
    /// picked up from where it stands.
    pub live: bool,
    /// How long since any subagent last wrote to its own transcript, or `None`
    /// when there is nothing to judge by. A plain fact rather than a clock, so
    /// the tracker stays testable. Not whether a subagent's call has a result:
    /// a background one gets that within a second of starting and works on.
    pub background_quiet_for: Option<std::time::Duration>,
}

#[derive(Default)]
pub struct TurnTracker {
    phase: Phase,
    /// Tool calls made in the current turn that no result has been seen for.
    open_calls: HashSet<Arc<str>>,
    /// Whether this transcript has ever recorded a turn as closed by its own
    /// line. Once it has, a reply is not the end of the turn: hooks that run
    /// between the reply and that line can still make the agent carry on, and
    /// announcing at the reply would announce a turn that then continues.
    closes_explicitly: bool,
    /// Whether the current `Ended` was announced off a reply alone, with the
    /// store's closing line still to come. Only then can that line retract it.
    ended_on_reply: bool,
    /// The reply the last `end_turn` block belonged to. A turn that opens with
    /// no prompt of its own -- the agent answering a subagent's hand-back or a
    /// slash command, neither of which is a prompt -- is recognised by a reply
    /// this has not seen, and a second block of the same reply is recognised by
    /// one it has.
    last_reply: Option<Arc<str>>,
    /// Passes spent in `Answered` or `Held` since entering it, not counting the
    /// one that entered it: that pass is what the wait is for, not part of it.
    idle_passes: u32,
    entered_this_pass: bool,
    awaiting_approval: bool,
}

impl TurnTracker {
    /// Folds one pass over the transcript.
    pub fn apply(&mut self, pass: TurnPass<'_>) -> Vec<TurnEvent> {
        let mut events = Vec::new();
        for mark in pass.marks {
            match mark {
                TurnMark::Prompt => {
                    // A prompt supersedes whatever the previous turn left
                    // outstanding; a call it never got a result for belongs to
                    // a turn that is over.
                    self.open_calls.clear();
                    self.begin(&mut events);
                }
                TurnMark::Working => self.begin(&mut events),
                TurnMark::ToolCall(id) => {
                    self.begin(&mut events);
                    self.open_calls.insert(id.clone());
                }
                TurnMark::EndTurn { message_id } => self.reply(message_id, &mut events),
                TurnMark::TurnDuration { background_pending } => {
                    self.closes_explicitly = true;
                    // The first turn of a transcript is announced off its reply,
                    // before anything has shown that a closing line exists. If
                    // that line then says work is still running in the
                    // background, the announcement was early. `Started` is what
                    // takes it back: a subscriber holding the `Ended` back
                    // (as the notifier does, briefly) drops it on seeing the
                    // turn start again, and the follow-up turn announces itself.
                    if self.phase == Phase::Ended && self.ended_on_reply && *background_pending {
                        self.enter(Phase::Held);
                        events.push(TurnEvent::Started);
                    }
                    if matches!(self.phase, Phase::Running | Phase::Answered) {
                        if *background_pending {
                            self.enter(Phase::Held);
                            self.open_calls.clear();
                        } else {
                            self.end(&mut events);
                        }
                    }
                }
                TurnMark::Interrupt => {
                    if self.phase != Phase::Ended {
                        self.end(&mut events);
                        events.push(TurnEvent::Interrupted);
                    }
                }
            }
        }
        for id in pass.finished {
            self.open_calls.remove(id);
        }
        if !pass.live {
            // Nothing is running that this editor started, so a turn the past
            // left waiting on a closing line or on background agents will never
            // get either. Leaving it open would end it, loudly, a few passes
            // after the tab opened.
            if matches!(self.phase, Phase::Answered | Phase::Held) {
                self.phase = Phase::Ended;
                self.ended_on_reply = false;
            }
            return Vec::new();
        }
        self.wait_out_the_closing_line(pass.background_quiet_for, &mut events);
        events
    }

    /// One `end_turn` block. `message_id` is the reply it belongs to.
    fn reply(&mut self, message_id: &Option<Arc<str>>, events: &mut Vec<TurnEvent>) {
        let is_new_reply = message_id.is_some() && *message_id != self.last_reply;
        if message_id.is_some() {
            self.last_reply = message_id.clone();
        }
        match self.phase {
            Phase::Running => self.close_reply(events),
            // The reply is already being waited out.
            Phase::Answered => {}
            Phase::Idle | Phase::Ended | Phase::Held => {
                // A reply nobody opened a turn for: the model answering
                // something that was not a prompt. A block of the reply that
                // just ended it is not that.
                if is_new_reply {
                    self.begin(events);
                    self.close_reply(events);
                }
            }
        }
    }

    fn close_reply(&mut self, events: &mut Vec<TurnEvent>) {
        if self.closes_explicitly {
            self.enter(Phase::Answered);
        } else {
            self.end(events);
            self.ended_on_reply = true;
        }
    }

    /// Ends a turn that has been left waiting too long, in the two states
    /// whose way out is something the transcript may never write.
    fn wait_out_the_closing_line(
        &mut self,
        background_quiet_for: Option<std::time::Duration>,
        events: &mut Vec<TurnEvent>,
    ) {
        if std::mem::take(&mut self.entered_this_pass) {
            return;
        }
        match self.phase {
            // Waiting on background agents has no deadline of its own: only a
            // later turn ends it, or proof that nothing is alive to start one.
            Phase::Held => match background_quiet_for {
                Some(quiet) => {
                    self.idle_passes = 0;
                    if quiet >= BACKGROUND_QUIET_LIMIT {
                        self.end(events);
                    }
                }
                None => {
                    self.idle_passes += 1;
                    if self.idle_passes >= HELD_UNJUDGED_PASSES {
                        self.end(events);
                    }
                }
            },
            Phase::Answered => {
                self.idle_passes += 1;
                if self.idle_passes >= ANSWERED_PATIENCE_PASSES {
                    self.end(events);
                }
            }
            Phase::Idle | Phase::Running | Phase::Ended => {}
        }
    }

    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.idle_passes = 0;
        self.entered_this_pass = true;
    }

    fn begin(&mut self, events: &mut Vec<TurnEvent>) {
        self.ended_on_reply = false;
        // From `Answered` the turn never stopped as far as anyone was told, so
        // there is nothing to announce starting.
        if matches!(self.phase, Phase::Idle | Phase::Ended | Phase::Held) {
            events.push(TurnEvent::Started);
        }
        self.enter(Phase::Running);
    }

    fn end(&mut self, events: &mut Vec<TurnEvent>) {
        self.ended_on_reply = false;
        self.enter(Phase::Ended);
        self.open_calls.clear();
        events.push(TurnEvent::Ended);
    }

    /// Whether the agent has made a tool call it has not had the result of.
    /// The only state in which it can be waiting on a permission dialog.
    pub fn tool_pending(&self) -> bool {
        self.phase == Phase::Running && !self.open_calls.is_empty()
    }

    pub fn awaiting_approval(&self) -> bool {
        self.awaiting_approval
    }

    /// Records whether the screen shows a permission dialog, reporting only
    /// the change.
    pub fn set_awaiting_approval(&mut self, awaiting: bool) -> Option<TurnEvent> {
        if self.awaiting_approval == awaiting {
            return None;
        }
        self.awaiting_approval = awaiting;
        Some(if awaiting {
            TurnEvent::ApprovalNeeded
        } else {
            TurnEvent::ApprovalCleared
        })
    }
}

/// Whether the terminal's visible tail is Claude's permission dialog.
///
/// A question line ("Do you want to proceed?", "Do you want to make this edit
/// to …", "Do you want to create …") with a first option, `1. Yes`, somewhere
/// below it. Both, because either alone is ordinary text: the first is
/// something an agent says in prose, and the second is any numbered list.
///
/// Wrong in one direction only. A dialog worded differently is missed, and the
/// cost of that is one lost notification; the caller only asks while a tool
/// call is pending and the pty is quiet, so it cannot fire on a turn that is
/// merely talking.
pub fn screen_asks_for_approval(lines: &[String]) -> bool {
    lines.iter().enumerate().any(|(index, line)| {
        strip_frame(line).starts_with("Do you want to")
            && lines[index + 1..]
                .iter()
                .any(|below| below.contains("1. Yes"))
    })
}

/// How many pty writes in the approval window still read as a quiet terminal.
///
/// Its own number, not `RESPONDING_WRITES` stretched over
/// a longer window: that one is calibrated for one second, and the window here
/// is three. A real permission dialog repaints about 1.7 times a second, which
/// is five writes in three seconds and sat right on the old threshold of eight
/// -- a few extra repaints and it flapped. A model streaming a reply writes at
/// least that many per *second*, so the gap between "dialog" and "reply" is wide
/// at fifteen.
const APPROVAL_QUIET_MAX_WRITES: usize = 15;

/// Whether `writes` pty writes inside the approval window make the terminal
/// quiet enough for a dialog to be what is holding the agent.
pub fn approval_is_quiet(writes: usize) -> bool {
    writes < APPROVAL_QUIET_MAX_WRITES
}

/// Whether the agent is waiting on a permission dialog, given what is known.
///
/// Quiet is a condition for *noticing* a dialog, not for keeping hold of one.
/// A dialog repaints, and a handful of repaints in a window would otherwise
/// drop an announced wait and re-announce it a moment later. Once asking, the
/// answer is "is the dialog still on screen" and nothing else; the wait ends
/// when the dialog does, or when no tool call is outstanding any more.
pub fn approval_next(asking: bool, tool_pending: bool, quiet: bool, dialog_visible: bool) -> bool {
    tool_pending && dialog_visible && (asking || quiet)
}

/// Notices a tab whose transcript never produces a turn mark.
///
/// The turn marks come from a format this editor does not own. If it changes,
/// the failure is silent: no turn ever starts or ends, and nothing is
/// announced. This is the one place that failure can say so.
#[derive(Default)]
pub struct SilenceWatch {
    saw_marks: bool,
    writing_for: std::time::Duration,
    warned: bool,
}

/// How much terminal activity, with no turn mark ever seen, is long enough to
/// say so.
const SILENT_WRITING_BEFORE_WARNING: std::time::Duration = std::time::Duration::from_secs(60);

impl SilenceWatch {
    pub fn saw_marks(&mut self) {
        self.saw_marks = true;
    }

    /// Accounts for `elapsed` of observation, `cli_writing` saying whether the
    /// agent's output was flowing during it. True exactly once, when the
    /// total passes the limit.
    ///
    /// Counts only time the CLI was writing: an agent left idle at its prompt
    /// has nothing to put in a transcript, and that is not a fault.
    pub fn observe(&mut self, cli_writing: bool, elapsed: std::time::Duration) -> bool {
        if self.saw_marks || self.warned || !cli_writing {
            return false;
        }
        self.writing_for += elapsed;
        self.warned = self.writing_for >= SILENT_WRITING_BEFORE_WARNING;
        self.warned
    }
}

/// A dialog is drawn inside a box, so its text may be preceded by border
/// characters that are not part of what it says.
fn strip_frame(line: &str) -> &str {
    line.trim_matches(|character: char| {
        character.is_whitespace() || ('\u{2500}'..='\u{257F}').contains(&character)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn call(id: &str) -> TurnMark {
        TurnMark::ToolCall(Arc::from(id))
    }

    fn done(ids: &[&str]) -> Vec<Arc<str>> {
        ids.iter().map(|id| Arc::from(*id)).collect()
    }

    fn duration(background_pending: bool) -> TurnMark {
        TurnMark::TurnDuration { background_pending }
    }

    fn pass(
        tracker: &mut TurnTracker,
        marks: &[TurnMark],
        finished: &[Arc<str>],
        live: bool,
        background_quiet_for: Option<Duration>,
    ) -> Vec<TurnEvent> {
        tracker.apply(TurnPass {
            marks,
            finished,
            live,
            background_quiet_for,
        })
    }

    fn fold(tracker: &mut TurnTracker, marks: &[TurnMark]) -> Vec<TurnEvent> {
        pass(tracker, marks, &[], true, None)
    }

    fn reply(id: &str) -> TurnMark {
        TurnMark::EndTurn {
            message_id: Some(Arc::from(id)),
        }
    }

    fn end_turn() -> TurnMark {
        TurnMark::EndTurn { message_id: None }
    }

    /// Passes with nothing new in the transcript, `n` of them, collecting what
    /// they announce.
    fn wait(
        tracker: &mut TurnTracker,
        passes: u32,
        background_quiet_for: Option<Duration>,
    ) -> Vec<TurnEvent> {
        (0..passes)
            .flat_map(|_| pass(tracker, &[], &[], true, background_quiet_for))
            .collect()
    }

    /// One reply writes `end_turn` once per content block, and the turn is over
    /// once.
    #[test]
    fn two_end_turn_lines_of_one_reply_end_the_turn_once() {
        let mut tracker = TurnTracker::default();
        let events = fold(&mut tracker, &[TurnMark::Prompt, end_turn(), end_turn()]);
        assert_eq!(events, [TurnEvent::Started, TurnEvent::Ended]);
    }

    #[test]
    fn end_turn_and_turn_duration_in_one_chunk_end_the_turn_once() {
        let mut tracker = TurnTracker::default();
        let events = fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), end_turn(), duration(false)],
        );
        assert_eq!(events, [TurnEvent::Started, TurnEvent::Ended]);
    }

    /// The same turn read in two passes, split between the reply and the line
    /// that closes it.
    #[test]
    fn end_turn_and_turn_duration_across_two_passes_end_the_turn_once() {
        let mut tracker = TurnTracker::default();
        let first = fold(&mut tracker, &[TurnMark::Prompt, end_turn()]);
        let second = fold(&mut tracker, &[duration(false)]);
        assert_eq!(first, [TurnEvent::Started, TurnEvent::Ended]);
        assert!(second.is_empty(), "already announced: {second:?}");
    }

    /// The race the blocking Stop hooks open: once a transcript has shown it
    /// closes turns by its own line, the reply alone announces nothing.
    #[test]
    fn once_turns_close_explicitly_the_reply_waits_for_the_closing_line() {
        let mut tracker = TurnTracker::default();
        fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
        );

        let reply = fold(&mut tracker, &[TurnMark::Prompt, end_turn()]);
        assert_eq!(reply, [TurnEvent::Started], "the reply must not end it");

        let closed = fold(&mut tracker, &[duration(false)]);
        assert_eq!(closed, [TurnEvent::Ended]);
    }

    /// A hook that sends the agent back to work between the reply and the
    /// closing line makes the reply a mid-turn event, not an end.
    #[test]
    fn a_hook_continuation_after_the_reply_does_not_announce_anything() {
        let mut tracker = TurnTracker::default();
        fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
        );

        let events = fold(
            &mut tracker,
            &[
                TurnMark::Prompt,
                end_turn(),
                TurnMark::Working,
                end_turn(),
                duration(false),
            ],
        );
        assert_eq!(events, [TurnEvent::Started, TurnEvent::Ended]);
    }

    /// Without a closing line to wait for, the same continuation shows as the
    /// end of one turn and the start of the next.
    #[test]
    fn a_continuation_after_an_announced_end_starts_a_new_turn() {
        let mut tracker = TurnTracker::default();
        let events = fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), TurnMark::Working],
        );
        assert_eq!(
            events,
            [TurnEvent::Started, TurnEvent::Ended, TurnEvent::Started]
        );
    }

    #[test]
    fn an_interrupt_ends_the_turn_as_interrupted_and_the_trailing_line_is_silent() {
        let mut tracker = TurnTracker::default();
        let events = fold(
            &mut tracker,
            &[TurnMark::Prompt, TurnMark::Interrupt, duration(false)],
        );
        assert_eq!(
            events,
            [TurnEvent::Started, TurnEvent::Ended, TurnEvent::Interrupted]
        );
    }

    /// The user's decision: a turn that closes with background work still
    /// running is not announced; the turn the agent starts when that work
    /// reports back is.
    #[test]
    fn a_turn_closing_with_background_work_pending_waits_for_the_follow_up() {
        let mut tracker = TurnTracker::default();
        // A transcript's first turn has no closing line to wait for yet, so it
        // is primed with an ordinary one: see `closes_explicitly`.
        fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
        );
        let first = fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(true)],
        );
        assert_eq!(first, [TurnEvent::Started]);

        let follow_up = fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
        );
        assert_eq!(follow_up, [TurnEvent::Started, TurnEvent::Ended]);
    }

    /// A fresh transcript's first turn is announced off its reply; the closing
    /// line that follows says background work is pending, so the announcement
    /// is taken back with a `Started` and the follow-up turn announces itself.
    #[test]
    fn a_first_turn_announced_early_is_retracted_when_background_work_is_pending() {
        let mut tracker = TurnTracker::default();
        let events = fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(true)],
        );
        assert_eq!(
            events,
            [TurnEvent::Started, TurnEvent::Ended, TurnEvent::Started]
        );

        let follow_up = fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
        );
        assert_eq!(follow_up, [TurnEvent::Started, TurnEvent::Ended]);
    }

    /// An interrupt is not an announcement a closing line can retract.
    #[test]
    fn an_interrupted_turn_is_not_restarted_by_a_pending_closing_line() {
        let mut tracker = TurnTracker::default();
        let events = fold(
            &mut tracker,
            &[TurnMark::Prompt, TurnMark::Interrupt, duration(true)],
        );
        assert_eq!(
            events,
            [TurnEvent::Started, TurnEvent::Ended, TurnEvent::Interrupted]
        );
    }

    /// The first pass describes the past. Nothing in it is news, but the phase
    /// it leaves behind is what the next pass continues from.
    #[test]
    fn the_baseline_pass_emits_nothing_and_leaves_the_turn_ended() {
        let mut tracker = TurnTracker::default();
        let baseline = pass(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
            &[],
            false,
            None,
        );
        assert!(baseline.is_empty());

        let next = fold(&mut tracker, &[TurnMark::Prompt]);
        assert_eq!(next, [TurnEvent::Started]);
    }

    #[test]
    fn a_baseline_that_stops_mid_turn_still_ends_it_once() {
        let mut tracker = TurnTracker::default();
        pass(
            &mut tracker,
            &[TurnMark::Prompt, TurnMark::Working],
            &[],
            false,
            None,
        );
        let events = fold(&mut tracker, &[end_turn()]);
        assert_eq!(events, [TurnEvent::Ended]);
    }

    #[test]
    fn a_call_and_its_result_in_one_chunk_is_not_pending() {
        let mut tracker = TurnTracker::default();
        pass(
            &mut tracker,
            &[TurnMark::Prompt, TurnMark::Working, call("toolu_one")],
            &done(&["toolu_one"]),
            true,
            None,
        );
        assert!(!tracker.tool_pending());
    }

    #[test]
    fn a_call_without_a_result_is_pending_until_one_arrives() {
        let mut tracker = TurnTracker::default();
        fold(
            &mut tracker,
            &[TurnMark::Prompt, TurnMark::Working, call("toolu_one")],
        );
        assert!(tracker.tool_pending());

        pass(&mut tracker, &[], &done(&["toolu_one"]), true, None);
        assert!(!tracker.tool_pending());
    }

    #[test]
    fn a_turn_that_ends_leaves_nothing_pending() {
        let mut tracker = TurnTracker::default();
        fold(
            &mut tracker,
            &[TurnMark::Prompt, TurnMark::Working, call("toolu_one")],
        );
        fold(&mut tracker, &[TurnMark::Interrupt]);
        assert!(!tracker.tool_pending());
    }

    #[test]
    fn approval_is_reported_on_the_edges_only() {
        let mut tracker = TurnTracker::default();
        assert_eq!(tracker.set_awaiting_approval(false), None);
        assert_eq!(
            tracker.set_awaiting_approval(true),
            Some(TurnEvent::ApprovalNeeded)
        );
        assert_eq!(tracker.set_awaiting_approval(true), None);
        assert_eq!(
            tracker.set_awaiting_approval(false),
            Some(TurnEvent::ApprovalCleared)
        );
    }

    /// A turn that opens with no prompt -- the agent answering a subagent's
    /// hand-back, whose line is marked meta -- and replies in text alone.
    #[test]
    fn a_text_only_reply_with_no_prompt_starts_and_ends_a_turn() {
        let mut tracker = TurnTracker::default();
        fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
        );
        let held = fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(true)],
        );
        assert_eq!(held, [TurnEvent::Started]);

        let follow_up = fold(&mut tracker, &[reply("msg_handback"), duration(false)]);
        assert_eq!(follow_up, [TurnEvent::Started, TurnEvent::Ended]);
    }

    #[test]
    fn a_text_only_reply_after_an_ended_turn_is_a_turn_of_its_own() {
        let mut tracker = TurnTracker::default();
        let first = fold(&mut tracker, &[TurnMark::Prompt, reply("msg_one")]);
        assert_eq!(first, [TurnEvent::Started, TurnEvent::Ended]);
        let second = fold(&mut tracker, &[reply("msg_two")]);
        assert_eq!(second, [TurnEvent::Started, TurnEvent::Ended]);
    }

    #[test]
    fn two_end_turn_lines_of_one_unprompted_reply_are_one_turn() {
        let mut tracker = TurnTracker::default();
        let events = fold(
            &mut tracker,
            &[reply("msg_one"), reply("msg_one"), reply("msg_one")],
        );
        assert_eq!(events, [TurnEvent::Started, TurnEvent::Ended]);
        // Across passes too: the second block can land a second later.
        assert!(fold(&mut tracker, &[reply("msg_one")]).is_empty());
    }

    #[test]
    fn a_reply_with_no_id_cannot_be_told_from_its_own_blocks_and_opens_nothing() {
        let mut tracker = TurnTracker::default();
        assert!(fold(&mut tracker, &[end_turn()]).is_empty());
    }

    /// The closing line is waited for, but not forever.
    #[test]
    fn an_answered_turn_is_ended_when_its_closing_line_never_comes() {
        let mut tracker = TurnTracker::default();
        fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
        );
        let reply = fold(&mut tracker, &[TurnMark::Prompt, end_turn()]);
        assert_eq!(reply, [TurnEvent::Started]);

        assert!(wait(&mut tracker, ANSWERED_PATIENCE_PASSES - 1, None).is_empty());
        assert_eq!(wait(&mut tracker, 1, None), [TurnEvent::Ended]);
        assert!(wait(&mut tracker, 20, None).is_empty(), "ended once");
    }

    #[test]
    fn a_closing_line_inside_the_patience_ends_the_turn_once() {
        let mut tracker = TurnTracker::default();
        fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
        );
        fold(&mut tracker, &[TurnMark::Prompt, end_turn()]);
        wait(&mut tracker, 3, None);
        assert_eq!(fold(&mut tracker, &[duration(false)]), [TurnEvent::Ended]);
        assert!(wait(&mut tracker, 20, None).is_empty());
    }

    fn held(tracker: &mut TurnTracker) {
        fold(tracker, &[TurnMark::Prompt, end_turn(), duration(false)]);
        let events = fold(tracker, &[TurnMark::Prompt, end_turn(), duration(true)]);
        assert_eq!(events, [TurnEvent::Started]);
    }

    const FRESH: Option<Duration> = Some(Duration::from_secs(3));

    /// The parent's transcript says a background agent's call has its result
    /// within a second of starting, so nothing there says it has stopped: only
    /// the agent's own file does.
    #[test]
    fn a_held_turn_stays_held_while_the_background_agents_keep_writing() {
        let mut tracker = TurnTracker::default();
        held(&mut tracker);
        assert!(wait(&mut tracker, 600, FRESH).is_empty());
        let almost = BACKGROUND_QUIET_LIMIT - Duration::from_secs(1);
        assert!(wait(&mut tracker, 10, Some(almost)).is_empty());
    }

    #[test]
    fn a_held_turn_with_nothing_to_judge_by_stays_held() {
        let mut tracker = TurnTracker::default();
        held(&mut tracker);
        assert!(wait(&mut tracker, 600, None).is_empty());
    }

    #[test]
    fn a_held_turn_nothing_can_judge_is_released_at_the_last_resort_cap() {
        let mut tracker = TurnTracker::default();
        held(&mut tracker);
        assert!(wait(&mut tracker, HELD_UNJUDGED_PASSES - 1, None).is_empty());
        assert_eq!(wait(&mut tracker, 1, None), [TurnEvent::Ended]);
        assert!(wait(&mut tracker, 20, None).is_empty(), "released once");
    }

    #[test]
    fn a_reading_resets_the_last_resort_count() {
        let mut tracker = TurnTracker::default();
        held(&mut tracker);
        assert!(wait(&mut tracker, HELD_UNJUDGED_PASSES - 1, None).is_empty());
        assert!(wait(&mut tracker, 1, FRESH).is_empty());
        assert!(wait(&mut tracker, HELD_UNJUDGED_PASSES - 1, None).is_empty());
    }

    #[test]
    fn a_held_turn_is_released_once_the_background_agents_have_gone_quiet() {
        let mut tracker = TurnTracker::default();
        held(&mut tracker);
        assert_eq!(
            wait(&mut tracker, 1, Some(BACKGROUND_QUIET_LIMIT)),
            [TurnEvent::Ended]
        );
        assert!(
            wait(&mut tracker, 20, Some(BACKGROUND_QUIET_LIMIT)).is_empty(),
            "released once"
        );
    }

    #[test]
    fn a_follow_up_turn_with_nothing_pending_ends_a_held_turn_once() {
        let mut tracker = TurnTracker::default();
        held(&mut tracker);
        wait(&mut tracker, 30, FRESH);
        let events = fold(&mut tracker, &[reply("msg_handback"), duration(false)]);
        assert_eq!(events, [TurnEvent::Started, TurnEvent::Ended]);
        assert!(wait(&mut tracker, 30, Some(BACKGROUND_QUIET_LIMIT)).is_empty());
    }

    #[test]
    fn a_turn_after_a_release_behaves_normally() {
        let mut tracker = TurnTracker::default();
        held(&mut tracker);
        assert_eq!(
            wait(&mut tracker, 1, Some(BACKGROUND_QUIET_LIMIT)),
            [TurnEvent::Ended]
        );
        let next = fold(
            &mut tracker,
            &[TurnMark::Prompt, end_turn(), duration(false)],
        );
        assert_eq!(next, [TurnEvent::Started, TurnEvent::Ended]);
        assert!(wait(&mut tracker, 20, None).is_empty());
    }

    /// A past that stopped waiting must not become a notification when the
    /// editor opens onto it.
    #[test]
    fn a_baseline_that_ends_held_or_answered_never_announces_it_later() {
        for last in [duration(true), end_turn()] {
            let mut tracker = TurnTracker::default();
            fold(
                &mut tracker,
                &[TurnMark::Prompt, end_turn(), duration(false)],
            );
            pass(
                &mut tracker,
                &[TurnMark::Prompt, end_turn(), last],
                &[],
                false,
                None,
            );
            assert!(wait(&mut tracker, 30, None).is_empty());
        }
    }

    #[test]
    fn a_new_prompt_forgets_calls_the_last_turn_left_open() {
        let mut tracker = TurnTracker::default();
        fold(
            &mut tracker,
            &[TurnMark::Prompt, TurnMark::Working, call("toolu_lost")],
        );
        assert!(tracker.tool_pending());
        fold(&mut tracker, &[TurnMark::Prompt]);
        assert!(!tracker.tool_pending());
    }

    #[test]
    fn approval_quiet_has_its_own_threshold_above_the_dialog_repaint_rate() {
        // A real dialog: about five repaints in three seconds.
        assert!(approval_is_quiet(5));
        assert!(approval_is_quiet(APPROVAL_QUIET_MAX_WRITES - 1));
        // A model streaming: eight a second or more, so 24 in three.
        assert!(!approval_is_quiet(24));
    }

    /// The flap the old rule had: a dialog whose repaint count wobbles across
    /// the threshold toggled the announcement on and off.
    #[test]
    fn a_dialog_that_stays_on_screen_stays_announced_however_the_writes_wobble() {
        let mut asking = false;
        let mut transitions = 0;
        for writes in [5, 20, 4, 30, 6, 16, 5] {
            let next = approval_next(asking, true, approval_is_quiet(writes), true);
            if next != asking {
                transitions += 1;
            }
            asking = next;
        }
        assert!(asking);
        assert_eq!(transitions, 1, "announced once, never withdrawn");
    }

    #[test]
    fn a_dialog_is_noticed_only_when_quiet_but_ended_when_it_leaves_the_screen() {
        assert!(!approval_next(false, true, false, true), "noisy: not yet");
        assert!(approval_next(false, true, true, true));
        assert!(!approval_next(true, true, false, false), "dialog gone");
        assert!(!approval_next(true, false, true, true), "no call pending");
    }

    /// The pty write count saturates at the terminal's history, so a threshold
    /// at or above it could never read a streaming agent as busy.
    #[test]
    fn a_saturated_write_history_is_never_quiet() {
        assert!(!approval_is_quiet(terminal::PTY_OUTPUT_HISTORY));
    }

    #[test]
    fn silence_warns_once_after_a_minute_of_output_with_no_marks() {
        let second = std::time::Duration::from_secs(1);
        let mut watch = SilenceWatch::default();
        for _ in 0..59 {
            assert!(!watch.observe(true, second));
        }
        assert!(!watch.observe(false, second), "idle time does not count");
        assert!(watch.observe(true, second));
        assert!(!watch.observe(true, second), "only once");
    }

    #[test]
    fn silence_never_warns_for_a_tab_that_has_produced_marks() {
        let mut watch = SilenceWatch::default();
        watch.saw_marks();
        assert!(!watch.observe(true, std::time::Duration::from_secs(3600)));
    }

    fn screen(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    /// Captured from a real `claude` in a pty, ANSI stripped.
    #[test]
    fn the_bash_permission_dialog_is_recognised() {
        assert!(screen_asks_for_approval(&screen(&[
            "curl -sI https://example.com",
            "This command requires approval",
            "Do you want to proceed?",
            "❯ 1. Yes",
            "  2. Yes, and don’t ask again for: curl*",
            "  4. No",
            "Esc to cancel · Tab to amend",
        ])));
    }

    /// Captured the same way, from a file-creation request. The question is
    /// inside a box, so border characters can precede it.
    #[test]
    fn the_create_and_edit_dialogs_are_recognised() {
        assert!(screen_asks_for_approval(&screen(&[
            "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
            " 1 hello",
            "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌",
            "Do you want to create scratchpad-probe.txt?",
            "❯ 1. Yes",
            "  3. No",
            "Esc to cancel · Tab to amend",
        ])));
        assert!(screen_asks_for_approval(&screen(&[
            "│ Do you want to make this edit to agent_view.rs? │",
            "│ ❯ 1. Yes                                        │",
            "│   3. No                                         │",
        ])));
    }

    #[test]
    fn ordinary_screens_are_not_mistaken_for_a_dialog() {
        assert!(!screen_asks_for_approval(&[]));
        assert!(!screen_asks_for_approval(&screen(&[
            "╭────────────────────╮",
            "│ > run the tests    │",
            "╰────────────────────╯",
        ])));
        // A transcript quoting the question, with no option under it.
        assert!(!screen_asks_for_approval(&screen(&[
            "Do you want to proceed? is what it asks.",
            "I ran the tests and they pass.",
        ])));
        // A numbered list with no question above it.
        assert!(!screen_asks_for_approval(&screen(&[
            "Options:",
            "1. Yes, keep going",
        ])));
        // The question below its options is not a dialog.
        assert!(!screen_asks_for_approval(&screen(&[
            "1. Yes",
            "Do you want to continue?",
        ])));
    }
}
