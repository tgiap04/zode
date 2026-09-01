use crate::Sidebar;
use gpui::{Context, px};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use util::ResultExt as _;

/// Persisted sidebar state. A session saved before this crate replaced the
/// old thread-based sidebar may carry extra fields (e.g. `active_view`) --
/// `serde` ignores unknown fields by default, so those blobs still
/// deserialize without panicking, just with `width` (or nothing) recovered.
#[derive(Default, Serialize, Deserialize)]
struct SerializedSidebar {
    #[serde(default)]
    width: Option<f32>,
    /// Projects the reader closed, by path.
    ///
    /// The closed ones rather than the open ones: projects are open by
    /// default, so a blob from before this field existed restores to
    /// "everything open", which is what it looked like when it was written.
    #[serde(default)]
    collapsed_projects: Vec<Vec<PathBuf>>,
}

impl Sidebar {
    pub(crate) fn serialize_to_string(&self) -> Option<String> {
        serde_json::to_string(&SerializedSidebar {
            width: Some(f32::from(self.width)),
            collapsed_projects: self.collapsed_projects.iter().cloned().collect(),
        })
        .log_err()
    }

    pub(crate) fn apply_serialized_state(&mut self, state: &str, cx: &mut Context<Self>) {
        let Some(serialized) = serde_json::from_str::<SerializedSidebar>(state).log_err() else {
            return;
        };
        if let Some(width) = serialized.width {
            self.width = px(width);
        }
        self.collapsed_projects = serialized.collapsed_projects.into_iter().collect();
        cx.notify();
    }
}
