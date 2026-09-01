//! Sessions grouped by the directory they ran in.
//!
//! Listing sessions is expensive: [`crate::list_all`] opens every transcript on
//! disk. Anything that asks "which sessions ran in this directory" once per row
//! of a list cannot afford to ask the providers, and cannot afford to scan the
//! answer either -- one linear filter per row is `O(rows * sessions)`.
//!
//! This is the answer computed once. Building costs `O(S)`; a lookup is one
//! hash. Deliberately free of `gpui`: the crate around it is synchronous and
//! window-free by design, so the entity that owns an index and refreshes it in
//! the background lives in the UI layer instead.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::SessionSummary;

/// Sessions plus a map from working directory to the sessions that ran there.
///
/// The map holds **indices**, not clones. A `SessionSummary` carries four
/// `String`s and two `PathBuf`s, so a second copy per group would cost six
/// allocations per session for nothing.
pub struct SessionIndex {
    sessions: Arc<[SessionSummary]>,
    by_cwd: HashMap<Arc<Path>, Vec<u32>>,
}

impl SessionIndex {
    pub fn empty() -> Self {
        Self {
            sessions: Arc::from([]),
            by_cwd: HashMap::new(),
        }
    }

    /// `O(S)`, once. Sessions past `u32::MAX` are dropped rather than silently
    /// aliased onto another index -- four billion transcripts is not a real
    /// case, but a wrapped index would point a row at the wrong session, and
    /// that is worse than a missing row.
    pub fn new(sessions: Vec<SessionSummary>) -> Self {
        let sessions: Arc<[SessionSummary]> = Arc::from(sessions);
        let mut by_cwd: HashMap<Arc<Path>, Vec<u32>> = HashMap::new();

        for (ix, session) in sessions.iter().enumerate() {
            let Ok(ix) = u32::try_from(ix) else { break };
            let cwd: Arc<Path> = Arc::from(session.cwd.as_path());
            by_cwd.entry(cwd).or_default().push(ix);
        }

        Self { sessions, by_cwd }
    }

    /// Every session, in the order the providers returned them (newest first).
    pub fn sessions(&self) -> &Arc<[SessionSummary]> {
        &self.sessions
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// The indices of the sessions that ran in `cwd`, newest first. One hash,
    /// no scan.
    ///
    /// Exact directory match, not prefix: a session that ran in a subdirectory
    /// of a worktree belongs to that subdirectory, and rolling it up would put
    /// one session under two rows.
    pub fn indices_for(&self, cwd: &Path) -> &[u32] {
        self.by_cwd.get(cwd).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The sessions that ran in `cwd`, borrowed. Nothing is cloned.
    pub fn sessions_for<'a>(&'a self, cwd: &Path) -> impl Iterator<Item = &'a SessionSummary> {
        self.indices_for(cwd)
            .iter()
            .filter_map(|ix| self.sessions.get(*ix as usize))
    }

    /// Drops one session and rebuilds the map. `O(S)`, and only ever called
    /// after a delete has already touched the disk -- the rebuild is not the
    /// expensive half of that operation.
    pub fn without(&self, id: &str) -> Self {
        let kept: Vec<SessionSummary> = self
            .sessions
            .iter()
            .filter(|session| session.id.as_ref() != id)
            .cloned()
            .collect();
        Self::new(kept)
    }
}

impl Default for SessionIndex {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentKind, SessionSummary};
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn session(id: &str, cwd: &str, age_secs: u64) -> SessionSummary {
        SessionSummary {
            id: Arc::from(id),
            agent: AgentKind::Claude,
            title: id.to_string(),
            preview: String::new(),
            preview_speaker: None,
            cwd: PathBuf::from(cwd),
            branch: None,
            model: None,
            updated_at: SystemTime::UNIX_EPOCH + Duration::from_secs(age_secs),
            log_path: None,
            log_bytes: 0,
        }
    }

    fn ids(index: &SessionIndex, cwd: &str) -> Vec<String> {
        index
            .sessions_for(Path::new(cwd))
            .map(|session| session.id.to_string())
            .collect()
    }

    #[test]
    fn a_lookup_finds_only_the_sessions_of_that_directory() {
        let index = SessionIndex::new(vec![
            session("a", "/repo/main", 3),
            session("b", "/repo/feature", 2),
            session("c", "/repo/main", 1),
        ]);

        assert_eq!(ids(&index, "/repo/main"), vec!["a", "c"]);
        assert_eq!(ids(&index, "/repo/feature"), vec!["b"]);
    }

    #[test]
    fn an_unknown_directory_yields_nothing() {
        let index = SessionIndex::new(vec![session("a", "/repo/main", 1)]);

        assert!(index.indices_for(Path::new("/repo/gone")).is_empty());
        assert_eq!(ids(&index, "/repo/gone"), Vec::<String>::new());
    }

    /// A worktree row must not pay six heap allocations per session per frame.
    /// `ptr::eq` is what makes "borrowed, not cloned" a fact rather than a
    /// claim in a doc comment.
    #[test]
    fn sessions_are_borrowed_not_cloned() {
        let index = SessionIndex::new(vec![session("a", "/repo/main", 1)]);

        let borrowed = index.sessions_for(Path::new("/repo/main")).next().unwrap();
        assert!(std::ptr::eq(borrowed, &index.sessions()[0]));
    }

    /// The providers hand back newest first and the index must not disturb
    /// that -- a worktree row shows the most recent session at the top.
    #[test]
    fn order_within_a_directory_follows_the_input() {
        let index = SessionIndex::new(vec![
            session("newest", "/repo/main", 300),
            session("middle", "/repo/main", 200),
            session("oldest", "/repo/main", 100),
        ]);

        assert_eq!(
            ids(&index, "/repo/main"),
            vec!["newest", "middle", "oldest"]
        );
    }

    #[test]
    fn a_deleted_session_leaves_the_rest_indexed() {
        let index = SessionIndex::new(vec![
            session("a", "/repo/main", 2),
            session("b", "/repo/main", 1),
        ]);

        let after = index.without("a");

        assert_eq!(ids(&after, "/repo/main"), vec!["b"]);
        assert_eq!(after.len(), 1);
    }

    /// Scale is the whole point of the type. Ten thousand sessions across a
    /// hundred directories must still answer for one directory exactly, with
    /// no leakage from its neighbours.
    #[test]
    fn ten_thousand_sessions_index_correctly() {
        let sessions: Vec<_> = (0..10_000)
            .map(|n| session(&format!("s{n}"), &format!("/repo/wt{}", n % 100), n as u64))
            .collect();
        let index = SessionIndex::new(sessions);

        let found = index.indices_for(Path::new("/repo/wt42"));

        assert_eq!(found.len(), 100);
        assert!(
            index
                .sessions_for(Path::new("/repo/wt42"))
                .all(|session| session.cwd == Path::new("/repo/wt42")),
            "a lookup must not leak a neighbouring directory's sessions"
        );
    }
}
