//! The names the user gave agent sessions, kept by session rather than by tab.
//!
//! A tab's name is written into that tab's row, and the row goes when the tab
//! closes -- so a session renamed, closed and reopened from the history or the
//! worktrees list used to come back under its transcript's title. This is the
//! record that outlives the tab.

use std::sync::Arc;

use collections::HashMap;
use gpui::{App, AppContext as _, Context, Entity, Global, SharedString};
use util::ResultExt as _;

use crate::agent_view::persistence::AgentViewDb;

pub struct SessionNames {
    names: HashMap<Arc<str>, SharedString>,
}

struct GlobalSessionNames(Entity<SessionNames>);

impl Global for GlobalSessionNames {}

impl SessionNames {
    /// The names, read from the database on first use.
    ///
    /// Read synchronously: the table holds one short row per session someone
    /// bothered to rename, and a tab opening onto a session has to know its name
    /// before it draws its label, not one frame later.
    pub fn global(cx: &mut App) -> Entity<Self> {
        if let Some(global) = cx.try_global::<GlobalSessionNames>() {
            return global.0.clone();
        }
        let names = AgentViewDb::global(cx)
            .session_names()
            .log_err()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, name)| (Arc::from(id), SharedString::from(name)))
            .collect();
        let entity = cx.new(|_| Self { names });
        cx.set_global(GlobalSessionNames(entity.clone()));
        entity
    }

    /// The names if anything has read them yet, without reading them. For a
    /// caller holding only `&App`.
    pub fn try_global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalSessionNames>()
            .map(|global| global.0.clone())
    }

    #[cfg(test)]
    pub(crate) fn for_test(names: &[(&str, &str)]) -> Self {
        Self {
            names: names
                .iter()
                .map(|(id, name)| (Arc::from(*id), SharedString::from(name.to_string())))
                .collect(),
        }
    }

    pub fn name(&self, session_id: &str) -> Option<&SharedString> {
        self.names.get(session_id)
    }

    /// Drops the names of sessions that have been deleted, so a deleted
    /// session's label does not stay behind in the table for good.
    pub fn forget_deleted(
        &mut self,
        session_ids: impl IntoIterator<Item = Arc<str>>,
        cx: &mut Context<Self>,
    ) {
        let named: Vec<Arc<str>> = session_ids
            .into_iter()
            .filter(|id| self.names.remove(id).is_some())
            .collect();
        if named.is_empty() {
            return;
        }
        let db = AgentViewDb::global(cx);
        cx.background_spawn(async move {
            for id in named {
                db.save_session_name(id.to_string(), None).await?;
            }
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
        cx.notify();
    }

    /// Names `session_id`, or forgets its name when `name` is `None`.
    pub fn set(
        &mut self,
        session_id: Arc<str>,
        name: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        if self.names.get(&session_id) == name.as_ref() {
            return;
        }
        match &name {
            Some(name) => self.names.insert(session_id.clone(), name.clone()),
            None => self.names.remove(&session_id),
        };
        let db = AgentViewDb::global(cx);
        cx.background_spawn(async move {
            db.save_session_name(session_id.to_string(), name.map(|name| name.to_string()))
                .await
        })
        .detach_and_log_err(cx);
        cx.notify();
    }
}
