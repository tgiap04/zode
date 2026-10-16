use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use agent_sessions::{AgentKind, SessionSummary};
use gpui::{TestAppContext, UpdateGlobal as _};
use remote_relay_protocol::{
    Control, MAX_INNER_PAYLOAD_LEN, SessionCursor, SessionEntry, encode_control,
};
use settings::{Settings as _, SettingsStore, WorktreeId};
use worktree::WorktreeSettings;

use crate::session_history::{
    DEFAULT_PAGE_LIMIT, MAX_PAGE_LIMIT, RootScope, clamp_limit, clip, deepest_root_index,
    is_plausible_session_id, open_roots, opened_a_tab, page, resume_toast, session_replies,
    wire_agent,
};

fn session(id: &str, cwd: &str, secs: u64) -> SessionSummary {
    SessionSummary {
        id: Arc::from(id),
        agent: AgentKind::Claude,
        title: format!("title of {id}"),
        preview: format!("preview of {id}"),
        preview_speaker: None,
        cwd: PathBuf::from(cwd),
        branch: Some("main".to_string()),
        model: None,
        updated_at: UNIX_EPOCH + Duration::from_secs(secs),
        log_path: None,
        log_bytes: 0,
    }
}

fn root(worktree_id: u64, path: &str, name: &str) -> RootScope {
    RootScope {
        worktree_id: WorktreeId::from_proto(worktree_id),
        path: PathBuf::from(path),
        name: name.to_string(),
        hidden: None,
    }
}

/// The settings a folder has once the user has marked `private` as private and
/// told the scan to leave out `excluded`.
fn settings_hiding(
    private: &[&str],
    excluded: &[&str],
    cx: &mut TestAppContext,
) -> Arc<WorktreeSettings> {
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.private_files = Some(
                    private
                        .iter()
                        .map(|glob| glob.to_string())
                        .collect::<Vec<_>>()
                        .into(),
                );
                settings.project.worktree.file_scan_exclusions =
                    Some(excluded.iter().map(|glob| glob.to_string()).collect());
            });
        });
        Arc::new(WorktreeSettings::get_global(cx).clone())
    })
}

fn no_open_tab(_: &str) -> Option<String> {
    None
}

fn ids(entries: &[SessionEntry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.id.as_str()).collect()
}

#[test]
fn ids_every_agent_mints_are_plausible() {
    for id in [
        "0d6c6a52-9a3f-4a51-9f0e-6c1b2a3d4e5f",
        "ses_1f2e3d4c5b6aAbCdEf",
        "a",
        "7",
        "rollout.2026_10-16",
        &"x".repeat(128),
    ] {
        assert!(is_plausible_session_id(id), "{id}");
    }
}

#[test]
fn ids_that_could_name_a_path_or_a_flag_or_nothing_are_not_plausible() {
    for id in [
        "",
        ".",
        "..",
        "-",
        "-rf",
        "--resume",
        ".hidden",
        "_private",
        "../etc/passwd",
        "a/b",
        "a\\b",
        "a b",
        "a\0b",
        "a\nb",
        "session\u{e9}",
        &"x".repeat(129),
    ] {
        assert!(!is_plausible_session_id(id), "{id:?}");
    }
}

#[test]
fn text_that_fits_is_left_alone() {
    assert_eq!(clip("", 5), "");
    assert_eq!(clip("short", 5), "short");
    assert_eq!(clip("fits", 200), "fits");
}

#[test]
fn text_that_is_too_long_is_cut_to_the_limit_and_ends_in_an_ellipsis() {
    let clipped = clip(&"a".repeat(201), 200);
    assert_eq!(clipped.chars().count(), 200);
    assert!(clipped.ends_with('…'));
    assert!(clipped.starts_with(&"a".repeat(199)));

    assert_eq!(clip("abcdef", 4), "abc…");
}

#[test]
fn a_cut_never_lands_inside_a_character() {
    let text = "Tiếng Việt đẹp quá 🙂🙂🙂🙂";
    for limit in 1..=text.chars().count() {
        let clipped = clip(text, limit);
        assert!(clipped.chars().count() <= limit, "{limit}: {clipped}");
    }
    let clipped = clip(&"🙂".repeat(300), 200);
    assert_eq!(clipped.chars().count(), 200);
    assert!(clipped.ends_with('…'));
}

