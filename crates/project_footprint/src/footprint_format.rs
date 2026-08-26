//! Display formatting for the footer badge and its popover.
//!
//! Split out of `project_footprint.rs` purely to keep that file under the
//! 200-line guidance -- there is no other reason these are not free functions
//! there. No such helper exists in `util` or `ui` (checked before adding
//! these).

use gpui::SharedString;

/// Formats a byte count as KB/MB/GB, one decimal place above MB.
pub fn format_rss(bytes: u64) -> SharedString {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB).into()
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB).into()
    } else {
        format!("{:.0} KB", bytes / KB).into()
    }
}

/// Formats a CPU percentage as a rounded integer percent.
pub fn format_cpu(percent: f32) -> SharedString {
    format!("{:.0}%", percent).into()
}
