use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use fs::{FakeFs, Fs as _, RealFs};
use gpui::{AppContext as _, Entity, TestAppContext, UpdateGlobal as _};
use project::Project;
use remote_relay_protocol::{Control, FileKind, MAX_INNER_PAYLOAD_LEN, encode_control, error_code};
use serde_json::json;
use settings::SettingsStore;
use util::path;

use crate::file_browse::{
    BrowseError, FileBrowser, FileReply, FileRequest, Listing, MAX_DIFF_BYTES, MAX_FILE_BYTES,
    MAX_LIST_ENTRIES, answer_within, list_replies,
};

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let store = SettingsStore::test(cx);
        cx.set_global(store);
    });
}

/// A browser made before the project, which is how the running app has it:
/// projects are found as they are created.
struct Fixture {
    browser: Entity<FileBrowser>,
    project: Entity<Project>,
    worktree_id: String,
}

async fn fixture(fs: Arc<dyn fs::Fs>, root: &Path, cx: &mut TestAppContext) -> Fixture {
    let browser = cx.new(FileBrowser::new);
    let project = Project::test(fs, [root], cx).await;
    cx.run_until_parked();
    let worktree_id = worktree_id_of(&project, cx);
    Fixture {
        browser,
        project,
        worktree_id,
    }
}

fn worktree_id_of(project: &Entity<Project>, cx: &mut TestAppContext) -> String {
    project.read_with(cx, |project, cx| {
        project
            .visible_worktrees(cx)
            .next()
            .expect("a worktree")
            .read(cx)
            .id()
            .to_proto()
            .to_string()
    })
}

async fn ask(
    fixture: &Fixture,
    request: FileRequest,
    cx: &mut TestAppContext,
) -> Result<FileReply, BrowseError> {
    let task = fixture
        .browser
        .update(cx, |browser, cx| browser.handle(request, cx));
    task.await
}

async fn list(
    fixture: &Fixture,
    path: &str,
    cx: &mut TestAppContext,
) -> Result<Listing, BrowseError> {
    let request = FileRequest::List {
        worktree_id: Some(fixture.worktree_id.clone()),
        path: path.to_string(),
    };
    match ask(fixture, request, cx).await? {
        FileReply::Listing(listing) => Ok(listing),
        other => panic!("not a listing: {other:?}"),
    }
}

async fn read(
    fixture: &Fixture,
    path: &str,
    cx: &mut TestAppContext,
) -> Result<Vec<u8>, BrowseError> {
    let request = FileRequest::Read {
        worktree_id: fixture.worktree_id.clone(),
        path: path.to_string(),
    };
    match ask(fixture, request, cx).await? {
        FileReply::File(bytes) => Ok(bytes),
        other => panic!("not a file: {other:?}"),
    }
}

fn names(listing: &Listing) -> Vec<&str> {
    listing
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect()
}

#[gpui::test]
async fn without_a_folder_the_open_folders_are_listed_with_their_ids(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({ "a.txt": "a" }))
        .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    let reply = ask(
        &fixture,
        FileRequest::List {
            worktree_id: None,
            path: String::new(),
        },
        cx,
    )
    .await
    .expect("a listing");
    let FileReply::Listing(listing) = reply else {
        panic!("not a listing");
    };
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].name, "work");
    assert_eq!(listing.entries[0].kind, FileKind::Directory);
    assert_eq!(
        listing.entries[0].worktree_id.as_deref(),
        Some(fixture.worktree_id.as_str())
    );
    assert!(!listing.truncated);
}

#[gpui::test]
async fn children_come_directories_first_then_by_name_without_regard_to_case(
    cx: &mut TestAppContext,
) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/work"),
        json!({
            "zeta.txt": "z",
            "Beta.txt": "bb",
            "alpha.txt": "a",
            "src": { "main.rs": "fn main() {}" },
            "Docs": { "guide.md": "# guide" },
        }),
    )
    .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    let root = list(&fixture, "", cx).await.expect("root");
    assert_eq!(
        names(&root),
        ["Docs", "src", "alpha.txt", "Beta.txt", "zeta.txt"]
    );
    assert_eq!(root.entries[0].kind, FileKind::Directory);
    assert_eq!(root.entries[0].size, None);
    assert_eq!(root.entries[2].kind, FileKind::File);
    assert_eq!(root.entries[3].size, Some(2));
    assert_eq!(
        names(&list(&fixture, "src", cx).await.expect("src")),
        ["main.rs"]
    );
}

