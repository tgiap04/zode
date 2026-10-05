//! Following the subagents one session spawned.
//!
//! Owned by the tab whose session it describes, not by a shared store. A watch
//! costs file reads, so it has to stop when nobody is looking — and a tab's own
//! lifetime is exactly that condition, already tracked, with no set of watchers
//! to keep in step and nothing to prune.
//!
//! Two facts, from two places, because the store keeps them apart. *Which*
//! subagents exist comes from the sidecar directory beside the transcript, where
//! each is a small file written when it started. Whether one has *finished*
//! comes from the parent transcript, which reports a `tool_result` under the
//! same id the sidecar recorded. The subagent's own file cannot answer the
//! second question: within a single run its writes were measured 165 seconds
//! apart, so no staleness threshold separates a thinking subagent from a
//! finished one.

use agent_sessions::{AgentKind, SessionProvider, SessionSummary, SubagentSummary, TurnMark};
use collections::HashSet;
use gpui::SharedString;
use std::sync::Arc;

/// What one background pass over a session's store found.
///
/// Carried back to the foreground rather than applied there: the scan reads
/// files, and the tracker it updates lives on the UI thread.
#[derive(Default)]
pub struct SubagentPass {
    session: Option<SessionSummary>,
    /// `None` means "this pass has nothing to say about the list" — either the
    /// sidecar could not be read, or it was not re-read because nothing could
    /// have changed. Distinct from `Some(vec![])`, which means the session
    /// genuinely has no subagents: overwriting a known list with an empty one
    /// on a transient read error would blink the whole disclosure away.
    subagents: Option<Vec<SubagentSummary>>,
    finished: Vec<Arc<str>>,
    /// The turn landmarks in the stretch this pass read. Not kept by the
    /// tracker: they are consumed once, by the turn state that folds them.
    turn_marks: Vec<TurnMark>,
    scanned_to: u64,
    /// The transcript under the path is a different, shorter file than the one
    /// the last pass read.
    restarted: bool,
    background_quiet_for: Option<std::time::Duration>,
    /// The store could not be read, as opposed to holding nothing yet. Such a
    /// pass says nothing about whether the session existed when the tab opened.
    unreadable: bool,
}

/// What a pass read that the turn state needs, handed back by
/// [`SubagentTracker::apply`] so the transcript is read once for both.
pub struct TurnInput {
    pub marks: Vec<TurnMark>,
    /// The tool calls the same stretch reported a result for.
    pub finished: Vec<Arc<str>>,
    /// Whether these marks are news. False for the stretch that was already in
    /// the transcript when the tab opened onto it, which describes the past:
    /// every turn in it ended long ago and none of them is worth announcing.
    pub live: bool,
    /// The transcript was replaced by a shorter one, so whatever was folded
    /// from the old one no longer describes anything.
    pub restarted: bool,
    /// How long since any subagent last wrote to its own transcript; `None`
    /// when there is no such transcript to judge by.
    pub background_quiet_for: Option<std::time::Duration>,
}

/// The subagents of one session, and which of them are still running.
#[derive(Default)]
pub struct SubagentTracker {
    /// Resolved once and kept. Finding a session means walking the store's
    /// project directories, and the answer cannot change for the life of a tab
    /// — the id it is looking for was fixed when the tab opened.
    session: Option<SessionSummary>,
    subagents: Arc<[SubagentSummary]>,
    /// The tool calls the parent has reported a result for. Only ever grows
    /// within one transcript, which is what lets the scan below be incremental.
    finished: HashSet<Arc<str>>,
    /// How far into the transcript the last pass read. Always the end of a
    /// complete line — see `ClaudeProvider::transcript_progress`.
    scanned_to: u64,
    /// Whether a pass has read bytes of this transcript. What separates the
    /// past from the present: not an offset, since a tab that opened onto an
    /// empty or absent transcript is at offset zero for its whole first turn.
    baselined: bool,
    /// Whether the first pass that could look found nothing to read. A tab
    /// that opened onto no transcript is a new session, and everything it ever
    /// writes is news -- including the first turn, which is what the user is
    /// most likely watching.
    opened_on_nothing: bool,
    first_pass_seen: bool,
    /// As of the last pass. Kept rather than recomputed: it is a fact about the
    /// disk, read where disk reads belong.
    background_quiet_for: Option<std::time::Duration>,
}

