//! The table of the fifteen items `initialize_workspace` puts on the status
//! bar, moved out of `zed.rs` so each one can carry a [`StatusBarItemSpec`]
//! and be shown, hidden and rebuilt from settings instead of only ever being
//! built once at startup.
//!
//! `register` runs once, from inside `initialize_workspace`'s `observe_new`
//! on `Workspace`, so it is handed `workspace: &mut Workspace` directly and
//! builds every item exactly as before. A toggle fires minutes later, with
//! none of that context left -- `window` and `Context<Workspace>` belonged to
//! that one call frame, and `workspace: &mut Workspace` cannot be reobtained
//! by then. Every [`ItemDescriptor::spec`]'s `build` closure below therefore
//! closes over a `WeakEntity<Workspace>` (downgraded from the strong handle
//! `register` is passed) and upgrades it fresh on each call -- never a strong
//! `Entity<Workspace>`, or a hidden item's closure would keep the workspace
//! alive forever and quietly defeat the point of dropping it.
//!
//! `ItemDescriptor` carries only what a menu row needs to decide and describe
//! itself (`id`, `label`, `side`, `is_shown`, `set_shown`) and nothing that
//! needs a live `App`, so the table's own invariants -- unique ids, every row
//! writing a setting, no two rows colliding -- are unit-tested without a
//! `Workspace` at all. `register` supplies the one thing a descriptor cannot
//! carry statically: the `rank` `add_left_item`/`add_right_item` returns, and
//! the `build` closure, which differs per item's constructor.
//!
//! Two items need more than a settings switch:
//! - `lsp_button`'s `ToggleMenu` action closes over the same
//!   `PopoverMenuHandle` used to build every rebuild of the button, so the
//!   action keeps opening the right menu across toggles.
//! - `agent_usage`'s `ToggleUsagePanel` action looks its target up through
//!   `StatusBar::item_of_type` at the moment it fires, rather than closing
//!   over one indicator's handle -- a rebuilt indicator would otherwise leave
//!   a captured handle pointing at a dropped entity.

use activity_indicator::ActivityIndicator;
use agent_usage::AgentUsageIndicator;
use diagnostics::items::DiagnosticIndicator;
use editor::EditorSettings;
use encoding_selector::ActiveBufferEncoding;
use go_to_line::cursor_position::CursorPosition;
use gpui::{App, AppContext as _, Context, Entity, Window};
use image_viewer::ImageInfo;
use keep_awake::{KeepAwake, KeepDisplayAwakeSetting};
use language_selector::ActiveBufferLanguage;
use language_tools::lsp_button::{self, LspButton};
use line_ending_selector::LineEndingIndicator;
use project::project_settings::ProjectSettings;
use project_footprint::{ProjectFootprintIndicator, ProjectFootprintSetting};
use search::search_status_button::SearchButton;
use settings::{EncodingDisplayOptions, Settings, SettingsContent};
use toolchain_selector::ActiveToolchain;
use ui::PopoverMenuHandle;
use util::ResultExt;
use vim::ModeIndicator;
use workspace::active_file_name::ActiveFileName;
use workspace::{
    StatusBarItemBuilder, StatusBarItemSpec, StatusBarSettings, StatusBarSide, Workspace,
};

/// Everything a menu row needs to describe and decide itself, independent of
/// any particular `Workspace` -- the piece of a [`StatusBarItemSpec`] that
/// can be written down as a `const` and unit-tested without an `App`.
#[derive(Clone, Copy)]
struct ItemDescriptor {
    id: &'static str,
    label: &'static str,
    side: StatusBarSide,
    is_shown: fn(&App) -> bool,
    /// Writes the opposite of what `is_shown` returned, to the root settings
    /// struct `update_settings_file`'s closure receives.
    set_shown: fn(&mut SettingsContent, bool),
}

impl ItemDescriptor {
    fn spec(self, rank: usize, build: StatusBarItemBuilder) -> StatusBarItemSpec {
        StatusBarItemSpec {
            id: self.id,
            label: self.label,
            side: self.side,
            rank,
            is_shown: self.is_shown,
            set_shown: self.set_shown,
            build,
        }
    }
}