#[gpui::test]
async fn a_long_directory_is_cut_at_two_thousand_entries_and_says_so(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({})).await;
    for index in 0..MAX_LIST_ENTRIES + 25 {
        fs.insert_file(
            format!("{}/file-{index:05}.txt", path!("/work")),
            Vec::new(),
        )
        .await;
    }
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    let listing = list(&fixture, "", cx).await.expect("a listing");
    assert_eq!(listing.entries.len(), MAX_LIST_ENTRIES);
    assert!(listing.truncated);
    assert_eq!(listing.entries[0].name, "file-00000.txt");
}

#[test]
fn a_listing_too_long_for_one_message_is_sent_as_several_that_all_fit() {
    let entries: Vec<_> = (0..MAX_LIST_ENTRIES)
        .map(|index| remote_relay_protocol::FileEntry {
            name: format!("a-fairly-long-file-name-{index:05}.txt"),
            kind: FileKind::File,
            size: Some(index as u64),
            worktree_id: None,
        })
        .collect();
    let replies = list_replies(
        9,
        Listing {
            entries: entries.clone(),
            truncated: true,
        },
    );
    assert!(replies.len() > 1);
    let mut joined = Vec::new();
    for (index, reply) in replies.iter().enumerate() {
        let bytes = encode_control(reply).expect("one reply fits one frame");
        assert!(bytes.len() <= MAX_INNER_PAYLOAD_LEN);
        let Control::FilesListReply {
            request_id,
            entries,
            truncated,
            more,
        } = reply
        else {
            panic!("not a listing reply");
        };
        assert_eq!(*request_id, 9);
        assert!(*truncated);
        assert_eq!(
            *more,
            index + 1 < replies.len(),
            "only the last has no more"
        );
        joined.extend(entries.clone());
    }
    assert_eq!(joined, entries);

    let empty = list_replies(
        1,
        Listing {
            entries: Vec::new(),
            truncated: false,
        },
    );
    assert_eq!(empty.len(), 1, "an empty directory is still answered");
}

/// Names made of characters JSON must escape are six times their length on the
/// wire, so a directory of them is far over what its entry count suggests.
#[test]
fn a_listing_that_serializes_past_the_reservation_is_cut_and_says_so() {
    let entries: Vec<_> = (0..MAX_LIST_ENTRIES)
        .map(|index| remote_relay_protocol::FileEntry {
            name: format!("{index:04}{}", "\u{1}".repeat(250)),
            kind: FileKind::File,
            size: None,
            worktree_id: None,
        })
        .collect();
    let replies = list_replies(
        3,
        Listing {
            entries: entries.clone(),
            truncated: false,
        },
    );
    let mut total = 0;
    let mut sent = 0;
    for reply in &replies {
        total += encode_control(reply).expect("fits one frame").len();
        let Control::FilesListReply {
            entries, truncated, ..
        } = reply
        else {
            panic!("not a listing reply");
        };
        assert!(*truncated, "a cut listing says so in every part");
        sent += entries.len();
    }
    assert!(
        sent > 0 && sent < entries.len(),
        "{sent} of {}",
        entries.len()
    );
    assert!(total <= MAX_DIFF_BYTES, "{total} bytes");
}

#[gpui::test]
async fn a_text_file_is_read(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/work"),
        json!({ "src": { "main.rs": "fn main() { println!(\"héllo\"); }\n" }, "empty.txt": "" }),
    )
    .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    assert_eq!(
        read(&fixture, "src/main.rs", cx).await.expect("contents"),
        "fn main() { println!(\"héllo\"); }\n".as_bytes()
    );
    assert_eq!(read(&fixture, "empty.txt", cx).await.expect("empty"), b"");
}