#[test]
fn every_agent_has_one_wire_name_the_other_end_can_read_back() {
    assert_eq!(
        [
            AgentKind::Claude,
            AgentKind::Codex,
            AgentKind::Copilot,
            AgentKind::OpenCode
        ]
        .map(wire_agent),
        ["claude", "codex", "copilot", "opencode"]
    );
    for kind in [AgentKind::Claude, AgentKind::OpenCode] {
        assert_eq!(
            AgentKind::from_builtin_agent_id(kind.builtin_agent_id()).map(wire_agent),
            Some(wire_agent(kind))
        );
    }
}

#[test]
fn a_page_size_is_defaulted_and_kept_in_range() {
    assert_eq!(clamp_limit(None), DEFAULT_PAGE_LIMIT);
    assert_eq!(DEFAULT_PAGE_LIMIT, 50);
    assert_eq!(clamp_limit(Some(0)), 1);
    assert_eq!(clamp_limit(Some(1)), 1);
    assert_eq!(clamp_limit(Some(120)), 120);
    assert_eq!(clamp_limit(Some(MAX_PAGE_LIMIT as u32)), 200);
    assert_eq!(clamp_limit(Some(201)), 200);
    assert_eq!(clamp_limit(Some(u32::MAX)), 200);
}

#[test]
fn the_deepest_open_folder_holding_a_directory_owns_it() {
    let roots = [
        root(1, "/a/zode", "zode"),
        root(2, "/a/zode/crates/terminal", "terminal"),
    ];
    assert_eq!(
        deepest_root_index(&roots, Path::new("/a/zode/crates/terminal/src")),
        Some(1)
    );
    assert_eq!(
        deepest_root_index(&roots, Path::new("/a/zode/crates/ui")),
        Some(0)
    );
    assert_eq!(deepest_root_index(&roots, Path::new("/a/zode")), Some(0));
}

#[test]
fn a_sibling_whose_name_starts_the_same_is_not_inside() {
    let only_zode = [root(1, "/a/zode", "zode")];
    assert_eq!(
        deepest_root_index(&only_zode, Path::new("/a/zode-kit/web")),
        None
    );
    let both = [
        root(1, "/a/zode", "zode"),
        root(2, "/a/zode-kit", "zode-kit"),
    ];
    assert_eq!(
        deepest_root_index(&both, Path::new("/a/zode-kit/web")),
        Some(1)
    );
}

#[test]
fn nothing_open_or_nothing_near_means_no_owner() {
    assert_eq!(deepest_root_index(&[], Path::new("/a/zode")), None);
    let roots = [root(1, "/a/zode", "zode")];
    assert_eq!(deepest_root_index(&roots, Path::new("/elsewhere")), None);
    assert_eq!(deepest_root_index(&roots, Path::new("relative/zode")), None);
    assert_eq!(deepest_root_index(&roots, Path::new("")), None);
}

#[test]
fn a_directory_that_climbs_out_of_a_folder_is_not_inside_it() {
    let roots = [root(1, "/a/zode", "zode")];
    for cwd in ["/a/zode/../../etc", "/a/zode/crates/../..", "/a/zode/.."] {
        assert_eq!(deepest_root_index(&roots, Path::new(cwd)), None, "{cwd}");
    }
}

#[test]
fn one_folder_open_twice_is_always_owned_by_the_same_one() {
    let forward = [root(2, "/a/zode", "zode"), root(9, "/a/zode", "zode")];
    let backward = [root(9, "/a/zode", "zode"), root(2, "/a/zode", "zode")];
    let owner = |roots: &[RootScope]| {
        deepest_root_index(roots, Path::new("/a/zode/x")).map(|index| roots[index].worktree_id)
    };
    assert_eq!(owner(&forward), Some(WorktreeId::from_proto(2)));
    assert_eq!(owner(&backward), Some(WorktreeId::from_proto(2)));
}

#[gpui::test]
async fn with_no_window_open_there_are_no_folders(cx: &mut TestAppContext) {
    assert!(cx.update(|cx| open_roots(cx)).is_empty());
}

