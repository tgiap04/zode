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
use std::{sync::Arc, time::SystemTime};

/// How many passes an open tool call may go by before the sidecar directory is
/// listed again with the transcript not having moved. A subagent's sidecar is
/// written when the call that spawns it starts running, after the parent's
/// `tool_use` line that announces it -- so a pass that reads that line can
/// list a directory that does not have it yet, and nothing in the transcript
/// moves again until the call ends. Five seconds, not every second: the listing
/// opens every sidecar, and a long build holds a call open for minutes in every
/// tab.
const RELIST_EVERY_PASSES: u32 = 5;

/// How many sessions this tab has left are remembered. Each is excluded from
/// being followed back to; a tab cleared more often than this in one run has
/// also written to far newer transcripts than any it could return to.
const REMEMBERED_SESSIONS: usize = 64;

/// How many passes go by between looks for a transcript the CLI has moved to,
/// while the current one has stopped growing under a CLI that keeps writing.
/// Each look reads a directory; the first one after a stall begins is not
/// delayed.
const SUCCESSOR_SEARCH_EVERY_PASSES: u32 = 5;

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
    /// Which file the transcript is; see `TranscriptProgress::identity`.
    identity: Option<u64>,
    /// The session the CLI moved to, when this pass went looking and found one.
    followed: Option<SessionSummary>,
    background_quiet_for: Option<std::time::Duration>,
    /// The store could not be read, as opposed to holding nothing yet. Such a
    /// pass says nothing about whether the session existed when the tab opened.
    unreadable: bool,
    /// What went wrong, if anything did. Kept as a message rather than logged
    /// where it happened: a failure that persists is met again on every pass,
    /// and the tracker is what knows it has said so already.
    failure: Option<String>,
}

/// What a background pass should do, decided on the UI thread from what the tab
/// knows and carried to the thread that reads files.
pub struct ScanRequest {
    session: Option<SessionSummary>,
    session_id: SharedString,
    from: u64,
    /// List the sidecars even though the transcript did not grow.
    relist: bool,
    /// Ask how long the subagents have been quiet, which stats every one of
    /// their files and is only wanted while a turn is held.
    quiet: bool,
    search: Option<SuccessorSearch>,
}

impl ScanRequest {
    /// Whether this pass will look for the session the CLI moved to, and which
    /// sessions it will pass over while it does.
    #[cfg(test)]
    pub(crate) fn search_skips(&self) -> Option<&[Arc<str>]> {
        self.search.as_ref().map(|search| search.claimed.as_slice())
    }
}

/// What looking for the session the CLI moved to needs.
pub struct SuccessorSearch {
    /// When the tab began: nothing written before it belongs to its CLI.
    pub since: SystemTime,
    /// The sessions other tabs follow.
    pub claimed: Vec<Arc<str>>,
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
    /// A different transcript stands where the old one did, or the CLI moved to
    /// another, so whatever was folded from the old one no longer describes
    /// anything.
    pub restarted: bool,
    /// The session the CLI moved to, when it did: the tab follows it from here,
    /// under its id.
    pub followed: Option<SessionSummary>,
    /// How long since any subagent last wrote to its own transcript; `None`
    /// when there is no such transcript to judge by.
    pub background_quiet_for: Option<std::time::Duration>,
}

impl TurnInput {
    /// A pass whose reading is to be thrown away: the transcript it came from is
    /// not the one that was being followed.
    fn reset(followed: Option<SessionSummary>) -> Self {
        Self {
            marks: Vec::new(),
            finished: Vec::new(),
            live: false,
            restarted: true,
            followed,
            background_quiet_for: None,
        }
    }
}