#[gpui::test]
async fn a_file_that_is_not_text_is_refused_as_binary(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({})).await;
    fs.insert_file(
        path!("/work/image.png"),
        vec![0x89, b'P', b'N', b'G', 0, 1, 2],
    )
    .await;
    fs.insert_file(path!("/work/latin1.txt"), vec![b'c', b'a', b'f', 0xE9])
        .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    for name in ["image.png", "latin1.txt"] {
        let error = read(&fixture, name, cx).await.expect_err("refused");
        assert_eq!(error.code, error_code::BINARY, "{name}");
    }
}

#[gpui::test]
async fn a_file_over_a_mebibyte_is_refused_and_one_at_the_limit_is_sent(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({})).await;
    fs.insert_file(
        path!("/work/at-limit.txt"),
        vec![b'x'; MAX_FILE_BYTES as usize],
    )
    .await;
    fs.insert_file(
        path!("/work/too-big.txt"),
        vec![b'x'; MAX_FILE_BYTES as usize + 1],
    )
    .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    assert_eq!(
        read(&fixture, "at-limit.txt", cx)
            .await
            .expect("sent")
            .len(),
        MAX_FILE_BYTES as usize
    );
    let error = read(&fixture, "too-big.txt", cx)
        .await
        .expect_err("refused");
    assert_eq!(error.code, error_code::TOO_LARGE);
}

#[gpui::test]
async fn a_path_that_escapes_or_is_not_in_the_snapshot_is_not_found(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/work"),
        json!({ "src": { "main.rs": "x" }, "a.txt": "a" }),
    )
    .await;
    fs.insert_file(path!("/outside.txt"), b"outside".to_vec())
        .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    for path in [
        "../../etc/passwd",
        "../outside.txt",
        "/etc/passwd",
        "/outside.txt",
        "/work/a.txt",
        "src/../a.txt",
        "src//main.rs",
        "missing.txt",
        "src",
    ] {
        let error = read(&fixture, path, cx).await.expect_err(path);
        assert_eq!(error.code, error_code::NOT_FOUND, "read {path}");
    }
    for path in ["../..", "/", "/work", "a.txt", "nowhere", "src/.."] {
        let error = list(&fixture, path, cx).await.expect_err(path);
        assert_eq!(error.code, error_code::NOT_FOUND, "list {path}");
    }
}

#[gpui::test]
async fn an_unknown_or_malformed_folder_id_is_not_found(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({ "a.txt": "a" }))
        .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    for id in ["0", "123456789", "-1", "", "abc"] {
        let request = FileRequest::Read {
            worktree_id: id.to_string(),
            path: "a.txt".to_string(),
        };
        let error = ask(&fixture, request, cx).await.expect_err(id);
        assert_eq!(error.code, error_code::NOT_FOUND, "{id:?}");
    }
}

#[gpui::test]
async fn a_file_the_scan_excludes_is_invisible(cx: &mut TestAppContext) {
    init(cx);
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.file_scan_exclusions = Some(vec!["**/hidden.txt".into()]);
            });
        });
    });
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/work"),
        json!({ "hidden.txt": "secret", "shown.txt": "ok" }),
    )
    .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    assert_eq!(
        names(&list(&fixture, "", cx).await.expect("a listing")),
        ["shown.txt"]
    );
    let error = read(&fixture, "hidden.txt", cx).await.expect_err("hidden");
    assert_eq!(error.code, error_code::NOT_FOUND);
    assert!(read(&fixture, "shown.txt", cx).await.is_ok());
}

#[gpui::test]
async fn a_symlink_that_leaves_the_folder_is_not_reachable(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({ "a.txt": "a" }))
        .await;
    fs.insert_file(path!("/outside.txt"), b"outside".to_vec())
        .await;
    fs.insert_symlink(path!("/work/escape.txt"), path!("/outside.txt").into())
        .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    assert_eq!(
        names(&list(&fixture, "", cx).await.expect("a listing")),
        ["a.txt"]
    );
    let error = read(&fixture, "escape.txt", cx).await.expect_err("outside");
    assert_eq!(error.code, error_code::NOT_FOUND);
}