const SEARCH_BUTTON: ItemDescriptor = ItemDescriptor {
    id: "search_button",
    label: "Search",
    side: StatusBarSide::Left,
    is_shown: |cx| EditorSettings::get_global(cx).search.button,
    set_shown: |content, on| content.editor.search.get_or_insert_default().button = Some(on),
};

const LSP_BUTTON: ItemDescriptor = ItemDescriptor {
    id: "lsp_button",
    label: "Language Servers",
    side: StatusBarSide::Left,
    is_shown: |cx| ProjectSettings::get_global(cx).global_lsp_settings.button,
    set_shown: |content, on| content.global_lsp_settings.get_or_insert_default().button = Some(on),
};

const DIAGNOSTIC_SUMMARY: ItemDescriptor = ItemDescriptor {
    id: "diagnostic_summary",
    label: "Diagnostics",
    side: StatusBarSide::Left,
    is_shown: |cx| ProjectSettings::get_global(cx).diagnostics.button,
    set_shown: |content, on| content.diagnostics.get_or_insert_default().button = Some(on),
};

const ACTIVE_FILE_NAME: ItemDescriptor = ItemDescriptor {
    id: "active_file_name",
    label: "Active File Name",
    side: StatusBarSide::Left,
    is_shown: |cx| StatusBarSettings::get_global(cx).show_active_file,
    set_shown: |content, on| content.status_bar.get_or_insert_default().show_active_file = Some(on),
};

const ACTIVITY_INDICATOR: ItemDescriptor = ItemDescriptor {
    id: "activity_indicator",
    label: "Activity Indicator",
    side: StatusBarSide::Left,
    is_shown: |cx| StatusBarSettings::get_global(cx).activity_indicator,
    set_shown: |content, on| {
        content
            .status_bar
            .get_or_insert_default()
            .activity_indicator = Some(on)
    },
};

const AGENT_USAGE: ItemDescriptor = ItemDescriptor {
    id: "agent_usage",
    label: "Agent Usage",
    side: StatusBarSide::Left,
    is_shown: |cx| {
        let settings = StatusBarSettings::get_global(cx);
        settings.claude_usage_button || settings.codex_usage_button
    },
    // Governed by two keys, one per agent. Per-agent granularity stays where
    // it already lives -- the indicator's own right-click menu
    // (`agent_usage::status_bar_items::TOGGLEABLE_ITEMS`) -- so this row
    // switches both at once rather than duplicating those two rows. That
    // also fixes a real dead end: switching both agents off there leaves the
    // indicator rendering nothing, with no in-app way back on; this row is
    // that way back.
    set_shown: |content, on| {
        let status_bar = content.status_bar.get_or_insert_default();
        status_bar.claude_usage_button = Some(on);
        status_bar.codex_usage_button = Some(on);
    },
};

const KEEP_AWAKE: ItemDescriptor = ItemDescriptor {
    id: "keep_awake",
    label: "Keep Display Awake",
    side: StatusBarSide::Left,
    is_shown: |cx| KeepDisplayAwakeSetting::get_global(cx).0,
    set_shown: |content, on| content.keep_display_awake = Some(on),
};

const PROJECT_FOOTPRINT: ItemDescriptor = ItemDescriptor {
    id: "project_footprint",
    label: "Project Footprint",
    side: StatusBarSide::Right,
    is_shown: ProjectFootprintSetting::is_enabled,
    set_shown: |content, on| content.project_footprint_indicator = Some(on),
};

const ACTIVE_BUFFER_ENCODING: ItemDescriptor = ItemDescriptor {
    id: "active_buffer_encoding",
    label: "Active File Encoding",
    side: StatusBarSide::Right,
    is_shown: |cx| {
        StatusBarSettings::get_global(cx).active_encoding_button != EncodingDisplayOptions::Disabled
    },
    // A three-way enum, not a plain switch: hiding writes `Disabled`, and
    // showing writes `NonUtf8`, its default -- an explicit `Enabled` set
    // before hiding does not survive the round trip. Accepted: this row is a
    // switch, and remembering the prior value would be a stored-state
    // concept serving only this one row.
    set_shown: |content, on| {
        content
            .status_bar
            .get_or_insert_default()
            .active_encoding_button = Some(if on {
            EncodingDisplayOptions::NonUtf8
        } else {
            EncodingDisplayOptions::Disabled
        })
    },
};