#[test]
fn only_sessions_inside_an_open_folder_are_listed() {
    let sessions = [
        session("in-root", "/a/zode", 5),
        session("in-child", "/a/zode/crates/terminal", 4),
        session("sibling", "/a/zode-kit/web", 3),
        session("elsewhere", "/home/other", 2),
        session("relative", "crates", 1),
    ];
    let roots = [root(1, "/a/zode", "zode")];
    let (entries, next) = page(&sessions, &roots, None, 50, no_open_tab);
    assert_eq!(ids(&entries), ["in-root", "in-child"]);
    assert_eq!(next, None);

    assert!(page(&sessions, &[], None, 50, no_open_tab).0.is_empty());
}

#[test]
fn an_entry_names_its_folder_by_the_deepest_root_and_never_by_an_absolute_path() {
    let sessions = [
        session("at-root", "/a/zode", 3),
        session("nested", "/a/zode/crates/terminal/src", 2),
        session("deep-root", "/a/zode/crates/ui", 1),
    ];
    let roots = [
        root(7, "/a/zode", "zode"),
        root(8, "/a/zode/crates/ui", "ui"),
    ];
    let (entries, _) = page(&sessions, &roots, None, 50, no_open_tab);

    let seen: Vec<(&str, &str)> = entries
        .iter()
        .map(|entry| (entry.worktree_id.as_str(), entry.folder.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            ("7", "zode"),
            ("7", "zode/crates/terminal/src"),
            ("8", "ui")
        ]
    );
    for entry in &entries {
        assert!(!entry.folder.starts_with('/'), "{}", entry.folder);
        let mut characters = entry.folder.chars();
        let drive_letter = characters.next().is_some_and(|c| c.is_ascii_alphabetic())
            && characters.next() == Some(':');
        assert!(!drive_letter, "{}", entry.folder);
    }
}

#[gpui::test]
async fn a_session_under_a_private_or_excluded_path_of_its_folder_is_not_listed(
    cx: &mut TestAppContext,
) {
    let hiding = RootScope {
        hidden: Some(settings_hiding(&["**/vault"], &["**/dist"], cx)),
        ..root(1, "/r", "r")
    };
    let sessions = [
        session("plain", "/r/src", 6),
        session("at-root", "/r", 5),
        session("private", "/r/vault", 4),
        session("below-private", "/r/vault/keys/old", 3),
        session("excluded", "/r/dist", 2),
        session("below-excluded", "/r/app/dist/out", 1),
    ];
    let (entries, _) = page(
        &sessions,
        std::slice::from_ref(&hiding),
        None,
        50,
        no_open_tab,
    );
    assert_eq!(ids(&entries), ["plain", "at-root"]);

    assert!(hiding.hides(Path::new("/r/vault/keys")));
    assert!(hiding.hides(Path::new("/r/dist")));
    assert!(!hiding.hides(Path::new("/r/src")));
    assert!(!hiding.hides(Path::new("/r")));
    assert!(
        !hiding.hides(Path::new("/elsewhere/vault")),
        "what is not in the folder is not its to hide"
    );
}

#[gpui::test]
async fn each_folder_hides_by_its_own_settings(cx: &mut TestAppContext) {
    let outer = RootScope {
        hidden: Some(settings_hiding(&["**/vault"], &[], cx)),
        ..root(1, "/r", "r")
    };
    let inner = root(2, "/r/vault", "vault");
    let sessions = [
        session("in-the-outer-folder", "/r/vault/x", 2),
        session("hidden-from-the-outer", "/r/vault", 1),
    ];

    let (entries, _) = page(
        &sessions,
        std::slice::from_ref(&outer),
        None,
        50,
        no_open_tab,
    );
    assert!(entries.is_empty());

    let (entries, _) = page(&sessions, &[outer, inner], None, 50, no_open_tab);
    assert_eq!(
        ids(&entries),
        ["in-the-outer-folder", "hidden-from-the-outer"],
        "the deepest folder holds them, and it keeps nothing out"
    );
}

#[test]
fn newest_first_and_ties_broken_by_id() {
    let sessions = [
        session("b", "/r", 10),
        session("old", "/r", 1),
        session("c", "/r", 10),
        session("a", "/r", 10),
        session("new", "/r", 20),
    ];
    let (entries, _) = page(&sessions, &[root(1, "/r", "r")], None, 50, no_open_tab);
    assert_eq!(ids(&entries), ["new", "a", "b", "c", "old"]);
    assert_eq!(entries[0].updated_at, 20_000);
}