#[gpui::test]
async fn only_folders_the_user_can_see_are_offered(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({ "a.txt": "a" }))
        .await;
    fs.insert_tree(path!("/background"), json!({ "b.txt": "b" }))
        .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;
    let hidden = fixture
        .project
        .update(cx, |project, cx| {
            project.find_or_create_worktree(path!("/background"), false, cx)
        })
        .await
        .expect("a worktree")
        .0;
    cx.run_until_parked();
    let hidden_id = hidden.read_with(cx, |worktree, _| worktree.id().to_proto().to_string());

    let FileReply::Listing(roots) = ask(
        &fixture,
        FileRequest::List {
            worktree_id: None,
            path: String::new(),
        },
        cx,
    )
    .await
    .expect("roots") else {
        panic!("not a listing");
    };
    assert_eq!(names(&roots), ["work"]);
    let error = ask(
        &fixture,
        FileRequest::Read {
            worktree_id: hidden_id,
            path: "b.txt".into(),
        },
        cx,
    )
    .await
    .expect_err("not visible");
    assert_eq!(error.code, error_code::NOT_FOUND);
}

async fn git(directory: &Path, arguments: &[&str]) {
    let output = util::command::new_command("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(arguments)
        .current_dir(directory)
        .output()
        .await
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn wait_for_repository(fixture: &Fixture, cx: &mut TestAppContext) {
    for _ in 0..200 {
        let found = fixture
            .project
            .read_with(cx, |project, cx| !project.repositories(cx).is_empty());
        if found {
            return;
        }
        cx.background_executor
            .timer(std::time::Duration::from_millis(25))
            .await;
        cx.run_until_parked();
    }
}

async fn real_repository(cx: &mut TestAppContext) -> (tempfile::TempDir, Fixture) {
    init(cx);
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("a directory");
    let root = directory.path().canonicalize().expect("a real path");
    git(&root, &["init", "-q", "-b", "main"]).await;
    std::fs::write(root.join("tracked.txt"), "one\ntwo\nthree\n").expect("writes");
    std::fs::write(root.join("staged.txt"), "before\n").expect("writes");
    git(&root, &["add", "."]).await;
    git(&root, &["commit", "-q", "-m", "first"]).await;
    let fixture = fixture(Arc::new(RealFs::new(None, cx.executor())), &root, cx).await;
    wait_for_repository(&fixture, cx).await;
    (directory, fixture)
}

/// A repository holding two folders, with the app folder open: `app` holds a
/// file, a private file and one the scan excludes, `other` holds one more.
async fn monorepo(cx: &mut TestAppContext) -> (tempfile::TempDir, Fixture) {
    init(cx);
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.private_files =
                    Some(vec!["**/secret.env".to_string()].into());
                settings.project.worktree.file_scan_exclusions = Some(vec!["**/hidden.txt".into()]);
            });
        });
    });
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("a directory");
    let root = directory.path().canonicalize().expect("a real path");
    git(&root, &["init", "-q", "-b", "main"]).await;
    for path in [
        "app/inside.txt",
        "app/secret.env",
        "app/hidden.txt",
        "other/outside.txt",
    ] {
        std::fs::create_dir_all(root.join(path).parent().expect("a parent")).expect("a folder");
        std::fs::write(root.join(path), "before\n").expect("writes");
    }
    git(&root, &["add", "."]).await;
    git(&root, &["commit", "-q", "-m", "first"]).await;
    let fixture = fixture(
        Arc::new(RealFs::new(None, cx.executor())),
        &root.join("app"),
        cx,
    )
    .await;
    wait_for_repository(&fixture, cx).await;
    for path in [
        "app/inside.txt",
        "app/secret.env",
        "app/hidden.txt",
        "other/outside.txt",
    ] {
        std::fs::write(root.join(path), "after\n").expect("writes");
    }
    (directory, fixture)
}

async fn diff(fixture: &Fixture, cx: &mut TestAppContext) -> Result<(Vec<u8>, bool), BrowseError> {
    let request = FileRequest::Diff {
        worktree_id: fixture.worktree_id.clone(),
    };
    match ask(fixture, request, cx).await? {
        FileReply::Diff { text, truncated } => Ok((text, truncated)),
        other => panic!("not a diff: {other:?}"),
    }
}