const ACTIVE_BUFFER_LANGUAGE: ItemDescriptor = ItemDescriptor {
    id: "active_buffer_language",
    label: "Active Language",
    side: StatusBarSide::Right,
    is_shown: |cx| StatusBarSettings::get_global(cx).active_language_button,
    set_shown: |content, on| {
        content
            .status_bar
            .get_or_insert_default()
            .active_language_button = Some(on)
    },
};

const ACTIVE_TOOLCHAIN_LANGUAGE: ItemDescriptor = ItemDescriptor {
    id: "active_toolchain_language",
    label: "Active Toolchain",
    side: StatusBarSide::Right,
    is_shown: |cx| StatusBarSettings::get_global(cx).active_toolchain_button,
    set_shown: |content, on| {
        content
            .status_bar
            .get_or_insert_default()
            .active_toolchain_button = Some(on)
    },
};

const LINE_ENDING_INDICATOR: ItemDescriptor = ItemDescriptor {
    id: "line_ending_indicator",
    label: "Line Endings",
    side: StatusBarSide::Right,
    is_shown: |cx| StatusBarSettings::get_global(cx).line_endings_button,
    set_shown: |content, on| {
        content
            .status_bar
            .get_or_insert_default()
            .line_endings_button = Some(on)
    },
};

const VIM_MODE_INDICATOR: ItemDescriptor = ItemDescriptor {
    id: "vim_mode_indicator",
    label: "Vim Mode",
    side: StatusBarSide::Right,
    is_shown: |cx| StatusBarSettings::get_global(cx).vim_mode_button,
    set_shown: |content, on| content.status_bar.get_or_insert_default().vim_mode_button = Some(on),
};

const CURSOR_POSITION: ItemDescriptor = ItemDescriptor {
    id: "cursor_position",
    label: "Cursor Position",
    side: StatusBarSide::Right,
    is_shown: |cx| StatusBarSettings::get_global(cx).cursor_position_button,
    set_shown: |content, on| {
        content
            .status_bar
            .get_or_insert_default()
            .cursor_position_button = Some(on)
    },
};

const IMAGE_INFO: ItemDescriptor = ItemDescriptor {
    id: "image_info",
    label: "Image Info",
    side: StatusBarSide::Right,
    is_shown: |cx| StatusBarSettings::get_global(cx).image_info_button,
    set_shown: |content, on| {
        content.status_bar.get_or_insert_default().image_info_button = Some(on)
    },
};

/// The fifteen, in the order they render. `keep_awake` is included even
/// though `register` may skip adding it to a real bar -- the descriptor
/// itself is platform-independent, only its registration is not.
///
/// Test-only: `register` builds each `StatusBarItemSpec` from its own named
/// descriptor above, so nothing outside `#[cfg(test)]` needs this list.
#[cfg(test)]
const ALL_DESCRIPTORS: [ItemDescriptor; 15] = [
    SEARCH_BUTTON,
    LSP_BUTTON,
    DIAGNOSTIC_SUMMARY,
    ACTIVE_FILE_NAME,
    ACTIVITY_INDICATOR,
    AGENT_USAGE,
    KEEP_AWAKE,
    PROJECT_FOOTPRINT,
    ACTIVE_BUFFER_ENCODING,
    ACTIVE_BUFFER_LANGUAGE,
    ACTIVE_TOOLCHAIN_LANGUAGE,
    LINE_ENDING_INDICATOR,
    VIM_MODE_INDICATOR,
    CURSOR_POSITION,
    IMAGE_INFO,
];