#[test]
fn a_page_ends_where_asked_and_the_next_one_starts_after_it() {
    let sessions: Vec<_> = (0..5)
        .map(|index| session(&format!("s{index}"), "/r", 100 - index))
        .collect();
    let roots = [root(1, "/r", "r")];

    let (first, next) = page(&sessions, &roots, None, 2, no_open_tab);
    assert_eq!(ids(&first), ["s0", "s1"]);
    let cursor = next.expect("more follow");
    assert_eq!(
        cursor,
        SessionCursor {
            updated_at: 99_000,
            id: "s1".into()
        }
    );

    let (second, next) = page(&sessions, &roots, Some(&cursor), 2, no_open_tab);
    assert_eq!(ids(&second), ["s2", "s3"]);
    let cursor = next.expect("one more follows");

    let (last, next) = page(&sessions, &roots, Some(&cursor), 2, no_open_tab);
    assert_eq!(ids(&last), ["s4"]);
    assert_eq!(next, None, "nothing follows the last page");
}

#[test]
fn a_page_that_ends_exactly_at_the_end_offers_no_next() {
    let sessions = [session("a", "/r", 3), session("b", "/r", 2)];
    let (entries, next) = page(&sessions, &[root(1, "/r", "r")], None, 2, no_open_tab);
    assert_eq!(entries.len(), 2);
    assert_eq!(next, None);
}

#[test]
fn a_cursor_is_exclusive_even_in_the_middle_of_a_tie() {
    let sessions = [
        session("a", "/r", 10),
        session("b", "/r", 10),
        session("c", "/r", 10),
        session("older", "/r", 9),
    ];
    let cursor = SessionCursor {
        updated_at: 10_000,
        id: "a".into(),
    };
    let (entries, _) = page(
        &sessions,
        &[root(1, "/r", "r")],
        Some(&cursor),
        50,
        no_open_tab,
    );
    assert_eq!(ids(&entries), ["b", "c", "older"]);
}

#[test]
fn a_cursor_naming_a_session_that_has_gone_still_pages_on() {
    let sessions = [session("a", "/r", 10), session("c", "/r", 8)];
    let cursor = SessionCursor {
        updated_at: 9_000,
        id: "gone".into(),
    };
    let (entries, _) = page(
        &sessions,
        &[root(1, "/r", "r")],
        Some(&cursor),
        50,
        no_open_tab,
    );
    assert_eq!(ids(&entries), ["c"]);
}

#[test]
fn a_time_within_the_same_millisecond_is_a_tie_not_an_order() {
    let mut early = session("z", "/r", 0);
    early.updated_at = UNIX_EPOCH + Duration::from_micros(5_100);
    let mut late = session("a", "/r", 0);
    late.updated_at = UNIX_EPOCH + Duration::from_micros(5_900);
    let (entries, _) = page(&[early, late], &[root(1, "/r", "r")], None, 50, no_open_tab);
    assert_eq!(ids(&entries), ["a", "z"], "same millisecond, so by id");
}

#[test]
fn an_id_that_could_not_be_resumed_is_not_listed() {
    let sessions = [
        session("fine", "/r", 3),
        session("has space", "/r", 2),
        session("..", "/r", 1),
    ];
    let (entries, _) = page(&sessions, &[root(1, "/r", "r")], None, 50, no_open_tab);
    assert_eq!(ids(&entries), ["fine"]);
}

#[test]
fn an_entry_carries_what_the_session_knows_and_what_the_host_knows_about_it() {
    let mut codex = session("codex-one", "/r", 5);
    codex.agent = AgentKind::Codex;
    let sessions = [session("claude-one", "/r", 6), codex];
    let (entries, _) = page(&sessions, &[root(4, "/r", "r")], None, 50, |id| {
        (id == "claude-one").then(|| "terminal-42".to_string())
    });

    assert_eq!(
        entries[0],
        SessionEntry {
            id: "claude-one".into(),
            agent: "claude".into(),
            title: "title of claude-one".into(),
            preview: Some("preview of claude-one".into()),
            worktree_id: "4".into(),
            folder: "r".into(),
            branch: Some("main".into()),
            updated_at: 6_000,
            can_fork: true,
            open_agent_id: Some("terminal-42".into()),
        }
    );
    assert_eq!(entries[1].agent, "codex");
    assert!(!entries[1].can_fork, "Codex has no fork");
    assert_eq!(entries[1].open_agent_id, None);
}