impl SubagentTracker {
    /// Reads the store. For a background thread only.
    ///
    /// Takes the session by value rather than borrowing the tracker, so the
    /// caller can hand the work off without holding the UI thread's state
    /// across an await.
    pub fn scan(
        provider: &Arc<dyn SessionProvider>,
        session: Option<SessionSummary>,
        session_id: SharedString,
        from: u64,
    ) -> SubagentPass {
        // A session the store does not hold yet is the normal state of a tab
        // whose CLI has only just started: Claude writes the transcript on the
        // first exchange, not at spawn. Nothing is wrong, there is simply
        // nothing to read, and the next pass asks again.
        let session = match session {
            Some(session) => session,
            None => match provider.find(&session_id) {
                Ok(Some(session)) => session,
                Ok(None) => return SubagentPass::empty(from),
                Err(error) => {
                    log::warn!("looking up session {session_id} for its subagents: {error}");
                    return SubagentPass {
                        unreadable: true,
                        ..SubagentPass::empty(from)
                    };
                }
            },
        };

        let (completed, unreadable) = match provider.transcript_progress(&session, from) {
            Ok(completed) => (completed, false),
            Err(error) => {
                log::warn!("reading completed subagents of {session_id}: {error}");
                (
                    agent_sessions::TranscriptProgress {
                        scanned_to: from,
                        ..Default::default()
                    },
                    true,
                )
            }
        };

        // The sidecar is re-listed only when the transcript actually grew, and
        // that gate is exact rather than approximate: a subagent cannot appear
        // without the parent writing the tool call that spawned it, so a
        // transcript that has not moved cannot have gained one. Without it this
        // re-opens and re-parses every sidecar file once a second for the life
        // of every open tab -- twenty-five of them in the session that was
        // measured.
        //
        // `from == 0` is the first pass, which has to list whatever is already
        // there.
        let grew = completed.scanned_to != from;
        let subagents = (grew || from == 0).then(|| {
            provider
                .subagents(&session)
                .map_err(|error| log::warn!("reading subagents of {session_id}: {error}"))
                .ok()
        });

        let background_quiet_for =
            provider
                .background_quiet_for(&session)
                .unwrap_or_else(|error| {
                    log::warn!("reading subagent activity of {session_id}: {error}");
                    None
                });

        SubagentPass {
            background_quiet_for,
            session: Some(session),
            subagents: subagents.flatten(),
            finished: completed.tool_use_ids,
            turn_marks: completed.turn_marks,
            scanned_to: completed.scanned_to,
            restarted: completed.restarted,
            unreadable,
        }
    }

    pub fn apply(&mut self, pass: SubagentPass) -> TurnInput {
        if pass.restarted {
            // A different transcript: nothing derived from the old one holds.
            // The pass itself is read from the start of the new file and is
            // the past again.
            self.finished.clear();
            self.scanned_to = 0;
            self.baselined = false;
            // Not reset to "never seen a pass": a replaced file that has no
            // complete line yet would then read as a brand-new session, and
            // whatever it holds when it does fill would be announced as news.
            self.opened_on_nothing = false;
            self.first_pass_seen = true;
        }
        let read_bytes = pass.scanned_to > self.scanned_to;
        if !pass.unreadable && !self.first_pass_seen {
            self.first_pass_seen = true;
            self.opened_on_nothing = !read_bytes;
        }
        let live = self.baselined || self.opened_on_nothing;
        self.baselined |= read_bytes;
        self.background_quiet_for = pass.background_quiet_for;
        if pass.session.is_some() {
            self.session = pass.session;
        }
        // Kept when the pass had nothing to say. See `SubagentPass::subagents`.
        if let Some(subagents) = pass.subagents {
            self.subagents = Arc::from(subagents);
        }
        self.finished.extend(pass.finished.iter().cloned());
        self.scanned_to = pass.scanned_to;
        TurnInput {
            marks: pass.turn_marks,
            finished: pass.finished,
            live,
            restarted: pass.restarted,
            background_quiet_for: self.background_quiet_for,
        }
    }

    /// What the next pass needs to know, so the caller can hand it to a
    /// background thread without carrying the tracker along.
    pub fn cursor(&self) -> (Option<SessionSummary>, u64) {
        (self.session.clone(), self.scanned_to)
    }

    pub fn subagents(&self) -> &Arc<[SubagentSummary]> {
        &self.subagents
    }