/// The subagents of one session, and which of them are still running.
#[derive(Default)]
pub struct SubagentTracker {
    /// Resolved once and kept. Finding a session means walking the store's
    /// project directories, and the answer does not change while the CLI keeps
    /// writing the transcript it opened: the id was fixed when the tab opened. It
    /// is replaced outright when the CLI moves to another transcript, and is
    /// known from the start when whoever opened the tab had chosen the session.
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
    /// Which file `scanned_to` is an offset into, as the last pass saw it.
    identity: Option<u64>,
    /// Passes since the sidecars were last listed, and since a successor was
    /// last looked for. Counted rather than timed, as `RESPONDING_DEBOUNCE_TICKS`
    /// is, so a test with no clock can drive them.
    passes_since_list: u32,
    passes_since_search: u32,
    /// The failure the last pass reported, so a persistent one is logged once.
    last_failure: Option<String>,
    /// The sessions this tab has left, oldest first. Never followed back to: the
    /// store cannot tell a transcript the tab is about to move to from one it
    /// moved away from, and the second would send it round in circles.
    left: Vec<Arc<str>>,
}

impl SubagentTracker {
    /// A tracker that reads this session and no other, from its start.
    ///
    /// For a tab opened onto a session somebody already chose. Without it the
    /// tab looks the session up by id, and an id held by two transcripts in two
    /// project directories names whichever the directory listing reaches first.
    pub fn following(session: SessionSummary) -> Self {
        Self {
            session: Some(session),
            ..Self::default()
        }
    }

    /// Whether a look for the session the CLI moved to is due this pass.
    ///
    /// `stalled` is the tab's report that the transcript stopped producing marks
    /// while the CLI kept writing. The first look after a stall begins is not
    /// delayed; the ones after it are spaced out.
    pub fn ready_to_search(&mut self, stalled: bool) -> bool {
        if !stalled {
            self.passes_since_search = SUCCESSOR_SEARCH_EVERY_PASSES;
            return false;
        }
        if self.passes_since_search >= SUCCESSOR_SEARCH_EVERY_PASSES {
            self.passes_since_search = 0;
            return true;
        }
        self.passes_since_search += 1;
        false
    }

    /// What the next pass needs, so the caller can hand it to a background
    /// thread without carrying the tracker along.
    ///
    /// `call_open`: the turn has a tool call with no result yet. `quiet_wanted`:
    /// the turn is held, the one state the subagents' own files are consulted
    /// in.
    pub fn next_request(
        &mut self,
        session_id: SharedString,
        call_open: bool,
        quiet_wanted: bool,
        search: Option<SuccessorSearch>,
    ) -> ScanRequest {
        self.passes_since_list = self.passes_since_list.saturating_add(1);
        let relist = call_open && self.passes_since_list >= RELIST_EVERY_PASSES;
        if relist {
            self.passes_since_list = 0;
        }
        let search = search.map(|mut search| {
            search.claimed.extend(self.left.iter().cloned());
            search
        });
        ScanRequest {
            session: self.session.clone(),
            session_id,
            from: self.scanned_to,
            relist,
            quiet: quiet_wanted,
            search,
        }
    }