#[test]
fn which_agents_can_fork_is_asked_of_the_agent() {
    let sessions: Vec<_> = [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Copilot,
        AgentKind::OpenCode,
    ]
    .into_iter()
    .map(|agent| SessionSummary {
        agent,
        ..session(wire_agent(agent), "/r", 1)
    })
    .collect();
    let (entries, _) = page(&sessions, &[root(1, "/r", "r")], None, 50, no_open_tab);
    let forks: Vec<(&str, bool)> = entries
        .iter()
        .map(|entry| (entry.agent.as_str(), entry.can_fork))
        .collect();
    assert_eq!(
        forks,
        [
            ("claude", true),
            ("codex", false),
            ("copilot", false),
            ("opencode", true)
        ]
    );
}

#[test]
fn the_open_tab_is_asked_for_only_the_sessions_on_the_page() {
    use std::cell::RefCell;
    let asked = RefCell::new(Vec::new());
    let sessions: Vec<_> = (0..5)
        .map(|index| session(&format!("s{index}"), "/r", 100 - index))
        .collect();
    page(&sessions, &[root(1, "/r", "r")], None, 2, |id| {
        asked.borrow_mut().push(id.to_string());
        None
    });
    assert_eq!(*asked.borrow(), ["s0", "s1"]);
}

#[test]
fn folder_and_branch_are_clipped_like_the_text_around_them() {
    let deep = format!("/r/{}", "d".repeat(500));
    let mut long = session("long", &deep, 2);
    long.branch = Some("b".repeat(500));
    let mut multibyte = session("multibyte", "/r", 1);
    multibyte.branch = Some("🙂".repeat(300));

    let (entries, _) = page(
        &[long, multibyte],
        &[root(1, "/r", "r")],
        None,
        50,
        no_open_tab,
    );
    assert_eq!(entries[0].folder.chars().count(), 200);
    assert!(entries[0].folder.ends_with('…'));
    assert!(entries[0].folder.starts_with("r/ddd"));
    assert_eq!(
        entries[0]
            .branch
            .as_deref()
            .map(|branch| branch.chars().count()),
        Some(200)
    );
    assert_eq!(
        entries[1]
            .branch
            .as_deref()
            .map(|branch| branch.chars().count()),
        Some(200),
        "a cut never lands inside a character"
    );
}

#[test]
fn title_and_preview_are_clipped_and_an_empty_title_borrows_the_preview() {
    let mut long = session("long", "/r", 2);
    long.title = "t".repeat(500);
    long.preview = "p".repeat(500);
    let mut untitled = session("untitled", "/r", 1);
    untitled.title = "  ".to_string();
    untitled.preview = "the first words".to_string();
    let mut bare = session("bare", "/r", 0);
    bare.title = String::new();
    bare.preview = String::new();

    let (entries, _) = page(
        &[long, untitled, bare],
        &[root(1, "/r", "r")],
        None,
        50,
        no_open_tab,
    );
    assert_eq!(entries[0].title.chars().count(), 200);
    assert!(entries[0].title.ends_with('…'));
    assert_eq!(
        entries[0].preview.as_deref().map(|p| p.chars().count()),
        Some(200)
    );

    assert_eq!(entries[1].title, "the first words");
    assert_eq!(entries[2].title, "");
    assert_eq!(entries[2].preview, None, "an empty preview is left out");
}

fn entry(index: usize, text: &str) -> SessionEntry {
    SessionEntry {
        id: format!("session-{index:04}"),
        agent: "claude".into(),
        title: text.to_string(),
        preview: Some(text.to_string()),
        worktree_id: "1".into(),
        folder: "zode/crates/terminal".into(),
        branch: Some("main".into()),
        updated_at: 1_760_572_800_000 + index as u64,
        can_fork: true,
        open_agent_id: None,
    }
}

fn cursor_of(entry: &SessionEntry) -> SessionCursor {
    SessionCursor {
        updated_at: entry.updated_at,
        id: entry.id.clone(),
    }
}

#[test]
fn a_short_page_is_one_message_with_its_cursor() {
    let entries = vec![entry(0, "one"), entry(1, "two")];
    let next = cursor_of(&entries[1]);
    let replies = session_replies(7, entries.clone(), Some(next.clone()));
    assert_eq!(
        replies,
        [Control::SessionsListReply {
            request_id: 7,
            sessions: entries,
            more: false,
            next: Some(next),
        }]
    );
}