    /// Whether this one is still working.
    ///
    /// Spawned, and no result reported for it. A subagent whose result arrived
    /// in a stretch of transcript this tracker never read would be wrong here —
    /// which is why the scan is resumed from a complete line and never from the
    /// end of a file that was still being written.
    pub fn is_running(&self, subagent: &SubagentSummary) -> bool {
        !self.finished.contains(&subagent.tool_use_id)
    }

    pub fn any_running(&self) -> bool {
        self.subagents
            .iter()
            .any(|subagent| self.is_running(subagent))
    }
}

impl SubagentPass {
    fn empty(from: u64) -> Self {
        Self {
            scanned_to: from,
            ..Self::default()
        }
    }
}

/// The store to ask about this agent's sessions, or `None` for an agent this
/// editor has no reader for.
pub fn provider_for_agent(agent: &project::AgentId) -> Option<Arc<dyn SessionProvider>> {
    AgentKind::from_builtin_agent_id(agent.as_ref()).map(agent_sessions::provider_for)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn subagent(id: &str, tool_use_id: &str) -> SubagentSummary {
        SubagentSummary {
            id: Arc::from(id),
            kind: "reviewer".into(),
            description: "Review the diff".into(),
            tool_use_id: Arc::from(tool_use_id),
            spawned_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn pass(subagents: Vec<SubagentSummary>, finished: Vec<&str>, scanned_to: u64) -> SubagentPass {
        SubagentPass {
            session: None,
            subagents: Some(subagents),
            finished: finished.into_iter().map(Arc::from).collect(),
            turn_marks: Vec::new(),
            scanned_to,
            ..SubagentPass::default()
        }
    }

    /// Spawned and unreported is running; reported is not. The whole rule.
    #[test]
    fn a_subagent_runs_until_its_result_is_reported() {
        let mut tracker = SubagentTracker::default();
        tracker.apply(pass(
            vec![subagent("agent-one", "toolu_one")],
            Vec::new(),
            10,
        ));
        assert!(tracker.any_running());

        tracker.apply(pass(
            vec![subagent("agent-one", "toolu_one")],
            vec!["toolu_one"],
            20,
        ));
        assert!(!tracker.any_running());
    }

    /// The reason `finished` is extended rather than replaced.
    ///
    /// Each pass reads only the bytes added since the last one, so it reports
    /// only the results inside that stretch. Replacing the set would forget
    /// every subagent that finished earlier and light them all up again, which
    /// is the flicker this feature exists to remove — arriving by a different
    /// door.
    #[test]
    fn a_later_pass_does_not_forget_what_an_earlier_one_reported() {
        let mut tracker = SubagentTracker::default();
        let both = || {
            vec![
                subagent("agent-one", "toolu_one"),
                subagent("agent-two", "toolu_two"),
            ]
        };

        tracker.apply(pass(both(), vec!["toolu_one"], 10));
        tracker.apply(pass(both(), vec!["toolu_two"], 20));

        assert!(
            !tracker.any_running(),
            "both results have now been reported"
        );
        assert!(!tracker.is_running(&subagent("agent-one", "toolu_one")));
    }

    /// A pass with nothing to say about the list keeps the one already held.
    ///
    /// Two passes take this path and they must not be told apart here: a
    /// sidecar that could not be read this second, and a sidecar deliberately
    /// not re-read because the transcript had not moved. Replacing the list
    /// with an empty one in either case blinks the whole disclosure away and
    /// puts it back a second later.
    #[test]
    fn a_pass_with_nothing_to_say_keeps_the_list_it_already_had() {
        let mut tracker = SubagentTracker::default();
        tracker.apply(pass(
            vec![subagent("agent-one", "toolu_one")],
            Vec::new(),
            10,
        ));
        assert_eq!(tracker.subagents().len(), 1);

        tracker.apply(SubagentPass {
            scanned_to: 20,
            ..SubagentPass::default()
        });
        assert_eq!(
            tracker.subagents().len(),
            1,
            "a pass that read nothing must not report that there is nothing"
        );
    }

    /// A pass that found no session must not throw away what is already known —
    /// the store simply had nothing new to say.
    #[test]
    fn a_pass_that_resolved_nothing_keeps_the_cursor_where_it_was() {
        let mut tracker = SubagentTracker::default();
        tracker.apply(pass(
            vec![subagent("agent-one", "toolu_one")],
            vec!["toolu_one"],
            40,
        ));
        tracker.apply(SubagentPass::empty(40));

        let (_, scanned_to) = tracker.cursor();
        assert_eq!(scanned_to, 40);
        assert!(
            !tracker.is_running(&subagent("agent-one", "toolu_one")),
            "a result already read stays read"
        );
    }

    /// The tracker keeps no marks of its own; they pass through once, with the
    /// results found beside them, to whatever folds the turn.
    #[test]
    fn a_pass_hands_its_turn_marks_and_results_back_once() {
        let mut tracker = SubagentTracker::default();
        let marks = vec![TurnMark::Prompt, TurnMark::EndTurn { message_id: None }];
        let input = tracker.apply(SubagentPass {
            finished: vec![Arc::from("toolu_one")],
            turn_marks: marks.clone(),
            scanned_to: 10,
            ..SubagentPass::default()
        });
        assert_eq!(input.marks, marks);
        assert_eq!(input.finished, [Arc::<str>::from("toolu_one")]);

        let next = tracker.apply(SubagentPass::empty(10));
        assert!(next.marks.is_empty() && next.finished.is_empty());
    }

    fn reading(scanned_to: u64) -> SubagentPass {
        SubagentPass {
            scanned_to,
            ..SubagentPass::default()
        }
    }

    /// A tab opened onto an existing transcript: the first pass that reads
    /// anything is the past, and everything after it is news.
    #[test]
    fn the_first_pass_that_reads_bytes_is_the_past_and_later_ones_are_live() {
        let mut tracker = SubagentTracker::default();
        assert!(!tracker.apply(reading(500)).live);
        assert!(tracker.apply(reading(700)).live);
    }

    /// The offset alone cannot say this: `from != 0` called the first real
    /// pass live whenever an empty pass came before it.
    #[test]
    fn an_unreadable_pass_does_not_stand_in_for_the_baseline() {
        let mut tracker = SubagentTracker::default();
        let unreadable = SubagentPass {
            unreadable: true,
            ..SubagentPass::default()
        };
        assert!(!tracker.apply(unreadable).live);
        assert!(
            !tracker.apply(reading(500)).live,
            "the first bytes ever read are still the past"
        );
        assert!(tracker.apply(reading(600)).live);
    }

    /// A tab that opened onto no transcript is a new session: its first turn
    /// is news from byte zero.
    #[test]
    fn a_tab_that_opened_onto_no_transcript_is_live_from_byte_zero() {
        let mut tracker = SubagentTracker::default();
        assert!(tracker.apply(SubagentPass::empty(0)).live);
        assert!(tracker.apply(reading(300)).live, "its first turn");
        assert!(tracker.apply(reading(400)).live);
    }

    #[test]
    fn a_replaced_transcript_resets_the_tracker_and_is_read_silently() {
        let mut tracker = SubagentTracker::default();
        tracker.apply(pass(
            vec![subagent("agent-one", "toolu_one")],
            vec!["toolu_one"],
            900,
        ));
        assert!(tracker.apply(reading(1000)).live);

        let replaced = tracker.apply(SubagentPass {
            scanned_to: 120,
            restarted: true,
            ..SubagentPass::default()
        });
        assert!(replaced.restarted);
        assert!(!replaced.live, "the new file's contents are the past");
        assert!(
            tracker.is_running(&subagent("agent-one", "toolu_one")),
            "results of the old file no longer count"
        );
        assert!(tracker.apply(reading(200)).live);
    }

    #[test]
    fn a_replaced_transcript_with_no_complete_line_yet_is_still_the_past() {
        let mut tracker = SubagentTracker::default();
        tracker.apply(reading(1000));
        let replaced = tracker.apply(SubagentPass {
            restarted: true,
            ..SubagentPass::default()
        });
        assert!(!replaced.live);
        assert!(
            !tracker.apply(reading(200)).live,
            "its first lines are the past"
        );
        assert!(tracker.apply(reading(300)).live);
    }

    #[test]
    fn the_turn_input_carries_how_long_the_subagents_have_been_quiet() {
        let mut tracker = SubagentTracker::default();
        let quiet = std::time::Duration::from_secs(42);
        let input = tracker.apply(SubagentPass {
            background_quiet_for: Some(quiet),
            ..SubagentPass::default()
        });
        assert_eq!(input.background_quiet_for, Some(quiet));
        assert_eq!(tracker.apply(reading(1)).background_quiet_for, None);
    }
}