#[gpui::test]
async fn the_diff_is_the_working_tree_against_head_staged_changes_included(
    cx: &mut TestAppContext,
) {
    let (directory, fixture) = real_repository(cx).await;
    let root = directory.path().canonicalize().expect("a real path");
    std::fs::write(root.join("tracked.txt"), "one\nTWO\nthree\n").expect("writes");
    std::fs::write(root.join("staged.txt"), "after\n").expect("writes");
    git(&root, &["add", "staged.txt"]).await;

    let (text, truncated) = diff(&fixture, cx).await.expect("a diff");
    let text = String::from_utf8(text).expect("text");
    assert!(!truncated);
    assert!(
        text.contains("diff --git a/tracked.txt b/tracked.txt"),
        "{text}"
    );
    assert!(text.contains("-two\n+TWO\n"), "{text}");
    assert!(
        text.contains("diff --git a/staged.txt b/staged.txt") && text.contains("-before\n+after\n"),
        "a change that is only staged is still a change: {text}"
    );
}

#[gpui::test]
async fn a_diff_over_two_mebibytes_is_cut_and_says_so(cx: &mut TestAppContext) {
    let (directory, fixture) = real_repository(cx).await;
    let root = directory.path().canonicalize().expect("a real path");
    let big: String = (0..120_000)
        .map(|index| format!("line {index:08} of text\n"))
        .collect();
    std::fs::write(root.join("tracked.txt"), big).expect("writes");

    let (text, truncated) = diff(&fixture, cx).await.expect("a diff");
    assert!(truncated);
    assert!(text.len() <= MAX_DIFF_BYTES);
    assert!(text.starts_with(b"diff --git"));
    assert!(text.ends_with(b"\n"));
}

#[gpui::test]
async fn a_folder_outside_any_repository_has_no_diff(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(path!("/work"), json!({ "a.txt": "a" }))
        .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    let error = diff(&fixture, cx).await.expect_err("no repository");
    assert_eq!(error.code, error_code::NOT_FOUND);
}

#[gpui::test]
async fn the_diff_holds_only_this_folders_visible_changes(cx: &mut TestAppContext) {
    let (_directory, fixture) = monorepo(cx).await;

    let (text, truncated) = diff(&fixture, cx).await.expect("a diff");
    let text = String::from_utf8(text).expect("text");
    assert!(!truncated);
    assert!(
        text.contains("diff --git a/app/inside.txt b/app/inside.txt"),
        "{text}"
    );
    for left_out in ["outside.txt", "secret.env", "hidden.txt"] {
        assert!(!text.contains(left_out), "{left_out} leaked: {text}");
    }
}

#[gpui::test]
async fn a_repository_with_no_commit_has_no_diff_to_send(cx: &mut TestAppContext) {
    init(cx);
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("a directory");
    let root = directory.path().canonicalize().expect("a real path");
    git(&root, &["init", "-q", "-b", "main"]).await;
    std::fs::write(root.join("a.txt"), "a\n").expect("writes");
    let fixture = fixture(Arc::new(RealFs::new(None, cx.executor())), &root, cx).await;
    wait_for_repository(&fixture, cx).await;

    let error = diff(&fixture, cx)
        .await
        .expect_err("nothing to compare with");
    assert_eq!(error.code, error_code::NOT_FOUND);
}

#[gpui::test]
async fn private_files_are_not_listed_read_or_found_inside_private_folders(
    cx: &mut TestAppContext,
) {
    init(cx);
    cx.update(|cx| {
        SettingsStore::update_global(cx, |store, cx| {
            store.update_user_settings(cx, |settings| {
                settings.project.worktree.private_files =
                    Some(vec!["**/.env".to_string(), "**/vault".to_string()].into());
            });
        });
    });
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/work"),
        json!({
            ".env": "TOKEN=1",
            "shown.txt": "ok",
            "vault": { "key.txt": "k", "deeper": { "more.txt": "m" } },
        }),
    )
    .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    assert_eq!(
        names(&list(&fixture, "", cx).await.expect("a listing")),
        ["shown.txt"]
    );
    for path in [".env", "vault/key.txt", "vault/deeper/more.txt"] {
        let error = read(&fixture, path, cx).await.expect_err(path);
        assert_eq!(error.code, error_code::NOT_FOUND, "read {path}");
    }
    for path in ["vault", "vault/deeper"] {
        let error = list(&fixture, path, cx).await.expect_err(path);
        assert_eq!(error.code, error_code::NOT_FOUND, "list {path}");
    }
    assert!(read(&fixture, "shown.txt", cx).await.is_ok());
}