#[test]
fn an_empty_page_is_still_answered() {
    assert_eq!(
        session_replies(3, Vec::new(), None),
        [Control::SessionsListReply {
            request_id: 3,
            sessions: Vec::new(),
            more: false,
            next: None,
        }]
    );
}

/// Characters JSON must escape cost six bytes each, so 200 entries of them
/// cannot share a message.
#[test]
fn a_long_page_is_split_into_messages_that_each_fit_and_only_the_last_has_a_next() {
    let heavy = "\u{1}".repeat(200);
    let entries: Vec<_> = (0..200).map(|index| entry(index, &heavy)).collect();
    let next = cursor_of(&entries[199]);
    let replies = session_replies(9, entries.clone(), Some(next.clone()));

    assert!(replies.len() > 1, "{} message", replies.len());
    let mut joined = Vec::new();
    for (index, reply) in replies.iter().enumerate() {
        let bytes = encode_control(reply).expect("each message fits one frame");
        assert!(bytes.len() <= MAX_INNER_PAYLOAD_LEN);
        let Control::SessionsListReply {
            request_id,
            sessions,
            more,
            next: reply_next,
        } = reply
        else {
            panic!("not a list reply: {reply:?}");
        };
        assert_eq!(*request_id, 9);
        let is_last = index + 1 == replies.len();
        assert_eq!(*more, !is_last, "only the last has no more");
        assert_eq!(
            reply_next.as_ref(),
            is_last.then_some(&next),
            "the cursor rides on the last message alone"
        );
        joined.extend(sessions.clone());
    }
    assert_eq!(joined, entries);
}

#[test]
fn an_entry_too_big_for_any_message_is_left_out_and_the_rest_still_go() {
    let mut huge = entry(1, "huge");
    huge.branch = Some("b".repeat(MAX_INNER_PAYLOAD_LEN));
    let replies = session_replies(1, vec![entry(0, "before"), huge, entry(2, "after")], None);
    let sent: Vec<String> = replies
        .iter()
        .flat_map(|reply| match reply {
            Control::SessionsListReply { sessions, .. } => sessions.clone(),
            other => panic!("{other:?}"),
        })
        .map(|entry| entry.id)
        .collect();
    assert_eq!(sent, ["session-0000", "session-0002"]);
    for reply in &replies {
        encode_control(reply).expect("what is sent fits");
    }
}

#[test]
fn the_toast_names_the_device_the_way_it_acted_and_the_session() {
    assert_eq!(
        resume_toast("Ada's browser", false, "Fix the terminal snapshot"),
        "Ada's browser resumed “Fix the terminal snapshot”"
    );
    assert_eq!(
        resume_toast("Ada's browser", true, "Fix the terminal snapshot"),
        "Ada's browser forked “Fix the terminal snapshot”"
    );
}

#[test]
fn a_device_name_is_clipped_in_the_toast() {
    let toast = resume_toast(&"n".repeat(100), true, "x");
    let name = toast.trim_end_matches(" forked “x”");
    assert_eq!(name.chars().count(), 40);
    assert!(name.ends_with('…'));
    assert_eq!(
        resume_toast("  Ada's browser  ", false, "x"),
        "Ada's browser resumed “x”"
    );
}

#[test]
fn only_an_answer_that_started_something_counts_as_opening_a_tab() {
    let resumed = |open_agent_id: Option<&str>| Control::SessionResumed {
        request_id: 1,
        open_agent_id: open_agent_id.map(str::to_string),
    };
    assert!(opened_a_tab(&[resumed(None)]));
    assert!(!opened_a_tab(&[resumed(Some("terminal-3"))]));
    assert!(!opened_a_tab(&[]));
}

#[test]
fn the_toast_copes_with_no_device_name_no_title_and_a_long_title() {
    assert_eq!(resume_toast("", false, "x"), "A device resumed “x”");
    assert_eq!(resume_toast("Ada", false, "  "), "Ada resumed “Untitled”");
    let toast = resume_toast("Ada", false, &"w".repeat(100));
    let title: String = toast
        .trim_start_matches("Ada resumed “")
        .trim_end_matches('”')
        .to_string();
    assert_eq!(title.chars().count(), 60);
    assert!(title.ends_with('…'));
}
