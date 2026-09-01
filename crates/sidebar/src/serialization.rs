use crate::Sidebar;
use gpui::{Context, px};
use serde::{Deserialize, Serialize};
use util::ResultExt as _;

/// Persisted sidebar state. A session saved before this crate replaced the
/// old thread-based sidebar may carry extra fields (e.g. `active_view`) --
/// `serde` ignores unknown fields by default, so those blobs still
/// deserialize without panicking, just with `width` (or nothing) recovered.
#[derive(Default, Serialize, Deserialize)]
struct SerializedSidebar {
    #[serde(default)]
    width: Option<f32>,
}

impl Sidebar {
    pub(crate) fn serialize_to_string(&self) -> Option<String> {
        serde_json::to_string(&SerializedSidebar {
            width: Some(f32::from(self.width)),
        })
        .log_err()
    }

    pub(crate) fn apply_serialized_state(&mut self, state: &str, cx: &mut Context<Self>) {
        let Some(serialized) = serde_json::from_str::<SerializedSidebar>(state).log_err() else {
            return;
        };
        if let Some(width) = serialized.width {
            self.width = px(width);
            cx.notify();
        }
    }
}