#[gpui::test]
async fn a_symlink_is_not_followed_even_to_a_file_inside_the_folder(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/work"),
        json!({ "real.txt": "real", "folder": { "inner.txt": "inner" } }),
    )
    .await;
    fs.insert_symlink(path!("/work/link.txt"), "real.txt".into())
        .await;
    fs.insert_symlink(path!("/work/link-folder"), "folder".into())
        .await;
    let fixture = fixture(fs, Path::new(path!("/work")), cx).await;

    assert_eq!(
        names(&list(&fixture, "", cx).await.expect("a listing")),
        ["folder", "real.txt"]
    );
    for path in ["link.txt", "link-folder/inner.txt"] {
        let error = read(&fixture, path, cx).await.expect_err(path);
        assert_eq!(error.code, error_code::NOT_FOUND, "read {path}");
    }
    let error = list(&fixture, "link-folder", cx).await.expect_err("a link");
    assert_eq!(error.code, error_code::NOT_FOUND);
    assert!(read(&fixture, "real.txt", cx).await.is_ok());
}

/// A folder on the real disk holding `plain.txt` and `swapped.txt`, with
/// whatever `prepare` adds before the scan sees it.
async fn real_folder(
    cx: &mut TestAppContext,
    prepare: impl FnOnce(&Path),
) -> (tempfile::TempDir, Fixture) {
    init(cx);
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("a directory");
    let root = directory.path().canonicalize().expect("a real path");
    std::fs::write(root.join("plain.txt"), "plain\n").expect("writes");
    std::fs::write(root.join("swapped.txt"), "before\n").expect("writes");
    prepare(&root);
    let fixture = fixture(Arc::new(RealFs::new(None, cx.executor())), &root, cx).await;
    (directory, fixture)
}

#[cfg(unix)]
#[allow(
    clippy::disallowed_methods,
    reason = "a test preparing a throwaway folder before the scan, where blocking is harmless"
)]
fn make_fifo(path: &Path) {
    let status = util::command::new_std_command("mkfifo")
        .arg(path)
        .status()
        .expect("mkfifo runs");
    assert!(status.success());
}

#[cfg(unix)]
#[gpui::test]
async fn a_fifo_is_neither_listed_as_readable_nor_opened(cx: &mut TestAppContext) {
    let (_directory, fixture) = real_folder(cx, |root| make_fifo(&root.join("pipe"))).await;

    let error = read(&fixture, "pipe", cx).await.expect_err("a fifo");
    assert_eq!(error.code, error_code::NOT_FOUND);
    assert!(read(&fixture, "plain.txt", cx).await.is_ok());
}

/// The snapshot says an ordinary file; by the time it is read the disk holds
/// something else under that name.
#[cfg(unix)]
#[gpui::test]
async fn a_file_swapped_for_a_fifo_after_the_scan_is_not_opened(cx: &mut TestAppContext) {
    let (directory, fixture) = real_folder(cx, |_| {}).await;
    let root = directory.path().canonicalize().expect("a real path");
    std::fs::remove_file(root.join("swapped.txt")).expect("removes");
    make_fifo(&root.join("swapped.txt"));

    let error = read(&fixture, "swapped.txt", cx).await.expect_err("a fifo");
    assert_eq!(error.code, error_code::NOT_FOUND);
}

#[cfg(unix)]
#[gpui::test]
async fn a_file_swapped_for_a_symlink_after_the_scan_is_not_followed(cx: &mut TestAppContext) {
    let (directory, fixture) = real_folder(cx, |_| {}).await;
    let root = directory.path().canonicalize().expect("a real path");
    std::fs::write(root.join("target.txt"), "elsewhere\n").expect("writes");
    std::fs::remove_file(root.join("swapped.txt")).expect("removes");
    std::os::unix::fs::symlink(root.join("target.txt"), root.join("swapped.txt")).expect("links");

    let error = read(&fixture, "swapped.txt", cx).await.expect_err("a link");
    assert_eq!(error.code, error_code::NOT_FOUND);
}