pub(crate) fn register(
    workspace: &mut Workspace,
    workspace_handle: &Entity<Workspace>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let weak_workspace = workspace_handle.downgrade();

    let search_button = cx.new(|_| SearchButton::new());

    let lsp_button_menu_handle = PopoverMenuHandle::default();
    let lsp_button =
        cx.new(|cx| LspButton::new(workspace, lsp_button_menu_handle.clone(), window, cx));
    workspace.register_action({
        let lsp_button_menu_handle = lsp_button_menu_handle.clone();
        move |_, _: &lsp_button::ToggleMenu, window, cx| {
            lsp_button_menu_handle.toggle(window, cx);
        }
    });

    let diagnostic_summary = cx.new(|cx| DiagnosticIndicator::new(workspace, cx));
    let active_file_name = cx.new(|_| ActiveFileName::new());
    let activity_indicator = ActivityIndicator::new(
        workspace,
        workspace.project().read(cx).languages().clone(),
        window,
        cx,
    );

    let agent_usage = cx.new(|cx| AgentUsageIndicator::new(window, cx));
    workspace.register_action(
        move |workspace, _: &agent_usage::ToggleUsagePanel, window, cx| {
            // Hiding removes the entity, so a toggle fired while the item is off
            // has nothing live to look up. Silently doing nothing is chosen over
            // re-enabling the item from here: an action that rewrites the user's
            // settings behind their back is worse than one that does nothing.
            if let Some(agent_usage) = workspace
                .status_bar()
                .read(cx)
                .item_of_type::<AgentUsageIndicator>()
            {
                agent_usage.read(cx).panel_handle().toggle(window, cx);
            }
        },
    );

    // Not built at all where the OS will not hold the display: the entity is
    // both the policy and the indicator, so skipping it drops the icon, the
    // subscriptions and the power-check timer together. A dimmed control
    // opening a switch that can never do anything visible is worse than no
    // control -- and the same reasoning means no spec is registered for it
    // below, so the row simply is not in the menu either.
    let keep_awake = cx
        .can_keep_display_awake()
        .then(|| cx.new(|cx| KeepAwake::new(workspace, workspace_handle, cx)));

    let project_footprint =
        cx.new(|cx| ProjectFootprintIndicator::new(workspace, workspace_handle, window, cx));
    let active_buffer_encoding = cx.new(|_| ActiveBufferEncoding::new(workspace));
    let active_buffer_language = cx.new(|_| ActiveBufferLanguage::new(workspace));
    let active_toolchain_language = cx.new(|cx| ActiveToolchain::new(workspace, window, cx));
    let line_ending_indicator = cx.new(|_| LineEndingIndicator::default());
    let vim_mode_indicator = cx.new(|cx| ModeIndicator::new(window, cx));
    let cursor_position = cx.new(|_| CursorPosition::new(workspace));
    let image_info = cx.new(|_| ImageInfo::new(workspace));

    workspace.status_bar().update(cx, |status_bar, cx| {
        let rank = status_bar.add_left_item(search_button, window, cx);
        status_bar.register_toggleable_item(SEARCH_BUTTON.spec(
            rank,
            Box::new(|status_bar, rank, window, cx| {
                let item = cx.new(|_| SearchButton::new());
                status_bar.insert_item_at_rank(StatusBarSide::Left, rank, item, window, cx);
            }),
        ));

        let rank = status_bar.add_left_item(lsp_button, window, cx);
        status_bar.register_toggleable_item(LSP_BUTTON.spec(
            rank,
            Box::new({
                let weak_workspace = weak_workspace.clone();
                let lsp_button_menu_handle = lsp_button_menu_handle.clone();
                move |status_bar, rank, window, cx| {
                    // Hoisted rather than recreated on each rebuild, so
                    // `lsp_button::ToggleMenu` -- registered once, above --
                    // keeps opening the same menu across every toggle.
                    let Some(item) = weak_workspace
                        .update(cx, |workspace, cx| {
                            cx.new(|cx| {
                                LspButton::new(
                                    workspace,
                                    lsp_button_menu_handle.clone(),
                                    window,
                                    cx,
                                )
                            })
                        })
                        .log_err()
                    else {
                        return;
                    };
                    status_bar.insert_item_at_rank(StatusBarSide::Left, rank, item, window, cx);
                }
            }),
        ));

        let rank = status_bar.add_left_item(diagnostic_summary, window, cx);
        status_bar.register_toggleable_item(DIAGNOSTIC_SUMMARY.spec(
            rank,
            Box::new({
                let weak_workspace = weak_workspace.clone();
                move |status_bar, rank, window, cx| {
                    let Some(item) = weak_workspace
                        .update(cx, |workspace, cx| {
                            cx.new(|cx| DiagnosticIndicator::new(workspace, cx))
                        })
                        .log_err()
                    else {
                        return;
                    };
                    status_bar.insert_item_at_rank(StatusBarSide::Left, rank, item, window, cx);
                }
            }),
        ));

        let rank = status_bar.add_left_item(active_file_name, window, cx);
        status_bar.register_toggleable_item(ACTIVE_FILE_NAME.spec(
            rank,
            Box::new(|status_bar, rank, window, cx| {
                let item = cx.new(|_| ActiveFileName::new());
                status_bar.insert_item_at_rank(StatusBarSide::Left, rank, item, window, cx);
            }),
        ));

        let rank = status_bar.add_left_item(activity_indicator, window, cx);
        status_bar.register_toggleable_item(ACTIVITY_INDICATOR.spec(
            rank,
            Box::new({
                let weak_workspace = weak_workspace.clone();
                move |status_bar, rank, window, cx| {
                    let Some(item) = weak_workspace
                        .update(cx, |workspace, cx| {
                            let languages = workspace.project().read(cx).languages().clone();
                            ActivityIndicator::new(workspace, languages, window, cx)
                        })
                        .log_err()
                    else {
                        return;
                    };
                    status_bar.insert_item_at_rank(StatusBarSide::Left, rank, item, window, cx);
                }
            }),
        ));

        // Beside the activity indicator rather than among the right-hand
        // items: quota is something happening to your account over time, the
        // way indexing and downloads are, not a property of the buffer in
        // front of you.
        let rank = status_bar.add_left_item(agent_usage, window, cx);
        status_bar.register_toggleable_item(AGENT_USAGE.spec(
            rank,
            Box::new(|status_bar, rank, window, cx| {
                let item = cx.new(|cx| AgentUsageIndicator::new(window, cx));
                status_bar.insert_item_at_rank(StatusBarSide::Left, rank, item, window, cx);
            }),
        ));

        // Beside the quota indicator for the same reason: both report on
        // something the machine is doing for an agent, not on the buffer.
        if let Some(keep_awake) = keep_awake {
            let rank = status_bar.add_left_item(keep_awake, window, cx);
            status_bar.register_toggleable_item(KEEP_AWAKE.spec(
                rank,
                Box::new({
                    let weak_workspace = weak_workspace.clone();
                    move |status_bar, rank, window, cx| {
                        let Some(workspace) = weak_workspace.upgrade() else {
                            return;
                        };
                        let handle = workspace.clone();
                        let item = workspace.update(cx, |workspace, cx| {
                            cx.new(|cx| KeepAwake::new(workspace, &handle, cx))
                        });
                        status_bar.insert_item_at_rank(StatusBarSide::Left, rank, item, window, cx);
                    }
                }),
            ));
        }

        // Leftmost of the right-hand group: a running total the eye can rest
        // on, kept clear of the buffer-specific controls that follow and out
        // of the left group, which grows and shifts as activity indicators
        // come and go.
        let rank = status_bar.add_right_item(project_footprint, window, cx);
        status_bar.register_toggleable_item(PROJECT_FOOTPRINT.spec(
            rank,
            Box::new({
                let weak_workspace = weak_workspace.clone();
                move |status_bar, rank, window, cx| {
                    let Some(workspace) = weak_workspace.upgrade() else {
                        return;
                    };
                    let handle = workspace.clone();
                    let item = workspace.update(cx, |workspace, cx| {
                        cx.new(|cx| ProjectFootprintIndicator::new(workspace, &handle, window, cx))
                    });
                    status_bar.insert_item_at_rank(StatusBarSide::Right, rank, item, window, cx);
                }
            }),
        ));

        let rank = status_bar.add_right_item(active_buffer_encoding, window, cx);
        status_bar.register_toggleable_item(ACTIVE_BUFFER_ENCODING.spec(
            rank,
            Box::new({
                let weak_workspace = weak_workspace.clone();
                move |status_bar, rank, window, cx| {
                    let Some(item) = weak_workspace
                        .update(cx, |workspace, cx| {
                            cx.new(|_| ActiveBufferEncoding::new(workspace))
                        })
                        .log_err()
                    else {
                        return;
                    };
                    status_bar.insert_item_at_rank(StatusBarSide::Right, rank, item, window, cx);
                }
            }),
        ));

        let rank = status_bar.add_right_item(active_buffer_language, window, cx);
        status_bar.register_toggleable_item(ACTIVE_BUFFER_LANGUAGE.spec(
            rank,
            Box::new({
                let weak_workspace = weak_workspace.clone();
                move |status_bar, rank, window, cx| {
                    let Some(item) = weak_workspace
                        .update(cx, |workspace, cx| {
                            cx.new(|_| ActiveBufferLanguage::new(workspace))
                        })
                        .log_err()
                    else {
                        return;
                    };
                    status_bar.insert_item_at_rank(StatusBarSide::Right, rank, item, window, cx);
                }
            }),
        ));

        let rank = status_bar.add_right_item(active_toolchain_language, window, cx);
        status_bar.register_toggleable_item(ACTIVE_TOOLCHAIN_LANGUAGE.spec(
            rank,
            Box::new({
                let weak_workspace = weak_workspace.clone();
                move |status_bar, rank, window, cx| {
                    let Some(item) = weak_workspace
                        .update(cx, |workspace, cx| {
                            cx.new(|cx| ActiveToolchain::new(workspace, window, cx))
                        })
                        .log_err()
                    else {
                        return;
                    };
                    status_bar.insert_item_at_rank(StatusBarSide::Right, rank, item, window, cx);
                }
            }),
        ));

        let rank = status_bar.add_right_item(line_ending_indicator, window, cx);
        status_bar.register_toggleable_item(LINE_ENDING_INDICATOR.spec(
            rank,
            Box::new(|status_bar, rank, window, cx| {
                let item = cx.new(|_| LineEndingIndicator::default());
                status_bar.insert_item_at_rank(StatusBarSide::Right, rank, item, window, cx);
            }),
        ));

        let rank = status_bar.add_right_item(vim_mode_indicator, window, cx);
        status_bar.register_toggleable_item(VIM_MODE_INDICATOR.spec(
            rank,
            Box::new(|status_bar, rank, window, cx| {
                let item = cx.new(|cx| ModeIndicator::new(window, cx));
                status_bar.insert_item_at_rank(StatusBarSide::Right, rank, item, window, cx);
            }),
        ));

        let rank = status_bar.add_right_item(cursor_position, window, cx);
        status_bar.register_toggleable_item(CURSOR_POSITION.spec(
            rank,
            Box::new({
                let weak_workspace = weak_workspace.clone();
                move |status_bar, rank, window, cx| {
                    let Some(item) = weak_workspace
                        .update(cx, |workspace, cx| {
                            cx.new(|_| CursorPosition::new(workspace))
                        })
                        .log_err()
                    else {
                        return;
                    };
                    status_bar.insert_item_at_rank(StatusBarSide::Right, rank, item, window, cx);
                }
            }),
        ));

        let rank = status_bar.add_right_item(image_info, window, cx);
        status_bar.register_toggleable_item(IMAGE_INFO.spec(
            rank,
            Box::new({
                let weak_workspace = weak_workspace.clone();
                move |status_bar, rank, window, cx| {
                    let Some(item) = weak_workspace
                        .update(cx, |workspace, cx| cx.new(|_| ImageInfo::new(workspace)))
                        .log_err()
                    else {
                        return;
                    };
                    status_bar.insert_item_at_rank(StatusBarSide::Right, rank, item, window, cx);
                }
            }),
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{BorrowAppContext as _, TestAppContext};
    use settings::SettingsStore;

    #[test]
    fn ids_are_unique_and_labels_non_empty() {
        let mut ids: Vec<&str> = ALL_DESCRIPTORS.iter().map(|item| item.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(
            ids.len(),
            ALL_DESCRIPTORS.len(),
            "duplicate id in the status-bar item table"
        );
        for item in ALL_DESCRIPTORS {
            assert!(!item.label.is_empty(), "{} has an empty label", item.id);
        }
    }

    #[test]
    fn every_row_writes_a_setting_when_hidden() {
        for item in ALL_DESCRIPTORS {
            let mut content = SettingsContent::default();
            (item.set_shown)(&mut content, false);
            assert_ne!(
                content,
                SettingsContent::default(),
                "{} did not write anything when hidden",
                item.id
            );
        }
    }

    #[test]
    fn shown_and_hidden_write_different_content() {
        for item in ALL_DESCRIPTORS {
            let mut hidden = SettingsContent::default();
            (item.set_shown)(&mut hidden, false);
            let mut shown = SettingsContent::default();
            (item.set_shown)(&mut shown, true);
            assert_ne!(
                hidden, shown,
                "{} wrote the same content for shown and hidden",
                item.id
            );
        }
    }

    /// The drift guard: two menu rows silently writing one setting is the
    /// failure this catches.
    #[test]
    fn no_two_rows_collide_when_hidden() {
        let written: Vec<(&str, SettingsContent)> = ALL_DESCRIPTORS
            .iter()
            .map(|item| {
                let mut content = SettingsContent::default();
                (item.set_shown)(&mut content, false);
                (item.id, content)
            })
            .collect();

        for (index, (id_a, content_a)) in written.iter().enumerate() {
            for (id_b, content_b) in &written[index + 1..] {
                assert_ne!(
                    content_a, content_b,
                    "{id_a} and {id_b} write the same setting"
                );
            }
        }
    }

    /// The round trip the other table tests never exercised: `set_shown`
    /// writes, and the *same* `is_shown` the menu row reads must see it.
    /// `every_row_writes_a_setting_when_hidden` only proves a write differs
    /// from the default, which stays green even if the getter and the setter
    /// address different keys entirely -- the exact shape of a row that
    /// clicks, saves, and changes nothing.
    #[gpui::test]
    fn set_shown_round_trips_through_the_settings_store(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            EditorSettings::register(cx);
            ProjectSettings::register(cx);
            StatusBarSettings::register(cx);
            KeepDisplayAwakeSetting::register(cx);
            ProjectFootprintSetting::register(cx);

            for item in ALL_DESCRIPTORS {
                // Both directions, and hidden first: a getter reading a key
                // whose default is already `true` would pass a shown-only
                // check by accident.
                for wanted in [false, true, false] {
                    cx.update_global::<SettingsStore, ()>(|store, cx| {
                        store.update_user_settings(cx, |content| (item.set_shown)(content, wanted));
                    });
                    assert_eq!(
                        (item.is_shown)(cx),
                        wanted,
                        "{} wrote {wanted} but read back {}",
                        item.id,
                        (item.is_shown)(cx)
                    );
                }
            }
        });
    }

    #[gpui::test]
    fn is_shown_matches_todays_defaults(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let store = SettingsStore::test(cx);
            cx.set_global(store);
            EditorSettings::register(cx);
            ProjectSettings::register(cx);
            StatusBarSettings::register(cx);
            KeepDisplayAwakeSetting::register(cx);
            ProjectFootprintSetting::register(cx);

            for item in ALL_DESCRIPTORS {
                // `active_file_name` and `line_ending_indicator` default to
                // `false` in `default.json` today; asserting all fifteen
                // `true` would be wrong and would have shipped broken.
                let expected = !matches!(item.id, "active_file_name" | "line_ending_indicator");
                assert_eq!(
                    (item.is_shown)(cx),
                    expected,
                    "{} default visibility",
                    item.id
                );
            }
        });
    }
}
