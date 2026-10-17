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

use agent_sessions::{
    AgentKind, SessionProvider, SessionSummary, SubagentEvent, SubagentSummary, TurnMark,
};
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
    /// Background subagents that stopped or started again in this stretch, in
    /// file order.
    events: Vec<SubagentEvent>,
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
    /// Subagents (by sidecar id) whose latest run has ended. How a background
    /// subagent ends, since its tool result arrives at launch and says nothing.
    stopped: HashSet<Arc<str>>,
    /// Subagents started again since they last stopped. Running whatever their
    /// shape, because a resume is a fresh run the tool result already answered.
    resumed: HashSet<Arc<str>>,
    /// What the last fresh list did not name, per set. An id is pruned only when
    /// two lists in a row leave it out: a sidecar that fails to read once is
    /// dropped from that list and comes back in the next, and its stop must
    /// still be there when it does.
    unlisted: Unlisted,
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
            events: completed.subagent_events,
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
            self.stopped.clear();
            self.resumed.clear();
            self.unlisted = Unlisted::default();
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
        let listed = pass.subagents.is_some();
        if let Some(subagents) = pass.subagents {
            self.subagents = Arc::from(subagents);
        }
        self.finished.extend(pass.finished.iter().cloned());
        for event in pass.events {
            match event {
                SubagentEvent::Stopped(id) => {
                    self.resumed.remove(&id);
                    self.stopped.insert(id);
                }
                SubagentEvent::Resumed(id) => {
                    self.stopped.remove(&id);
                    self.resumed.insert(id);
                }
            }
        }
        // The list is read after the transcript, so a fresh one names every
        // subagent this pass could have seen an ending for. Whatever else the
        // sets hold belongs to no listed subagent -- a background shell's
        // notification, say -- and would only accumulate. Each set is bounded
        // by the listed subagents plus whatever one list left out.
        if listed {
            let subagents = &self.subagents;
            prune_after_two_misses(&mut self.finished, &mut self.unlisted.finished, |id| {
                subagents.iter().any(|subagent| subagent.tool_use_id == *id)
            });
            prune_after_two_misses(&mut self.stopped, &mut self.unlisted.stopped, |id| {
                subagents.iter().any(|subagent| subagent.id == *id)
            });
            prune_after_two_misses(&mut self.resumed, &mut self.unlisted.resumed, |id| {
                subagents.iter().any(|subagent| subagent.id == *id)
            });
        }
        if !live {
            self.settle_running();
        }
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
    ///
    /// A background subagent's result is the launch receipt, so it runs until
    /// a notification says it stopped; a resume restarts either kind. A
    /// foreground one ends on its result, or on a notification when it was
    /// moved to the background mid-run and kept its foreground shape.
    ///
    /// A resume whose notification never arrives would read as running for
    /// good. Measured on every local transcript, each resume was followed by
    /// one, and the caller only asks while the tab is working, which bounds it.
    pub fn is_running(&self, subagent: &SubagentSummary) -> bool {
        self.resumed.contains(&subagent.id)
            || if subagent.background {
                !self.stopped.contains(&subagent.id)
            } else {
                !self.finished.contains(&subagent.tool_use_id)
                    && !self.stopped.contains(&subagent.id)
            }
    }

    /// The subagents still working, in list order (newest first).
    pub fn running(&self) -> impl Iterator<Item = &SubagentSummary> {
        self.subagents
            .iter()
            .filter(|subagent| self.is_running(subagent))
    }

    /// Marks every subagent still reading as running as ended.
    ///
    /// For the stretch of transcript that predates this tab: it was written by
    /// a CLI process that is gone, and a run it left open will never see its
    /// ending. A later live resume revives one.
    fn settle_running(&mut self) {
        let orphans: Vec<SubagentSummary> = self.running().cloned().collect();
        for subagent in orphans {
            self.resumed.remove(&subagent.id);
            if subagent.background {
                self.stopped.insert(subagent.id);
            } else {
                self.finished.insert(subagent.tool_use_id);
            }
        }
    }

    pub fn any_running(&self) -> bool {
        self.running().next().is_some()
    }
}