/// A folder on the way to the file was a folder when the scan ran and is a
/// link to somewhere else by the time the file is read.
#[gpui::test]
async fn a_folder_swapped_for_a_symlink_after_the_scan_is_not_followed(cx: &mut TestAppContext) {
    init(cx);
    let fs = FakeFs::new(cx.executor());
    fs.insert_tree(
        path!("/work"),
        json!({ "sub": { "note.txt": "inside\n" }, "plain.txt": "plain\n" }),
    )
    .await;
    fs.insert_tree(path!("/elsewhere"), json!({ "note.txt": "outside\n" }))
        .await;
    let fixture = fixture(fs.clone(), Path::new(path!("/work")), cx).await;
    assert_eq!(
        read(&fixture, "sub/note.txt", cx).await.expect("read"),
        b"inside\n"
    );

    fs.remove_dir(
        Path::new(path!("/work/sub")),
        fs::RemoveOptions {
            recursive: true,
            ignore_if_not_exists: false,
        },
    )
    .await
    .expect("removes");
    fs.create_symlink(
        Path::new(path!("/work/sub")),
        PathBuf::from(path!("/elsewhere")),
    )
    .await
    .expect("links");

    let error = read(&fixture, "sub/note.txt", cx)
        .await
        .expect_err("the folder now leads outside");
    assert_eq!(error.code, error_code::NOT_FOUND);
    assert!(read(&fixture, "plain.txt", cx).await.is_ok());
}

/// A folder that is itself reached through a symlink is still the user's
/// folder; only what leaves it is refused.
#[cfg(unix)]
#[gpui::test]
async fn a_folder_opened_through_a_symlink_can_still_be_read(cx: &mut TestAppContext) {
    init(cx);
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().expect("a directory");
    let real_root = directory.path().canonicalize().expect("a real path");
    std::fs::create_dir(real_root.join("actual")).expect("creates");
    std::fs::write(real_root.join("actual").join("plain.txt"), "plain\n").expect("writes");
    std::os::unix::fs::symlink(real_root.join("actual"), real_root.join("linked")).expect("links");
    let fixture = fixture(
        Arc::new(RealFs::new(None, cx.executor())),
        &real_root.join("linked"),
        cx,
    )
    .await;

    assert_eq!(
        read(&fixture, "plain.txt", cx).await.expect("read"),
        b"plain\n"
    );
}

/// Not a pipe, not a link, not a folder, and still not an ordinary file.
#[cfg(unix)]
#[gpui::test]
async fn a_file_swapped_for_a_socket_after_the_scan_is_not_opened(cx: &mut TestAppContext) {
    let (directory, fixture) = real_folder(cx, |_| {}).await;
    let root = directory.path().canonicalize().expect("a real path");
    std::fs::remove_file(root.join("swapped.txt")).expect("removes");
    let _socket = std::os::unix::net::UnixListener::bind(root.join("swapped.txt")).expect("binds");

    let error = read(&fixture, "swapped.txt", cx)
        .await
        .expect_err("a socket");
    assert_eq!(error.code, error_code::NOT_FOUND);
}

#[gpui::test]
async fn a_request_that_never_finishes_is_answered_when_its_deadline_does(cx: &mut TestAppContext) {
    let deadline = cx
        .background_executor
        .timer(std::time::Duration::from_secs(30));
    let never = std::future::pending::<Result<FileReply, BrowseError>>();
    let answer = cx.background_executor.spawn(answer_within(deadline, never));
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(31));
    let error = answer.await.expect_err("it was cut off");
    assert_eq!(error.code, error_code::INTERNAL);

    let finished = answer_within(
        std::future::pending(),
        std::future::ready(Ok(FileReply::File(b"done".to_vec()))),
    )
    .await;
    assert_eq!(finished, Ok(FileReply::File(b"done".to_vec())));
}
