# The Docker button opened tabs into a pane nobody could see

**Date**: 2026-10-17 15:10
**Severity**: Medium — an intermittent dead button with no error anywhere
**Component**: `container_ui` (`ToggleContainer` handler), `workspace` (`force_remove_pane`, `MultiWorkspace`)
**Status**: ongoing — fix committed (d9871c8, PR #43); CI pending; the user has not confirmed the symptom stopped

## What Happened

In one project, clicking the Docker (containers) button on the sidebar rail sometimes did nothing. Switching to another project made it work. It happened now and then, on the installed Zode.app. The tooltip said "Show Containers" and the button was not lit (no container tab existed in that project). The other rail buttons still worked. Zode.log had no line mentioning docker or containers.

## The Brutal Truth

The galling part is the silence. For the user it meant a button that sometimes did nothing, with no error and no log line to start from. The real gut-punch is the state: the workspace kept pointing at a pane that was gone, and switching back did not clear it. The gates are green and the repro fails on the old code, but the user has not confirmed on a build, and the installed app keeps the bug until a new one ships.

## Technical Details

- **Dispatch:** the rail button dispatches only `zed_actions::container::ToggleContainer`. The handler is registered with `workspace.register_action` in `crates/container_ui/src/container_ui.rs`. With no container tab open, it ends in `standalone::open_in_editor_tab` → `Workspace::add_item_to_active_pane`.
- **Repro:** project A has two panes, the right one active. A goes to the background, where `MultiWorkspace` renders only the active workspace. The right pane's last tab is removed. Result: `panes=1`, but `active_pane` still names the removed pane, and still does after switching back.
- **Mechanism:** `Workspace::force_remove_pane` (inherited from upstream Zed) removed the pane and relied on `pane::Event::Focus` → `handle_pane_focused` → `set_active_pane` to activate the fallback. A background workspace paints nothing, so that event never fires. The container tab went into the orphan pane, off screen. `items_of_type` walks `self.panes`, so the button stayed unlit.
- **Fix** (`d9871c8`, `crates/workspace/src/workspace.rs`): when the removed pane was active, the replacement (`focus_on`, else the last remaining pane) becomes active. On screen this goes through `set_active_pane`. In the background only the two pointers are assigned, plus `refresh_active_item_state`, which refreshes the active path, git repo and `ActiveItemChanged` without retitling the window.
- **Tests** (`crates/workspace/src/multi_workspace_tests.rs`): `a_pane_closed_in_the_background_does_not_stay_active` fails on the unfixed code; `a_pane_closed_on_screen_hands_over_to_the_one_left` covers the on-screen path.
- **Gates, all exit 0:** `cargo test -p workspace` 344 with `test_hibernate_after_ms_zero_disables_hibernation` skipped; `-p sidebar` 76; `-p zode` 71; `-p editor` 704; `./script/clippy`; `cargo fmt --check`.

## What We Tried

- **Lost focus:** a test focused a handle in no rendered element and dispatched `ToggleContainer`. The tab still opened. Ruled out.
- **Zoom:** focusing a pane runs `dismiss_zoomed_items_to_reveal`, which unzooms other panes and hides zoomed docks. Ruled out.
- **Set active in the background too:** the first version did this. The tester pointed out it would retitle the shared window, which belongs to the project in front.
- **Pointers only in the background:** this avoids the retitle, but the reviewer pointed out the active path, repo and `ActiveItemChanged` go stale after switching back, since the focus event then finds the pane already active. Hence `refresh_active_item_state`.
- **Focus steal by background `window.focus`:** suspected and tested. The guard test `a_pane_closed_in_the_background_leaves_the_front_project_focused` passes on the unfixed code too, so focus handling was left alone.

## Root Cause Analysis

The removal path kept the active pane correct through one event, `pane::Event::Focus`, and that event only fires for a painted workspace. A window that renders only its front project never delivers it to the background ones, so the stale pointer survived. It stayed silent because nothing in the path reports failure: the tab lands in a real pane that is off screen, and the lit state reads `self.panes`, which no longer holds it.

## Lessons Learned

- A silent no-op with no log is worth testing hypotheses against before reading more code: two plausible causes were ruled out in two short experiments.
- State that is only kept right by an event breaks wherever that event cannot fire. In a multi-project window, anything relying on focus events breaks for background projects.

## Next Steps

- **User:** confirm on a build containing `d9871c8` that the Docker button opens its tab in the affected project. Not done; no date recorded.
- **Build:** the installed Zode.app keeps the bug until a new build ships. Owner not recorded.
- **CI:** PR #43 CI is pending. Owner not recorded.
- **Open:** find what closed a pane in a background project in the user's sessions. The repro removed a tab by hand, so the real trigger is unknown. Owner not recorded.
- **Pre-existing:** `test_hibernate_after_ms_zero_disables_hibernation` also fails on the unfixed `workspace.rs`. Unrelated; skipped in the gates.