    /// Reads the store. For a background thread only.
    pub fn scan(provider: &Arc<dyn SessionProvider>, request: ScanRequest) -> SubagentPass {
        let ScanRequest {
            session,
            session_id,
            from,
            relist,
            quiet,
            search,
        } = request;
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
                    return SubagentPass {
                        unreadable: true,
                        failure: Some(format!(
                            "looking up session {session_id} for its subagents: {error}"
                        )),
                        ..SubagentPass::empty(from)
                    };
                }
            },
        };

        let (completed, mut failure) = match provider.transcript_progress(&session, from) {
            Ok(completed) => (completed, None),
            Err(error) => (
                agent_sessions::TranscriptProgress {
                    scanned_to: from,
                    ..Default::default()
                },
                Some(format!(
                    "reading completed subagents of {session_id}: {error}"
                )),
            ),
        };
        let unreadable = failure.is_some();

        // The sidecar is re-listed only when something could have changed, and
        // that gate is exact rather than approximate: a subagent cannot appear
        // without the parent writing the tool call that spawns it, so a
        // transcript that has not moved cannot have gained one -- bar the one
        // case `relist` covers, a sidecar written a moment after the line that
        // announces it. Without the gate this re-opens and re-parses every
        // sidecar file once a second for the life of every open tab -- twenty-five
        // of them in the session that was measured.
        //
        // `from == 0` is the first pass, which has to list whatever is already
        // there.
        let grew = completed.scanned_to != from;
        let subagents = (grew || from == 0 || relist).then(|| {
            provider
                .subagents(&session)
                .map_err(|error| {
                    failure.get_or_insert_with(|| {
                        format!("reading subagents of {session_id}: {error}")
                    });
                })
                .ok()
        });

        let background_quiet_for = if quiet {
            provider
                .background_quiet_for(&session)
                .unwrap_or_else(|error| {
                    failure.get_or_insert_with(|| {
                        format!("reading subagent activity of {session_id}: {error}")
                    });
                    None
                })
        } else {
            None
        };

        // Only a transcript that did not move can have been left behind.
        let followed = search.filter(|_| !grew && !unreadable).and_then(|search| {
            provider
                .successor(&session, search.since, &search.claimed)
                .unwrap_or_else(|error| {
                    failure.get_or_insert_with(|| {
                        format!("looking for the session {session_id} moved to: {error}")
                    });
                    None
                })
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
            identity: completed.identity,
            followed,
            unreadable,
            failure,
        }
    }

    pub fn apply(&mut self, pass: SubagentPass) -> TurnInput {
        if let Some(message) = self.note_failure(pass.failure.clone()) {
            log::warn!("{message}");
        }
        if let Some(session) = pass.followed {
            self.follow(session.clone());
            return TurnInput::reset(Some(session));
        }
        // A transcript that is not the one the cursor belongs to, whatever its
        // length: the pass has read it from the middle of a line, so none of
        // what it found is worth keeping. Start over, and read it from its first
        // line next time.
        let replaced = !pass.restarted
            && matches!(
                (self.identity, pass.identity),
                (Some(known), Some(now)) if known != now
            );
        if replaced {
            self.forget_transcript();
            return TurnInput::reset(None);
        }
        if pass.restarted {
            // A different transcript: nothing derived from the old one holds.
            // The pass itself is read from the start of the new file and is
            // the past again.
            self.forget_transcript();
        }
        if pass.identity.is_some() && self.identity.is_none() {
            self.identity = pass.identity;
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
            self.passes_since_list = 0;
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
        //
        // Membership is a hash lookup. The first pass over a long history
        // carries the result of every tool call in it, and testing each against
        // every listed subagent in turn is the product of the two.
        if listed {
            let tool_use_ids: HashSet<&str> = self
                .subagents
                .iter()
                .map(|subagent| subagent.tool_use_id.as_ref())
                .collect();
            let ids: HashSet<&str> = self
                .subagents
                .iter()
                .map(|subagent| subagent.id.as_ref())
                .collect();
            prune_after_two_misses(&mut self.finished, &mut self.unlisted.finished, |id| {
                tool_use_ids.contains(id.as_ref())
            });
            prune_after_two_misses(&mut self.stopped, &mut self.unlisted.stopped, |id| {
                ids.contains(id.as_ref())
            });
            prune_after_two_misses(&mut self.resumed, &mut self.unlisted.resumed, |id| {
                ids.contains(id.as_ref())
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
            followed: None,
            background_quiet_for: self.background_quiet_for,
        }
    }

    /// Whether `failure` is news: not the one the last pass reported. A
    /// persistent failure -- a transcript unreadable for as long as the tab
    /// lives -- is met on every pass, and saying so once a second is a log
    /// nobody can read. Clears when a pass reports none.
    fn note_failure(&mut self, failure: Option<String>) -> Option<String> {
        match failure {
            Some(message) if self.last_failure.as_ref() != Some(&message) => {
                self.last_failure = Some(message.clone());
                Some(message)
            }
            Some(_) => None,
            None => {
                self.last_failure = None;
                None
            }
        }
    }

    /// Forgets everything derived from the transcript this tracker was reading,
    /// so the next pass reads whatever is at the path from its start and takes
    /// it as the past.
    fn forget_transcript(&mut self) {
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
        self.identity = None;
    }

    /// Moves to the session the CLI wrote next. Its sidecars are not the old
    /// session's, so the list goes too.
    fn follow(&mut self, session: SessionSummary) {
        if let Some(previous) = self.session.as_ref() {
            if self.left.len() >= REMEMBERED_SESSIONS {
                self.left.remove(0);
            }
            self.left.push(previous.id.clone());
        }
        self.forget_transcript();
        self.subagents = Arc::from([]);
        self.passes_since_list = 0;
        self.session = Some(session);
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

    /// The id of the session this pass found the CLI had moved to, if it did.
    pub fn followed_id(&self) -> Option<&str> {
        self.followed.as_ref().map(|session| &*session.id)
    }

    /// Drops what this pass found about the CLI having moved, for a move the tab
    /// is not to make. What else it read is kept: the transcript did not grow, and
    /// nothing else in it is in question.
    pub fn forgo_follow(&mut self) {
        self.followed = None;
    }

    /// What a scan that read to `scanned_to` and found `marks` would have
    /// produced.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn simulated_marks(marks: Vec<TurnMark>, scanned_to: u64) -> Self {
        Self {
            turn_marks: marks,
            scanned_to,
            ..Self::default()
        }
    }

    /// What a scan that found the CLI had moved to `session` would have
    /// produced.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn simulated_follow(session: SessionSummary) -> Self {
        Self {
            followed: Some(session),
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

        let scanned_to = tracker.scanned_to;
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

    // --- reading a store, with a provider that says exactly what it is told to ---

    use agent_sessions::{
        AgentCommand, Availability, Deletion, Fork, SessionCounts, TranscriptProgress,
    };
    use std::{
        collections::VecDeque,
        path::{Path, PathBuf},
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    fn summary(id: &str) -> SessionSummary {
        SessionSummary {
            id: Arc::from(id),
            agent: AgentKind::Claude,
            title: id.to_string(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from("/w/one"),
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH,
            log_path: Some(PathBuf::from(format!("/w/one/{id}.jsonl"))),
            log_bytes: 0,
        }
    }

    #[derive(Default)]
    struct Scripted {
        passes: Mutex<VecDeque<anyhow::Result<TranscriptProgress>>>,
        listed: Vec<SubagentSummary>,
        /// The transcripts the store holds that open on a clear, in the order it
        /// would offer them. Like the real store's, an answer never names the
        /// session asked about or one the caller says to pass over.
        successors: Vec<SessionSummary>,
        lists: AtomicUsize,
        quiets: AtomicUsize,
        searches: AtomicUsize,
    }

    impl Scripted {
        fn then(self, pass: anyhow::Result<TranscriptProgress>) -> Self {
            self.passes.lock().unwrap().push_back(pass);
            self
        }
    }

    impl SessionProvider for Scripted {
        fn agent(&self) -> AgentKind {
            AgentKind::Claude
        }
        fn availability(&self) -> Availability {
            Availability::Ready
        }
        fn list(&self) -> anyhow::Result<Vec<SessionSummary>> {
            Ok(Vec::new())
        }
        fn find(&self, id: &str) -> anyhow::Result<Option<SessionSummary>> {
            Ok(Some(summary(id)))
        }
        fn new_session_command(&self, _id: &str, _cwd: &Path) -> Option<AgentCommand> {
            None
        }
        fn counts(&self, _session: &SessionSummary) -> anyhow::Result<SessionCounts> {
            Ok(SessionCounts::default())
        }
        fn subagents(&self, _session: &SessionSummary) -> anyhow::Result<Vec<SubagentSummary>> {
            self.lists.fetch_add(1, Ordering::SeqCst);
            Ok(self.listed.clone())
        }
        fn transcript_progress(
            &self,
            _session: &SessionSummary,
            from: u64,
        ) -> anyhow::Result<TranscriptProgress> {
            self.passes.lock().unwrap().pop_front().unwrap_or_else(|| {
                Ok(TranscriptProgress {
                    scanned_to: from,
                    ..TranscriptProgress::default()
                })
            })
        }
        fn background_quiet_for(
            &self,
            _session: &SessionSummary,
        ) -> anyhow::Result<Option<Duration>> {
            self.quiets.fetch_add(1, Ordering::SeqCst);
            Ok(Some(Duration::from_secs(5)))
        }
        fn successor(
            &self,
            session: &SessionSummary,
            _since: SystemTime,
            claimed: &[Arc<str>],
        ) -> anyhow::Result<Option<SessionSummary>> {
            self.searches.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .successors
                .iter()
                .find(|candidate| candidate.id != session.id && !claimed.contains(&candidate.id))
                .cloned())
        }
        fn reports_turns(&self) -> bool {
            true
        }
        fn resume_command(&self, _session: &SessionSummary, _fork: Fork) -> Option<AgentCommand> {
            None
        }
        fn deletion(&self, _session: &SessionSummary) -> Deletion {
            Deletion::Nothing
        }
    }

    fn progress(scanned_to: u64, identity: Option<u64>) -> TranscriptProgress {
        TranscriptProgress {
            scanned_to,
            identity,
            ..TranscriptProgress::default()
        }
    }

    /// One pass as the tab runs it: ask the tracker what to read, read it, fold
    /// the answer back in.
    fn run_pass(
        tracker: &mut SubagentTracker,
        provider: &Arc<dyn SessionProvider>,
        call_open: bool,
        quiet_wanted: bool,
        search: Option<SuccessorSearch>,
    ) -> TurnInput {
        let request = tracker.next_request("session-one".into(), call_open, quiet_wanted, search);
        tracker.apply(SubagentTracker::scan(provider, request))
    }

    fn search() -> Option<SuccessorSearch> {
        Some(SuccessorSearch {
            since: SystemTime::UNIX_EPOCH,
            claimed: Vec::new(),
        })
    }

    /// The sidecar is written when the call that spawns the subagent starts
    /// running, after the parent line that announces it. A pass that lands in
    /// between lists a directory that does not have it yet, and a lone
    /// foreground subagent leaves the parent silent for as long as it runs.
    #[test]
    fn the_sidecars_are_listed_again_while_a_call_is_open_though_the_transcript_is_still() {
        let scripted = Arc::new(Scripted::default().then(Ok(progress(100, Some(1)))));
        let provider: Arc<dyn SessionProvider> = scripted.clone();
        let mut tracker = SubagentTracker::default();

        run_pass(&mut tracker, &provider, true, false, None);
        assert_eq!(
            scripted.lists.load(Ordering::SeqCst),
            1,
            "the first pass lists"
        );

        for _ in 0..(RELIST_EVERY_PASSES - 1) {
            run_pass(&mut tracker, &provider, true, false, None);
        }
        assert_eq!(
            scripted.lists.load(Ordering::SeqCst),
            1,
            "not every second: each listing opens every sidecar"
        );
        run_pass(&mut tracker, &provider, true, false, None);
        assert_eq!(
            scripted.lists.load(Ordering::SeqCst),
            2,
            "the listing that follows a few quiet passes with a call still open"
        );
    }

    #[test]
    fn with_no_call_open_a_still_transcript_is_never_listed_again() {
        let scripted = Arc::new(Scripted::default().then(Ok(progress(100, Some(1)))));
        let provider: Arc<dyn SessionProvider> = scripted.clone();
        let mut tracker = SubagentTracker::default();
        for _ in 0..(RELIST_EVERY_PASSES * 3) {
            run_pass(&mut tracker, &provider, false, false, None);
        }
        assert_eq!(scripted.lists.load(Ordering::SeqCst), 1);
    }

    /// `background_quiet_for` stats every file of every subagent. Only a held
    /// turn reads the answer.
    #[test]
    fn the_subagent_files_are_only_consulted_while_a_turn_is_held() {
        let scripted = Arc::new(Scripted::default());
        let provider: Arc<dyn SessionProvider> = scripted.clone();
        let mut tracker = SubagentTracker::default();

        for _ in 0..4 {
            let input = run_pass(&mut tracker, &provider, false, false, None);
            assert_eq!(input.background_quiet_for, None);
        }
        assert_eq!(scripted.quiets.load(Ordering::SeqCst), 0);

        let input = run_pass(&mut tracker, &provider, false, true, None);
        assert_eq!(scripted.quiets.load(Ordering::SeqCst), 1);
        assert_eq!(input.background_quiet_for, Some(Duration::from_secs(5)));
    }

    /// The same bytes a pass later, or more, is not proof of the same file.
    #[test]
    fn a_different_file_at_the_same_path_is_read_again_from_its_first_line() {
        let scripted = Arc::new(
            Scripted::default()
                .then(Ok(progress(500, Some(1))))
                .then(Ok(progress(900, Some(1))))
                // Longer than the cursor, so the length test sees nothing.
                .then(Ok(progress(2000, Some(2))))
                .then(Ok(progress(300, Some(2)))),
        );
        let provider: Arc<dyn SessionProvider> = scripted;
        let mut tracker = SubagentTracker::default();

        run_pass(&mut tracker, &provider, false, false, None);
        assert!(run_pass(&mut tracker, &provider, false, false, None).live);

        let replaced = run_pass(&mut tracker, &provider, false, false, None);
        assert!(replaced.restarted, "the tab is told to drop what it folded");
        assert!(
            replaced.marks.is_empty() && replaced.finished.is_empty(),
            "what was read from the middle of another file's line is discarded"
        );
        assert_eq!(
            tracker.scanned_to, 0,
            "the next pass starts at its first line"
        );

        let reread = run_pass(&mut tracker, &provider, false, false, None);
        assert!(!reread.restarted);
        assert!(!reread.live, "the new file's contents are the past");
        assert_eq!(tracker.scanned_to, 300);
        assert!(run_pass(&mut tracker, &provider, false, false, None).live);
    }

    #[test]
    fn a_first_line_that_is_not_whole_yet_never_reads_as_a_different_file() {
        let scripted = Arc::new(
            Scripted::default()
                .then(Ok(progress(500, Some(1))))
                .then(Ok(progress(600, None))),
        );
        let provider: Arc<dyn SessionProvider> = scripted;
        let mut tracker = SubagentTracker::default();
        run_pass(&mut tracker, &provider, false, false, None);
        let next = run_pass(&mut tracker, &provider, false, false, None);
        assert!(!next.restarted);
        assert_eq!(tracker.scanned_to, 600);
    }

    #[test]
    fn a_transcript_that_shrank_takes_its_new_identity() {
        let scripted = Arc::new(
            Scripted::default()
                .then(Ok(progress(500, Some(1))))
                .then(Ok(TranscriptProgress {
                    restarted: true,
                    ..progress(80, Some(2))
                }))
                .then(Ok(progress(120, Some(2)))),
        );
        let provider: Arc<dyn SessionProvider> = scripted;
        let mut tracker = SubagentTracker::default();
        run_pass(&mut tracker, &provider, false, false, None);
        assert!(run_pass(&mut tracker, &provider, false, false, None).restarted);
        let after = run_pass(&mut tracker, &provider, false, false, None);
        assert!(
            !after.restarted,
            "the replacement's own identity is the one held now, so it is not replaced again"
        );
    }

    /// What clearing a conversation does: the old transcript never grows again
    /// and a new one opens beside it.
    #[test]
    fn a_session_the_cli_moved_to_is_followed_through_the_restart_path() {
        let moved_to = summary("session-two");
        let scripted = Arc::new(
            Scripted {
                successors: vec![moved_to],
                listed: vec![background("agent-bg", "toolu_bg")],
                ..Scripted::default()
            }
            .then(Ok(progress(500, Some(1)))),
        );
        let provider: Arc<dyn SessionProvider> = scripted;
        let mut tracker = SubagentTracker::default();
        run_pass(&mut tracker, &provider, false, false, None);
        assert!(run_pass(&mut tracker, &provider, false, false, None).live);

        let moved = run_pass(&mut tracker, &provider, false, false, search());

        assert!(
            moved.restarted,
            "everything folded from the old file is dropped"
        );
        assert_eq!(moved.followed.as_ref().map(|s| &*s.id), Some("session-two"));
        assert_eq!(
            tracker.scanned_to, 0,
            "the new transcript is read from its start"
        );
        assert!(
            tracker.subagents().is_empty(),
            "the old session's subagents went with it"
        );
        let request = tracker.next_request("session-one".into(), false, false, None);
        assert_eq!(
            request.session.as_ref().map(|session| &*session.id),
            Some("session-two"),
            "later passes read the new session and never look the old id up again"
        );
        assert!(
            !run_pass(&mut tracker, &provider, false, false, None).live,
            "what the new transcript already holds is the past"
        );
    }

    /// Cleared twice. The second transcript also opens on a clear and was
    /// written during the tab's life, so nothing but having been left marks it
    /// as not the way forward -- and a tab that went back to it would be sent on
    /// to the third again, and back, for as long as it stalled.
    #[test]
    fn a_tab_never_follows_its_way_back_to_a_session_it_has_left() {
        let scripted = Arc::new(Scripted {
            successors: vec![summary("session-two"), summary("session-three")],
            ..Scripted::default()
        });
        let provider: Arc<dyn SessionProvider> = scripted;
        let mut tracker = SubagentTracker::following(summary("session-one"));
        let followed = |input: TurnInput| input.followed.map(|session| session.id.to_string());

        assert_eq!(
            followed(run_pass(&mut tracker, &provider, false, false, search())),
            Some("session-two".to_string()),
            "the first clear"
        );
        assert_eq!(
            followed(run_pass(&mut tracker, &provider, false, false, search())),
            Some("session-three".to_string()),
            "the second"
        );

        for _ in 0..3 {
            assert_eq!(
                followed(run_pass(&mut tracker, &provider, false, false, search())),
                None,
                "every other transcript there is, is one the tab left"
            );
        }
        let request = tracker.next_request("session-three".into(), false, false, search());
        assert_eq!(
            request.search_skips(),
            Some(&[Arc::<str>::from("session-one"), Arc::from("session-two")][..]),
            "oldest first, and the one it is on is the provider's to pass over"
        );
    }

    #[test]
    fn only_so_many_left_sessions_are_remembered() {
        let mut tracker = SubagentTracker::following(summary("s0"));
        for n in 1..=(REMEMBERED_SESSIONS + 10) {
            tracker.follow(summary(&format!("s{n}")));
        }
        assert_eq!(tracker.left.len(), REMEMBERED_SESSIONS);
        assert_eq!(
            &*tracker.left[REMEMBERED_SESSIONS - 1],
            format!("s{}", REMEMBERED_SESSIONS + 9)
        );
    }

    #[test]
    fn a_follow_the_tab_forgoes_changes_nothing_else() {
        let mut tracker = SubagentTracker::following(summary("session-one"));
        tracker.apply(SubagentPass {
            scanned_to: 500,
            ..SubagentPass::default()
        });
        let mut pass = SubagentPass {
            scanned_to: 500,
            ..SubagentPass::simulated_follow(summary("session-two"))
        };
        assert_eq!(pass.followed_id(), Some("session-two"));
        pass.forgo_follow();
        assert_eq!(pass.followed_id(), None);

        let input = tracker.apply(pass);

        assert!(input.followed.is_none() && !input.restarted);
        assert_eq!(
            tracker.scanned_to, 500,
            "the transcript it is on is untouched"
        );
        let request = tracker.next_request("session-one".into(), false, false, None);
        assert_eq!(
            request.session.as_ref().map(|session| &*session.id),
            Some("session-one")
        );
    }

    #[test]
    fn a_session_is_only_looked_for_when_the_transcript_did_not_grow() {
        let scripted = Arc::new(
            Scripted {
                successors: vec![summary("session-two")],
                ..Scripted::default()
            }
            .then(Ok(progress(500, Some(1))))
            .then(Ok(progress(900, Some(1)))),
        );
        let provider: Arc<dyn SessionProvider> = scripted.clone();
        let mut tracker = SubagentTracker::default();
        run_pass(&mut tracker, &provider, false, false, None);

        let grown = run_pass(&mut tracker, &provider, false, false, search());
        assert!(
            grown.followed.is_none(),
            "a transcript that moved is not abandoned"
        );
        assert_eq!(scripted.searches.load(Ordering::SeqCst), 0);

        let still = run_pass(&mut tracker, &provider, false, false, search());
        assert!(still.followed.is_some());
        assert_eq!(scripted.searches.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_look_for_the_session_is_due_at_once_and_then_spaced_out() {
        let mut tracker = SubagentTracker::default();
        assert!(!tracker.ready_to_search(false), "not while the CLI is well");
        assert!(
            tracker.ready_to_search(true),
            "the first look is not delayed"
        );
        for _ in 0..SUCCESSOR_SEARCH_EVERY_PASSES {
            assert!(!tracker.ready_to_search(true));
        }
        assert!(tracker.ready_to_search(true));

        assert!(!tracker.ready_to_search(false));
        assert!(
            tracker.ready_to_search(true),
            "a stall that begins again is looked into at once"
        );
    }

    #[test]
    fn a_tab_opened_onto_a_chosen_session_reads_that_one_and_never_looks_it_up() {
        let chosen = SessionSummary {
            cwd: PathBuf::from("/vetted/cwd"),
            ..summary("session-one")
        };
        let mut tracker = SubagentTracker::following(chosen.clone());
        let request = tracker.next_request("session-one".into(), false, false, None);
        assert_eq!(request.session, Some(chosen));
        assert_eq!(request.from, 0);
    }

    #[test]
    fn a_failure_is_news_once_and_again_only_after_it_has_cleared() {
        let mut tracker = SubagentTracker::default();
        assert_eq!(tracker.note_failure(None), None);
        assert_eq!(
            tracker
                .note_failure(Some("reading x: denied".into()))
                .as_deref(),
            Some("reading x: denied")
        );
        for _ in 0..5 {
            assert_eq!(
                tracker.note_failure(Some("reading x: denied".into())),
                None,
                "the same failure every second is one line in the log"
            );
        }
        assert_eq!(
            tracker
                .note_failure(Some("reading x: gone".into()))
                .as_deref(),
            Some("reading x: gone"),
            "a different one is new"
        );
        assert_eq!(tracker.note_failure(None), None);
        assert_eq!(
            tracker
                .note_failure(Some("reading x: gone".into()))
                .as_deref(),
            Some("reading x: gone"),
            "once it cleared, coming back is news"
        );
    }

    #[test]
    fn a_failing_read_is_carried_back_as_a_message_and_not_logged_per_pass() {
        let scripted =
            Arc::new(Scripted::default().then(Err(anyhow::anyhow!("permission denied"))));
        let provider: Arc<dyn SessionProvider> = scripted;
        let mut tracker = SubagentTracker::default();
        let request = tracker.next_request("session-one".into(), false, false, None);
        let pass = SubagentTracker::scan(&provider, request);
        assert!(pass.unreadable);
        assert!(
            pass.failure
                .as_deref()
                .is_some_and(|message| message.contains("permission denied")),
            "{:?}",
            pass.failure
        );
    }

    /// The first pass over a long history hands over the result of every tool
    /// call in it, and a transcript can list hundreds of subagents.
    #[test]
    fn a_long_history_against_many_subagents_is_pruned_to_the_listed_ones() {
        let mut tracker = live_tracker();
        let listed: Vec<SubagentSummary> = (0..500)
            .map(|n| subagent(&format!("agent-{n}"), &format!("toolu_{n}")))
            .collect();
        let finished: Vec<String> = (0..20_000).map(|n| format!("toolu_{n}")).collect();
        let finished_refs: Vec<&str> = finished.iter().map(String::as_str).collect();
        tracker.apply(live_pass(listed.clone(), finished_refs, Vec::new(), 10));
        tracker.apply(live_pass(listed, Vec::new(), Vec::new(), 20));

        assert_eq!(
            tracker.finished.len(),
            500,
            "the results nobody listed are dropped, the listed ones kept"
        );
        assert!(tracker.finished.contains("toolu_499"));
        assert!(!tracker.finished.contains("toolu_500"));
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