/// Which ids the previous fresh list left out, one set per kept set.
#[derive(Default)]
struct Unlisted {
    finished: HashSet<Arc<str>>,
    stopped: HashSet<Arc<str>>,
    resumed: HashSet<Arc<str>>,
}

/// Drops from `kept` every id that `is_listed` rejects now and `missed` already
/// recorded as rejected by the list before, then records this list's rejects.
fn prune_after_two_misses(
    kept: &mut HashSet<Arc<str>>,
    missed: &mut HashSet<Arc<str>>,
    is_listed: impl Fn(&Arc<str>) -> bool,
) {
    let absent: HashSet<Arc<str>> = kept.iter().filter(|id| !is_listed(id)).cloned().collect();
    kept.retain(|id| !(absent.contains(id) && missed.contains(id)));
    *missed = absent.difference(missed).cloned().collect();
}

impl SubagentPass {
    /// What a scan that found `listed` and the notifications for `stopped`
    /// would have produced, for tests that have no transcript to read.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn simulated(listed: Vec<SubagentSummary>, stopped: &[Arc<str>]) -> Self {
        Self {
            subagents: Some(listed),
            events: stopped
                .iter()
                .map(|id| SubagentEvent::Stopped(id.clone()))
                .collect(),
            ..Self::default()
        }
    }

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
            background: false,
            spawned_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn background(id: &str, tool_use_id: &str) -> SubagentSummary {
        SubagentSummary {
            background: true,
            ..subagent(id, tool_use_id)
        }
    }

    fn stopped(id: &str) -> SubagentEvent {
        SubagentEvent::Stopped(Arc::from(id))
    }

    fn resumed(id: &str) -> SubagentEvent {
        SubagentEvent::Resumed(Arc::from(id))
    }

    /// A pass over bytes appended after the tab opened, so it is news and
    /// nothing in it is settled.
    fn live_pass(
        subagents: Vec<SubagentSummary>,
        finished: Vec<&str>,
        events: Vec<SubagentEvent>,
        scanned_to: u64,
    ) -> SubagentPass {
        SubagentPass {
            events,
            ..pass(subagents, finished, scanned_to)
        }
    }

    /// A tracker that has already read its past, so later passes are live.
    fn live_tracker() -> SubagentTracker {
        let mut tracker = SubagentTracker::default();
        tracker.apply(SubagentPass::empty(0));
        tracker
    }

    #[test]
    fn a_background_subagent_runs_past_its_launch_result_until_it_stops() {
        let mut tracker = live_tracker();
        let agents = || vec![background("agent-bg", "toolu_bg")];
        tracker.apply(live_pass(agents(), vec!["toolu_bg"], Vec::new(), 10));
        assert!(tracker.any_running(), "the launch result is not an ending");

        tracker.apply(live_pass(
            agents(),
            Vec::new(),
            vec![stopped("agent-bg")],
            20,
        ));
        assert!(!tracker.any_running());
    }

    #[test]
    fn a_resumed_background_subagent_runs_again_until_its_second_stop() {
        let mut tracker = live_tracker();
        let agents = || vec![background("agent-bg", "toolu_bg")];
        tracker.apply(live_pass(
            agents(),
            vec!["toolu_bg"],
            vec![stopped("agent-bg")],
            10,
        ));
        assert!(!tracker.any_running());

        tracker.apply(live_pass(
            agents(),
            Vec::new(),
            vec![resumed("agent-bg")],
            20,
        ));
        assert!(tracker.any_running());

        tracker.apply(live_pass(
            agents(),
            Vec::new(),
            vec![stopped("agent-bg")],
            30,
        ));
        assert!(!tracker.any_running());
    }

    #[test]
    fn events_in_one_pass_apply_in_file_order() {
        let mut tracker = live_tracker();
        tracker.apply(live_pass(
            vec![background("agent-bg", "toolu_bg")],
            Vec::new(),
            vec![stopped("agent-bg"), resumed("agent-bg")],
            10,
        ));
        assert!(tracker.any_running(), "the later resume wins");
    }

    #[test]
    fn a_foreground_subagent_keeps_the_result_rule_and_a_resume_revives_it() {
        let mut tracker = live_tracker();
        let agents = || vec![subagent("agent-fg", "toolu_fg")];
        tracker.apply(live_pass(agents(), Vec::new(), Vec::new(), 10));
        assert!(tracker.any_running());
        tracker.apply(live_pass(agents(), vec!["toolu_fg"], Vec::new(), 20));
        assert!(!tracker.any_running());

        tracker.apply(live_pass(
            agents(),
            Vec::new(),
            vec![resumed("agent-fg")],
            30,
        ));
        assert!(tracker.any_running());
        tracker.apply(live_pass(
            agents(),
            Vec::new(),
            vec![stopped("agent-fg")],
            40,
        ));
        assert!(!tracker.any_running());
    }

    #[test]
    fn a_notification_for_a_task_that_is_not_a_listed_subagent_changes_nothing() {
        let mut tracker = live_tracker();
        tracker.apply(live_pass(
            vec![background("agent-bg", "toolu_bg")],
            Vec::new(),
            vec![stopped("agent-bq1w2e3r4")],
            10,
        ));
        tracker.apply(live_pass(
            vec![background("agent-bg", "toolu_bg")],
            Vec::new(),
            Vec::new(),
            20,
        ));
        assert!(tracker.any_running());
        assert!(
            tracker.stopped.is_empty(),
            "an id that two lists in a row leave out is pruned, not kept"
        );
    }

    /// A launch left over from an earlier CLI process never gets its
    /// notification now, so the history read when a tab opens must not leave it
    /// spinning.
    #[test]
    fn unfinished_subagents_in_the_history_read_count_as_ended() {
        let mut tracker = SubagentTracker::default();
        let past = tracker.apply(live_pass(
            vec![
                background("agent-bg", "toolu_bg"),
                subagent("agent-fg", "toolu_fg"),
            ],
            vec!["toolu_bg"],
            Vec::new(),
            500,
        ));
        assert!(!past.live);
        assert!(!tracker.any_running());
    }

    #[test]
    fn a_live_resume_revives_a_subagent_the_history_settled() {
        let mut tracker = SubagentTracker::default();
        let agents = || vec![background("agent-bg", "toolu_bg")];
        tracker.apply(live_pass(agents(), vec!["toolu_bg"], Vec::new(), 500));
        assert!(!tracker.any_running());

        tracker.apply(live_pass(
            agents(),
            Vec::new(),
            vec![resumed("agent-bg")],
            600,
        ));
        assert!(tracker.any_running());
    }

    #[test]
    fn a_resume_inside_the_history_is_settled_too() {
        let mut tracker = SubagentTracker::default();
        tracker.apply(live_pass(
            vec![background("agent-bg", "toolu_bg")],
            Vec::new(),
            vec![resumed("agent-bg")],
            500,
        ));
        assert!(!tracker.any_running());
    }

    #[test]
    fn a_restarted_transcript_clears_what_was_known_and_settles_again() {
        let mut tracker = live_tracker();
        tracker.apply(live_pass(
            vec![background("agent-bg", "toolu_bg")],
            Vec::new(),
            vec![stopped("agent-bg")],
            900,
        ));
        tracker.apply(SubagentPass {
            subagents: Some(vec![background("agent-bg", "toolu_bg")]),
            scanned_to: 100,
            restarted: true,
            ..SubagentPass::default()
        });
        assert!(!tracker.any_running(), "the new file's past is settled");
        assert!(
            tracker.resumed.is_empty() && tracker.finished.is_empty(),
            "nothing of the old file survives"
        );
    }

    #[test]
    fn state_stays_bounded_by_the_listed_subagents() {
        let mut tracker = live_tracker();
        let finished: Vec<String> = (0..50).map(|n| format!("toolu_{n}")).collect();
        let events: Vec<SubagentEvent> = (0..50)
            .flat_map(|n| {
                [
                    stopped(&format!("agent-gone-{n}")),
                    resumed(&format!("agent-x-{n}")),
                ]
            })
            .collect();
        let agents = || vec![background("agent-bg", "toolu_0")];
        tracker.apply(live_pass(
            agents(),
            finished.iter().map(String::as_str).collect(),
            events,
            10,
        ));
        tracker.apply(live_pass(agents(), Vec::new(), Vec::new(), 20));
        assert!(tracker.finished.len() <= 1);
        assert!(tracker.stopped.len() <= 1);
        assert!(tracker.resumed.len() <= 1);
    }

    /// A sidecar that cannot be read for one pass is missing from that list
    /// and back in the next; its stop must have survived the gap.
    #[test]
    fn a_stop_survives_one_list_that_left_its_subagent_out() {
        let mut tracker = live_tracker();
        let agents = || vec![background("agent-bg", "toolu_bg")];
        tracker.apply(live_pass(agents(), Vec::new(), Vec::new(), 10));
        tracker.apply(live_pass(
            Vec::new(),
            Vec::new(),
            vec![stopped("agent-bg")],
            20,
        ));
        tracker.apply(live_pass(agents(), Vec::new(), Vec::new(), 30));
        assert!(
            !tracker.any_running(),
            "the sidecar came back, still stopped"
        );
    }

    #[test]
    fn a_stop_for_a_subagent_absent_from_two_lists_in_a_row_is_dropped() {
        let mut tracker = live_tracker();
        tracker.apply(live_pass(
            Vec::new(),
            Vec::new(),
            vec![stopped("agent-bg")],
            10,
        ));
        tracker.apply(live_pass(Vec::new(), Vec::new(), Vec::new(), 20));
        assert!(tracker.stopped.is_empty());
    }

    /// A list that names the id again in between resets the count.
    #[test]
    fn two_misses_must_be_consecutive() {
        let mut tracker = live_tracker();
        let agents = || vec![background("agent-bg", "toolu_bg")];
        tracker.apply(live_pass(
            Vec::new(),
            Vec::new(),
            vec![stopped("agent-bg")],
            10,
        ));
        tracker.apply(live_pass(agents(), Vec::new(), Vec::new(), 20));
        tracker.apply(live_pass(Vec::new(), Vec::new(), Vec::new(), 30));
        assert!(tracker.stopped.contains("agent-bg"));
    }

    /// A tab that opened onto no transcript is live from byte zero, so a
    /// background launch in its very first scanned chunk is news, not history.
    #[test]
    fn a_launch_in_a_new_tabs_first_chunk_reads_as_running() {
        let mut tracker = SubagentTracker::default();
        assert!(tracker.apply(SubagentPass::empty(0)).live);
        let first_chunk = tracker.apply(live_pass(
            vec![background("agent-bg", "toolu_bg")],
            vec!["toolu_bg"],
            Vec::new(),
            300,
        ));
        assert!(first_chunk.live);
        assert!(tracker.any_running());
    }

    #[test]
    fn a_foreground_subagent_moved_to_the_background_ends_on_its_stop() {
        let mut tracker = live_tracker();
        let agents = || vec![subagent("agent-fg", "toolu_fg")];
        tracker.apply(live_pass(agents(), Vec::new(), Vec::new(), 10));
        assert!(tracker.any_running());
        tracker.apply(live_pass(
            agents(),
            Vec::new(),
            vec![stopped("agent-fg")],
            20,
        ));
        assert!(!tracker.any_running());
    }

    #[test]
    fn a_pass_with_no_fresh_list_does_not_prune() {
        let mut tracker = live_tracker();
        tracker.apply(live_pass(
            vec![background("agent-bg", "toolu_bg")],
            Vec::new(),
            Vec::new(),
            10,
        ));
        tracker.apply(SubagentPass {
            events: vec![stopped("agent-bg")],
            scanned_to: 20,
            ..SubagentPass::default()
        });
        assert!(!tracker.any_running());
        assert!(tracker.stopped.contains("agent-bg"));
    }

    #[test]
    fn running_lists_only_what_is_running_in_list_order() {
        let mut tracker = live_tracker();
        tracker.apply(live_pass(
            vec![
                background("agent-new", "toolu_1"),
                background("agent-done", "toolu_2"),
                subagent("agent-old", "toolu_3"),
            ],
            Vec::new(),
            vec![stopped("agent-done")],
            10,
        ));
        let ids: Vec<&str> = tracker.running().map(|one| &*one.id).collect();
        assert_eq!(ids, vec!["agent-new", "agent-old"]);
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
        let mut tracker = live_tracker();
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
            !tracker.is_running(&subagent("agent-one", "toolu_one")),
            "the new file's past is settled, whatever the old file said"
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
