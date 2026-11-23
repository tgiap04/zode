//! The init gate, the focus-check guard, click routing, and the restart rule.
//! `agent_notify_tests` covers the quiet-period and dedup rules; see its
//! module doc for the boundary both files share (real bookkeeping, a
//! synthetic answering signal).

use std::{cell::RefCell, rc::Rc, time::Duration};

use agent_ui::TurnEvent;
use gpui::{AppContext as _, EntityId, TestAppContext};

use crate::agent_notify_signals::needs_reread;
use crate::agent_notify_tests::{notifier, open_tab, open_tab_where};
use crate::{AgentAttention, AgentNotifier, AttentionKind};

/// A platform that cannot post -- macOS outside an app bundle, which is every
/// `cargo run` build -- still tells another device: only the desktop
/// notification depends on the platform.
#[gpui::test]
async fn a_platform_that_cannot_post_still_raises_attention_but_posts_nothing(
    cx: &mut TestAppContext,
) {
    let (_workspace, id, cx) = open_tab_where(cx, false).await;
    let notifier = cx.update(|_, cx| AgentNotifier::global(cx));
    assert!(
        notifier.is_some(),
        "the notifier is built whatever the platform can post"
    );
    let Some(notifier) = notifier else {
        return;
    };
    notifier.update(cx, |notifier, _| notifier.set_attention_wanted(true));
    let heard = Rc::new(RefCell::new(Vec::new()));
    let _subscription = cx.update(|_, cx| {
        let heard = heard.clone();
        cx.subscribe(&notifier, move |_, event: &AgentAttention, _| {
            heard.borrow_mut().push(event.kind);
        })
    });

    notifier.update(cx, |n, cx| {
        n.on_turn(
            id,
            TurnEvent::ApprovalNeeded,
            true,
            "Claude Code".into(),
            cx,
        );
        n.on_turn(id, TurnEvent::Ended, true, "Claude Code".into(), cx);
    });
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();
    notifier.update(cx, |n, cx| n.fire_exit(id, &"Claude Code".into(), cx));
    cx.run_until_parked();

    assert_eq!(
        *heard.borrow(),
        vec![
            AttentionKind::ApprovalNeeded,
            AttentionKind::Answered {
                from_transcript: true
            },
            AttentionKind::Ended,
        ]
    );
    assert!(
        cx.posted_notifications().is_empty(),
        "nothing is posted where the platform cannot show it"
    );
}

/// A guard against a future contributor "improving" this feature by adding
/// the focus check the user explicitly refused. See the crate's module doc.
///
/// Scans production files only -- `agent_notify_tests`* files necessarily
/// name both strings in order to look for them, so they are excluded rather
/// than made to fail on their own guard.
#[test]
fn no_focus_check_anywhere() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&src).expect("crate source directory must exist") {
        let entry = entry.expect("directory entry must be readable");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let is_test_file = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("agent_notify_tests"));
        if is_test_file {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("source file must be readable");
        assert!(
            !text.contains("is_focused") && !text.contains("active_window"),
            "{} must not gate notifications on window focus -- the user refused it",
            path.display()
        );
    }
}

#[gpui::test]
async fn a_click_activates_the_agent_it_named_not_merely_any_agent_tab(cx: &mut TestAppContext) {
    let (workspace, claude_id, cx) = open_tab(cx).await;
    workspace.update_in(cx, |workspace, window, cx| {
        agent_ui::AgentView::open(workspace, project::CODEX_AGENT_ID, None, window, cx);
    });
    cx.run_until_parked();
    let codex_id = workspace
        .read_with(cx, |workspace, cx| {
            workspace
                .items_of_type::<agent_ui::AgentView>(cx)
                .find(|view| view.entity_id() != claude_id)
        })
        .expect("a second tab must exist")
        .entity_id();

    let notifier = notifier(cx);
    notifier.update(cx, |n, cx| {
        n.handle_click(&format!("{claude_id}-answer"), cx);
    });
    cx.run_until_parked();

    workspace.read_with(cx, |workspace, cx| {
        let active = workspace
            .active_item(cx)
            .expect("an item must be active after the click");
        assert_eq!(
            active.item_id(),
            claude_id,
            "the click must bring forward the tab its id named, not the other agent's"
        );
        assert_ne!(active.item_id(), codex_id);
    });
}

/// The restart case: a tab whose terminal has been replaced must be reread,
/// even though a completion task for the dead terminal is still around. A
/// gate on "is a task running" answers this wrongly, and the restarted
/// agent's exit would never be noticed for the rest of that tab's life.
#[gpui::test]
fn a_replaced_terminal_must_be_reread(cx: &mut TestAppContext) {
    let first = tab(cx);
    let second = tab(cx);

    assert!(needs_reread(Some(first), Some(second)));
    assert!(!needs_reread(Some(first), Some(first)));
    assert!(needs_reread(None, Some(first)));
    assert!(needs_reread(Some(first), None));
    assert!(!needs_reread(None, None));
}

/// A stand-in id, the same trick `keep_awake_tests` uses: only its identity
/// matters to `needs_reread`.
fn tab(cx: &mut TestAppContext) -> EntityId {
    cx.update(|cx| cx.new(|_| ()).entity_id())
}
